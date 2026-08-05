//! Project registration, lookup, and metadata management.
use serde::{Deserialize, Serialize};
use surrealdb::types::{RecordId, Value};

use crate::runtime::RuntimeContext;

use super::ServiceError;

/// Parameters for registering a new project.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterParams {
    pub config_project_id: Option<String>,
    pub name: String,
    pub slug: String,
    pub root_path: String,
}

/// What a `ProjectService::register` call decided to do, for user-facing
/// progress output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAction {
    Created,
    CreatedStaleConfig,
    Reused,
    ReusedStaleConfig,
    Moved,
}

/// Result returned after registering a project.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterResult {
    pub project_id: String,
    pub action: ProjectAction,
    /// Duplicate project records reconciled (deleted) at the same root_path.
    pub duplicates_removed: usize,
}

/// Summary information about a registered project.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub id: String,
    pub root_path: String,
    pub slug: String,
}

/// Parameters for updating a project's language/framework metadata.
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateMetadataParams {
    pub project_id: String,
    pub language: Option<String>,
    pub framework: Option<String>,
}

/// Service for project registration and metadata management.
#[derive(Debug)]
pub struct ProjectService;

impl ProjectService {
    /// Register a new project (or update an existing one).
    pub async fn register(
        ctx: &RuntimeContext,
        params: RegisterParams,
    ) -> Result<RegisterResult, ServiceError> {
        enum Action {
            Create(ProjectAction),
            Update(Value, ProjectAction),
        }

        let db = ctx.database()?;
        let root_path = crate::util::canonicalize_root_path(&params.root_path);
        // A project rooted at the whole filesystem is never legitimate: both a
        // literal "/" and fold artifacts of empty/"."/".."/"foo/.." collapse to
        // "/" here, so reject it outright.
        if root_path == "/" {
            return Err(ServiceError::Validation(
                "root_path must not be the filesystem root (got an empty, '.', '..', or '/' path)"
                    .into(),
            ));
        }

        let users: Vec<Value> = db.query("SELECT id FROM users LIMIT 1").await?.take(0)?;
        let owner_id = users
            .first()
            .and_then(|u| u.as_object())
            .and_then(|o| o.get("id"))
            .cloned()
            .ok_or_else(|| {
                ServiceError::Validation(
                    "No user profile found. Run `mercury-cortex profile` first.".into(),
                )
            })?;

        let by_root: Vec<Value> = db
            .query("SELECT id, root_path, updated_at FROM projects WHERE root_path = $root_path")
            .bind(("root_path", root_path.clone()))
            .await?
            .take(0)?;

        let (by_root_record, duplicates_removed) =
            reconcile_root_records(&db, params.config_project_id.as_deref(), by_root).await?;

        let by_id = if let Some(ref pid) = params.config_project_id {
            let rid = RecordId::parse_simple(pid)
                .map_err(|_| ServiceError::Validation(format!("Invalid project_id: {pid}")))?;
            let result: Vec<Value> = db
                .query("SELECT id, root_path FROM projects WHERE id = $id")
                .bind(("id", Value::RecordId(rid)))
                .await?
                .take(0)?;
            result.into_iter().next()
        } else {
            None
        };

        let config_pid = params.config_project_id.clone();
        let action = match (&params.config_project_id, &by_root_record, &by_id) {
            (None, Some(_), _) => {
                Action::Update(by_root_record.clone().unwrap(), ProjectAction::Reused)
            }
            // Config references a project that no longer exists, but this path
            // has a live record → the config is stale, the directory record is
            // authoritative. Reuse it.
            (Some(_), Some(_), None) => Action::Update(
                by_root_record.clone().unwrap(),
                ProjectAction::ReusedStaleConfig,
            ),
            (Some(_), Some(existing), _) => {
                let existing_id = existing
                    .as_object()
                    .and_then(|o| o.get("id"))
                    .ok_or_else(|| ServiceError::Internal("Project record missing id".into()))?;
                let existing_id_str = crate::util::record_thing_to_string(existing_id)
                    .ok_or_else(|| ServiceError::Internal("Project record missing id".into()))?;
                if config_pid.as_ref().is_some_and(|p| p == &existing_id_str) {
                    Action::Update(existing.clone(), ProjectAction::Reused)
                } else {
                    return Err(ServiceError::Validation(format!(
                        "Identity conflict: this directory is registered as project {existing_id_str}, \
                         but config.json identifies it as {}. \
                         The config's project exists at another location, so it was not reused here. \
                         Run `mercury-cortex project` in that other directory to keep it, or delete \
                         `.mercury-cortex/config.json` here to re-register this directory.",
                        config_pid.as_ref().unwrap()
                    )));
                }
            }
            (Some(_), None, Some(moved_record)) => {
                Action::Update(moved_record.clone(), ProjectAction::Moved)
            }
            (None, None, _) => Action::Create(ProjectAction::Created),
            (Some(_), None, None) => Action::Create(ProjectAction::CreatedStaleConfig),
        };

        let (project_id_str, action) = match action {
            Action::Create(action) => {
                let result: Vec<Value> = db
                    .query(
                        "CREATE projects SET owner_id = $owner_id, name = $name, \
                         slug = $slug, root_path = $root_path, \
                         created_at = time::now(), updated_at = time::now()",
                    )
                    .bind(("owner_id", owner_id))
                    .bind(("name", params.name))
                    .bind(("slug", params.slug))
                    .bind(("root_path", root_path.clone()))
                    .await?
                    .take(0)?;
                let record = result
                    .first()
                    .ok_or_else(|| ServiceError::Internal("Create returned no data".into()))?;
                let id = record
                    .as_object()
                    .and_then(|o| o.get("id"))
                    .ok_or_else(|| ServiceError::Internal("Created record missing id".into()))?;
                let project_id = crate::util::record_thing_to_string(id)
                    .ok_or_else(|| ServiceError::Internal("Created record missing id".into()))?;
                (project_id, action)
            }
            Action::Update(existing, action) => {
                let obj = existing.as_object().ok_or_else(|| {
                    ServiceError::Internal("Existing record not an object".into())
                })?;
                let record_id = obj
                    .get("id")
                    .ok_or_else(|| ServiceError::Internal("Existing record missing id".into()))?;
                let pid_str = crate::util::record_thing_to_string(record_id)
                    .ok_or_else(|| ServiceError::Internal("Existing record missing id".into()))?;
                db.query(
                    "UPDATE $id SET owner_id = $owner_id, name = $name, \
                     slug = $slug, root_path = $root_path, updated_at = time::now()",
                )
                .bind(("id", record_id.clone()))
                .bind(("owner_id", owner_id))
                .bind(("name", params.name))
                .bind(("slug", params.slug))
                .bind(("root_path", root_path.clone()))
                .await?
                .take::<Vec<Value>>(0)?;
                (pid_str, action)
            }
        };

        Ok(RegisterResult {
            project_id: project_id_str,
            action,
            duplicates_removed,
        })
    }

