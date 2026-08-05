//! Central coordinator of the Knowledge Engine.
//!
//! [`KnowledgeEngine`] owns the engine lifecycle, shared runtime state,
//! database connection, and index engine. Subsystems such as the importer
//! and search are driven directly through this coordinator.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tokio::sync::RwLock;

use crate::engine::ProjectStatus;
use crate::engine::error::EngineError;
use crate::engine::{EngineInfo, EngineState, EventLog};
use crate::engine::{FileEntry, ImportResult, IndexEngine};
use crate::engine::{SearchQuery, SearchResult};
use tracing::info;

/// The central Knowledge Engine.
///
/// # Lifecycle
///
/// 1. **Construction** — [`new`](Self::new) initialises all components.
/// 2. **Start** — [`start`](Self::start) marks the engine as running.
/// 3. **Stop** — [`stop`](Self::stop) clears runtime state.
///
/// The engine enforces a single-instance guard: calling `start` on an
/// already-running engine returns [`EngineError::AlreadyRunning`].
pub struct KnowledgeEngine {
    context: Arc<RwLock<EngineState>>,
    running: Arc<AtomicBool>,

    // ── Phase 2 additions ───────────────────────────────────
    /// Embedded `SurrealDB` connection, created at construction.
    db: Surreal<Db>,
    /// Index engine — holds the importer, runtime index, and search.
    index_engine: Arc<IndexEngine>,

    /// In-memory event log / audit trail.
    event_log: EventLog,
}

impl KnowledgeEngine {
    /// Create a new engine with the given DB connection.
    ///
    /// Call [`start`](Self::start) to begin running the engine.
    #[must_use]
    pub fn new(db: Surreal<Db>) -> Self {
        let context = EngineState::new();

        // Index engine is created once a project is configured via
        // `set_project`.  For now, use a placeholder.
        let project_id = String::new();
        let project_root = PathBuf::new();
        let repo = Arc::new(crate::engine::SurrealFileDataRepository::new(db.clone()));
        let index_engine = IndexEngine::new(repo, db.clone(), &project_id, &project_root);

        Self {
            context: Arc::new(RwLock::new(context)),
            running: Arc::new(AtomicBool::new(false)),
            db,
            index_engine: Arc::new(index_engine),
            event_log: EventLog::new(),
        }
    }

    /// Configure the active project for the engine.
    ///
    /// Creates a new `IndexEngine` targeting the given project.
    ///
    /// The importer is rebuilt for the new project root, so it picks up a
    /// fresh [`McIgnore`](crate::engine::McIgnore) and temp directory.
    pub async fn set_project(&self, project_id: String, project_root: PathBuf) {
        {
            let mut ctx = self.context.write().await;
            ctx.set_project(project_id.clone(), project_root.clone());
        }

        self.index_engine
            .set_project(project_id.clone(), project_root)
            .await;
        self.event_log.push("project_created", project_id).await;
    }

