//! Connection pool wrapper and circuit breaker for SurrealDB.
use std::sync::Arc;
use std::time::Duration;

use surrealdb::Surreal;
use surrealdb::engine::local::Db;

/// Circuit breaker state machine for database operations.
///
/// Protects the database from cascading failures by rejecting requests
/// when consecutive failures exceed a threshold.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    inner: Arc<std::sync::Mutex<CircuitBreakerInner>>,
}

#[derive(Debug)]
struct CircuitBreakerInner {
    state: BreakerState,
    failure_count: u64,
    opened_at: std::time::Instant,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BreakerState {
    Closed,
    Open,
    HalfOpen,
}

const MAX_CONSECUTIVE_FAILURES: u64 = 3;
const COOLDOWN: Duration = Duration::from_secs(5);

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

impl CircuitBreaker {
    /// Create a new circuit breaker in the closed state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(std::sync::Mutex::new(CircuitBreakerInner {
                state: BreakerState::Closed,
                failure_count: 0,
                opened_at: std::time::Instant::now(),
            })),
        }
    }

    /// Check whether a request is allowed through the circuit breaker.
    #[must_use]
    pub fn allow_request(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        match inner.state {
            BreakerState::Closed | BreakerState::HalfOpen => true,
            BreakerState::Open => {
                if inner.opened_at.elapsed() >= COOLDOWN {
                    inner.state = BreakerState::HalfOpen;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Record a successful request, resetting the failure count.
    pub fn record_success(&self) {
        let mut inner = self.inner.lock().unwrap();
        match inner.state {
            BreakerState::HalfOpen => {
                inner.state = BreakerState::Closed;
                inner.failure_count = 0;
            }
            BreakerState::Closed => {
                inner.failure_count = 0;
            }
            BreakerState::Open => {}
        }
    }

    /// Record a failed request, potentially opening the circuit.
    pub fn record_failure(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.failure_count += 1;
        match inner.state {
            BreakerState::Closed => {
                if inner.failure_count >= MAX_CONSECUTIVE_FAILURES {
                    inner.state = BreakerState::Open;
                    inner.opened_at = std::time::Instant::now();
                }
            }
            BreakerState::HalfOpen => {
                inner.state = BreakerState::Open;
                inner.opened_at = std::time::Instant::now();
            }
            BreakerState::Open => {}
        }
    }

    /// Manually reset the circuit breaker to the closed state.
    pub fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.state = BreakerState::Closed;
        inner.failure_count = 0;
    }
}

/// Thin wrapper around a `Surreal<Db>` connection that provides a consistent
/// interface for pool access and health checking.
///
/// The actual connection pooling is handled internally by `SurrealDB`'s `SurrealKV`
/// engine; this struct provides a uniform abstraction layer so callers don't
/// depend on the concrete connection type.
#[derive(Debug, Clone)]
pub struct DbPool {
    db: Surreal<Db>,
}

impl DbPool {
    /// Wrap a SurrealDB connection in a `DbPool`.
    #[must_use]
    pub fn new(db: Surreal<Db>) -> Self {
        Self { db }
    }

    /// Get a reference to the underlying SurrealDB connection.
    #[must_use]
    pub fn get(&self) -> &Surreal<Db> {
        &self.db
    }

    /// Run a quick health check by executing `RETURN 1`.
    pub async fn health_check(&self) -> Result<(), surrealdb::Error> {
        self.db.query("RETURN 1").await?;
        Ok(())
    }
}