    /// Fetch a project's info by its SurrealDB record ID.
    pub async fn get_project(
        ctx: &RuntimeContext,
        project_id: &str,
    ) -> Result<ProjectInfo, ServiceError> {
        use surrealdb::types::RecordId;

        let rid = RecordId::parse_simple(project_id)
            .map_err(|_| ServiceError::Validation(format!("Invalid project_id: {project_id}")))?;

        let result: Vec<Value> = ctx
            .database()?
            .query("SELECT id, root_path, slug FROM projects WHERE id = $id")
            .bind(("id", Value::RecordId(rid)))
            .await?
            .take(0)?;

        let record = result
            .into_iter()
            .next()
            .ok_or_else(|| ServiceError::NotFound(format!("Project not found: {project_id}")))?;

        let obj = record
            .as_object()
            .ok_or_else(|| ServiceError::Internal("project record not an object".into()))?;

        let extract = |key: &str| -> Result<String, ServiceError> {
            obj.get(key)
                .and_then(|v| {
                    if let Value::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .ok_or_else(|| ServiceError::Internal(format!("project record missing {key}")))
        };

        let root_path = extract("root_path")?;
        let slug = extract("slug")?;

        Ok(ProjectInfo {
            id: project_id.to_string(),
            root_path,
            slug,
        })
    }

    /// Update language/framework metadata for a project.
    pub async fn update_metadata(
        ctx: &RuntimeContext,
        params: UpdateMetadataParams,
    ) -> Result<(), ServiceError> {
        use surrealdb::types::RecordId;

        let rid = RecordId::parse_simple(&params.project_id).map_err(|_| {
            ServiceError::Validation(format!("Invalid project_id: {}", params.project_id))
        })?;

        let mut set_clauses: Vec<&str> = vec!["updated_at = time::now()"];
        if params.language.is_some() {
            set_clauses.push("language = $language");
        }
        if params.framework.is_some() {
            set_clauses.push("framework = $framework");
        }

        let query_str = format!("UPDATE $id SET {}", set_clauses.join(", "));
        let db = ctx.database()?;
        let mut query = db.query(&query_str).bind(("id", Value::RecordId(rid)));

        if let Some(v) = params.language {
            query = query.bind(("language", v));
        }
        if let Some(v) = params.framework {
            query = query.bind(("framework", v));
        }

        query.await?;
        Ok(())
    }
}

/// Reconcile multiple project records that point at the same `root_path`.
///
/// Returns the canonical record plus the number of duplicate records deleted.
///
/// A stale DB view (e.g. two processes sharing a `SurrealKV` file) can leave
/// duplicate records behind. When more than one record exists at `root_path`,
/// the canonical record is the one referenced by `config_project_id` when that
/// id is among them; otherwise the most recently updated record wins. All other
/// records — and their `file_data` — are deleted so that re-registering
/// self-heals instead of bailing with an identity conflict.
async fn reconcile_root_records(
    db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
    config_project_id: Option<&str>,
    mut records: Vec<Value>,
) -> Result<(Option<Value>, usize), ServiceError> {
    if records.len() <= 1 {
        return Ok((records.pop(), 0));
    }

    let canonical_index = config_project_id.and_then(|pid| {
        records.iter().position(|r| {
            r.as_object()
                .and_then(|o| o.get("id"))
                .and_then(crate::util::record_thing_to_string)
                .as_deref()
                == Some(pid)
        })
    });

    let canonical = match canonical_index {
        Some(i) => records.remove(i),
        None => {
            records.sort_by_key(|b| std::cmp::Reverse(record_updated_at(b)));
            records.remove(0)
        }
    };

    let deleted_ids: Vec<Value> = records
        .iter()
        .filter_map(|r| r.as_object().and_then(|o| o.get("id")).cloned())
        .collect();

    let duplicates_removed = deleted_ids.len();

    if !deleted_ids.is_empty() {
        // `project_id` is a record-typed field, and `IN $ids` against an array
        // of record ids silently matches nothing. Delete per-id with the EQ
        // form instead (mirrors the v005 migration), so orphaned file_data
        // never leaks when duplicate projects are reconciled.
        for id in &deleted_ids {
            db.query("DELETE file_data WHERE project_id = $id")
                .bind(("id", id.clone()))
                .await?;
        }
        db.query("DELETE projects WHERE id IN $deleted_ids")
            .bind(("deleted_ids", deleted_ids))
            .await?;
    }

    Ok((Some(canonical), duplicates_removed))
}

/// Extract the `updated_at` timestamp (epoch seconds) from a project record.
fn record_updated_at(record: &Value) -> i64 {
    record
        .as_object()
        .and_then(|o| o.get("updated_at"))
        .and_then(|v| match v {
            Value::Datetime(dt) => Some(dt.timestamp()),
            _ => None,
        })
        .unwrap_or(0)
}
