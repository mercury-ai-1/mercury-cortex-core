//! Processes AI-generated metadata JSON files into the database.
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::types::Value;
use walkdir::WalkDir;

use crate::engine::McIgnore;
use crate::engine::error::EngineError;
use crate::engine::index::hash;
use crate::engine::index::runtime_index::{FileEntry, RuntimeIndex};
use crate::util;

/// Maximum number of concurrent imports.
const MAX_CONCURRENT_IMPORTS: usize = 8;

/// Return `Some(joined)` when `relative` stays inside `root`; `None` when the
/// path is absolute, contains a `..` component, or is otherwise unsafe.
///
/// The safety check is purely lexical (see [`util::is_safe_relative_path`]);
/// symlinks inside the root are trusted.
fn join_within_root(root: &Path, relative: &str) -> Option<PathBuf> {
    if !util::is_safe_relative_path(relative) {
        return None;
    }
    Some(root.join(relative))
}

/// Metadata extracted by the AI tool for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FileMetadata {
    pub path: String,
    #[serde(rename = "type")]
    pub file_type: Option<String>,
    pub purpose: Option<String>,
    pub summary: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub exported_functions: Vec<String>,
}

/// Result of importing a single metadata file.
#[derive(Debug, serde::Serialize)]
pub struct ImportResult {
    pub path: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Processes AI-generated metadata JSON files from `.mercury-cortex/temp/`.
#[derive(Debug, Clone)]
pub(crate) struct Importer {
    db: Surreal<Db>,
    runtime_index: RuntimeIndex,
    project_id: String,
    temp_dir: PathBuf,
    project_root: PathBuf,
    mcignore: McIgnore,
}

impl Importer {
    /// Create a new `Importer` for the given project.
    #[must_use]
    pub fn new(
        db: Surreal<Db>,
        runtime_index: RuntimeIndex,
        project_id: String,
        temp_dir: PathBuf,
        project_root: PathBuf,
    ) -> Self {
        let mcignore = McIgnore::load(&project_root.join(".mercury-cortex").join(".mcignore"))
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "failed to load .mcignore for importer; using empty ignore set");
                McIgnore::default()
            });
        Self {
            db,
            runtime_index,
            project_id,
            temp_dir,
            project_root,
            mcignore,
        }
    }

    /// Validate that `temp_dir` is safe to use: non-empty, no illegal
    /// characters, and writable.
    fn validate_temp_dir(temp_dir: &Path) -> Result<(), EngineError> {
        let path_str = temp_dir.to_string_lossy();
        if path_str.is_empty() {
            return Err(EngineError::Internal(anyhow::anyhow!("temp_dir is empty")));
        }
        if path_str.contains('\0') {
            return Err(EngineError::Internal(anyhow::anyhow!(
                "temp_dir contains null bytes: {path_str}"
            )));
        }
        if temp_dir.exists() {
            let write_test = temp_dir.join(".mercury_write_test");
            match std::fs::File::create(&write_test) {
                Ok(f) => {
                    drop(f);
                    let _ = std::fs::remove_file(&write_test);
                }
                Err(e) => {
                    return Err(EngineError::Internal(anyhow::anyhow!(
                        "temp_dir is not writable at {}: {e}",
                        temp_dir.display()
                    )));
                }
            }
        }
        Ok(())
    }

    /// Import all pending metadata JSON files from the temp directory.
    ///
    /// Files are processed concurrently (up to 8 at a time) to avoid
    /// sequential DB round-trips and file hashing from dominating the
    /// wall-clock time.  Individual files are deleted on success.  When
    /// every import succeeds, the temp directory itself is removed so
    /// there is no stale state for the AI tool to clean up.
    pub async fn import_pending(&self) -> Result<Vec<ImportResult>, EngineError> {
        Self::validate_temp_dir(&self.temp_dir)?;

        if !self.temp_dir.exists() {
            return Ok(Vec::new());
        }

        let temp_dir = self.temp_dir.clone();
        let paths: Vec<PathBuf> = tokio::task::spawn_blocking(move || {
            WalkDir::new(&temp_dir)
                .into_iter()
                .filter_entry(|e| {
                    e.depth() == 0 || !e.file_name().to_str().is_some_and(|s| s.starts_with('.'))
                })
                .filter_map(std::result::Result::ok)
                .filter(|e| {
                    e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "json")
                })
                .map(|e| e.path().to_path_buf())
                .collect()
        })
        .await
        .map_err(|e| EngineError::Internal(anyhow::anyhow!("walk task join error: {e}")))?;

        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_IMPORTS));
        let mut handles = Vec::with_capacity(paths.len());
        for path in &paths {
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .expect("semaphore should not be closed");
            let db = self.db.clone();
            let runtime_index = self.runtime_index.clone();
            let project_id = self.project_id.clone();
            let project_root = self.project_root.clone();
            let path = path.clone();
            let passed_temp_dir = self.temp_dir.clone();
            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let importer =
                    Importer::new(db, runtime_index, project_id, passed_temp_dir, project_root);
                importer.import_file(&path).await
            }));
        }

        let mut results: Vec<ImportResult> = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(r) => results.push(r),
                Err(e) => results.push(ImportResult {
                    path: String::new(),
                    success: false,
                    error: Some(format!("task panicked: {e}")),
                }),
            }
        }

        // Remove the temp directory when every import succeeded
        if !results.is_empty() && results.iter().all(|r| r.success) {
            let _ = std::fs::remove_dir_all(&self.temp_dir);
        }

        Ok(results)
    }

    /// Import a single metadata JSON file.
    ///
    /// The `file_data` record is always created or updated from the
    /// AI-generated metadata, whether or not the source file exists on disk.
    /// When the source file exists, it is also hashed and registered in the
    /// runtime index; when it is missing, hashing and the runtime-index insert
    /// are skipped.
    pub async fn import_file(&self, json_path: &Path) -> ImportResult {
        let content = match std::fs::read_to_string(json_path) {
            Ok(c) => c,
            Err(e) => {
                return ImportResult {
                    path: json_path.to_string_lossy().to_string(),
                    success: false,
                    error: Some(format!("cannot read file: {e}")),
                };
            }
        };

        let metadata: FileMetadata = match serde_json::from_str(&content) {
            Ok(m) => m,
            Err(e) => {
                return ImportResult {
                    path: json_path.to_string_lossy().to_string(),
                    success: false,
                    error: Some(format!("invalid JSON: {e}")),
                };
            }
        };

        let absolute_path = match join_within_root(&self.project_root, &metadata.path) {
            Some(p) => p,
            None => {
                return ImportResult {
                    path: metadata.path.clone(),
                    success: false,
                    error: Some(format!(
                        "path escapes project root (must be relative and inside {}): {}",
                        self.project_root.display(),
                        metadata.path
                    )),
                };
            }
        };
        // Engine-side `.mcignore` enforcement: never index an excluded path.
        // The staged JSON is a benign artifact — remove it and count the
        // import as handled (not a failure).
        if self.mcignore.is_ignored(&metadata.path, false) {
            let _ = std::fs::remove_file(json_path);
            return ImportResult {
                path: metadata.path,
                success: true,
                error: None,
            };
        }
        let result = if absolute_path.is_file() {
            self.upsert_and_index(&metadata, &absolute_path)
                .await
                .map(|_| ())
        } else {
            self.upsert_metadata(&metadata).await.map(|_| ())
        };

        match result {
            Ok(()) => {
                let _ = std::fs::remove_file(json_path);
                ImportResult {
                    path: metadata.path,
                    success: true,
                    error: None,
                }
            }
            Err(e) => ImportResult {
                path: metadata.path,
                success: false,
                error: Some(format!("{e}")),
            },
        }
    }

    /// Upsert metadata and update the runtime index for an existing file.
    async fn upsert_and_index(
        &self,
        metadata: &FileMetadata,
        absolute_path: &Path,
    ) -> Result<String, EngineError> {
        let path_for_hash = absolute_path.to_path_buf();
        let hash = tokio::task::spawn_blocking(move || hash::hash_file(&path_for_hash))
            .await
            .map_err(|e| EngineError::Internal(anyhow::anyhow!("hash task join error: {e}")))??;
        let last_modified = absolute_path
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::now());

        let file_data_id = self.upsert_metadata(metadata).await?;

        self.runtime_index
            .insert(FileEntry {
                project_id: self.project_id.clone(),
                file_data_id: file_data_id.clone(),
                relative_path: metadata.path.clone(),
                content_hash: hash,
                last_modified,
            })
            .await;

        Ok(file_data_id)
    }

    async fn upsert_metadata(&self, metadata: &FileMetadata) -> Result<String, EngineError> {
        let pid = util::project_id_value(&self.project_id)?;

        // Check for an existing record first
        let existing: Vec<Value> = self
            .db
            .query(
                "SELECT * FROM file_data WHERE project_id = $project_id AND path = $path LIMIT 1",
            )
            .bind(("project_id", pid.clone()))
            .bind(("path", metadata.path.as_str()))
            .await
            .map_err(EngineError::Database)?
            .take(0)
            .map_err(EngineError::Database)?;

        if let Some(record) = existing.into_iter().next() {
            let record_id = util::record_id_to_string(&record).ok_or_else(|| {
                EngineError::Internal(anyhow::anyhow!("existing record missing id"))
            })?;

            self.db
                .query(
                    "UPDATE file_data SET \
                     type = $type, purpose = $purpose, summary = $summary, \
                     features = $features, tags = $tags, \
                     exported_functions = $exported_functions, \
                     updated_at = time::now() \
                     WHERE project_id = $project_id AND path = $path",
                )
                .bind(("project_id", pid))
                .bind(("path", metadata.path.as_str()))
                .bind(("type", metadata.file_type.clone()))
                .bind(("purpose", metadata.purpose.clone()))
                .bind(("summary", metadata.summary.clone()))
                .bind(("features", metadata.features.clone()))
                .bind(("tags", metadata.tags.clone()))
                .bind(("exported_functions", metadata.exported_functions.clone()))
                .await
                .map_err(EngineError::Database)?;

            Ok(record_id)
        } else {
            let created: Vec<Value> = self
                .db
                .query(
                    "CREATE file_data SET \
                     project_id = $project_id, path = $path, \
                     type = $type, purpose = $purpose, summary = $summary, \
                     features = $features, tags = $tags, \
                     exported_functions = $exported_functions, \
                     indexed_at = time::now(), updated_at = time::now() \
                     RETURN id",
                )
                .bind(("project_id", pid))
                .bind(("path", metadata.path.as_str()))
                .bind(("type", metadata.file_type.clone()))
                .bind(("purpose", metadata.purpose.clone()))
                .bind(("summary", metadata.summary.clone()))
                .bind(("features", metadata.features.clone()))
                .bind(("tags", metadata.tags.clone()))
                .bind(("exported_functions", metadata.exported_functions.clone()))
                .await
                .map_err(EngineError::Database)?
                .take(0)
                .map_err(EngineError::Database)?;

            let record = created.into_iter().next().ok_or_else(|| {
                EngineError::Internal(anyhow::anyhow!("CREATE returned no records"))
            })?;

            Ok(util::record_id_to_string(&record).ok_or_else(|| {
                EngineError::Internal(anyhow::anyhow!("created record missing id"))
            })?)
        }
    }
}
