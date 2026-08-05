//! Retry wrappers with exponential backoff and circuit breaker protection.

use std::future::Future;
use std::time::Duration;

use crate::db::pool::CircuitBreaker;

const RETRY_MAX_ATTEMPTS: u32 = 3;
const RETRY_BASE_DELAY_MS: u64 = 100;

/// Retry wrapper: calls `operation` up to `max_retries + 1` times with
/// exponential backoff (100ms, 200ms, 400ms, …).  Only retries on
/// surrealdb errors that are likely transient (connection, lock, timeout).
pub async fn retry<F, Fut, T>(operation: F) -> Result<T, surrealdb::Error>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, surrealdb::Error>>,
{
    retry_with_breaker(&CircuitBreaker::new(), operation).await
}

/// Retry wrapper with circuit breaker protection.
///
/// Checks the circuit breaker before each attempt. If the circuit is open,
/// the request is rejected immediately without calling the operation. On
/// success records the success; on failure records the failure.
pub async fn retry_with_breaker<F, Fut, T>(
    breaker: &CircuitBreaker,
    operation: F,
) -> Result<T, surrealdb::Error>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, surrealdb::Error>>,
{
    let max_retries = RETRY_MAX_ATTEMPTS;
    let base_delay = Duration::from_millis(RETRY_BASE_DELAY_MS);

    let mut last_error = None;
    for attempt in 0..=max_retries {
        if !breaker.allow_request() {
            return Err(surrealdb::Error::connection(
                "circuit breaker open — rejecting request".into(),
                None::<surrealdb::types::ConnectionError>,
            ));
        }

        if attempt > 0 {
            let delay = base_delay * 2u32.pow(attempt - 1);
            tokio::time::sleep(delay).await;
        }

        match operation().await {
            Ok(result) => {
                breaker.record_success();
                return Ok(result);
            }
            Err(e) => {
                breaker.record_failure();
                let msg = e.to_string().to_lowercase();
                let is_transient = msg.contains("lock")
                    || msg.contains("timeout")
                    || msg.contains("connection")
                    || msg.contains("retry");
                if !is_transient || attempt == max_retries {
                    return Err(e);
                }
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        surrealdb::Error::from(std::io::Error::other(
            "database operation failed without an error",
        ))
    }))
}

/// A static circuit breaker shared across database operations.
static DB_BREAKER: std::sync::LazyLock<CircuitBreaker> =
    std::sync::LazyLock::new(CircuitBreaker::new);

/// Retry a database operation with the shared circuit breaker.
pub async fn retry_shared<F, Fut, T>(operation: F) -> Result<T, surrealdb::Error>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, surrealdb::Error>>,
{
    retry_with_breaker(&DB_BREAKER, operation).await
}

/// Reset the shared circuit breaker to closed state.
pub fn reset_shared_breaker() {
    DB_BREAKER.reset();
}
