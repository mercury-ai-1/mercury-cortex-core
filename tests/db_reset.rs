//! Integration tests for the `db::reset` domain logic.
//!
//! These live in `tests/` per repo policy. They run against throwaway
//! SurrealKV databases in `TempDir`s, never the real user database.

use tempfile::TempDir;

use mercury_cortex_core::db::initialize;
use mercury_cortex_core::db::reset::{ResetMode, list_resettable_tables, reset, table_counts};
use mercury_cortex_core::schema;

#[tokio::test]
async fn empty_selection_clears_nothing_and_leaves_data_intact() {
    let tmp = TempDir::new().unwrap();
    let db = initialize(&tmp.path().join("test.db")).await.unwrap();

    db.query("CREATE file_data SET path = 'a.rs', content = 'x'")
        .await
        .unwrap();
    db.query("CREATE projects SET name = 'p'").await.unwrap();

    let summary = reset(&db, ResetMode::Selected(vec![])).await.unwrap();
    assert_eq!(summary.cleared, vec![]);

    let file_data = count(&db, "file_data").await;
    let projects = count(&db, "projects").await;
    assert_eq!(file_data, 1);
    assert_eq!(projects, 1);
}

#[tokio::test]
async fn selected_reset_clears_only_the_named_tables() {
    let tmp = TempDir::new().unwrap();
    let db = initialize(&tmp.path().join("test.db")).await.unwrap();

    db.query("CREATE file_data SET path = 'a.rs', content = 'x'")
        .await
        .unwrap();
    db.query("CREATE file_data SET path = 'b.rs', content = 'y'")
        .await
        .unwrap();
    db.query("CREATE projects SET name = 'p'").await.unwrap();

    let summary = reset(&db, ResetMode::Selected(vec!["file_data".into()]))
        .await
        .unwrap();
    assert_eq!(summary.cleared, vec![("file_data".to_string(), 2)]);

    assert_eq!(count(&db, "file_data").await, 0);
    assert_eq!(count(&db, "projects").await, 1);
}

#[tokio::test]
async fn all_reset_clears_every_expected_table() {
    let tmp = TempDir::new().unwrap();
    let db = initialize(&tmp.path().join("test.db")).await.unwrap();

    for table in [
        "users",
        "projects",
        "file_data",
        "owns",
        "contains",
        "imports",
        "calls",
        "depends_on",
        "part_of_pattern",
    ] {
        db.query(format!("DEFINE TABLE {table}")).await.unwrap();
    }
    db.query("CREATE users SET email = 'u@e.com'")
        .await
        .unwrap();
    db.query("CREATE file_data SET path = 'a.rs', content = 'x'")
        .await
        .unwrap();

    let summary = reset(&db, ResetMode::All).await.unwrap();
    let cleared_tables: Vec<&str> = summary.cleared.iter().map(|(t, _)| t.as_str()).collect();
    assert!(cleared_tables.contains(&"file_data"));
    assert!(cleared_tables.contains(&"users"));
    assert_eq!(count(&db, "users").await, 0);
    assert_eq!(count(&db, "file_data").await, 0);
}

#[tokio::test]
async fn counts_report_real_and_zero_rows() {
    let tmp = TempDir::new().unwrap();
    let db = initialize(&tmp.path().join("test.db")).await.unwrap();

    db.query("DEFINE TABLE projects").await.unwrap();
    db.query("CREATE file_data SET path = 'a.rs', content = 'x'")
        .await
        .unwrap();

    let counts = table_counts(&db, &["file_data".into(), "projects".into()])
        .await
        .unwrap();
    assert_eq!(
        counts,
        vec![("file_data".to_string(), 1), ("projects".to_string(), 0)]
    );
}

#[tokio::test]
async fn list_resettable_tables_only_reports_present_expected_tables() {
    let tmp = TempDir::new().unwrap();
    let db = initialize(&tmp.path().join("test.db")).await.unwrap();

    db.query("CREATE file_data SET path = 'a.rs'")
        .await
        .unwrap();
    db.query("CREATE some_adhoc_table SET x = 1").await.unwrap();

    let tables = list_resettable_tables(&db).await.unwrap();
    assert!(tables.contains(&"file_data".to_string()));
    assert!(!tables.contains(&"some_adhoc_table".to_string()));
    assert!(!tables.contains(&"_migrations".to_string()));
}

#[tokio::test]
async fn reset_all_clears_migrated_tables() {
    let tmp = TempDir::new().unwrap();
    let db = initialize(&tmp.path().join("test.db")).await.unwrap();
    schema::run_pending(&db).await.unwrap();

    db.query("CREATE users:u1 SET name = 'U', email = 'u@example.com', type = '', agent_name = 'agent-u1', created_at = time::now(), updated_at = time::now()")
        .await
        .unwrap();
    db.query("CREATE projects:p1 SET owner_id = users:u1, name = 'P1', slug = 'p1', root_path = '/p1', created_at = time::now(), updated_at = time::now()")
        .await
        .unwrap();
    db.query("CREATE file_data:f1 SET project_id = projects:p1, path = '/a.rs', indexed_at = time::now(), updated_at = time::now()")
        .await
        .unwrap();

    let tables = list_resettable_tables(&db).await.unwrap();
    assert!(tables.contains(&"users".to_string()));
    assert!(tables.contains(&"projects".to_string()));
    assert!(tables.contains(&"file_data".to_string()));

    assert_eq!(count(&db, "users").await, 1);
    assert_eq!(count(&db, "projects").await, 1);
    assert_eq!(count(&db, "file_data").await, 1);

    let summary = reset(&db, ResetMode::All).await.unwrap();
    let cleared: Vec<(&str, u64)> = summary
        .cleared
        .iter()
        .map(|(t, n)| (t.as_str(), *n))
        .collect();
    assert!(
        cleared.iter().any(|(t, n)| *t == "users" && *n == 1),
        "users not cleared: {cleared:?}"
    );
    assert!(
        cleared.iter().any(|(t, n)| *t == "projects" && *n == 1),
        "projects not cleared: {cleared:?}"
    );
    assert!(
        cleared.iter().any(|(t, n)| *t == "file_data" && *n == 1),
        "file_data not cleared: {cleared:?}"
    );

    assert_eq!(count(&db, "users").await, 0);
    assert_eq!(count(&db, "projects").await, 0);
    assert_eq!(count(&db, "file_data").await, 0);
}

async fn count(db: &surrealdb::Surreal<surrealdb::engine::local::Db>, table: &str) -> u64 {
    let rows: Vec<surrealdb::types::Value> = db
        .query(format!("SELECT count() AS n FROM {table} GROUP ALL"))
        .await
        .unwrap()
        .take(0)
        .unwrap();
    match rows.first() {
        Some(surrealdb::types::Value::Object(obj)) => obj
            .get("n")
            .and_then(|n| n.clone().into_t::<i64>().ok())
            .unwrap_or(0) as u64,
        _ => 0,
    }
}
