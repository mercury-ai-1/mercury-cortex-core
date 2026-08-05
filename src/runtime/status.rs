//! Runtime phase, health, and error tracking.
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Lifecycle phase of the Mercury Cortex runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePhase {
    ConfigLoaded,
    DatabaseConnecting,
    DatabaseConnected,
    Validating,
    EngineStarting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

impl RuntimePhase {
    /// Returns `true` if the phase is `Stopped` or `Failed`.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, RuntimePhase::Stopped | RuntimePhase::Failed)
    }
}

impl fmt::Display for RuntimePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimePhase::ConfigLoaded => write!(f, "ConfigLoaded"),
            RuntimePhase::DatabaseConnecting => write!(f, "DatabaseConnecting"),
            RuntimePhase::DatabaseConnected => write!(f, "DatabaseConnected"),
            RuntimePhase::Validating => write!(f, "Validating"),
            RuntimePhase::EngineStarting => write!(f, "EngineStarting"),
            RuntimePhase::Running => write!(f, "Running"),
            RuntimePhase::Stopping => write!(f, "Stopping"),
            RuntimePhase::Stopped => write!(f, "Stopped"),
            RuntimePhase::Failed => write!(f, "Failed"),
        }
    }
}

/// Overall health status of the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

/// Categorised error codes for runtime failures.
#[derive(Debug, Clone)]
pub enum ErrorCode {
    HomeDirMissing,
    DataDirCreateFailed,
    SocketBindFailed,
    DatabaseCorrupt {
        path: String,
    },
    StaleLock {
        path: String,
    },
    PermissionDenied {
        path: String,
    },
    DatabaseGeneric {
        source: String,
    },
    MigrationFailed {
        version: u64,
        name: String,
        source: String,
    },
    SchemaIncomplete {
        missing_tables: Vec<String>,
    },
    EngineStartFailed {
        source: String,
    },
}

impl ErrorCode {
    /// Return a human-readable error message for this code.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            ErrorCode::HomeDirMissing => {
                "$HOME is not set or the home directory cannot be resolved".into()
            }
            ErrorCode::DataDirCreateFailed => {
                "The Mercury Cortex data directory could not be created".into()
            }
            ErrorCode::SocketBindFailed => {
                "The Unix socket could not be bound (address in use or permissions)".into()
            }
            ErrorCode::DatabaseCorrupt { path } => {
                format!("The database at {path} could not be opened — the WAL may be corrupted")
            }
            ErrorCode::StaleLock { path } => {
                format!("A stale lock file was found at {path} and could not be cleared")
            }
            ErrorCode::PermissionDenied { path } => {
                format!("Permission denied accessing {path}")
            }
            ErrorCode::DatabaseGeneric { source } => {
                format!("Database error: {source}")
            }
            ErrorCode::MigrationFailed {
                version,
                name,
                source,
            } => {
                format!("Migration v{version} ({name}) failed: {source}")
            }
            ErrorCode::SchemaIncomplete { missing_tables } => {
                format!(
                    "Schema is incomplete — missing tables: {}",
                    missing_tables.join(", ")
                )
            }
            ErrorCode::EngineStartFailed { source } => {
                format!("The knowledge engine failed to start: {source}")
            }
        }
    }

    /// Return a suggested recovery action for this error code.
    #[must_use]
    pub fn recovery(&self) -> String {
        match self {
            ErrorCode::HomeDirMissing => {
                "Set $HOME to a writable directory and restart".into()
            }
            ErrorCode::DataDirCreateFailed => {
                "Check permissions on ~/.mercury and ensure the disk is not full".into()
            }
            ErrorCode::SocketBindFailed => {
                "Remove ~/.mercury/cortex/runtime.sock and restart".into()
            }
            ErrorCode::DatabaseCorrupt { .. } => {
                "The database is corrupted. Run 'mercury-cortex db repair' for recovery options, or restore from a backup. If no backup exists, run 'mercury-cortex setup --force' to recreate the database (this will lose all data).".into()
            }
            ErrorCode::StaleLock { path } => {
                format!("Remove {path}/LOCK manually and restart")
            }
            ErrorCode::PermissionDenied { .. } => {
                "Check read/write permissions on the database directory".into()
            }
            ErrorCode::DatabaseGeneric { .. } | ErrorCode::EngineStartFailed { .. } => {
                "Check the server logs for details and restart the server".into()
            }
            ErrorCode::MigrationFailed { .. } => {
                "Run 'mercury-cortex migration apply' to retry, or check the migration file for errors".into()
            }
            ErrorCode::SchemaIncomplete { .. } => {
                "Run 'mercury-cortex setup' to reinitialize the schema".into()
            }
        }
    }

    /// Return a short uppercase code string (e.g. `"HOME_DIR_MISSING"`).
    #[must_use]
    pub fn code_str(&self) -> &'static str {
        match self {
            ErrorCode::HomeDirMissing => "HOME_DIR_MISSING",
            ErrorCode::DataDirCreateFailed => "DATA_DIR_CREATE_FAILED",
            ErrorCode::SocketBindFailed => "SOCKET_BIND_FAILED",
            ErrorCode::DatabaseCorrupt { .. } => "DATABASE_CORRUPT",
            ErrorCode::StaleLock { .. } => "STALE_LOCK",
            ErrorCode::PermissionDenied { .. } => "PERMISSION_DENIED",
            ErrorCode::DatabaseGeneric { .. } => "DATABASE_ERROR",
            ErrorCode::MigrationFailed { .. } => "MIGRATION_FAILED",
            ErrorCode::SchemaIncomplete { .. } => "SCHEMA_INCOMPLETE",
            ErrorCode::EngineStartFailed { .. } => "ENGINE_START_FAILED",
        }
    }
}

