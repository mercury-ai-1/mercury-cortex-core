use serde_json::Value;

use crate::service::graph::GraphService;

use super::{CoreClient, CoreError};

/// Knowledge-graph edge queries, bound to a [`CoreClient`].
pub struct GraphClient<'a> {
    pub(crate) client: &'a CoreClient,
}

impl GraphClient<'_> {
    /// List all edges from every relation table.
    pub async fn list_all(&self) -> Result<Vec<Value>, CoreError> {
        self.client.ensure_connected().await?;
        Ok(GraphService::list_all(self.client.ctx()).await?)
    }

    /// List edges touching a given project record id.
    pub async fn list_by_project(&self, project_id: &str) -> Result<Vec<Value>, CoreError> {
        self.client.ensure_connected().await?;
        Ok(GraphService::list_by_project(self.client.ctx(), project_id).await?)
    }
}
