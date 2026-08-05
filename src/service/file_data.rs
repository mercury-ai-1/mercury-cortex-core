//! CRUD operations for indexed file metadata.
use serde::Deserialize;
use surrealdb::types::{RecordId, Value};

use crate::runtime::RuntimeContext;

use super::ServiceError;

/// Filter parameters for listing `file_data` records.
#[derive(Debug, Default, Deserialize)]
pub struct FileDataFilterParams {
    pub page: Option<u64>,
    pub limit: Option<u64>,
    pub project_id: Option<String>,
    pub path_filter: Option<String>,
    pub purpose_filter: Option<String>,
}

/// Service for querying and managing `file_data` records.
#[derive(Debug)]
pub struct FileDataService;

impl FileDataService {
    /// List `file_data` records with pagination and optional filters.
    pub async fn list(
        ctx: &RuntimeContext,
        params: &FileDataFilterParams,
    ) -> Result<Vec<Value>, ServiceError> {
        let db = ctx.database()?;
        let page = params.page.unwrap_or(1);
        let limit = params.limit.unwrap_or(50);
        let start = (page.saturating_sub(1)) * limit;

        let mut sql = String::from("SELECT * FROM file_data WHERE 1=1");
        if params.project_id.is_some() {
            sql.push_str(" AND project_id = $project_id");
        }
        if params.path_filter.is_some() {
            sql.push_str(" AND string::lowercase(path) CONTAINS string::lowercase($path_filter)");
        }
        if params.purpose_filter.is_some() {
            sql.push_str(
                " AND string::lowercase(purpose) CONTAINS string::lowercase($purpose_filter)",
            );
        }
        sql.push_str(" ORDER BY indexed_at DESC LIMIT $limit START $start");

        let mut q = db
            .query(&sql)
            .bind(("limit", limit as i64))
            .bind(("start", start as i64));

        if let Some(pid) = &params.project_id {
            let rid = RecordId::parse_simple(pid)
                .map_err(|e| ServiceError::Validation(format!("invalid project_id: {e}")))?;
            q = q.bind(("project_id", Value::RecordId(rid)));
        }
        if let Some(p) = &params.path_filter {
            q = q.bind(("path_filter", p.clone()));
        }
        if let Some(p) = &params.purpose_filter {
            q = q.bind(("purpose_filter", p.clone()));
        }

        let results: Vec<Value> = q.await?.take(0)?;
        Ok(results)
    }

    /// Fetch a single `file_data` record by its SurrealDB record ID.
    pub async fn get_by_id(ctx: &RuntimeContext, id: &str) -> Result<Value, ServiceError> {
        let db = ctx.database()?;
        let rid = RecordId::parse_simple(id)
            .map_err(|e| ServiceError::Validation(format!("invalid file_data id: {e}")))?;

        let results: Vec<Value> = db
            .query("SELECT * FROM $id")
            .bind(("id", Value::RecordId(rid)))
            .await?
            .take(0)?;

        results
            .into_iter()
            .next()
            .ok_or_else(|| ServiceError::NotFound(format!("file_data not found: {id}")))
    }

    /// Delete a `file_data` record by its path.
    pub async fn delete_by_path(ctx: &RuntimeContext, path: &str) -> Result<(), ServiceError> {
        ctx.database()?
            .query("DELETE file_data WHERE path = $path")
            .bind(("path", path))
            .await?;
        Ok(())
    }
}
