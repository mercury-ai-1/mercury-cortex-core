//! Integration tests for the `db::export` domain logic.
//!
//! These run against throwaway SurrealKV databases in `TempDir`s, never the
//! real user database (mirrors `tests/db_reset.rs`).

use std::path::Path;

use serde_json::json;
use tempfile::TempDir;

use mercury_cortex_core::db::export::{ExportFilter, export_tables, list_tables, table_has_field};
use mercury_cortex_core::db::initialize;
use mercury_cortex_core::schema;

/// Init + migrate a throwaway DB and seed projects/file_data/users rows.
///
/// Records satisfy the SCHEMAFULL migrations' required, non-nullable fields
/// (`projects.owner_id`, `users.agent_name`, `file_data.project_id`) so the
/// rows actually persist and `SELECT *` sees them (see `tests/zz_probe.rs`).
async fn seed(tmp: &Path) -> surrealdb::Surreal<surrealdb::engine::local::Db> {
    let db = initialize(&tmp.join("export.db")).await.unwrap();
    schema::run_pending(&db).await.unwrap();
    db.query("CREATE users:u1 SET name = 'U', email = 'u@example.com', type = '', agent_name = 'agent-u1', created_at = time::now(), updated_at = time::now()")
        .await
        .unwrap();
    db.query("CREATE projects:p1 SET owner_id = users:u1, name = 'P1', slug = 'p1', root_path = '/p1', created_at = time::now(), updated_at = time::now()")
        .await
        .unwrap();
    db.query("CREATE projects:p2 SET owner_id = users:u1, name = 'P2', slug = 'p2', root_path = '/p2', created_at = time::now(), updated_at = time::now()")
        .await
        .unwrap();
    db.query("CREATE file_data:f1 SET project_id = projects:p1, path = '/a.rs', indexed_at = time::now(), updated_at = time::now()")
        .await
        .unwrap();
    db.query("CREATE file_data:f3 SET project_id = projects:p1, path = '/c.rs', indexed_at = time::now(), updated_at = time::now()")
        .await
        .unwrap();
    db.query("CREATE file_data:f2 SET project_id = projects:p2, path = '/b.rs', indexed_at = time::now(), updated_at = time::now()")
        .await
        .unwrap();
    db
}

#[tokio::test]
async fn list_tables_excludes_internal_and_sorts() {
    let tmp = TempDir::new().unwrap();
    let db = seed(tmp.path()).await;
    db.query("CREATE some_adhoc_table SET x = 1").await.unwrap();

    let tables = list_tables(&db).await.unwrap();
    assert!(tables.contains(&"file_data".to_string()));
    assert!(tables.contains(&"some_adhoc_table".to_string()));
    assert!(!tables.iter().any(|t| t.starts_with('_')));
    let mut sorted = tables.clone();
    sorted.sort();
    assert_eq!(tables, sorted);
}

#[tokio::test]
async fn table_has_field_detects_project_id() {
    let tmp = TempDir::new().unwrap();
    let db = seed(tmp.path()).await;
    assert!(
        table_has_field(&db, "file_data", "project_id")
            .await
            .unwrap()
    );
    assert!(
        !table_has_field(&db, "projects", "project_id")
            .await
            .unwrap()
    );
    assert!(!table_has_field(&db, "users", "project_id").await.unwrap());
}

#[tokio::test]
async fn export_selected_table_writes_sorted_rows() {
    let tmp = TempDir::new().unwrap();
    let db = seed(tmp.path()).await;
    let out = tmp.path().join("out");

    let summary = export_tables(&db, &["file_data".into()], &[], &out)
        .await
        .unwrap();
    assert_eq!(summary.files.len(), 1);
    assert_eq!(summary.files[0].table, "file_data");
    assert_eq!(summary.files[0].filename, "file_data.json");
    assert_eq!(summary.files[0].rows, 3);

    let content = std::fs::read_to_string(out.join("file_data.json")).unwrap();
    assert!(content.ends_with('\n'));
    let rows: serde_json::Value = serde_json::from_str(&content).unwrap();
    let ids: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    // f1, f3, f2 were created out of order; export must sort to f1, f2, f3.
    assert_eq!(ids, vec!["file_data:f1", "file_data:f2", "file_data:f3"]);
}

