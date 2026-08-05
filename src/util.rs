//! Shared utility functions used across modules.

use surrealdb::types::{RecordId, RecordIdKey, Value};

/// Convert a `SurrealDB` `Value::RecordId` into its canonical string
/// representation (e.g. `"projects:⟨id⟩"`) for storage in config.json.
///
/// Returns `None` when the value is not a `RecordId`.
pub fn record_thing_to_string(value: &Value) -> Option<String> {
    match value {
        Value::RecordId(rid) => {
            let key = match &rid.key {
                RecordIdKey::String(s) => s.clone(),
                RecordIdKey::Number(n) => n.to_string(),
                RecordIdKey::Uuid(u) => u.to_string(),
                _ => return None,
            };
            Some(format!("{}:{key}", rid.table))
        }
        _ => None,
    }
}

/// Convert a `SurrealDB` Value into its canonical `RecordId` string.
///
/// Accepts both a bare `Value::RecordId` and a `Value::Object` containing
/// an `"id"` field that is a `RecordId`.
///
/// Returns `None` when the value is not a `RecordId` or object with `id`.
pub fn record_id_to_string(value: &Value) -> Option<String> {
    let rid = match value {
        Value::RecordId(rid) => rid,
        Value::Object(map) => match map.get("id") {
            Some(Value::RecordId(rid)) => rid,
            _ => return None,
        },
        _ => return None,
    };
    let key = match &rid.key {
        RecordIdKey::String(s) => s.clone(),
        RecordIdKey::Number(n) => n.to_string(),
        RecordIdKey::Uuid(u) => u.to_string(),
        other => format!("{other:?}"),
    };
    Some(format!("{}:{key}", rid.table))
}

/// Parse a string like `"projects:abc123"` into a `SurrealDB` `Value::RecordId`.
///
/// Generic: accepts any `table:key` string, not just project records.
pub fn record_id_value(record: &str) -> Result<Value, anyhow::Error> {
    RecordId::parse_simple(record).map(Value::RecordId)
}

/// Backward-compatible alias for [`record_id_value`].
pub fn project_id_value(project_id: &str) -> Result<Value, anyhow::Error> {
    record_id_value(project_id)
}

/// Lexical root-path canonicalization: resolve `.`/`..` components, collapse
/// duplicate separators, strip a trailing `/` (except root), and return a
/// single canonical spelling of the path.
///
/// Pure and I/O-free — must work in the migration even when the directory no
/// longer exists, and be deterministic for tests.
pub fn canonicalize_root_path(path: &str) -> String {
    use std::path::{Component, Path};

    let out = Path::new(path)
        .components()
        .fold(std::path::PathBuf::new(), |mut acc, comp| {
            match comp {
                Component::CurDir => {}
                Component::ParentDir => {
                    acc.pop();
                }
                other => acc.push(other.as_os_str()),
            }
            acc
        });

    let mut s = out.to_string_lossy().into_owned();
    // Preserve a leading `/` if the input was absolute; strip a single
    // trailing `/` (but never the root itself).
    if !s.starts_with('/') && path.starts_with('/') {
        s.insert(0, '/');
    }
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    if s.is_empty() {
        return "/".to_string();
    }
    s
}

/// Return `true` when `relative` is a safe, purely lexical relative path.
///
/// Returns `false` for absolute paths, empty strings, `"."`, or any path
/// containing a `..` (parent) component. The check is lexical only — it does
/// not canonicalize, so a symlink inside the root that points outside the
/// root is trusted and could still be followed.
pub(crate) fn is_safe_relative_path(relative: &str) -> bool {
    use std::path::{Component, Path};

    if relative.is_empty() || relative == "." {
        return false;
    }
    let path = Path::new(relative);
    if path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_id_value_parses_table_key() {
        let v = record_id_value("projects:p1").unwrap();
        assert!(matches!(v, Value::RecordId(_)));
    }

    #[test]
    fn record_id_value_rejects_invalid_strings() {
        assert!(record_id_value("").is_err());
        assert!(record_id_value("not a record id").is_err());
    }

    #[test]
    fn project_id_value_still_parses() {
        let v = project_id_value("projects:p1").unwrap();
        assert!(matches!(v, Value::RecordId(_)));
    }
}
