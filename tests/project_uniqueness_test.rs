use mercury_cortex_core::db::DB_FILENAME;
use mercury_cortex_core::db::initialize;
use mercury_cortex_core::runtime::RuntimeContext;
use mercury_cortex_core::runtime::status::RuntimePhase;
use mercury_cortex_core::schema::run_pending;
use mercury_cortex_core::service::ServiceError;
use mercury_cortex_core::service::profile::{ProfileService, UpsertParams};
use mercury_cortex_core::service::project::{ProjectAction, ProjectService, RegisterParams};
use tempfile::TempDir;

async fn ctx_with_user() -> (RuntimeContext, TempDir) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join(DB_FILENAME);
    let db = initialize(&db_path).await.unwrap();
    run_pending(&db).await.unwrap();
    let ctx = RuntimeContext::new_for_test();
    ctx.set_database(db, RuntimePhase::DatabaseConnected);
    ProfileService::upsert(
        &ctx,
        UpsertParams {
            id: None,
            name: "Test User".into(),
            email: "test@example.com".into(),
            agent_name: "agent-test".into(),
            profile_type: "personal".into(),
        },
    )
    .await
    .unwrap();
    (ctx, tmp)
}

#[tokio::test]
async fn register_canonicalizes_root_path() {
    let (ctx, _tmp) = ctx_with_user().await;
    let p1 = RegisterParams {
        config_project_id: None,
        name: "A".into(),
        slug: "a".into(),
        root_path: "/tmp/example".into(),
    };
    let r1 = ProjectService::register(&ctx, p1).await.unwrap();
    assert_eq!(r1.action, ProjectAction::Created);

    let p2 = RegisterParams {
        config_project_id: None,
        name: "A".into(),
        slug: "a".into(),
        root_path: "/tmp/example/".into(),
    };
    let r2 = ProjectService::register(&ctx, p2).await.unwrap();
    assert_ne!(
        r2.action,
        ProjectAction::Created,
        "variant must be reconciled"
    );

    let rows: Vec<serde_json::Value> = ctx
        .database()
        .unwrap()
        .query("SELECT count() AS n FROM projects GROUP ALL")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    let count = rows
        .first()
        .and_then(|r| r.as_object())
        .and_then(|o| o.get("n"))
        .and_then(|n| n.as_i64())
        .unwrap();
    assert_eq!(count, 1, "both spellings must converge to one project");
}

