//! User profile read and upsert.
use serde::{Deserialize, Serialize};
use surrealdb::types::{RecordId, Value};

use crate::runtime::RuntimeContext;

use super::ServiceError;

/// A user profile record.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileData {
    pub id: Option<String>,
    pub name: String,
    pub email: String,
    pub agent_name: String,
    #[serde(rename = "type")]
    pub profile_type: String,
    pub created_at: Option<String>,
}

/// Parameters for creating or updating a user profile.
#[derive(Debug, Serialize, Deserialize)]
pub struct UpsertParams {
    pub id: Option<String>,
    pub name: String,
    pub email: String,
    pub agent_name: String,
    #[serde(rename = "type")]
    pub profile_type: String,
}

/// Service for user profile CRUD operations.
#[derive(Debug)]
pub struct ProfileService;

impl ProfileService {
    /// Get the first user profile (if any).
    pub async fn get(ctx: &RuntimeContext) -> Result<Option<ProfileData>, ServiceError> {
        let db = ctx.database()?;

        // A fresh (un-migrated) database has no `users` table yet; treat it
        // as "no profile" rather than failing the query.
        let info: Value = db.query("INFO FOR DB").await?.take(0)?;
        let users_exist = match &info {
            Value::Object(root) => match root.get("tables") {
                Some(Value::Object(tbls)) => tbls.contains_key("users"),
                _ => false,
            },
            _ => false,
        };
        if !users_exist {
            return Ok(None);
        }

        let results: Vec<Value> = db.query("SELECT * FROM users LIMIT 1").await?.take(0)?;
        let Some(record) = results.into_iter().next() else {
            return Ok(None);
        };
        let obj = record
            .as_object()
            .ok_or_else(|| ServiceError::Internal("User record not an object".into()))?;

        let id = obj.get("id").and_then(crate::util::record_thing_to_string);
        let extract = |key: &str| -> String {
            obj.get(key)
                .and_then(|v| {
                    if let Value::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        };

        Ok(Some(ProfileData {
            id,
            name: extract("name"),
            email: extract("email"),
            agent_name: extract("agent_name"),
            profile_type: extract("type"),
            created_at: obj.get("created_at").and_then(|v| {
                serde_json::to_value(v)
                    .ok()
                    .and_then(|j| j.as_str().map(String::from))
            }),
        }))
    }

    /// Create or update a user profile.
    pub async fn upsert(
        ctx: &RuntimeContext,
        params: UpsertParams,
    ) -> Result<String, ServiceError> {
        let db = ctx.database()?;
        let set_clause = "name = $name, email = $email, agent_name = $agent_name, \
                          type = $type, updated_at = time::now()";

        let query = if let Some(ref id_str) = params.id {
            let rid = RecordId::parse_simple(id_str)
                .map_err(|_| ServiceError::Validation(format!("Invalid id: {id_str}")))?;
            db.query(format!("UPDATE $id SET {set_clause} RETURN id"))
                .bind(("id", Value::RecordId(rid)))
        } else {
            db.query(format!(
                "CREATE users SET {set_clause}, created_at = time::now() RETURN id"
            ))
        };

        let results: Vec<Value> = query
            .bind(("name", params.name))
            .bind(("email", params.email))
            .bind(("agent_name", params.agent_name))
            .bind(("type", params.profile_type))
            .await?
            .take(0)?;

        let id_val = match (params.id.as_deref(), results.first()) {
            (Some(id_str), None) => {
                return Err(ServiceError::Validation(format!(
                    "Profile not found: {id_str}"
                )));
            }
            (None, None) => {
                return Err(ServiceError::Internal("User record missing id".into()));
            }
            (_, Some(record)) => record
                .as_object()
                .and_then(|o| o.get("id"))
                .ok_or_else(|| ServiceError::Internal("User record missing id".into()))?,
        };
        crate::util::record_thing_to_string(id_val)
            .ok_or_else(|| ServiceError::Internal("User record returned an unexpected id".into()))
    }

    /// Check whether an email is already in use by a profile other than the
    /// one identified by `current_email` (which is treated as self).
    pub async fn email_exists(
        ctx: &RuntimeContext,
        email: &str,
        current_email: Option<&str>,
    ) -> Result<bool, ServiceError> {
        if Some(email) == current_email {
            return Ok(false);
        }

        let db = ctx.database()?;
        let existing: Vec<Value> = db
            .query("SELECT id FROM users WHERE email = $email")
            .bind(("email", email.to_string()))
            .await?
            .take(0)?;

        Ok(!existing.is_empty())
    }
}
