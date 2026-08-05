//! Shared runtime state for the Knowledge Engine.
//!
//! [`EngineState`] layers dynamic runtime state on top of the engine — it is
//! wrapped in `Arc<RwLock<>>` at the engine level so that every component can
//! read the current state without a direct reference to the engine instance.

use std::path::PathBuf;
use std::time::Instant;

/// Dynamic runtime state shared across engine components.
///
/// Every field is guarded by the `RwLock` held by [`KnowledgeEngine`]
/// so that components always read a consistent snapshot.
#[derive(Debug)]
pub struct EngineState {
    started_at: Option<Instant>,

    // ── Project state (Phase 2) ─────────────────────────────
    /// The `SurrealDB` record ID of the active project, if one is loaded.
    project_id: Option<String>,
    /// Absolute path to the project root, if a project is loaded.
    project_root: Option<PathBuf>,
}

impl EngineState {
    /// Create a new engine state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            started_at: None,
            project_id: None,
            project_root: None,
        }
    }

    // ── Lifecycle ───────────────────────────────────────────

    /// Returns the elapsed wall-clock time since the engine started, or
    /// `Duration::ZERO` if the engine is not running.
    #[must_use]
    pub fn uptime(&self) -> std::time::Duration {
        self.started_at
            .map_or(std::time::Duration::ZERO, |t| t.elapsed())
    }

    /// Records the engine start time (called by [`KnowledgeEngine::start`]).
    pub fn set_started_at(&mut self, now: Instant) {
        self.started_at = Some(now);
    }

    /// Clears the start time (called by [`KnowledgeEngine::stop`]).
    pub fn clear_started_at(&mut self) {
        self.started_at = None;
    }

    // ── Project ─────────────────────────────────────────────

    /// Sets the active project identity.
    pub fn set_project(&mut self, project_id: String, project_root: PathBuf) {
        self.project_id = Some(project_id);
        self.project_root = Some(project_root);
    }

    /// Clears the active project identity.
    pub fn clear_project(&mut self) {
        self.project_id = None;
        self.project_root = None;
    }

    /// Returns the project ID, if a project is active.
    #[must_use]
    pub fn project_id(&self) -> Option<&str> {
        self.project_id.as_deref()
    }

    /// Returns the project root path, if a project is active.
    #[must_use]
    pub fn project_root(&self) -> Option<&PathBuf> {
        self.project_root.as_ref()
    }

    /// Returns `true` if a project is currently active.
    #[must_use]
    pub fn has_project(&self) -> bool {
        self.project_id.is_some()
    }
}

impl Default for EngineState {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of engine state for status queries.
///
/// Build via [`KnowledgeEngine::info`], which populates the fields from the
/// live [`EngineState`] and engine lifecycle state.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineInfo {
    pub version: String,
    pub running: bool,
    /// Wall-clock uptime in milliseconds.
    pub uptime_ms: u128,
}

/// Status of the active project.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectStatus {
    pub project_id: String,
    pub project_root: Option<String>,
    pub uptime: std::time::Duration,
}
