//! Database export logic for the `db export` workflow.
//!
//! Transport-agnostic domain logic: depends only on `surrealdb`. Callers
//! (CLI, MCP, TUI, API) own the interactive and presentational layer; this
//! module decides *what* gets exported and reports the result.

use std::path::Path;
use std::time::Instant;

use surrealdb::types::Value;

use crate::service::ServiceError;
use crate::util::record_id_value;

/// A single row filter applied to every exported table that has `field`.
///
/// Generic and database-agnostic: carries only strings, so the public API
/// never leaks SurrealDB types. Future filters (user_id, agent_name, …) are
/// just new `ExportFilter` values; no API change is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFilter {
    pub field: String,
    pub record: String,
}

impl ExportFilter {
    /// Build a filter for a record-link field from a `"table:key"` string
    /// (e.g. `ExportFilter::record("project_id", "projects:p1")`).
    ///
    /// Validates that `record` parses as a SurrealDB record id and fails fast;
    /// the `Value` conversion happens inside [`export_tables`].
    pub fn record(field: &str, record: &str) -> Result<Self, anyhow::Error> {
        record_id_value(record)?;
        Ok(Self {
            field: field.to_owned(),
            record: record.to_owned(),
        })
    }
}

/// One exported table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFile {
    pub table: String,
    pub filename: String,
    pub rows: u64,
}

/// Complete result of an export run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSummary {
    pub files: Vec<ExportFile>,
    pub skipped_filters: Vec<String>,
    pub duration_ms: u64,
}

/// Keep non-internal tables and sort alphabetically (deterministic).
fn list_tables_filter(mut names: Vec<String>) -> Vec<String> {
    names.retain(|n| !n.starts_with('_'));
    names.sort();
    names
}

/// All tables present in the database (`INFO FOR DB`), excluding
/// SurrealDB-internal tables with a `_` prefix. Returned sorted.
pub async fn list_tables(db: &crate::SurrealDb) -> Result<Vec<String>, ServiceError> {
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
    Ok(list_tables_filter(tables.keys().cloned().collect()))
}

/// Whether a table defines the given field (`INFO FOR TABLE <name>`).
pub async fn table_has_field(
    db: &crate::SurrealDb,
    table: &str,
    field: &str,
) -> Result<bool, ServiceError> {
    let info = db
        .query(format!("INFO FOR TABLE {table}"))
        .await?
        .take::<Value>(0)?;
    match &info {
        Value::Object(root) => match root.get("fields") {
            Some(Value::Object(fields)) => Ok(fields.contains_key(field)),
            _ => Ok(false),
        },
        _ => Err(ServiceError::Internal(format!(
            "INFO FOR TABLE {table} returned unexpected format"
        ))),
    }
}

/// Sort rows ascending by their serialized `id` string; rows without an `id`
/// sort last in their original relative order (stable sort).
fn sort_rows(rows: &mut [serde_json::Value]) {
    rows.sort_by_key(|row| match row.get("id").and_then(|v| v.as_str()) {
        Some(id) => (0u8, id.to_owned()),
        None => (1u8, String::new()),
    });
}

/// Write rows to `<out_dir>/<name>` as pretty JSON with a trailing newline.
/// Overwrites existing files.
fn write_json(out_dir: &Path, name: &str, rows: &[serde_json::Value]) -> Result<(), ServiceError> {
    let path = out_dir.join(name);
    let mut json = serde_json::to_string_pretty(rows)?;
    json.push('\n');
    std::fs::write(&path, json)
        .map_err(|e| ServiceError::Internal(format!("failed to write {}: {e}", path.display())))?;
    Ok(())
}

/// Create `out_dir` if missing and verify it is writable via a throwaway
/// probe file, so permission errors surface before any table is queried.
fn prepare_out_dir(out_dir: &Path) -> Result<(), ServiceError> {
    std::fs::create_dir_all(out_dir).map_err(|e| {
        ServiceError::Internal(format!(
            "cannot create output directory {}: {e}",
            out_dir.display()
        ))
    })?;
    let probe = out_dir.join(".mercury-export-probe");
    std::fs::write(&probe, b"")
        .and_then(|_| std::fs::remove_file(&probe))
        .map_err(|e| {
            ServiceError::Internal(format!(
                "output directory {} is not writable: {e}",
                out_dir.display()
            ))
        })?;
    Ok(())
}

