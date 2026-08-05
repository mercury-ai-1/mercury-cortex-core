//! Database initialization, connection retry, shared circuit breaker, backup,
//! export, and reset.

pub mod backup;
pub mod connect;
pub mod export;
pub mod pool;
pub mod reset;
pub mod retry;

pub use connect::{RetryConfig, connect, connect_with_retry, data_dir, initialize, lock_is_held};
pub use pool::{CircuitBreaker, DbPool};
pub use retry::{reset_shared_breaker, retry, retry_shared, retry_with_breaker};

/// Database directory name inside the data dir.
pub const DB_FILENAME: &str = "mercury_cortex_global_knowledge.db";