/// A structured runtime error with code, message, and recovery hint.
#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub code: ErrorCode,
    pub message: String,
    pub recovery: String,
    pub source: Option<String>,
}

impl From<ErrorCode> for RuntimeError {
    fn from(code: ErrorCode) -> Self {
        let message = code.message();
        let recovery = code.recovery();
        let source = match &code {
            ErrorCode::DatabaseGeneric { source }
            | ErrorCode::MigrationFailed { source, .. }
            | ErrorCode::EngineStartFailed { source, .. } => Some(source.clone()),
            ErrorCode::DatabaseCorrupt { path }
            | ErrorCode::StaleLock { path }
            | ErrorCode::PermissionDenied { path } => Some(format!("path: {path}")),
            _ => None,
        };
        RuntimeError {
            code,
            message,
            recovery,
            source,
        }
    }
}

/// A single timing trace entry recorded during startup.
#[derive(Debug, Clone)]
pub struct StartupTraceEntry {
    pub phase: RuntimePhase,
    pub duration_ms: u64,
    pub error: Option<RuntimeError>,
}

impl StartupTraceEntry {
    /// Create a new trace entry for a phase and its duration.
    #[must_use]
    pub fn new(phase: RuntimePhase, duration_ms: u64) -> Self {
        StartupTraceEntry {
            phase,
            duration_ms,
            error: None,
        }
    }
}

/// Aggregate runtime status: phase, health, errors, and startup trace.
#[derive(Debug, Clone)]
pub struct RuntimeStatus {
    pub phase: RuntimePhase,
    pub health: HealthStatus,
    pub error: Option<RuntimeError>,
    pub started_at: u64,
    pub startup_trace: Vec<StartupTraceEntry>,
}

impl Default for RuntimeStatus {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeStatus {
    /// Create a new `RuntimeStatus` in the `ConfigLoaded` / `Degraded` state.
    #[must_use]
    pub fn new() -> Self {
        let started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .expect("SystemTime::now() is before UNIX_EPOCH");
        RuntimeStatus {
            phase: RuntimePhase::ConfigLoaded,
            health: HealthStatus::Degraded,
            error: None,
            started_at,
            startup_trace: Vec::new(),
        }
    }

    /// Advance the runtime to a new phase, updating health if `Running`.
    pub fn transition_to(&mut self, phase: RuntimePhase) {
        self.phase = phase;
        if phase == RuntimePhase::Running {
            self.health = HealthStatus::Healthy;
        }
    }

    /// Record a fatal error, transitioning to `Failed` / `Unhealthy`.
    pub fn record_error(&mut self, code: ErrorCode) {
        self.health = HealthStatus::Unhealthy;
        self.phase = RuntimePhase::Failed;
        self.error = Some(RuntimeError::from(code));
    }

    /// Append a startup trace entry to the log.
    pub fn add_trace_entry(&mut self, entry: StartupTraceEntry) {
        self.startup_trace.push(entry);
    }
}
