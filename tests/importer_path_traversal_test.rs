use std::path::Path;

use mercury_cortex_core::db::DB_FILENAME;
use mercury_cortex_core::db::initialize;
use mercury_cortex_core::engine::KnowledgeEngine;
use mercury_cortex_core::runtime::RuntimeContext;
use mercury_cortex_core::runtime::status::RuntimePhase;
use mercury_cortex_core::schema::run_pending;
use tempfile::TempDir;

async fn ctx_with_db() -> (RuntimeContext, TempDir) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join(DB_FILENAME);
    let db = initialize(&db_path).await.unwrap();
    run_pending(&db).await.unwrap();
    let ctx = RuntimeContext::new_for_test();
    ctx.set_database(db, RuntimePhase::DatabaseConnected);
    (ctx, tmp)
}

/// Build a project root with a `.mercury-cortex/temp/` directory containing:
/// - `x.json` whose `path` escapes the project root via `..`;
/// - `y.json` whose `path` is an absolute path (another escape form);
/// - `z.json` pointing at a real file **inside** the project root (positive
///   control, proving valid imports still work through the same fixture).
fn build_traversal_fixture(root: &Path) {
    let mctx = root.join(".mercury-cortex/temp");
    std::fs::create_dir_all(&mctx).unwrap();
    // The target "file" lives one level above the project root.
    std::fs::write(root.parent().unwrap().join("outside.txt"), "secret").unwrap();
    std::fs::write(
        mctx.join("x.json"),
        r#"{"path":"../outside.txt","type":"text","purpose":"x","summary":"x","features":[],"tags":[]}"#,
    )
    .unwrap();
    // Absolute-path escape variant.
    std::fs::write(
        mctx.join("y.json"),
        format!(
            r#"{{"path":"{}","type":"text"}}"#,
            root.parent().unwrap().join("outside.txt").display()
        ),
    )
    .unwrap();
    // Positive control: a real file under the project root.
    std::fs::write(root.join("hello.txt"), "hello").unwrap();
    std::fs::write(
        mctx.join("z.json"),
        r#"{"path":"hello.txt","type":"text","purpose":"z","summary":"z","features":[],"tags":[]}"#,
    )
    .unwrap();
}

/// Run the importer over the fixture via the public `KnowledgeEngine` surface
/// (`set_project` + `submit_metadata`, which reaches `Importer::import_pending`).
async fn run_import(
    db: surrealdb::Surreal<surrealdb::engine::local::Db>,
    root: &Path,
    project_id: &str,
) -> Vec<mercury_cortex_core::engine::ImportResult> {
    let engine = KnowledgeEngine::new(db);
    engine
        .set_project(project_id.to_string(), root.to_path_buf())
        .await;
    engine.submit_metadata().await.unwrap()
}

async fn stored_paths(db: &surrealdb::Surreal<surrealdb::engine::local::Db>) -> Vec<String> {
    db.query("SELECT path FROM file_data")
        .await
        .unwrap()
        .take(0)
        .map(|rows: Vec<serde_json::Value>| {
            rows.iter()
                .filter_map(|r| r.get("path").and_then(|p| p.as_str()).map(String::from))
                .collect()
        })
        .unwrap()
}

#[tokio::test]
async fn importer_rejects_paths_escaping_the_project_root() {
    let (ctx, tmp) = ctx_with_db().await;
    let db = ctx.database().unwrap();

    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    build_traversal_fixture(&project_root);

    let results = run_import(db.clone(), &project_root, "projects:test").await;

    assert_eq!(results.len(), 3, "all three JSON files must be processed");
    for result in &results {
        if result.path == "hello.txt" {
            assert!(result.success, "in-root import must succeed: {result:?}");
            continue;
        }
        assert!(!result.success, "escaping import must fail: {result:?}");
        let error = result.error.as_deref().unwrap_or_default();
        assert!(
            error.contains("path escapes"),
            "error should mention path escaping, got: {error}"
        );
    }

    // Only the in-root file may be indexed; no `file_data` row may be created
    // for the escaping paths.
    let stored = stored_paths(&db).await;
    assert_eq!(
        stored,
        vec!["hello.txt"],
        "only the in-root file may be indexed: {stored:?}"
    );
}

#[tokio::test]
async fn importer_skips_mcignored_staged_paths() {
    let (ctx, tmp) = ctx_with_db().await;
    let db = ctx.database().unwrap();

    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::create_dir_all(project_root.join(".mercury-cortex/temp")).unwrap();
    std::fs::write(
        project_root.join(".mercury-cortex").join(".mcignore"),
        "ignored.txt\n",
    )
    .unwrap();
    std::fs::write(project_root.join("ignored.txt"), "secret").unwrap();
    std::fs::write(project_root.join("hello.txt"), "hello").unwrap();
    std::fs::write(
        project_root.join(".mercury-cortex/temp/ignored.json"),
        r#"{"path":"ignored.txt","type":"text","purpose":"i","summary":"i","features":[],"tags":[]}"#,
    )
    .unwrap();
    std::fs::write(
        project_root.join(".mercury-cortex/temp/hello.json"),
        r#"{"path":"hello.txt","type":"text","purpose":"h","summary":"h","features":[],"tags":[]}"#,
    )
    .unwrap();

    let results = run_import(db.clone(), &project_root, "projects:test").await;

    assert_eq!(results.len(), 2);
    let ignored = results.iter().find(|r| r.path == "ignored.txt").unwrap();
    assert!(
        ignored.success,
        "ignored staged path must be a benign skip, not an error: {ignored:?}"
    );
    let hello = results.iter().find(|r| r.path == "hello.txt").unwrap();
    assert!(hello.success);

    let stored = stored_paths(&db).await;
    assert_eq!(stored, vec!["hello.txt"]);

    assert!(
        !project_root
            .join(".mercury-cortex/temp/ignored.json")
            .exists(),
        "engine must remove the staged JSON for an ignored path"
    );
}

