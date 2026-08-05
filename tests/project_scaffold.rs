use std::fs;

use mercury_cortex_core::service::scaffold;
use tempfile::TempDir;

#[test]
fn agents_md_created_then_noop_on_rerun() {
    let tmp = TempDir::new().unwrap();
    scaffold::create_or_update_agents_md(tmp.path()).unwrap();

    let path = tmp.path().join("AGENTS.md");
    assert!(path.exists());
    let first = fs::read_to_string(&path).unwrap();

    scaffold::create_or_update_agents_md(tmp.path()).unwrap();
    assert_eq!(first, fs::read_to_string(&path).unwrap(), "idempotent");
}

#[test]
fn instructions_md_explains_both_workflows() {
    let tmp = TempDir::new().unwrap();
    let mc_dir = tmp.path().join(".mercury-cortex");
    fs::create_dir_all(&mc_dir).unwrap();

    scaffold::create_or_update_instructions_md(&mc_dir).unwrap();

    let content = fs::read_to_string(mc_dir.join("instructions.md")).unwrap();
    assert!(
        content.contains("mercury-cortex:dev"),
        "documents the dev workflow"
    );
    assert!(
        content.contains("mercury-cortex:init"),
        "documents the init workflow"
    );
    assert!(
        content.contains("workflow/session"),
        "explains how to drive a workflow"
    );
    assert!(
        content.contains("workflow/step"),
        "explains how to drive a workflow"
    );
}

#[test]
fn agents_md_appends_when_mercury_section_missing() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("AGENTS.md"), "# My project\n").unwrap();

    scaffold::create_or_update_agents_md(tmp.path()).unwrap();

    let content = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
    assert!(
        content.starts_with("# My project\n"),
        "preserves existing content"
    );
    assert!(
        content.contains("## Mercury Cortex"),
        "appends the pointer section"
    );
}

#[test]
fn mcignore_merges_missing_defaults() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join(".mcignore");
    fs::write(&path, "node_modules\n.env\n").unwrap();

    scaffold::create_or_update_mcignore(&path).unwrap();

    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("node_modules"), "keeps user rules");
    assert!(content.contains("target"), "appends missing default");
    assert!(content.contains(".git"), "appends missing default");
}

#[test]
fn config_write_read_round_trip() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("config.json");

    assert_eq!(scaffold::read_config_project_id(&path).unwrap(), None);
    scaffold::write_config(&path, "projects:abc").unwrap();
    assert_eq!(
        scaffold::read_config_project_id(&path).unwrap(),
        Some("projects:abc".into())
    );
}

#[test]
fn slugify_normalizes_names() {
    assert_eq!(scaffold::slugify("My Project"), "my-project");
    assert_eq!(scaffold::slugify("Hello.World!"), "hello-world");
    assert_eq!(scaffold::slugify("already-slug"), "already-slug");
    assert_eq!(scaffold::slugify(""), "");
}
