//! Graph relation queries over the knowledge-graph edge tables.

use serde_json::Value;
use surrealdb::types::{RecordId, Value as SurrealValue};

use crate::runtime::RuntimeContext;

use super::ServiceError;

const RELATION_TABLES: &[&str] = &[
    "owns",
    "contains",
    "imports",
    "calls",
    "depends_on",
    "part_of_pattern",
];

/// Relation tables that exist in the current database.
///
/// A fresh (un-migrated) database has none of the schema tables yet, so the
/// graph queries must iterate only the tables actually present rather than
/// erroring on the missing ones.
async fn existing_tables(db: &crate::SurrealDb) -> Result<Vec<&'static str>, ServiceError> {
    let info: SurrealValue = db.query("INFO FOR DB").await?.take(0)?;

    let tables = match &info {
        SurrealValue::Object(root) => match root.get("tables") {
            Some(SurrealValue::Object(tbls)) => tbls,
            // Fresh, un-migrated DB: no tables yet → empty list, not an error.
            _ => return Ok(Vec::new()),
        },
        _ => return Ok(Vec::new()),
    };

    Ok(RELATION_TABLES
        .iter()
        .copied()
        .filter(|t| tables.contains_key(*t))
        .collect())
}

/// Service for knowledge-graph edge queries.
#[derive(Debug)]
pub struct GraphService;

impl GraphService {
    /// List all edges from every relation table.
    pub async fn list_all(ctx: &RuntimeContext) -> Result<Vec<Value>, ServiceError> {
        let db = ctx.database()?;
        let mut edges = Vec::new();
        for table in existing_tables(&db).await? {
            let rows: Vec<Value> = db
                .query(format!("SELECT * FROM {table} LIMIT 500"))
                .await?
                .take(0)?;
            edges.extend(rows);
        }
        Ok(edges)
    }

    /// List edges touching a given project record id.
    pub async fn list_by_project(
        ctx: &RuntimeContext,
        project_id: &str,
    ) -> Result<Vec<Value>, ServiceError> {
        let rid = RecordId::parse_simple(project_id)
            .map_err(|e| ServiceError::Validation(format!("invalid project_id: {e}")))?;
        let pid = SurrealValue::RecordId(rid);

        let db = ctx.database()?;
        let mut edges = Vec::new();
        for table in existing_tables(&db).await? {
            let rows: Vec<Value> = db
                .query(format!(
                    "SELECT * FROM {table} WHERE out = $project_id OR in = $project_id LIMIT 200"
                ))
                .bind(("project_id", pid.clone()))
                .await?
                .take(0)?;
            edges.extend(rows);
        }
        Ok(edges)
    }
}