#[tokio::test]
async fn v005_dedups_existing_duplicates() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join(DB_FILENAME);
    let db = initialize(&db_path).await.unwrap();

    // Apply the full schema, then roll v005 back so we can seed the
    // pre-uniqueness state it exists to repair: duplicate projects sharing a
    // root_path with no unique index yet.
    run_pending(&db).await.unwrap();
    db.query("REMOVE INDEX unique_root_path ON TABLE projects")
        .await
        .unwrap();
    db.query("DELETE _migrations WHERE version = 5")
        .await
        .unwrap();

    // A user to own both projects.
    db.query(
        "CREATE users:owner SET name = 'Owner', email = 'owner@example.com', \
         agent_name = 'agent-owner', type = 'personal', \
         created_at = time::now(), updated_at = time::now()",
    )
    .await
    .unwrap();

    // Two projects at the same root_path with different updated_at (older
    // first). The newer one uses a trailing-slash spelling so the migration's
    // canonicalize-then-UPDATE step (step 1) is exercised on the survivor.
    db.query(
        "CREATE projects SET owner_id = users:owner, name = 'Old', slug = 'old', \
         root_path = '/dup/root', created_at = d'2024-01-01T00:00:00Z', \
         updated_at = d'2024-01-01T00:00:00Z'",
    )
    .await
    .unwrap();
    db.query(
        "CREATE projects SET owner_id = users:owner, name = 'New', slug = 'new', \
         root_path = '/dup/root/', created_at = d'2024-01-02T00:00:00Z', \
         updated_at = d'2024-01-02T00:00:00Z'",
    )
    .await
    .unwrap();

    // Attach file_data to both projects so we can prove the removed
    // duplicate's file_data is cleaned up too.
    let pids: Vec<surrealdb::types::Value> = db
        .query("SELECT VALUE id FROM projects")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    assert_eq!(pids.len(), 2, "two duplicate projects must be seeded");
    for pid in pids {
        db.query("CREATE file_data SET project_id = $pid, path = 'x.rs', indexed_at = time::now(), updated_at = time::now()")
            .bind(("pid", pid))
            .await
            .unwrap();
    }

    // v005 runs now: it must pre-dedup, keeping the most recently updated.
    run_pending(&db).await.unwrap();

    let names: Vec<surrealdb::types::Value> = db
        .query("SELECT name FROM projects")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    assert_eq!(names.len(), 1, "dedup must leave exactly one project");
    let name = names[0]
        .as_object()
        .and_then(|o| o.get("name"))
        .and_then(|v| v.as_string())
        .map(|s| s.as_str())
        .unwrap();
    assert_eq!(
        name, "New",
        "the most recently updated project must survive"
    );

    let root_paths: Vec<surrealdb::types::Value> = db
        .query("SELECT root_path FROM projects")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    let root = root_paths[0]
        .as_object()
        .and_then(|o| o.get("root_path"))
        .and_then(|v| v.as_string())
        .map(|s| s.as_str())
        .unwrap();
    assert_eq!(
        root, "/dup/root",
        "the surviving root_path must be canonicalized (trailing slash stripped)"
    );

    let fd_rows: Vec<serde_json::Value> = db
        .query("SELECT count() AS n FROM file_data GROUP ALL")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    let fd_count = fd_rows
        .first()
        .and_then(|r| r.as_object())
        .and_then(|o| o.get("n"))
        .and_then(|n| n.as_i64())
        .unwrap();
    assert_eq!(
        fd_count, 1,
        "the removed duplicate's file_data must be gone"
    );

    // The unique index now rejects a third project at the same root_path.
    let third = db
        .query(
            "CREATE projects SET owner_id = users:owner, name = 'Third', slug = 'third', \
             root_path = '/dup/root', created_at = time::now(), updated_at = time::now()",
        )
        .await
        .unwrap();
    assert!(
        third.check().is_err(),
        "unique index must reject a duplicate root_path"
    );
}

#[tokio::test]
async fn v005_drops_filesystem_root_projects() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join(DB_FILENAME);
    let db = initialize(&db_path).await.unwrap();

    run_pending(&db).await.unwrap();
    db.query("REMOVE INDEX unique_root_path ON TABLE projects")
        .await
        .unwrap();
    db.query("DELETE _migrations WHERE version = 5")
        .await
        .unwrap();

    db.query(
        "CREATE users:owner SET name = 'Owner', email = 'owner@example.com', \
         agent_name = 'agent-owner', type = 'personal', \
         created_at = time::now(), updated_at = time::now()",
    )
    .await
    .unwrap();

    // A pre-existing whole-filesystem project — the state register() rejects,
    // but which could exist in a database created before v005's policy.
    db.query(
        "CREATE projects SET owner_id = users:owner, name = 'Root', slug = 'root', \
         root_path = '/', created_at = d'2024-01-01T00:00:00Z', \
         updated_at = d'2024-01-01T00:00:00Z'",
    )
    .await
    .unwrap();
    let pids: Vec<surrealdb::types::Value> = db
        .query("SELECT VALUE id FROM projects")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    assert_eq!(pids.len(), 1, "one filesystem-root project must be seeded");
    for pid in pids {
        db.query("CREATE file_data SET project_id = $pid, path = 'etc/passwd', indexed_at = time::now(), updated_at = time::now()")
            .bind(("pid", pid))
            .await
            .unwrap();
    }

    // v005 runs now: it must drop the filesystem-root project and its file_data.
    run_pending(&db).await.unwrap();

    let count: Vec<serde_json::Value> = db
        .query("SELECT count() AS n FROM projects GROUP ALL")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    let n = count
        .first()
        .and_then(|r| r.as_object())
        .and_then(|o| o.get("n"))
        .and_then(|n| n.as_i64())
        .unwrap();
    assert_eq!(n, 0, "the filesystem-root project must be removed");

    let fd_count: Vec<serde_json::Value> = db
        .query("SELECT count() AS n FROM file_data GROUP ALL")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    let fd_n = fd_count
        .first()
        .and_then(|r| r.as_object())
        .and_then(|o| o.get("n"))
        .and_then(|n| n.as_i64())
        .unwrap();
    assert_eq!(fd_n, 0, "the removed project's file_data must be gone");
}

