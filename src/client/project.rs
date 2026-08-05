use std::path::Path;

use crate::service::project::{ProjectService, RegisterParams, RegisterResult};

use super::{CoreClient, CoreError};

/// Project registration and scaffolding, bound to a [`CoreClient`].
pub struct ProjectClient<'a> {
    pub(crate) client: &'a CoreClient,
}

impl ProjectClient<'_> {
    /// Register a project (create or update), returning its record id,
    /// the action taken, and any duplicate records reconciled.
    pub async fn register(&self, params: RegisterParams) -> Result<RegisterResult, CoreError> {
        self.client.ensure_connected().await?;
        Ok(ProjectService::register(self.client.ctx(), params).await?)
    }

    /// Convert a directory name to a URL-safe slug.
    pub fn slugify(&self, input: &str) -> String {
        crate::service::scaffold::slugify(input)
    }

    /// Create or update the project's `.mcignore`.
    pub fn create_or_update_mcignore(&self, path: &Path) -> Result<(), CoreError> {
        Ok(crate::service::scaffold::create_or_update_mcignore(path)?)
    }

    /// Create or update the project's `AGENTS.md`.
    pub fn create_or_update_agents_md(&self, project_root: &Path) -> Result<(), CoreError> {
        Ok(crate::service::scaffold::create_or_update_agents_md(
            project_root,
        )?)
    }

    /// Create or update `.mercury-cortex/instructions.md`.
    pub fn create_or_update_instructions_md(&self, mc_dir: &Path) -> Result<(), CoreError> {
        Ok(crate::service::scaffold::create_or_update_instructions_md(
            mc_dir,
        )?)
    }

    /// Read the `project_id` from `.mercury-cortex/config.json`.
    pub fn read_config_project_id(&self, path: &Path) -> Result<Option<String>, CoreError> {
        Ok(crate::service::scaffold::read_config_project_id(path)?)
    }

    /// Write the project config to `.mercury-cortex/config.json`.
    pub fn write_config(&self, path: &Path, project_id: &str) -> Result<(), CoreError> {
        Ok(crate::service::scaffold::write_config(path, project_id)?)
    }
}