#[tokio::test]
async fn export_all_tables_writes_every_present_table() {
    let tmp = TempDir::new().unwrap();
    let db = seed(tmp.path()).await;
    let out = tmp.path().join("out");

    let tables = list_tables(&db).await.unwrap();
    let summary = export_tables(&db, &tables, &[], &out).await.unwrap();
    assert_eq!(summary.files.len(), tables.len());
    for t in &tables {
        assert!(out.join(format!("{t}.json")).exists(), "missing {t}.json");
    }
}

#[tokio::test]
async fn empty_table_writes_empty_array() {
    let tmp = TempDir::new().unwrap();
    let db = seed(tmp.path()).await;
    db.query("DEFINE TABLE IF NOT EXISTS empty_tbl")
        .await
        .unwrap();
    let out = tmp.path().join("out");

    let summary = export_tables(&db, &["empty_tbl".into()], &[], &out)
        .await
        .unwrap();
    assert_eq!(summary.files[0].rows, 0);
    let content = std::fs::read_to_string(out.join("empty_tbl.json")).unwrap();
    assert_eq!(content.trim_end(), "[]");
}

#[tokio::test]
async fn project_id_filter_exports_only_matching_rows() {
    let tmp = TempDir::new().unwrap();
    let db = seed(tmp.path()).await;
    let out = tmp.path().join("out");

    let filter = ExportFilter::record("project_id", "projects:p1").unwrap();
    let summary = export_tables(&db, &["file_data".into()], &[filter], &out)
        .await
        .unwrap();
    assert_eq!(summary.files[0].rows, 2);
    assert!(summary.skipped_filters.is_empty());

    let content = std::fs::read_to_string(out.join("file_data.json")).unwrap();
    let rows: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    for row in rows.as_array().unwrap() {
        assert_eq!(row["project_id"], json!("projects:p1"));
    }
}

#[tokio::test]
async fn filter_on_table_without_field_exports_unfiltered_and_skips() {
    let tmp = TempDir::new().unwrap();
    let db = seed(tmp.path()).await;
    let out = tmp.path().join("out");

    let filter = ExportFilter::record("project_id", "projects:p1").unwrap();
    let summary = export_tables(&db, &["users".into()], &[filter], &out)
        .await
        .unwrap();
    assert_eq!(summary.files[0].rows, 1);
    assert!(
        summary
            .skipped_filters
            .iter()
            .any(|s| s.contains("users") && s.contains("project_id"))
    );

    let content = std::fs::read_to_string(out.join("users.json")).unwrap();
    let rows: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn invalid_filter_record_fails_before_any_file() {
    let tmp = TempDir::new().unwrap();
    let db = seed(tmp.path()).await;
    let out = tmp.path().join("out");

    // Construct directly to bypass the constructor and reach core validation.
    let filter = ExportFilter {
        field: "project_id".into(),
        record: "not a record id".into(),
    };
    let result = export_tables(&db, &["file_data".into()], &[filter], &out).await;
    assert!(result.is_err());
    assert!(!out.join("file_data.json").exists());
}

#[tokio::test]
async fn unknown_table_fails_before_writing() {
    let tmp = TempDir::new().unwrap();
    let db = seed(tmp.path()).await;
    let out = tmp.path().join("out");

    let result = export_tables(&db, &["nonexistent".into()], &[], &out).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("nonexistent"));
    assert!(!out.join("nonexistent.json").exists());
}

#[tokio::test]
#[cfg(unix)]
async fn unwritable_out_dir_fails_before_writing() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new().unwrap();
    let db = seed(tmp.path()).await;
    let out = tmp.path().join("ro");
    std::fs::create_dir(&out).unwrap();
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o500)).unwrap();

    let result = export_tables(&db, &["file_data".into()], &[], &out).await;
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
}

#[tokio::test]
async fn export_is_deterministic_across_runs() {
    let tmp = TempDir::new().unwrap();
    let db = seed(tmp.path()).await;
    let out1 = tmp.path().join("out1");
    let out2 = tmp.path().join("out2");

    let tables = list_tables(&db).await.unwrap();
    export_tables(&db, &tables, &[], &out1).await.unwrap();
    export_tables(&db, &tables, &[], &out2).await.unwrap();
    for t in &tables {
        let a = std::fs::read(out1.join(format!("{t}.json"))).unwrap();
        let b = std::fs::read(out2.join(format!("{t}.json"))).unwrap();
        assert_eq!(a, b, "{t}.json differs between runs");
    }
}
