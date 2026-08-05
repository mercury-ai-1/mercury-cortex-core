use std::fs::File;
use std::path::Path;

use std::time::Duration;

use fs2::FileExt;
use mercury_cortex_core::db::backup::{BACKUP_DIR, backup, list_backups, restore};
use mercury_cortex_core::db::{DB_FILENAME, initialize, lock_is_held};
use tempfile::TempDir;

/// Create a fake database directory with a known file.
fn make_db(dir: &Path) {
    let db_path = dir.join(DB_FILENAME);
    std::fs::create_dir_all(&db_path).unwrap();
    std::fs::write(db_path.join("data"), b"hello").unwrap();
}

#[test]
fn backup_creates_timestamped_copy_with_size() {
    let tmp = TempDir::new().unwrap();
    make_db(tmp.path());

    let result = backup(tmp.path()).unwrap();

    assert!(result.path.starts_with(tmp.path().join("backups")));
    assert!(
        result
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(&format!("{DB_FILENAME}.")),
        "backup name should be timestamped: {}",
        result.path.display()
    );
    assert!(result.size >= 5, "size should cover the 'hello' payload");
    assert!(result.path.join("data").exists());
}

#[test]
fn list_backups_returns_sorted_entries() {
    let tmp = TempDir::new().unwrap();
    make_db(tmp.path());

    backup(tmp.path()).unwrap();
    backup(tmp.path()).unwrap();

    let list = list_backups(tmp.path()).unwrap();
    assert!(!list.missing);
    assert_eq!(list.entries.len(), 2);
    let names: Vec<&String> = list.entries.iter().map(|e| &e.name).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "entries should be sorted by name");
}

#[test]
fn list_backups_marks_missing_dir() {
    let tmp = TempDir::new().unwrap();
    let list = list_backups(tmp.path()).unwrap();
    assert!(list.missing);
    assert!(list.entries.is_empty());
}

#[test]
fn restore_round_trips_data() {
    let tmp = TempDir::new().unwrap();
    make_db(tmp.path());

    let backup_result = backup(tmp.path()).unwrap();
    std::fs::remove_dir_all(tmp.path().join(DB_FILENAME)).unwrap();

    let result = restore(tmp.path(), &backup_result.path).unwrap();
    assert_eq!(result.db_path, tmp.path().join(DB_FILENAME));
    assert_eq!(
        std::fs::read_to_string(result.db_path.join("data")).unwrap(),
        "hello"
    );
}

#[test]
fn backup_refused_while_lock_held() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join(DB_FILENAME);
    std::fs::create_dir_all(&db_path).unwrap();
    std::fs::write(db_path.join("data"), b"hello").unwrap();

    let lock = File::create(db_path.join("LOCK")).unwrap();
    lock.lock_exclusive().unwrap();

    assert!(
        backup(tmp.path()).is_err(),
        "backup must refuse while locked"
    );
    lock.unlock().unwrap();
}

/// Recursively copy the directory tree at `src` into `dst`, creating `dst`
/// and any missing parent directories along the way.
fn copy_tree(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_tree(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Wait until the SurrealKV lock is no longer held after a connection has
/// been dropped. SurrealDB shuts the datastore down asynchronously on drop,
/// so the flock may persist briefly.
async fn wait_for_unlock(db_path: &Path) {
    for _ in 0..50 {
        if !lock_is_held(db_path).unwrap() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("database lock was never released after connection drop");
}

#[tokio::test]
async fn restore_replaces_db_and_leaves_no_artifacts() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let data_dir = tmp.path();
    let db_path = data_dir.join(DB_FILENAME);

    // Seed a DB with a row.
    {
        let db = initialize(&db_path).await?;
        db.query("CREATE users SET name = 'alice'").await?;
        db.query("CREATE users SET name = 'bob'").await?;
    }
    wait_for_unlock(&db_path).await;

    // Backup, then mutate the live DB.
    let backup = backup(data_dir)?;
    {
        let db = initialize(&db_path).await?;
        db.query("DELETE users").await?;
    }
    wait_for_unlock(&db_path).await;

    // Restore.
    let result = restore(data_dir, &backup.path)?;
    assert_eq!(result.db_path, db_path);

    // Data is back.
    let db = initialize(&db_path).await?;
    let rows: Vec<serde_json::Value> = db
        .query("SELECT name FROM users ORDER BY name")
        .await?
        .take(0)?;
    assert_eq!(rows.len(), 2, "restore must bring the data back");

    // No temp/old artifacts remain.
    let leftovers: Vec<String> = std::fs::read_dir(data_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".restore-tmp") || n.ends_with(".old"))
        .collect();
    assert!(leftovers.is_empty(), "leftover artifacts: {leftovers:?}");

    Ok(())
}

#[tokio::test]
async fn restore_into_fresh_datadir_creates_db() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let data_dir = tmp.path();
    let db_path = data_dir.join(DB_FILENAME);

    // Create a backup from one temp dir.
    let seed = TempDir::new()?;
    {
        let db = initialize(&seed.path().join(DB_FILENAME)).await?;
        db.query("CREATE users SET name = 'alice'").await?;
    }
    // Move the seeded DB under data_dir/backups manually to produce a backup dir.
    std::fs::create_dir_all(data_dir.join(BACKUP_DIR))?;
    let backup_path = data_dir.join(BACKUP_DIR).join(format!("{DB_FILENAME}.1"));
    copy_tree(&seed.path().join(DB_FILENAME), &backup_path)?;

    // No live DB exists yet.
    assert!(!db_path.exists());
    let result = restore(data_dir, &backup_path)?;
    assert!(db_path.is_dir(), "restore must materialize the DB");
    assert_eq!(result.db_path, db_path);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn restore_failure_cleans_temp_and_keeps_live_db() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let data_dir = tmp.path();
    let db_path = data_dir.join(DB_FILENAME);

    // Seed a live DB.
    {
        let db = initialize(&db_path).await?;
        db.query("CREATE users SET name = 'alice'").await?;
    }
    wait_for_unlock(&db_path).await;

    // Build a backup whose copy cannot complete. A dangling symlink makes
    // `std::fs::copy` fail with ENOENT regardless of privileges — unlike an
    // unreadable (0o000) file, which root can still read — so the failure
    // trigger stays deterministic even when the test suite runs as root.
    let seed = TempDir::new()?;
    let seed_db = seed.path().join(DB_FILENAME);
    std::fs::create_dir_all(&seed_db)?;
    std::fs::write(seed_db.join("data"), b"hello")?;

    let backups_dir = data_dir.join(BACKUP_DIR);
    std::fs::create_dir_all(&backups_dir)?;
    let backup_path = backups_dir.join(format!("{DB_FILENAME}.1"));
    copy_tree(&seed_db, &backup_path)?;
    std::os::unix::fs::symlink("missing-target", backup_path.join("dangling"))?;

    // Restore must fail...
    assert!(
        restore(data_dir, &backup_path).is_err(),
        "restore must fail on a broken backup"
    );

    // ...the live DB is untouched (still reconnectable with original data)...
    let db = initialize(&db_path).await?;
    let rows: Vec<serde_json::Value> = db
        .query("SELECT name FROM users ORDER BY name")
        .await?
        .take(0)?;
    assert_eq!(
        rows.len(),
        1,
        "live DB must be untouched after failed restore"
    );
    assert_eq!(rows[0]["name"], "alice");

    // ...and the temp dir was cleaned up.
    let temp_path = db_path.with_extension("restore-tmp");
    assert!(
        !temp_path.exists(),
        "temp dir must be cleaned up after failed restore"
    );

    Ok(())
}