#[tokio::test]
async fn register_reconciles_duplicates_and_their_file_data() {
    let (ctx, _tmp) = ctx_with_user().await;
    let db = ctx.database().unwrap();
    let root = "/tmp/reconcile-example";

    // Drop the unique index so a duplicate can be seeded by hand, then
    // register the first project normally.
    db.query("REMOVE INDEX unique_root_path ON TABLE projects")
        .await
        .unwrap();
    let r1 = RegisterParams {
        config_project_id: None,
        name: "A".into(),
        slug: "a".into(),
        root_path: root.into(),
    };
    ProjectService::register(&ctx, r1).await.unwrap();

    // Seed an older duplicate project at the same root.
    db.query(
        "CREATE projects SET owner_id = (SELECT VALUE id FROM users LIMIT 1)[0], \
         name = 'Old', slug = 'old', root_path = $root, \
         created_at = d'2024-01-01T00:00:00Z', updated_at = d'2024-01-01T00:00:00Z'",
    )
    .bind(("root", root))
    .await
    .unwrap();

    // Attach file_data to both projects so a leaked row is observable.
    let pids: Vec<surrealdb::types::Value> = db
        .query("SELECT VALUE id FROM projects")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    assert_eq!(pids.len(), 2, "two duplicate projects must be seeded");
    for pid in pids {
        db.query("CREATE file_data SET project_id = $pid, path = 'x.rs', indexed_at = time::now(), updated_at = time::now()")
            .bind(("pid", pid))
            .await
            .unwrap();
    }

    // Re-registering triggers reconcile_root_records: it must delete the older
    // duplicate and that duplicate's file_data.
    let r2 = RegisterParams {
        config_project_id: None,
        name: "A".into(),
        slug: "a".into(),
        root_path: root.into(),
    };
    let res = ProjectService::register(&ctx, r2).await.unwrap();
    assert_eq!(
        res.duplicates_removed, 1,
        "one duplicate must be reconciled"
    );

    let fd_rows: Vec<serde_json::Value> = db
        .query("SELECT count() AS n FROM file_data GROUP ALL")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    let fd_count = fd_rows
        .first()
        .and_then(|r| r.as_object())
        .and_then(|o| o.get("n"))
        .and_then(|n| n.as_i64())
        .unwrap();
    assert_eq!(
        fd_count, 1,
        "the removed duplicate's file_data must be gone (per-id delete)"
    );
}

#[tokio::test]
async fn register_rejects_filesystem_root() {
    let (ctx, _tmp) = ctx_with_user().await;
    for bad in ["/", ".", "..", "foo/..", ""] {
        let p = RegisterParams {
            config_project_id: None,
            name: "A".into(),
            slug: "a".into(),
            root_path: bad.into(),
        };
        let err = ProjectService::register(&ctx, p).await.unwrap_err();
        assert!(
            matches!(err, ServiceError::Validation(_)),
            "expected Validation error for {bad:?}, got {err:?}"
        );
    }

    let rows: Vec<serde_json::Value> = ctx
        .database()
        .unwrap()
        .query("SELECT count() AS n FROM projects GROUP ALL")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    let count = rows
        .first()
        .and_then(|r| r.as_object())
        .and_then(|o| o.get("n"))
        .and_then(|n| n.as_i64())
        .unwrap();
    assert_eq!(
        count, 0,
        "no project may be registered at the filesystem root"
    );
}
