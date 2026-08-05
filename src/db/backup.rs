//! Database backup, list, and restore.
//!
//! Pure filesystem operations — no live database connection is required.
//! All functions take the resolved data directory so they are testable.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::DB_FILENAME;
use super::connect::lock_is_held;

/// Subdirectory under the data dir that holds backups.
pub const BACKUP_DIR: &str = "backups";

/// Result of creating a backup.
#[derive(Debug, Clone)]
pub struct BackupResult {
    pub path: PathBuf,
    pub size: u64,
}

/// A single available backup entry.
#[derive(Debug, Clone)]
pub struct BackupEntry {
    pub name: String,
    pub size: u64,
}

/// Result of listing backups.
#[derive(Debug, Clone)]
pub struct BackupList {
    /// The backups directory that was scanned.
    pub dir: PathBuf,
    /// Whether the backups directory does not exist at all.
    pub missing: bool,
    pub entries: Vec<BackupEntry>,
}

/// Result of restoring a backup.
#[derive(Debug, Clone)]
pub struct RestoreResult {
    pub backup_path: PathBuf,
    pub db_path: PathBuf,
}

/// Create a timestamped copy of the `SurrealKV` database directory.
///
/// Refuses to run while the daemon holds the database lock, since a
/// directory copy of a live database is not guaranteed consistent.
pub fn backup(data_dir: &Path) -> Result<BackupResult, anyhow::Error> {
    let db_path = data_dir.join(DB_FILENAME);
    let backups_dir = data_dir.join(BACKUP_DIR);

    if lock_is_held(&db_path)? {
        anyhow::bail!(
            "Database is locked by a running process ({}). \
             Stop it before creating a backup.",
            db_path.join("LOCK").display()
        );
    }

    if !db_path.exists() || !db_path.is_dir() {
        anyhow::bail!("Database directory not found at {}", db_path.display());
    }

    std::fs::create_dir_all(&backups_dir)?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let backup_name = format!("{DB_FILENAME}.{timestamp}");
    let backup_path = backups_dir.join(&backup_name);

    copy_dir_recursive(&db_path, &backup_path).inspect_err(|_| {
        // A partial copy would match the list_backups filter and be advertised
        // as valid, so remove it on failure.
        let _ = std::fs::remove_dir_all(&backup_path);
    })?;

    // The lock check above happened before the copy; a daemon may have
    // started during it. Re-check so we never advertise an inconsistent
    // backup as valid.
    if lock_is_held(&db_path)? {
        let _ = std::fs::remove_dir_all(&backup_path);
        anyhow::bail!(
            "Database was locked during backup ({}). Remove the partial backup and retry.",
            db_path.join("LOCK").display()
        );
    }

    let size = dir_size(&backup_path);

    Ok(BackupResult {
        path: backup_path,
        size,
    })
}

/// List the available timestamped database backups, sorted by name.
pub fn list_backups(data_dir: &Path) -> Result<BackupList, anyhow::Error> {
    let backups_dir = data_dir.join(BACKUP_DIR);

    if !backups_dir.exists() {
        return Ok(BackupList {
            dir: backups_dir,
            missing: true,
            entries: Vec::new(),
        });
    }

    let mut entries: Vec<_> = std::fs::read_dir(&backups_dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(&format!("{DB_FILENAME}.")))
        })
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    let entries = entries
        .iter()
        .map(|e| BackupEntry {
            name: e.file_name().to_string_lossy().into_owned(),
            size: dir_size(&e.path()),
        })
        .collect();

    Ok(BackupList {
        dir: backups_dir,
        missing: false,
        entries,
    })
}

