//! In-memory runtime index for tracked files.
//!
//! The [`RuntimeIndex`] caches metadata for indexed files so that file
//! metadata lookups can be answered without querying the database.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use tokio::sync::RwLock;

/// Lightweight metadata for a single indexed file.
#[derive(Clone, Debug, serde::Serialize)]
pub struct FileEntry {
    /// `SurrealDB` record ID of the owning project.
    pub project_id: String,
    /// `SurrealDB` record ID of the `file_data` row.
    pub file_data_id: String,
    /// Relative path from the project root.
    pub relative_path: String,
    /// SHA-256 content hash.
    pub content_hash: String,
    /// Last modification time from the filesystem.
    pub last_modified: SystemTime,
}

/// In-memory index of tracked files, keyed by relative path.
///
/// Every operation goes through an `Arc<RwLock<>>` so the index can be
/// shared safely between the importer and (in Phase 3) MCP handlers.
#[derive(Clone, Debug, Default)]
pub struct RuntimeIndex {
    entries: Arc<RwLock<HashMap<String, FileEntry>>>,
}

impl RuntimeIndex {
    /// Create an empty runtime index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace an entry, keyed by `relative_path`.
    pub async fn insert(&self, entry: FileEntry) {
        let mut map = self.entries.write().await;
        map.insert(entry.relative_path.clone(), entry);
    }

    /// Look up an entry by its relative path.
    pub async fn get(&self, relative_path: &str) -> Option<FileEntry> {
        let map = self.entries.read().await;
        map.get(relative_path).cloned()
    }
}