/// Export `tables` to per-table `<table>.json` files in `out_dir`, applying
/// `filters` to every table that defines the filter's field.
///
/// Guarantees:
/// - Validation (output dir, table names, filters) completes before any
///   table is queried or any file is written.
/// - All queries run inside a single SurrealDB transaction (consistent
///   snapshot); files are written only after every query succeeds.
/// - Tables are processed alphabetically; rows are sorted by record id;
///   existing files are overwritten; empty tables produce `[]`.
pub async fn export_tables(
    db: &crate::SurrealDb,
    tables: &[String],
    filters: &[ExportFilter],
    out_dir: &Path,
) -> Result<ExportSummary, ServiceError> {
    let started = Instant::now();

    // 1. Validate output directory (create + writability probe).
    prepare_out_dir(out_dir)?;

    // 2. Validate table names against the live table list.
    let present = list_tables(db).await?;
    let mut unknown: Vec<&str> = tables
        .iter()
        .filter(|t| !present.contains(t))
        .map(String::as_str)
        .collect();
    unknown.sort();
    if !unknown.is_empty() {
        return Err(ServiceError::Validation(format!(
            "unknown table(s): {}",
            unknown.join(", ")
        )));
    }

    // 3. Validate filters: convert record strings to bound parameter values.
    let filter_params: Vec<(String, Value)> = filters
        .iter()
        .map(|f| {
            let v = record_id_value(&f.record).map_err(|e| {
                ServiceError::Validation(format!("invalid record id for '{}': {e}", f.field))
            })?;
            Ok((f.field.clone(), v))
        })
        .collect::<Result<_, ServiceError>>()?;

    let mut sorted_tables = tables.to_vec();
    sorted_tables.sort();

    // 4. Query every table inside a single transaction (consistent snapshot).
    let tx = db.clone().begin().await?;
    let mut collected: Vec<(String, Vec<serde_json::Value>)> =
        Vec::with_capacity(sorted_tables.len());
    let mut skipped_filters = Vec::new();

    for table in &sorted_tables {
        let mut clauses: Vec<(&str, &Value)> = Vec::new();
        for (field, value) in &filter_params {
            if table_has_field(db, table, field).await? {
                clauses.push((field.as_str(), value));
            } else {
                skipped_filters.push(format!("skipping filter: {table} has no {field}"));
            }
        }

        let sql = if clauses.is_empty() {
            format!("SELECT * FROM {table}")
        } else {
            let where_clause = clauses
                .iter()
                .enumerate()
                .map(|(i, (field, _))| format!("{field} = $f{i}"))
                .collect::<Vec<_>>()
                .join(" AND ");
            format!("SELECT * FROM {table} WHERE {where_clause}")
        };

        let mut q = tx.query(&sql);
        for (i, (_, value)) in clauses.iter().enumerate() {
            q = q.bind((format!("f{i}"), (*value).clone()));
        }
        let mut rows: Vec<serde_json::Value> = q.await?.take(0)?;
        sort_rows(&mut rows);
        collected.push((table.clone(), rows));
    }
    tx.commit().await?;

    // 5. Write files (only after every query succeeded).
    let mut files = Vec::with_capacity(collected.len());
    for (table, rows) in &collected {
        let filename = format!("{table}.json");
        write_json(out_dir, &filename, rows)?;
        files.push(ExportFile {
            table: table.clone(),
            filename,
            rows: rows.len() as u64,
        });
    }

    Ok(ExportSummary {
        files,
        skipped_filters,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_filter_record_validates_and_stores() {
        let f = ExportFilter::record("project_id", "projects:p1").unwrap();
        assert_eq!(f.field, "project_id");
        assert_eq!(f.record, "projects:p1");
    }

    #[test]
    fn export_filter_record_rejects_invalid_record_id() {
        assert!(ExportFilter::record("project_id", "not a record id").is_err());
    }

    #[test]
    fn list_tables_filter_keeps_public_tables() {
        let names = vec![
            "_migrations".to_string(),
            "file_data".to_string(),
            "projects".to_string(),
        ];
        let filtered: Vec<String> = list_tables_filter(names.clone());
        assert_eq!(
            filtered,
            vec!["file_data".to_string(), "projects".to_string()]
        );
    }

    #[test]
    fn list_tables_filter_sorts_alphabetically() {
        let names = vec![
            "users".to_string(),
            "projects".to_string(),
            "file_data".to_string(),
        ];
        let filtered = list_tables_filter(names);
        assert_eq!(
            filtered,
            vec![
                "file_data".to_string(),
                "projects".to_string(),
                "users".to_string()
            ]
        );
    }

    #[test]
    fn sort_rows_orders_by_id_with_missing_ids_last() {
        use serde_json::json;
        let mut rows = vec![
            json!({"id": "projects:p2", "name": "b"}),
            json!({"name": "no-id"}),
            json!({"id": "projects:p1", "name": "a"}),
        ];
        sort_rows(&mut rows);
        let ids: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| r.get("id").cloned().unwrap_or(serde_json::Value::Null))
            .collect();
        assert_eq!(
            ids,
            vec![json!("projects:p1"), json!("projects:p2"), json!(null)]
        );
    }

    #[test]
    fn sort_rows_is_stable_for_missing_ids() {
        use serde_json::json;
        let mut rows = vec![json!({"name": "x"}), json!({"name": "y"})];
        sort_rows(&mut rows);
        assert_eq!(rows[0]["name"], "x");
        assert_eq!(rows[1]["name"], "y");
    }

    #[test]
    fn write_json_is_pretty_with_trailing_newline() {
        use serde_json::json;
        let dir = tempfile::TempDir::new().unwrap();
        write_json(dir.path(), "t.json", &[json!({"a": 1})]).unwrap();
        let content = std::fs::read_to_string(dir.path().join("t.json")).unwrap();
        assert!(content.ends_with('\n'));
        assert!(content.contains("{\n"));
    }

    #[test]
    fn prepare_out_dir_creates_and_probes() {
        let dir = tempfile::TempDir::new().unwrap();
        let out = dir.path().join("nested/out");
        prepare_out_dir(&out).unwrap();
        assert!(out.exists());
    }
}
