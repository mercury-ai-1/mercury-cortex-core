//! Canonicalize `root_path`, pre-dedup duplicates, and add a UNIQUE index.

use std::collections::HashMap;

use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use surrealdb::types::Value;

use crate::util::canonicalize_root_path;

/// Canonicalize stored `root_path` values, delete duplicate projects that
/// share a root, then enforce uniqueness going forward.
pub async fn run(db: &Surreal<Db>) -> Result<(), surrealdb::Error> {
    // 1. Canonicalize stored root_path values.
    let projects: Vec<Value> = db
        .query("SELECT id, root_path, updated_at FROM projects")
        .await?
        .take(0)?;

    let mut by_root: HashMap<String, Vec<Value>> = HashMap::new();
    for p in projects {
        let obj = match p.as_object() {
            Some(o) => o,
            None => continue,
        };
        let Some(root) = obj.get("root_path").and_then(|v| v.as_string()) else {
            continue;
        };
        let canonical = canonicalize_root_path(root);
        if canonical != *root
            && let Some(id) = obj.get("id")
        {
            db.query("UPDATE $id SET root_path = $canonical")
                .bind(("id", id.clone()))
                .bind(("canonical", canonical.clone()))
                .await?;
        }
        by_root.entry(canonical).or_default().push(p);
    }

    // 2. Pre-dedup: keep the most recently updated record per canonical root,
    // and drop any record whose canonical root is the filesystem root — a
    // whole-filesystem project is never legitimate and register() rejects it.
    let mut deleted: Vec<Value> = Vec::new();
    for (canonical_root, mut group) in by_root {
        if canonical_root == "/" {
            for p in &group {
                if let Some(id) = p.as_object().and_then(|o| o.get("id")) {
                    deleted.push(id.clone());
                }
            }
            continue;
        }
        if group.len() <= 1 {
            continue;
        }
        group.sort_by_key(|b| {
            b.as_object()
                .and_then(|o| o.get("updated_at"))
                .and_then(|v| v.as_datetime().map(|d| d.timestamp()))
                .unwrap_or(0)
        });
        for dup in &group[..group.len() - 1] {
            if let Some(id) = dup.as_object().and_then(|o| o.get("id")) {
                deleted.push(id.clone());
            }
        }
    }
    if !deleted.is_empty() {
        for id in &deleted {
            db.query("DELETE file_data WHERE project_id = $id")
                .bind(("id", id.clone()))
                .await?;
        }
        db.query("DELETE projects WHERE id IN $ids")
            .bind(("ids", deleted))
            .await?;
    }

    // 3. Unique index.
    db.query(
        "DEFINE INDEX IF NOT EXISTS unique_root_path ON TABLE projects COLUMNS root_path UNIQUE",
    )
    .await?;
    Ok(())
}
