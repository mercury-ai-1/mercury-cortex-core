//! Top-level runtime that owns the database, engine, and services.
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;
use tokio::sync::watch;

use surrealdb::Surreal;
use surrealdb::engine::local::Db;

use super::context::{RuntimeConfig, RuntimeContext};
use super::lock::RwLockExt;
use super::signal::wait_shutdown_signal;
use super::status::{ErrorCode, RuntimePhase, RuntimeStatus, StartupTraceEntry};
use crate::engine::KnowledgeEngine;

/// Single Mercury Cortex runtime — owns the database connection, knowledge
/// engine, and shared services.
///
/// `SurrealKV` is an embedded engine that uses exclusive file locking, so only
/// **one process** may hold a [`Runtime`] at a time.  All services that need
/// database access (MCP, API, dashboard, …) must live in the same process
/// and share this runtime.
pub struct Runtime {
    pub ctx: Arc<RuntimeContext>,
}

impl Runtime {
    /// Open the database, create the knowledge engine, and start the IPC
    /// server on the configured Unix socket.
    ///
    /// Creates the data directory if it doesn't exist, so the database can
    /// be opened without having run `mercury-cortex setup` first.
    ///
    /// Fatal errors (config resolution, data dir creation) are returned to the caller.
    /// Non-fatal startup errors (DB, migration, engine) are captured in `RuntimeStatus`.
    pub async fn new() -> Result<Self, std::io::Error> {
        let config = RuntimeConfig::new()?;
        std::fs::create_dir_all(&config.data_dir)?;

        let status = Arc::new(RwLock::new(RuntimeStatus::new()));
        status
            .write_unpoison()
            .add_trace_entry(StartupTraceEntry::new(RuntimePhase::ConfigLoaded, 0u64));

        // Phase 2: DatabaseConnecting — connect db first so it can be stored
        // in RuntimeContext via OnceLock (no RwLock on reads).
        let phase_start = Instant::now();
        let db_result = crate::db::connect().await;

        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let ctx = Arc::new(RuntimeContext {
            config: config.clone(),
            status: status.clone(),
            db: OnceLock::new(),
            engine: RwLock::new(None),
            shutdown_tx: OnceLock::new(),
        });
        ctx.shutdown_tx.set(shutdown_tx).ok();

        let db = match db_result {
            Ok((_path, db)) => {
                status
                    .write_unpoison()
                    .add_trace_entry(StartupTraceEntry::new(
                        RuntimePhase::DatabaseConnecting,
                        phase_start.elapsed().as_millis() as u64,
                    ));
                ctx.set_database(db.clone(), RuntimePhase::DatabaseConnected);

                status
                    .write_unpoison()
                    .add_trace_entry(StartupTraceEntry::new(
                        RuntimePhase::DatabaseConnected,
                        phase_start.elapsed().as_millis() as u64,
                    ));
                Arc::new(db)
            }
            Err(e) => {
                let err_str = e.to_string();
                let err_lower = err_str.to_lowercase();
                let error_code = if err_lower.contains("lock") {
                    ErrorCode::StaleLock {
                        path: config.data_dir.to_string_lossy().into(),
                    }
                } else if err_str.to_lowercase().contains("permission") {
                    ErrorCode::PermissionDenied {
                        path: config.data_dir.to_string_lossy().into(),
                    }
                } else if err_lower.contains("corrupt") {
                    ErrorCode::DatabaseCorrupt {
                        path: config.data_dir.to_string_lossy().into(),
                    }
                } else {
                    ErrorCode::DatabaseGeneric { source: err_str }
                };
                status.write_unpoison().record_error(error_code);
                return Ok(Self { ctx });
            }
        };

        // Phase 4: Validating
        let phase_start = Instant::now();
        if let Err(e) = crate::schema::run_pending(db.as_ref()).await {
            ctx.record_error(ErrorCode::MigrationFailed {
                version: 0,
                name: "pending".into(),
                source: e.to_string(),
            });
            return Ok(Self { ctx });
        }
        status
            .write_unpoison()
            .add_trace_entry(StartupTraceEntry::new(
                RuntimePhase::Validating,
                phase_start.elapsed().as_millis() as u64,
            ));

        // REL-008: Verify schema consistency after migration.
        // Catches interrupted-migration scenarios where tables are partially
        // defined or missing.
        if let Err(e) = crate::schema::verify_schema(db.as_ref()).await {
            ctx.record_error(ErrorCode::SchemaIncomplete {
                missing_tables: vec![e.to_string()],
            });
            return Ok(Self { ctx });
        }

        // Phase 5: EngineStarting
        let phase_start = Instant::now();
        let engine = Arc::new(KnowledgeEngine::new(db.as_ref().clone()));
        if let Err(e) = engine.start().await {
            ctx.record_error(ErrorCode::EngineStartFailed {
                source: e.to_string(),
            });
            return Ok(Self { ctx });
        }
        ctx.set_engine(engine, RuntimePhase::EngineStarting);
        status
            .write_unpoison()
            .add_trace_entry(StartupTraceEntry::new(
                RuntimePhase::EngineStarting,
                phase_start.elapsed().as_millis() as u64,
            ));

        // Phase 6: Running — start signal handler
        let phase_start = Instant::now();
        let shutdown_ctx = ctx.clone();
        tokio::spawn(async move {
            wait_shutdown_signal().await;
            tracing::info!("[runtime] shutdown signal received; shutting down...");
            Runtime::trigger_shutdown(&shutdown_ctx).await;
        });
        status.write_unpoison().transition_to(RuntimePhase::Running);
        status
            .write_unpoison()
            .add_trace_entry(StartupTraceEntry::new(
                RuntimePhase::Running,
                phase_start.elapsed().as_millis() as u64,
            ));

        Ok(Self { ctx })
    }

    /// Shorthand for accessing the database.
    pub fn db(&self) -> Result<Surreal<Db>, crate::service::ServiceError> {
        self.ctx.database()
    }

    /// Shorthand for accessing the engine.
    pub fn engine(&self) -> Result<Arc<KnowledgeEngine>, crate::service::ServiceError> {
        self.ctx.engine()
    }

    /// Perform a graceful shutdown: stop the engine and mark status as `Stopped`.
    pub async fn trigger_shutdown(ctx: &RuntimeContext) {
        {
            let mut status = ctx.status.write_unpoison();
            if status.phase == RuntimePhase::Stopping || status.phase == RuntimePhase::Stopped {
                return;
            }
            status.phase = RuntimePhase::Stopping;
        }

        let engine_opt = ctx.engine.read_unpoison().clone();
        if let Some(engine) = engine_opt {
            const ENGINE_STOP_TIMEOUT_SECS: u64 = 5;
            tokio::time::timeout(
                std::time::Duration::from_secs(ENGINE_STOP_TIMEOUT_SECS),
                engine.stop(),
            )
            .await
            .ok();
        }

        ctx.status.write_unpoison().phase = RuntimePhase::Stopped;
    }

    /// Initiate graceful shutdown. Callers should drop the Runtime after this.
    pub async fn shutdown(&self) {
        Self::trigger_shutdown(&self.ctx).await;
    }
}
