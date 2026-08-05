//! `CoreClient` — the single public entry point to Mercury Cortex for the
//! CLI command surface. Owns an internal [`RuntimeContext`] and exposes
//! domain clients for profiles, projects, database maintenance, and graphs.
//!
//! Callers receive data structures, never raw database handles.

mod database;
mod graph;
mod profile;
mod project;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use surrealdb::Surreal;
use surrealdb::engine::local::Db;

use crate::runtime::RuntimeConfig;
use crate::runtime::context::RuntimeContext;
use crate::runtime::status::{RuntimePhase, RuntimeStatus};
use crate::service::ServiceError;

pub use database::DatabaseClient;
pub use graph::GraphClient;
pub use profile::ProfileClient;
pub use project::ProjectClient;

// Data types returned by the facade, re-exported so callers never reach into
// core internals (`db::*`, `schema::*`, `service::*`).
pub use crate::db::backup::{BackupEntry, BackupList, BackupResult, RestoreResult};
pub use crate::db::export::{ExportFile, ExportFilter, ExportSummary};
pub use crate::db::reset::{ResetMode, ResetSummary};
pub use crate::schema::migration::run::MigrationReport;
pub use crate::service::profile::{ProfileData, UpsertParams};
pub use crate::service::project::{ProjectAction, RegisterParams, RegisterResult};

/// Installation paths resolved from the user's data directory.
#[derive(Debug, Clone)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
}

/// Error type for the `CoreClient` facade.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Database(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("service error: {0}")]
    Service(#[from] ServiceError),
}

impl From<surrealdb::Error> for CoreError {
    fn from(e: surrealdb::Error) -> Self {
        CoreError::Database(e.to_string())
    }
}

impl From<anyhow::Error> for CoreError {
    fn from(e: anyhow::Error) -> Self {
        CoreError::Service(e.into())
    }
}

/// Facade over the engine for the CLI command surface.
pub struct CoreClient {
    ctx: Arc<RuntimeContext>,
}

impl CoreClient {
    /// Resolve installation paths from the user's data dir, without opening
    /// a connection. Used for pre-connect guards and the `version` command.
    pub fn paths() -> Result<Paths, CoreError> {
        let data_dir = crate::db::data_dir()?;
        Ok(Paths {
            db_path: data_dir.join(crate::db::DB_FILENAME),
            data_dir,
        })
    }

    /// Open the engine against the default data directory. The database
    /// connection is established lazily on the first domain operation, so
    /// constructing a client is cheap and never touches the database lock.
    pub fn open() -> Result<Self, CoreError> {
        let config = RuntimeConfig::new()?;
        Ok(Self::from_config(config))
    }

    /// Test/advanced constructor: open against an explicit data directory.
    #[doc(hidden)]
    pub fn open_with_data_dir(data_dir: PathBuf) -> Result<Self, CoreError> {
        Ok(Self::from_config(RuntimeConfig {
            socket_path: data_dir.join("runtime.sock"),
            data_dir,
        }))
    }

    /// Test/advanced constructor: wrap an already-open connection, so tests
    /// can assert directly on the shared database handle.
    #[doc(hidden)]
    pub fn from_connection(db: Surreal<Db>, data_dir: PathBuf) -> Result<Self, CoreError> {
        let client = Self::open_with_data_dir(data_dir)?;
        client.ctx.set_database(db, RuntimePhase::DatabaseConnected);
        Ok(client)
    }

    fn from_config(config: RuntimeConfig) -> Self {
        let ctx = Arc::new(RuntimeContext {
            config,
            status: Arc::new(std::sync::RwLock::new(RuntimeStatus::new())),
            db: OnceLock::new(),
            engine: std::sync::RwLock::new(None),
            shutdown_tx: OnceLock::new(),
        });
        Self { ctx }
    }

    /// Profile operations.
    pub fn profile(&self) -> ProfileClient<'_> {
        ProfileClient { client: self }
    }

    /// Project registration and scaffolding.
    pub fn project(&self) -> ProjectClient<'_> {
        ProjectClient { client: self }
    }

    /// Database maintenance (backup, restore, reset, migrations, schema).
    pub fn database(&self) -> DatabaseClient<'_> {
        DatabaseClient { client: self }
    }

    /// Knowledge-graph edge queries.
    pub fn graph(&self) -> GraphClient<'_> {
        GraphClient { client: self }
    }

    pub(crate) fn ctx(&self) -> &Arc<RuntimeContext> {
        &self.ctx
    }

    pub(crate) fn data_dir(&self) -> &Path {
        &self.ctx.config.data_dir
    }

    /// Lazily establish the database connection on the configured data dir.
    pub(crate) async fn ensure_connected(&self) -> Result<(), CoreError> {
        if self.ctx.database().is_ok() {
            return Ok(());
        }
        let db_path = self.ctx.config.data_dir.join(crate::db::DB_FILENAME);
        let (_path, db) =
            crate::db::connect::connect_at(&db_path, &crate::db::RetryConfig::default()).await?;
        self.ctx.set_database(db, RuntimePhase::DatabaseConnected);
        Ok(())
    }
}
