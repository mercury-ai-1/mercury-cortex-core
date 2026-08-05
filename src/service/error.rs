//! Unified error type for the service layer.
use thiserror::Error;

use crate::engine::error::EngineError;

/// Errors returned by service-layer operations.
#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("runtime not ready: {0}")]
    RuntimeNotReady(String),
}

impl From<surrealdb::Error> for ServiceError {
    fn from(e: surrealdb::Error) -> Self {
        ServiceError::Database(e.to_string())
    }
}

impl From<serde_json::Error> for ServiceError {
    fn from(e: serde_json::Error) -> Self {
        ServiceError::Internal(e.to_string())
    }
}

impl From<EngineError> for ServiceError {
    fn from(e: EngineError) -> Self {
        ServiceError::Internal(e.to_string())
    }
}

impl From<anyhow::Error> for ServiceError {
    fn from(e: anyhow::Error) -> Self {
        ServiceError::Internal(e.to_string())
    }
}
