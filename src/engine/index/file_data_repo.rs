//! Trait for `file_data` table operations.
//!
//! [`FileDataRepository`] abstracts `SurrealDB` queries behind an interface so
//! that [`IndexEngine`](crate::engine::IndexEngine) can be tested with
//! in-memory mock repositories instead of a real database connection.

use async_trait::async_trait;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::types::Value;

use crate::engine::error::EngineError;

/// Repository for `file_data` table read/write operations.
///
/// Implementations wrap a `SurrealDB` connection or an in-memory data store
/// for testing.
#[async_trait]
pub trait FileDataRepository: Send + Sync {
    /// Execute a query against the `file_data` table and return raw JSON rows.
    async fn search_file_data(
        &self,
        sql: &str,
        binds: Vec<(String, Value)>,
    ) -> Result<Vec<serde_json::Value>, EngineError>;
}

/// SurrealDB-backed implementation of [`FileDataRepository`].
pub struct SurrealFileDataRepository {
    db: Surreal<Db>,
}

impl SurrealFileDataRepository {
    /// Wrap a `Surreal<Db>` connection.
    #[must_use]
    pub fn new(db: Surreal<Db>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl FileDataRepository for SurrealFileDataRepository {
    async fn search_file_data(
        &self,
        sql: &str,
        binds: Vec<(String, Value)>,
    ) -> Result<Vec<serde_json::Value>, EngineError> {
        let mut q = self.db.query(sql);
        for (k, v) in binds {
            q = q.bind((k, v));
        }
        q.await
            .map_err(EngineError::Database)?
            .take(0)
            .map_err(EngineError::Database)
    }
}
