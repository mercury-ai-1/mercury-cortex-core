use std::path::{Path, PathBuf};
use std::sync::Arc;

use surrealdb::Surreal;
use surrealdb::engine::local::Db;

use crate::engine::error::EngineError;
use crate::engine::index::cache::FileMetadataCache;
use crate::engine::index::file_data_repo::FileDataRepository;
use crate::engine::index::importer::Importer;
use crate::engine::index::runtime_index::RuntimeIndex;

#[derive(Debug)]
pub(crate) struct ProjectState {
    pub project_id: String,
    pub project_root: PathBuf,
    pub(super) importer: Importer,
}

impl ProjectState {
    pub fn new(
        db: Surreal<Db>,
        runtime_index: RuntimeIndex,
        project_id: &str,
        project_root: &Path,
    ) -> Self {
        let temp_dir = project_root.join(".mercury-cortex").join("temp");
        Self {
            project_id: project_id.to_string(),
            project_root: project_root.to_path_buf(),
            importer: Importer::new(
                db,
                runtime_index,
                project_id.to_string(),
                temp_dir,
                project_root.to_path_buf(),
            ),
        }
    }
}

pub struct IndexEngine {
    pub runtime_index: RuntimeIndex,

    pub(super) db: Surreal<Db>,

    pub(super) repo: Arc<dyn FileDataRepository>,

    pub(super) project: tokio::sync::RwLock<ProjectState>,

    pub(super) metadata_cache: FileMetadataCache,
}

impl IndexEngine {
    #[must_use]
    pub fn new(
        repo: Arc<dyn FileDataRepository>,
        db: Surreal<Db>,
        project_id: &str,
        project_root: &Path,
    ) -> Self {
        let runtime_index = RuntimeIndex::new();
        let project =
            ProjectState::new(db.clone(), runtime_index.clone(), project_id, project_root);
        Self {
            runtime_index,
            db,
            repo,
            project: tokio::sync::RwLock::new(project),
            metadata_cache: FileMetadataCache::new(),
        }
    }

    pub async fn set_project(&self, project_id: String, project_root: PathBuf) {
        let mut p = self.project.write().await;
        *p = ProjectState::new(
            self.db.clone(),
            self.runtime_index.clone(),
            &project_id,
            &project_root,
        );
    }

    pub async fn project_root(&self) -> PathBuf {
        self.project.read().await.project_root.clone()
    }

    pub async fn get_file_metadata(
        &self,
        path: &str,
    ) -> Option<crate::engine::index::runtime_index::FileEntry> {
        if let Some(entry) = self.metadata_cache.get(path).await {
            return Some(entry);
        }
        if let Some(entry) = self.runtime_index.get(path).await {
            self.metadata_cache
                .put(path.to_owned(), entry.clone())
                .await;
            return Some(entry);
        }
        None
    }

    pub async fn import_pending(
        &self,
    ) -> Result<Vec<crate::engine::index::importer::ImportResult>, EngineError> {
        let importer = { self.project.read().await.importer.clone() };
        importer.import_pending().await
    }
}
