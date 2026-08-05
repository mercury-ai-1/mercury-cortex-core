//! Shared application state owned by the runtime.
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tokio::sync::watch;

use crate::engine::KnowledgeEngine;
use crate::runtime::RwLockExt;
use crate::runtime::status::{ErrorCode, RuntimePhase, RuntimeStatus};
use crate::service::error::ServiceError;

/// Runtime configuration — resolved once at startup.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub data_dir: PathBuf,
    pub socket_path: PathBuf,
}

impl RuntimeConfig {
    /// Default: `~/.mercury/cortex`, socket at `runtime.sock`.
    pub fn new() -> Result<Self, std::io::Error> {
        let data_dir = crate::db::data_dir()?;
        let socket_path = data_dir.join("runtime.sock");
        Ok(Self {
            data_dir,
            socket_path,
        })
    }
}

/// Shared application state, wrapped in `Arc` and passed to every service.
///
/// Owned by [`Runtime`](super::Runtime) and shared concurrently between the IPC
/// server, MCP server, API server, and background tasks.
///
/// `db` uses [`OnceLock`] — set once at startup, then accessed lock-free on
/// every read.  This eliminates the `RwLock<Option<Db>>` contention that the
/// original code suffered from (see audit BUG-023 / TD-010 / PERF-004).
pub struct RuntimeContext {
    pub config: RuntimeConfig,
    pub status: Arc<std::sync::RwLock<RuntimeStatus>>,
    pub db: OnceLock<Arc<Surreal<Db>>>,
    pub engine: std::sync::RwLock<Option<Arc<KnowledgeEngine>>>,
    pub shutdown_tx: OnceLock<watch::Sender<bool>>,
}

impl RuntimeContext {
    /// Return a clone of the database handle, or `RuntimeNotReady` if unset.
    pub fn database(&self) -> Result<Surreal<Db>, ServiceError> {
        self.db.get().map(|arc| (**arc).clone()).ok_or_else(|| {
            let phase = self.status.read_unpoison().phase.to_string();
            ServiceError::RuntimeNotReady(phase)
        })
    }

    /// Return a clone of the engine handle, or `RuntimeNotReady` if unset.
    pub fn engine(&self) -> Result<Arc<KnowledgeEngine>, ServiceError> {
        self.engine
            .read_unpoison()
            .as_ref()
            .ok_or_else(|| {
                let phase = self.status.read_unpoison().phase.to_string();
                ServiceError::RuntimeNotReady(phase)
            })
            .cloned()
    }

    /// Set the database handle and transition to the given phase.
    pub fn set_database(&self, db: Surreal<Db>, phase: RuntimePhase) {
        self.status.write_unpoison().transition_to(phase);
        self.db.set(Arc::new(db)).unwrap_or_else(|_| {
            tracing::warn!("database already set — ignoring duplicate set_database call")
        });
    }

    /// Set the knowledge engine handle and transition to the given phase.
    pub fn set_engine(&self, engine: Arc<KnowledgeEngine>, phase: RuntimePhase) {
        self.status.write_unpoison().transition_to(phase);
        *self.engine.write_unpoison() = Some(engine);
    }

    /// Record an error code in the runtime status.
    pub fn record_error(&self, code: ErrorCode) {
        self.status.write_unpoison().record_error(code);
    }

    /// Signal full process shutdown. Wakes the daemon's graceful shutdown
    /// future, which then drains the HTTP server and exits the process.
    /// Safe to call multiple times — only the first send has an effect.
    pub fn signal_shutdown(&self) {
        if let Some(tx) = self.shutdown_tx.get() {
            let _ = tx.send(true);
        }
    }

    /// Create a `RuntimeContext` with test defaults (tmp dirs).
    #[must_use]
    pub fn new_for_test() -> Self {
        RuntimeContext {
            config: RuntimeConfig {
                data_dir: PathBuf::from("/tmp/mercury-test"),
                socket_path: PathBuf::from("/tmp/mercury-test/runtime.sock"),
            },
            status: Arc::new(std::sync::RwLock::new(RuntimeStatus::new())),
            db: OnceLock::new(),
            engine: std::sync::RwLock::new(None),
            shutdown_tx: OnceLock::new(),
        }
    }
}
