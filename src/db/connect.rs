//! Database connection: initialization, retry logic, and connection pooling.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fs2::FileExt;

use surrealdb::Surreal;
use surrealdb::engine::local::{Db, SurrealKv};

/// Resolve the data directory.
///
/// Precedence:
/// 1. `MERCURY_CORTEX_DATA_DIR` — explicit override (used by tests and by
///    users who want to relocate the data directory). The value is used
///    verbatim, so it must point at the data directory itself (the
///    `~/.mercury/cortex` equivalent), not a home directory.
/// 2. `~/.mercury/cortex/` via the cross-platform `dirs` crate.
///
/// On Unix, `dirs::home_dir()` honours `$HOME`; on Windows it calls
/// `SHGetKnownFolderPath` and ignores `USERPROFILE`/`HOME`, so the env
/// override is the only reliable way to redirect the data directory there.
pub fn data_dir() -> Result<PathBuf, std::io::Error> {
    if let Some(dir) = std::env::var_os("MERCURY_CORTEX_DATA_DIR") {
        if dir.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "MERCURY_CORTEX_DATA_DIR must not be empty",
            ));
        }
        return Ok(PathBuf::from(dir));
    }
    dirs::home_dir()
        .map(|h| h.join(".mercury").join("cortex"))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not resolve home directory. Ensure $HOME is set.",
            )
        })
}

/// Open (or create) a `SurrealKV` database at the given path and select the
/// `mercury_cortex.global_knowledge` namespace + database.
///
/// Idempotent — connecting to an already-existing database reuses its data.
///
/// The encryption key is resolved in this priority order:
/// 1. `MERCURY_DB_ENCRYPTION_KEY_FILE` — read the key from the given file path
/// 2. `MERCURY_DB_ENCRYPTION_KEY` — read the key directly from the environment variable
///
/// When the key is set, it is passed as a connection query parameter, percent-
/// encoded so it can never corrupt the engine's config-string parse.
///
/// NOTE: the pinned SurrealKV engine (surrealdb 3.2.3) does not consume an
/// `encryption_key` parameter — `SurrealKvConfig` has no encryption field and
/// `surrealkv` exposes no encryption option — so at-rest encryption is not
/// actually enabled today. The key is still forwarded because the encoding is
/// correct and the parameter is what the engine is expected to accept once
/// encryption is wired up upstream. See the B5 design note.
pub async fn initialize(db_path: &Path) -> Result<Surreal<Db>, surrealdb::Error> {
    let encryption_key = resolve_encryption_key();

    let db = match encryption_key {
        Some(key) if !key.is_empty() => {
            tracing::warn!(
                "a database encryption key is set, but the pinned SurrealKV engine does not \
                 support at-rest encryption; data is stored unencrypted"
            );
            let path_str = format!(
                "{}?encryption_key={}",
                db_path.display(),
                percent_encode(&key)
            );
            Surreal::new::<SurrealKv>(path_str.as_str()).await?
        }
        _ => Surreal::new::<SurrealKv>(db_path).await?,
    };

    db.use_ns("mercury_cortex")
        .use_db("global_knowledge")
        .await?;
    Ok(db)
}

fn resolve_encryption_key() -> Option<String> {
    if let Ok(path) = std::env::var("MERCURY_DB_ENCRYPTION_KEY_FILE") {
        match std::fs::read_to_string(&path) {
            Ok(key) => {
                let trimmed = key.trim().to_owned();
                if !trimmed.is_empty() {
                    tracing::info!("database encryption key read from {}", path);
                    return Some(trimmed);
                }
                tracing::warn!("MERCURY_DB_ENCRYPTION_KEY_FILE={} is empty", path);
            }
            Err(e) => {
                tracing::error!(
                    "failed to read MERCURY_DB_ENCRYPTION_KEY_FILE={}: {e}",
                    path
                );
            }
        }
    }
    std::env::var("MERCURY_DB_ENCRYPTION_KEY").ok()
}

/// Percent-encode all bytes except RFC-3986 unreserved characters
/// (`A-Z a-z 0-9 - . _ ~`).
#[doc(hidden)]
pub fn percent_encode(input: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Retry configuration for database connections with exponential backoff.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub base_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_secs(1),
        }
    }
}

/// Connect once (no retry), handling stale-lock recovery.
async fn try_connect(db_dir: &Path) -> Result<(PathBuf, Surreal<Db>), surrealdb::Error> {
    let result = initialize(db_dir).await;

    match result {
        Ok(db) => Ok((db_dir.to_path_buf(), db)),
        Err(e) => {
            let lock_path = db_dir.join("LOCK");
            if lock_path.exists() {
                tracing::warn!("stale SurrealKV lock file detected, removing and retrying");
                let _ = std::fs::remove_file(&lock_path);
                initialize(db_dir)
                    .await
                    .map(|db| (db_dir.to_path_buf(), db))
            } else {
                Err(e)
            }
        }
    }
}

/// One-shot helper: resolve [`data_dir`], append the database filename, call
/// [`connect_at`] with the default [`RetryConfig`].
///
/// Returns `(db_path, db)` so callers can display the path on success.
pub async fn connect() -> Result<(PathBuf, Surreal<Db>), surrealdb::Error> {
    connect_at(
        &data_dir()?.join(super::DB_FILENAME),
        &RetryConfig::default(),
    )
    .await
}

/// Connect with exponential-backoff retry against the default data dir.
pub async fn connect_with_retry(
    config: &RetryConfig,
) -> Result<(PathBuf, Surreal<Db>), surrealdb::Error> {
    connect_at(&data_dir()?.join(super::DB_FILENAME), config).await
}

/// Connect to a specific database directory with exponential-backoff retry.
///
/// Attempts the connection up to `config.max_retries + 1` times, sleeping
/// `config.base_delay * 2^attempt` between retries.  Each individual attempt
/// already contains a stale-lock recovery path.
pub(crate) async fn connect_at(
    db_dir: &Path,
    config: &RetryConfig,
) -> Result<(PathBuf, Surreal<Db>), surrealdb::Error> {
    let mut last_error = None;
    for attempt in 0..=config.max_retries {
        if attempt > 0 {
            let delay = config.base_delay * 2u32.pow(attempt - 1);
            tokio::time::sleep(delay).await;
            tracing::debug!(
                "[db] retry attempt {attempt}/{max}",
                max = config.max_retries
            );
        }

        match try_connect(db_dir).await {
            Ok(result) => return Ok(result),
            Err(e) => last_error = Some(e),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        surrealdb::Error::from(std::io::Error::other(
            "database operation failed without an error",
        ))
    }))
}

/// Returns `true` when the `SurrealKV` LOCK file's flock is currently held by
/// a live process, `false` when it is absent or free.
///
/// `SurrealKV` never deletes the LOCK file on shutdown — its presence is
/// informational; the actual lock is an OS-level flock. Callers (e.g. `db
/// reset`, backup, restore) must probe the flock rather than check for file
/// existence, or a stale LOCK file would block legitimate operations forever.
pub fn lock_is_held(db_dir: &Path) -> std::io::Result<bool> {
    let lock_path = db_dir.join("LOCK");
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };

    match file.try_lock_exclusive() {
        Ok(()) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(true),
        Err(e) => Err(e),
    }
}
