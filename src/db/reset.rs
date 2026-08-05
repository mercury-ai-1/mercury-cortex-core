//! Database reset logic for the `db reset` workflow.
//!
//! Transport-agnostic domain logic: depends only on `surrealdb` and the
//! schema's canonical table list. Callers (CLI, MCP, TUI, API) own the
//! interactive and presentational layer; this module decides *what* gets
//! cleared and reports *how much* was deleted.

use surrealdb::types::Value;

use crate::schema::migration::registry::expected_tables;
use crate::service::ServiceError;

/// Which tables to clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetMode {
    /// Clear every resettable schema table.
    All,
    /// Clear only the named tables.
    Selected(Vec<String>),
}

/// Result of a completed reset.
///
/// Describes what happened at the domain level; the caller decides how to
/// present it. Each entry is a cleared table and the number of records deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetSummary {
    pub cleared: Vec<(String, u64)>,
}

/// List the schema tables that currently exist and can be reset.
///
/// Returns the intersection of the canonical [`expected_tables`] list and the
/// tables actually present in the database (`INFO FOR DB`), so callers never
/// see `_migrations` or ad-hoc tables.
pub async fn list_resettable_tables(db: &crate::SurrealDb) -> Result<Vec<String>, ServiceError> {
    let info = db.query("INFO FOR DB").await?.take::<Value>(0)?;

    let tables = match &info {
        Value::Object(root) => match root.get("tables") {
            Some(Value::Object(tbls)) => tbls,
            _ => {
                return Err(ServiceError::Internal(
                    "INFO FOR DB returned no 'tables' key".into(),
                ));
            }
        },
        _ => {
            return Err(ServiceError::Internal(
                "INFO FOR DB returned unexpected format".into(),
            ));
        }
    };

    Ok(expected_tables()
        .into_iter()
        .filter(|t| tables.contains_key(*t))
        .map(ToOwned::to_owned)
        .collect())
}

/// Count records in each of the given tables.
///
/// Nonexistent or empty tables report `0`.
pub async fn table_counts(
    db: &crate::SurrealDb,
    tables: &[String],
) -> Result<Vec<(String, u64)>, ServiceError> {
    let mut counts = Vec::with_capacity(tables.len());
    for table in tables {
        counts.push((table.clone(), count_in_table(db, table).await?));
    }
    Ok(counts)
}

/// Reset tables according to `mode`.
///
/// `All` clears every canonical schema table. `Selected(names)` clears only
/// the names that appear in the canonical list; unknown names are silently
/// dropped. Empty targets return an empty summary immediately.
///
/// Clears run inside a single transaction, so a reset is all-or-nothing:
/// any failure propagates and rolls back every change made so far.
pub async fn reset(db: &crate::SurrealDb, mode: ResetMode) -> Result<ResetSummary, ServiceError> {
    let canonical = expected_tables();
    let targets: Vec<&'static str> = match mode {
        ResetMode::All => canonical,
        ResetMode::Selected(names) => canonical
            .into_iter()
            .filter(|t| names.iter().any(|n| n == t))
            .collect(),
    };

    if targets.is_empty() {
        return Ok(ResetSummary {
            cleared: Vec::new(),
        });
    }

    let tx = db.clone().begin().await?;
    let mut cleared = Vec::with_capacity(targets.len());
    for table in targets {
        let count = count_in_transaction(&tx, table).await?;
        tx.query(format!("DELETE FROM {table}")).await?;
        cleared.push((table.to_owned(), count));
    }
    tx.commit().await?;

    Ok(ResetSummary { cleared })
}

/// Count rows in a table using the shared connection.
async fn count_in_table(db: &crate::SurrealDb, table: &str) -> Result<u64, ServiceError> {
    let rows: Vec<Value> = db
        .query(format!("SELECT count() AS n FROM {table} GROUP ALL"))
        .await?
        .take(0)?;
    Ok(extract_count(rows.first()))
}

/// Count rows in a table inside an open transaction.
async fn count_in_transaction(
    tx: &surrealdb::method::Transaction<surrealdb::engine::local::Db>,
    table: &str,
) -> Result<u64, ServiceError> {
    let rows: Vec<Value> = tx
        .query(format!("SELECT count() AS n FROM {table} GROUP ALL"))
        .await?
        .take(0)?;
    Ok(extract_count(rows.first()))
}

/// Extract the `n` value from a `SELECT count() AS n` result row.
///
/// `count()` always yields an integer, but be defensive: any malformed or
/// missing value reports `0` rather than failing the reset.
fn extract_count(row: Option<&Value>) -> u64 {
    match row {
        Some(Value::Object(obj)) => obj
            .get("n")
            .and_then(|n| n.clone().into_t::<i64>().ok())
            .unwrap_or(0) as u64,
        _ => 0,
    }
}
