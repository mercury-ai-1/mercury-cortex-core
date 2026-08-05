use mercury_cortex_core::db::DB_FILENAME;
use mercury_cortex_core::db::initialize;
use tempfile::TempDir;

#[tokio::test]
async fn initialize_with_special_char_key_connects() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    // Key containing the config-string delimiters the engine splits on.
    let key = "a&b=c?d#e f%g";
    // MERCURY_DB_ENCRYPTION_KEY is process-global.
    // SAFETY: the key is only read by `resolve_encryption_key()` on this same
    // task, after `set_var` completes; no other code in this process accesses
    // this variable concurrently.
    unsafe {
        std::env::set_var("MERCURY_DB_ENCRYPTION_KEY", key);
    }
    let db_path = tmp.path().join(DB_FILENAME);
    let db = initialize(&db_path).await?;
    db.query("CREATE users SET name = 'alice'").await?;
    let rows: Vec<serde_json::Value> = db.query("SELECT name FROM users").await?.take(0)?;
    assert_eq!(rows.len(), 1, "connection must round-trip a write");
    Ok(())
}