    /// Start the engine.
    ///
    /// Records the start timestamp.
    pub async fn start(&self) -> Result<(), EngineError> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(EngineError::AlreadyRunning);
        }

        info!("engine start");

        {
            let mut ctx = self.context.write().await;
            ctx.set_started_at(Instant::now());
        }

        Ok(())
    }

    /// Stop the engine.
    ///
    /// Clears runtime state.
    pub async fn stop(&self) -> Result<(), EngineError> {
        if !self.running.swap(false, Ordering::SeqCst) {
            return Err(EngineError::NotRunning);
        }

        info!("engine stop");

        {
            let mut ctx = self.context.write().await;
            ctx.clear_started_at();
        }

        Ok(())
    }

    /// Shared reference to the runtime context.
    pub fn context(&self) -> &Arc<RwLock<EngineState>> {
        &self.context
    }

    /// Shared reference to the database connection.
    pub fn db(&self) -> &Surreal<Db> {
        &self.db
    }

    /// Shared reference to the event log.
    pub fn event_log(&self) -> &EventLog {
        &self.event_log
    }

    /// Returns engine version and running status.
    pub async fn info(&self) -> EngineInfo {
        let ctx = self.context.read().await;
        EngineInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            running: self.running.load(Ordering::SeqCst),
            uptime_ms: ctx.uptime().as_millis(),
        }
    }

    /// Returns the active project status, if any.
    pub async fn project_status(&self) -> Option<ProjectStatus> {
        let ctx = self.context.read().await;
        ctx.project_id().map(|pid| ProjectStatus {
            project_id: pid.to_string(),
            project_root: ctx.project_root().map(|p| p.to_string_lossy().to_string()),
            uptime: ctx.uptime(),
        })
    }

    /// Clears the active project context.
    pub async fn clear_project(&self) {
        let mut ctx = self.context.write().await;
        ctx.clear_project();
    }

    /// Import all pending metadata JSON files from `.mercury-cortex/temp/`.
    pub async fn submit_metadata(&self) -> Result<Vec<ImportResult>, EngineError> {
        let start = std::time::Instant::now();
        let result = self.index_engine.import_pending().await;
        let elapsed = start.elapsed();
        let count = result.as_ref().map_or(0, std::vec::Vec::len);
        info!(elapsed_ms = elapsed.as_millis(), count, "submit_metadata");
        result
    }

    /// Get metadata for a specific file from the runtime index.
    pub async fn get_file_metadata(&self, path: &str) -> Option<FileEntry> {
        self.index_engine.get_file_metadata(path).await
    }

    /// Count `file_data` rows for the active project.
    ///
    /// This is the authoritative "indexed files" number reported by
    /// `metadata/import`.
    pub async fn count_indexed_files(&self) -> Result<usize, EngineError> {
        use surrealdb::types::RecordId;

        let ctx = self.context.read().await;
        let project_id = ctx
            .project_id()
            .ok_or_else(|| EngineError::Internal(anyhow::anyhow!("no active project")))?;

        let rid = RecordId::parse_simple(project_id)
            .map_err(|e| EngineError::Internal(anyhow::anyhow!("invalid project_id: {e}")))?;

        let rows: Vec<serde_json::Value> = self
            .db
            .query(
                "SELECT count() AS total FROM file_data WHERE project_id = $project_id GROUP ALL",
            )
            .bind(("project_id", surrealdb::types::Value::RecordId(rid)))
            .await
            .map_err(EngineError::Database)?
            .take(0)
            .map_err(EngineError::Database)?;

        let total = rows
            .first()
            .and_then(|r| r.get("total"))
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|n| n as u64)))
            .unwrap_or(0);
        Ok(total as usize)
    }

    /// Search indexed metadata.
    pub async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>, EngineError> {
        if !self.context.read().await.has_project() {
            return Ok(Vec::new());
        }
        let start = std::time::Instant::now();
        let result = self.index_engine.search(query).await;
        let elapsed = start.elapsed();
        let count = result.as_ref().map_or(0, std::vec::Vec::len);
        info!(elapsed_ms = elapsed.as_millis(), count, "search");
        if result.is_ok() {
            self.event_log
                .push("search_performed", query.query.clone().unwrap_or_default())
                .await;
        }
        result
    }

    /// Return all indexed relative paths for the active project.
    ///
    /// Used by the AI tool during re-run gap-fill to determine which files
    /// already have metadata and only generate metadata for missing files.
    pub async fn list_indexed_paths(&self) -> Result<Vec<String>, EngineError> {
        use surrealdb::types::RecordId;
        let ctx = self.context.read().await;
        let project_id = ctx
            .project_id()
            .ok_or_else(|| EngineError::Internal(anyhow::anyhow!("no active project")))?;

        let rid = RecordId::parse_simple(project_id)
            .map_err(|e| EngineError::Internal(anyhow::anyhow!("invalid project_id: {e}")))?;

        let records: Vec<serde_json::Value> = self
            .db
            .query("SELECT path FROM file_data WHERE project_id = $project_id")
            .bind(("project_id", surrealdb::types::Value::RecordId(rid)))
            .await
            .map_err(EngineError::Database)?
            .take(0)
            .map_err(EngineError::Database)?;

        let paths: Vec<String> = records
            .into_iter()
            .filter_map(|v| v.get("path").and_then(|p| p.as_str()).map(String::from))
            .collect();

        Ok(paths)
    }
}
