//! Error types for the Knowledge Engine.
//!
//! [`EngineError`] is the canonical error type for all engine operations.
//! It stays within the engine boundary.  The CLI layer converts it to
//! `anyhow::Error` at the command boundary when needed.

use thiserror::Error;

/// Errors produced by the Knowledge Engine.
#[derive(Error, Debug)]
pub enum EngineError {
    /// Returned when `start()` is called on an already-running engine.
    #[error("engine is already running")]
    AlreadyRunning,

    /// Returned when an operation requires the engine to be running.
    #[error("engine is not running")]
    NotRunning,

    /// Wraps an I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Wraps a `SurrealDB` error.
    #[error("database error: {0}")]
    Database(#[from] surrealdb::Error),

    /// Catch-all for internal errors that do not fit any variant above.
    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}