#[tokio::test]
async fn importer_upserts_row_for_missing_source_file() {
    let (ctx, tmp) = ctx_with_db().await;
    let db = ctx.database().unwrap();

    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let temp_dir = project_root.join(".mercury-cortex").join("temp");
    std::fs::create_dir_all(&temp_dir).unwrap();

    // Metadata references a file that does NOT exist on disk. The record must
    // still be created/kept — the importer is the source of truth.
    std::fs::write(
        temp_dir.join("ghost.json"),
        r#"{"path":"src/ghost.rs","type":"rs","purpose":"kept","summary":"missing source"}"#,
    )
    .unwrap();

    run_import(db.clone(), &project_root, "projects:test").await;

    let stored = stored_paths(&db).await;
    assert!(
        stored.contains(&"src/ghost.rs".to_string()),
        "missing-source file must still be imported: {stored:?}"
    );
}

#[tokio::test]
async fn importer_keeps_and_updates_row_when_source_deleted() {
    let (ctx, tmp) = ctx_with_db().await;
    let db = ctx.database().unwrap();

    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let temp_dir = project_root.join(".mercury-cortex").join("temp");
    std::fs::create_dir_all(&temp_dir).unwrap();

    // First: source file exists, import it (creates the row).
    std::fs::create_dir_all(project_root.join("src")).unwrap();
    std::fs::write(project_root.join("src").join("entry.rs"), "fn main() {}\n").unwrap();
    std::fs::write(
        temp_dir.join("entry.json"),
        r#"{"path":"src/entry.rs","type":"rs","purpose":"kept","summary":"v1"}"#,
    )
    .unwrap();
    run_import(db.clone(), &project_root, "projects:test").await;
    assert!(
        stored_paths(&db)
            .await
            .contains(&"src/entry.rs".to_string()),
        "existing source must be imported"
    );

    // Second: source file deleted, re-import same metadata. The row must
    // survive (importer is the source of truth — never delete on missing
    // source). `import_pending` removes the temp dir on full success, so
    // recreate it before staging the next import.
    std::fs::remove_file(project_root.join("src").join("entry.rs")).unwrap();
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::fs::write(
        temp_dir.join("entry.json"),
        r#"{"path":"src/entry.rs","type":"rs","purpose":"kept","summary":"v2"}"#,
    )
    .unwrap();
    run_import(db.clone(), &project_root, "projects:test").await;

    let stored = stored_paths(&db).await;
    assert!(
        stored.contains(&"src/entry.rs".to_string()),
        "row must survive source deletion and re-import: {stored:?}"
    );
    // Metadata was updated (v2), not just preserved.
    let rows: Vec<serde_json::Value> = db
        .query("SELECT summary FROM file_data WHERE project_id = projects:test AND path = 'src/entry.rs'")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    assert_eq!(
        rows[0].get("summary").and_then(|s| s.as_str()),
        Some("v2"),
        "re-import must update the record's metadata"
    );
}

#[tokio::test]
async fn importer_runtime_index_tracks_only_files_with_sources() {
    let (ctx, tmp) = ctx_with_db().await;
    let db = ctx.database().unwrap();

    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let temp_dir = project_root.join(".mercury-cortex").join("temp");
    std::fs::create_dir_all(&temp_dir).unwrap();

    // One file exists on disk; the other is metadata-only (no source file).
    std::fs::create_dir_all(project_root.join("src")).unwrap();
    std::fs::write(project_root.join("src").join("entry.rs"), "fn main() {}\n").unwrap();
    std::fs::write(
        temp_dir.join("entry.json"),
        r#"{"path":"src/entry.rs","type":"rs","purpose":"kept","summary":"present"}"#,
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("ghost.json"),
        r#"{"path":"src/ghost.rs","type":"rs","purpose":"kept","summary":"missing source"}"#,
    )
    .unwrap();

    // Hold the engine so the in-memory runtime index survives the import
    // (unlike `run_import`, which drops it).
    let engine = KnowledgeEngine::new(db.clone());
    engine
        .set_project("projects:test".to_string(), project_root.to_path_buf())
        .await;
    engine.submit_metadata().await.unwrap();

    // A file with a source on disk is registered in the runtime index...
    assert!(
        engine.get_file_metadata("src/entry.rs").await.is_some(),
        "file with source on disk must be in the runtime index"
    );

    // ...but a missing-source file is not (it cannot be hashed). Accepted gap:
    // the runtime index only tracks files that exist on disk. The DB row still
    // exists — the importer is the source of truth.
    assert!(
        engine.get_file_metadata("src/ghost.rs").await.is_none(),
        "missing-source file must not be in the runtime index"
    );

    let stored = stored_paths(&db).await;
    assert!(
        stored.contains(&"src/ghost.rs".to_string()),
        "missing-source file must still persist to file_data: {stored:?}"
    );
    assert!(
        stored.contains(&"src/entry.rs".to_string()),
        "present-source file must persist to file_data: {stored:?}"
    );
}
