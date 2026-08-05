use mercury_cortex_core::db::DB_FILENAME;
use mercury_cortex_core::db::initialize;
use mercury_cortex_core::runtime::RuntimeContext;
use mercury_cortex_core::runtime::status::RuntimePhase;
use mercury_cortex_core::service::graph::GraphService;
use tempfile::TempDir;

async fn ctx_with_db() -> (RuntimeContext, TempDir) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join(DB_FILENAME);
    let db = initialize(&db_path).await.unwrap();
    let ctx = RuntimeContext::new_for_test();
    ctx.set_database(db, RuntimePhase::DatabaseConnected);
    (ctx, tmp)
}

#[tokio::test]
async fn list_all_returns_rows_from_every_relation_table() {
    let (ctx, _tmp) = ctx_with_db().await;
    let db = ctx.database().unwrap();
    for table in [
        "owns",
        "contains",
        "imports",
        "calls",
        "depends_on",
        "part_of_pattern",
    ] {
        db.query(format!("CREATE {table} SET out = projects:1, in = users:1"))
            .await
            .unwrap();
    }

    let rows = GraphService::list_all(&ctx).await.unwrap();
    assert_eq!(rows.len(), 6, "one row from each relation table");
}

#[tokio::test]
async fn list_all_includes_rows_from_a_populated_db() {
    let (ctx, _tmp) = ctx_with_db().await;
    let db = ctx.database().unwrap();
    db.query("CREATE owns SET out = projects:1, in = users:1")
        .await
        .unwrap();

    let rows = GraphService::list_all(&ctx).await.unwrap();
    assert_eq!(rows.len(), 1, "the seeded edge row appears in the listing");
}

#[tokio::test]
async fn list_all_on_unmigrated_db_returns_empty() {
    // initialize() does NOT run migrations, so the graph tables don't exist
    // yet — existing_tables must tolerate this and return an empty list.
    let (ctx, _tmp) = ctx_with_db().await;
    let rows = GraphService::list_all(&ctx).await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn list_by_project_filters_to_matching_edges() {
    let (ctx, _tmp) = ctx_with_db().await;
    let db = ctx.database().unwrap();
    db.query("CREATE owns SET out = projects:aaa, in = users:1")
        .await
        .unwrap();
    db.query("CREATE contains SET out = projects:bbb, in = users:1")
        .await
        .unwrap();
    db.query("CREATE imports SET out = projects:aaa, in = projects:bbb")
        .await
        .unwrap();

    let rows = GraphService::list_by_project(&ctx, "projects:aaa")
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "only edges touching projects:aaa");
}

#[tokio::test]
async fn list_by_project_rejects_invalid_id() {
    let (ctx, _tmp) = ctx_with_db().await;
    let err = GraphService::list_by_project(&ctx, "not-a-record-id")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        mercury_cortex_core::service::ServiceError::Validation(_)
    ));
}