/// Restore the `SurrealKV` database from a backup directory.
///
/// Replaces the current database directory with a copy of the backup.
/// Refuses to run while the daemon holds the database lock, since
/// replacing a live database directory would corrupt the daemon's state.
/// The lock is probed both before and after the copy: if a daemon starts
/// while the copy is in flight the restore aborts with the temp copy left in
/// place, so stopping the daemon and retrying resumes cleanly.
///
/// The swap is atomic: the backup is copied to a temp sibling directory
/// first, then renamed over the live database. The live database is never
/// deleted before its replacement is fully materialized on disk, so a crash
/// mid-restore cannot lose the original data. The copied tree and the swap
/// renames are fsync'd so that guarantee survives a crash.
pub fn restore(data_dir: &Path, backup_path: &Path) -> Result<RestoreResult, anyhow::Error> {
    let db_path = data_dir.join(DB_FILENAME);
    let temp_path = db_path.with_extension("restore-tmp");
    let old_path = db_path.with_extension("old");

    if !backup_path.exists() || !backup_path.is_dir() {
        anyhow::bail!(
            "Backup path does not exist or is not a directory: {}",
            backup_path.display()
        );
    }

    if db_path.exists() && lock_is_held(&db_path)? {
        anyhow::bail!(
            "Database is locked by a running process ({}). \
             Stop it before restoring a backup.",
            db_path.join("LOCK").display()
        );
    }

    // Remove stale artifacts from a previously-crashed restore so the
    // renames below are reliable on re-run.
    for stale in [&temp_path, &old_path] {
        if stale.exists() {
            std::fs::remove_dir_all(stale)?;
        }
    }

    // Copy into a temp sibling dir (same filesystem → rename is atomic).
    // On failure, clean up the partial temp dir before propagating.
    if let Err(e) = copy_dir_recursive(backup_path, &temp_path) {
        let _ = std::fs::remove_dir_all(&temp_path);
        return Err(e);
    }

    // The lock check above happened before the copy; a daemon may have
    // started during it. Restoring over a live database would corrupt the
    // daemon's state, so re-check before swapping. The temp copy is left in
    // place so the caller can stop the daemon and retry.
    if lock_is_held(&db_path)? {
        anyhow::bail!(
            "Database was locked during restore ({}). The copied backup \
             remains at {}. Stop the daemon and retry.",
            db_path.join("LOCK").display(),
            temp_path.display()
        );
    }

    // Flush the copied tree so the swap below is durable against a crash.
    sync_tree(&temp_path)?;

    // Swap: old → .old (if present), temp → live. The live DB is never
    // deleted before the replacement is fully materialized on disk.
    if db_path.exists()
        && let Err(e) = std::fs::rename(&db_path, &old_path)
    {
        // The live DB is still in place; the temp copy is unusable here.
        let _ = std::fs::remove_dir_all(&temp_path);
        return Err(e.into());
    }
    if let Err(e) = std::fs::rename(&temp_path, &db_path) {
        // Attempt to roll the original back into place; if that fails the
        // original stays at old_path. Either way, disclose where everything
        // ended up so the caller is not left guessing.
        let _ = std::fs::rename(&old_path, &db_path);
        anyhow::bail!(
            "Failed to move the restored database into place: {e}. \
             The original data is at {} (rolled back to {} if the \
             rollback succeeded) and the new copy remains at {}. Stop any \
             daemon and retry.",
            old_path.display(),
            db_path.display(),
            temp_path.display()
        );
    }
    let _ = std::fs::remove_dir_all(&old_path);

    // Flush the rename entries so the swap survives a crash.
    fsync_path(data_dir)?;

    Ok(RestoreResult {
        backup_path: backup_path.to_path_buf(),
        db_path,
    })
}

/// Recursively copy the directory tree at `src` into `dst`, creating `dst`
/// and any missing parent directories along the way.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), anyhow::Error> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Total size in bytes of every file under `path`, recursively.
fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    total += dir_size(&entry.path());
                } else if let Ok(m) = entry.metadata() {
                    total += m.len();
                }
            }
        }
    }
    total
}

/// Flush `path` to disk (files and, on unix, directories opened as files).
#[cfg(unix)]
fn fsync_path(path: &Path) -> std::io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn fsync_path(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Recursively flush every file and directory under `root`, deepest first.
#[cfg(unix)]
fn sync_tree(root: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            sync_tree(&entry.path())?;
        } else {
            fsync_path(&entry.path())?;
        }
    }
    fsync_path(root)
}

#[cfg(not(unix))]
fn sync_tree(_root: &Path) -> std::io::Result<()> {
    Ok(())
}
