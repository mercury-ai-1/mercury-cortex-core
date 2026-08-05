use mercury_cortex_core::db::DB_FILENAME;
use mercury_cortex_core::db::initialize;
use mercury_cortex_core::runtime::RuntimeContext;
use mercury_cortex_core::runtime::status::RuntimePhase;
use mercury_cortex_core::service::profile::{ProfileService, UpsertParams};
use tempfile::TempDir;

async fn ctx_with_user() -> (RuntimeContext, TempDir) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join(DB_FILENAME);
    let db = initialize(&db_path).await.unwrap();
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
async fn detects_existing_email() {
    let (ctx, _tmp) = ctx_with_user().await;
    assert!(
        ProfileService::email_exists(&ctx, "test@example.com", None)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn no_false_positive_for_new_email() {
    let (ctx, _tmp) = ctx_with_user().await;
    assert!(
        !ProfileService::email_exists(&ctx, "other@example.com", None)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn self_match_is_not_duplicate() {
    let (ctx, _tmp) = ctx_with_user().await;
    assert!(
        !ProfileService::email_exists(&ctx, "test@example.com", Some("test@example.com"))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn upsert_returns_created_user_id() {
    let (ctx, _tmp) = ctx_with_user().await;
    let id1 = ProfileService::upsert(
        &ctx,
        UpsertParams {
            id: None,
            name: "Alice".into(),
            email: "alice@example.com".into(),
            agent_name: "agent-alice".into(),
            profile_type: "personal".into(),
        },
    )
    .await
    .unwrap();

    let id2 = ProfileService::upsert(
        &ctx,
        UpsertParams {
            id: None,
            name: "Bob".into(),
            email: "bob@example.com".into(),
            agent_name: "agent-bob".into(),
            profile_type: "personal".into(),
        },
    )
    .await
    .unwrap();

    assert!(id2.starts_with("users:"), "returned id must be a user id");
    assert_ne!(id1, id2, "two created users must not share an id");
}

#[tokio::test]
async fn upsert_with_explicit_id_returns_that_id() {
    let (ctx, _tmp) = ctx_with_user().await;

    let id1 = ProfileService::upsert(
        &ctx,
        UpsertParams {
            id: None,
            name: "Alice".into(),
            email: "alice@example.com".into(),
            agent_name: "agent-alice".into(),
            profile_type: "personal".into(),
        },
    )
    .await
    .unwrap();

    let id2 = ProfileService::upsert(
        &ctx,
        UpsertParams {
            id: None,
            name: "Bob".into(),
            email: "bob@example.com".into(),
            agent_name: "agent-bob".into(),
            profile_type: "personal".into(),
        },
    )
    .await
    .unwrap();

    let updated = ProfileService::upsert(
        &ctx,
        UpsertParams {
            id: Some(id2.clone()),
            name: "Bob Updated".into(),
            email: "bob.updated@example.com".into(),
            agent_name: "agent-bob".into(),
            profile_type: "personal".into(),
        },
    )
    .await
    .unwrap();

    assert_eq!(
        updated, id2,
        "update must return the written record's id, not another user's"
    );
    assert_ne!(updated, id1, "update must not return another user's id");
}
