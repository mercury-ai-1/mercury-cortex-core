use serde::{Deserialize, Serialize};
use std::time::Instant;

use surrealdb::types::{RecordId, SurrealValue, Value};

use crate::engine::error::EngineError;
use crate::engine::index::engine::IndexEngine;
use tracing::debug;

const SCORE_FULL_PATH: f64 = 5.0;
const SCORE_FULL_PURPOSE: f64 = 5.0;
const SCORE_FULL_SUMMARY: f64 = 4.0;
const SCORE_FULL_FEATURES: f64 = 5.0;
const SCORE_FULL_TAGS: f64 = 4.0;
const SCORE_FULL_EXPORTED_FUNCTIONS: f64 = 4.0;
const SCORE_TOKEN_FEATURES: f64 = 4.0;
const SCORE_TOKEN_TAGS: f64 = 3.0;
const SCORE_TOKEN_PURPOSE: f64 = 3.0;
const SCORE_TOKEN_SUMMARY: f64 = 2.0;
const SCORE_TOKEN_PATH: f64 = 3.0;
const SCORE_TOKEN_EXPORTED_FUNCTIONS: f64 = 3.0;
const SCORE_TOKEN_FILE_TYPE: f64 = 1.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: Option<String>,
    pub path: Option<String>,
    pub purpose: Option<String>,
    pub features: Option<Vec<String>>,
    pub language: Option<String>,
    pub framework: Option<String>,
    pub limit: Option<usize>,
    pub search_all_projects: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub path: String,
    pub file_type: String,
    pub purpose: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub file_data_id: String,
    pub project_id: String,
    pub project_root: String,
    pub score: f64,

    #[serde(rename = "features")]
    pub(crate) features_json: String,
    #[serde(rename = "exported_functions")]
    pub(crate) exported_functions_json: String,
}

impl SearchResult {
    #[must_use]
    pub fn get_features(&self) -> Vec<String> {
        parse_json_str_array(&self.features_json)
    }

    #[must_use]
    pub fn get_exported_functions(&self) -> Vec<String> {
        parse_json_str_array(&self.exported_functions_json)
    }
}

fn parse_json_str_array(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

impl IndexEngine {
    pub(super) fn build_search_query(
        project_clause: &str,
        project_binds: Vec<(String, Value)>,
        query: &SearchQuery,
        query_tokens: &[String],
        limit: usize,
        is_prefix: bool,
    ) -> (String, Vec<(String, Value)>) {
        let path_val = query
            .path
            .as_deref()
            .map_or(Value::None, SurrealValue::into_value);
        let purpose_val = query
            .purpose
            .as_deref()
            .map_or(Value::None, SurrealValue::into_value);

        let project_where = if project_clause.is_empty() {
            "WHERE 1=1".to_string()
        } else {
            format!("WHERE {project_clause}")
        };

        let (token_sql, extra_binds) = if query_tokens.is_empty() {
            let field_sql = if is_prefix {
                "($q0 IS NONE OR \
                     string::lowercase(path) LIKE string::lowercase($q0) || '%' OR \
                     string::lowercase(type) LIKE string::lowercase($q0) || '%' OR \
                     string::lowercase(purpose) LIKE string::lowercase($q0) || '%' OR \
                     string::lowercase(summary) LIKE string::lowercase($q0) || '%' OR \
                     string::lowercase(content) LIKE string::lowercase($q0) || '%' OR \
                     features CONTAINS string::lowercase($q0) OR \
                     tags CONTAINS string::lowercase($q0) OR \
                     exported_functions CONTAINS string::lowercase($q0))"
                    .to_string()
            } else {
                "($q0 IS NONE OR \
                     string::lowercase(path) CONTAINS string::lowercase($q0) OR \
                     string::lowercase(type) CONTAINS string::lowercase($q0) OR \
                     string::lowercase(purpose) CONTAINS string::lowercase($q0) OR \
                     string::lowercase(summary) CONTAINS string::lowercase($q0) OR \
                     string::lowercase(content) CONTAINS string::lowercase($q0) OR \
                     features CONTAINS string::lowercase($q0) OR \
                     tags CONTAINS string::lowercase($q0) OR \
                     exported_functions CONTAINS string::lowercase($q0))"
                    .to_string()
            };
            (field_sql, vec![])
        } else {
            let field_sql = if is_prefix {
                let clauses: Vec<String> = query_tokens
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        format!(
                            "string::lowercase(path) LIKE string::lowercase($qt{i}) || '%' OR \
                             string::lowercase(type) LIKE string::lowercase($qt{i}) || '%' OR \
                             string::lowercase(purpose) LIKE string::lowercase($qt{i}) || '%' OR \
                             string::lowercase(summary) LIKE string::lowercase($qt{i}) || '%' OR \
                             string::lowercase(content) LIKE string::lowercase($qt{i}) || '%' OR \
                             features CONTAINS string::lowercase($qt{i}) OR \
                             tags CONTAINS string::lowercase($qt{i}) OR \
                             exported_functions CONTAINS string::lowercase($qt{i})"
                        )
                    })
                    .collect();
                clauses.join(" OR ")
            } else {
                let clauses: Vec<String> = query_tokens
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        format!(
                            "string::lowercase(path) CONTAINS string::lowercase($qt{i}) OR \
                             string::lowercase(type) CONTAINS string::lowercase($qt{i}) OR \
                             string::lowercase(purpose) CONTAINS string::lowercase($qt{i}) OR \
                             string::lowercase(summary) CONTAINS string::lowercase($qt{i}) OR \
                             string::lowercase(content) CONTAINS string::lowercase($qt{i}) OR \
                             features CONTAINS string::lowercase($qt{i}) OR \
                             tags CONTAINS string::lowercase($qt{i}) OR \
                             exported_functions CONTAINS string::lowercase($qt{i})"
                        )
                    })
                    .collect();
                clauses.join(" OR ")
            };
            (field_sql, vec![])
        };

        let sql = format!(
            "SELECT * FROM file_data {project_where} \
             AND ($path IS NONE OR string::lowercase(path) CONTAINS string::lowercase($path)) \
             AND ($purpose IS NONE OR string::lowercase(purpose) CONTAINS string::lowercase($purpose)) \
             AND ({token_sql}) \
             LIMIT {limit}",
        );

        let mut binds: Vec<(String, Value)> = vec![
            ("path".to_string(), path_val),
            ("purpose".to_string(), purpose_val),
        ];
        if query_tokens.is_empty() {
            let query_val = query
                .query
                .as_deref()
                .map_or(Value::None, SurrealValue::into_value);
            binds.push(("q0".to_string(), query_val));
        } else {
            for (i, token) in query_tokens.iter().enumerate() {
                let bind_name = format!("qt{i}");
                binds.push((bind_name, SurrealValue::into_value(token.as_str())));
            }
        }
        binds.extend(project_binds);
        binds.extend(extra_binds);

        (sql, binds)
    }

    async fn resolve_project_clause(
        &self,
        current_rid: &RecordId,
        query: &SearchQuery,
    ) -> Result<(String, Vec<(String, Value)>), EngineError> {
        if query.search_all_projects == Some(true) {
            return Ok((String::new(), vec![]));
        }

        let has_override = query.language.is_some() || query.framework.is_some();
        let lang: Option<String>;
        let fw: Option<String>;

        if has_override {
            lang = query.language.clone().filter(|s| !s.is_empty());
            fw = query.framework.clone().filter(|s| !s.is_empty());
        } else {
            let mut resp = self
                .db
                .query("SELECT language, framework FROM $id")
                .bind(("id", Value::RecordId(current_rid.clone())))
                .await
                .map_err(EngineError::Database)?;
            let rows: Vec<serde_json::Value> = resp.take(0).map_err(EngineError::Database)?;
            let row = rows.into_iter().next();
            lang = row
                .as_ref()
                .and_then(|r| r.get("language"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);
            fw = row
                .as_ref()
                .and_then(|r| r.get("framework"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);
        }

        match (lang.as_deref(), fw.as_deref()) {
            (None, None) => Ok((
                "project_id = $project_id".into(),
                vec![(
                    "project_id".to_string(),
                    Value::RecordId(current_rid.clone()),
                )],
            )),
            (Some(l), fw_opt) => {
                let mut sql = String::from(
                    "project_id IN (SELECT VALUE id FROM projects WHERE \
                     string::lowercase(language) = string::lowercase($lang)",
                );
                let mut binds: Vec<(String, Value)> =
                    vec![("lang".to_string(), SurrealValue::into_value(l.to_string()))];
                if let Some(f) = fw_opt {
                    sql.push_str(" AND string::lowercase(framework) = string::lowercase($fw)");
                    binds.push(("fw".to_string(), SurrealValue::into_value(f.to_string())));
                }
                sql.push(')');
                Ok((sql, binds))
            }
            (None, Some(f)) => Ok((
                "project_id IN (SELECT VALUE id FROM projects WHERE \
                 string::lowercase(framework) = string::lowercase($fw))"
                    .into(),
                vec![("fw".to_string(), SurrealValue::into_value(f.to_string()))],
            )),
        }
    }

    fn score_result(r: &SearchResult, q: &str, tokens: &[String]) -> f64 {
        let q_lower = q.to_lowercase();
        let mut score = 0.0_f64;

        if !q_lower.is_empty() {
            if r.path.to_lowercase().contains(&q_lower) {
                score += SCORE_FULL_PATH;
            }
            if r.purpose.to_lowercase().contains(&q_lower) {
                score += SCORE_FULL_PURPOSE;
            }
            if r.summary.to_lowercase().contains(&q_lower) {
                score += SCORE_FULL_SUMMARY;
            }
            if r.get_features()
                .iter()
                .any(|f| f.to_lowercase().contains(&q_lower))
            {
                score += SCORE_FULL_FEATURES;
            }
            if r.tags.iter().any(|t| t.to_lowercase().contains(&q_lower)) {
                score += SCORE_FULL_TAGS;
            }
            if r.get_exported_functions()
                .iter()
                .any(|ef| ef.to_lowercase().contains(&q_lower))
            {
                score += SCORE_FULL_EXPORTED_FUNCTIONS;
            }
        }

        for t in tokens {
            if t.is_empty() {
                continue;
            }
            if r.get_features()
                .iter()
                .any(|f| f.to_lowercase().contains(t))
            {
                score += SCORE_TOKEN_FEATURES;
            }
            if r.tags.iter().any(|tag| tag.to_lowercase().contains(t)) {
                score += SCORE_TOKEN_TAGS;
            }
            if r.purpose.to_lowercase().contains(t) {
                score += SCORE_TOKEN_PURPOSE;
            }
            if r.summary.to_lowercase().contains(t) {
                score += SCORE_TOKEN_SUMMARY;
            }
            if r.path.to_lowercase().contains(t) {
                score += SCORE_TOKEN_PATH;
            }
            if r.get_exported_functions()
                .iter()
                .any(|ef| ef.to_lowercase().contains(t))
            {
                score += SCORE_TOKEN_EXPORTED_FUNCTIONS;
            }
            if r.file_type.to_lowercase().contains(t) {
                score += SCORE_TOKEN_FILE_TYPE;
            }
        }

        score
    }

    async fn execute_search(
        &self,
        sql: String,
        binds: Vec<(String, Value)>,
    ) -> Result<Vec<serde_json::Value>, EngineError> {
        self.repo.search_file_data(&sql, binds).await
    }

    fn process_search_results(records: Vec<serde_json::Value>) -> Vec<SearchResult> {
        records
            .into_iter()
            .filter_map(|v| {
                let path = v.get("path")?.as_str()?.to_string();
                let file_type = v
                    .get("file_type")
                    .or_else(|| v.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let purpose = v
                    .get("purpose")
                    .and_then(|p| p.as_str())
                    .unwrap_or("")
                    .to_string();
                let summary = v
                    .get("summary")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                let features_json = v
                    .get("features")
                    .map(|f| serde_json::to_string(f).unwrap_or_default())
                    .unwrap_or_default();
                let tags = v
                    .get("tags")
                    .and_then(|t| t.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let exported_functions_json = v
                    .get("exported_functions")
                    .map(|f| serde_json::to_string(f).unwrap_or_default())
                    .unwrap_or_default();
                let file_data_id = match v.get("id") {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                    None => String::new(),
                };
                let project_id = v
                    .get("project_id")
                    .and_then(|p| match p {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Object(o) => {
                            o.get("id").and_then(|id| id.as_str()).map(String::from)
                        }
                        _ => None,
                    })
                    .unwrap_or_default();

                Some(SearchResult {
                    path,
                    file_type,
                    purpose,
                    summary,
                    features_json,
                    tags,
                    exported_functions_json,
                    file_data_id,
                    project_id,
                    project_root: String::new(),
                    score: 0.0,
                })
            })
            .collect()
    }

    pub async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>, EngineError> {
        let start = Instant::now();
        debug!(?query, "search");

        if let Some(ref q) = query.query {
            if q.len() > 1000 {
                return Err(EngineError::Internal(anyhow::anyhow!(
                    "query exceeds maximum length of 1000 characters"
                )));
            }
            if q.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
                return Err(EngineError::Internal(anyhow::anyhow!(
                    "query contains invalid control characters"
                )));
            }
        }

        let project_id = { self.project.read().await.project_id.clone() };

        let raw_query = query.query.as_deref().unwrap_or("");
        let (stripped_query, is_prefix) = if raw_query.ends_with('*') {
            (raw_query.trim_end_matches('*'), true)
        } else {
            (raw_query, false)
        };

        let query_tokens: Vec<String> = stripped_query
            .split_whitespace()
            .map(str::to_lowercase)
            .filter(|s| !s.is_empty())
            .collect();

        // Global search (`search_all_projects`) does not need an active
        // project, so parse the record id only when scoping by project.
        let rid = if query.search_all_projects == Some(true) {
            RecordId::new("file_data", "")
        } else if project_id.is_empty() {
            return Err(EngineError::Internal(anyhow::anyhow!(
                "no active project: open a project before searching"
            )));
        } else {
            RecordId::parse_simple(&project_id)
                .map_err(|e| EngineError::Internal(anyhow::anyhow!("invalid project_id: {e}")))?
        };

        let (project_clause, project_binds) = self.resolve_project_clause(&rid, query).await?;
        let limit = query.limit.unwrap_or(50);
        let (sql, binds) = Self::build_search_query(
            &project_clause,
            project_binds,
            query,
            &query_tokens,
            limit,
            is_prefix,
        );

        let records = self.execute_search(sql, binds).await?;

        let mut results = Self::process_search_results(records);

        {
            let current_root = self.project_root().await;
            let current_pid = { self.project.read().await.project_id.clone() };

            let mut root_map: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            root_map.insert(
                current_pid.clone(),
                current_root.to_string_lossy().to_string(),
            );

            let missing: Vec<String> = results
                .iter()
                .map(|r| r.project_id.clone())
                .filter(|pid| pid != &current_pid && !root_map.contains_key(pid))
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            if !missing.is_empty() {
                let ids: Vec<Value> = missing
                    .iter()
                    .filter_map(|pid| RecordId::parse_simple(pid).ok().map(Value::RecordId))
                    .collect();
                if !ids.is_empty() {
                    let mut q = self
                        .db
                        .query("SELECT id, root_path FROM projects WHERE id IN $ids");
                    q = q.bind(("ids", Value::Array(ids.into())));
                    if let Ok(mut rows) = q.await
                        && let Ok(mut projects) = rows.take::<Vec<serde_json::Value>>(0)
                    {
                        for row in projects.drain(..) {
                            if let Some(id_val) = row.get("id") {
                                let id_str = match id_val {
                                    serde_json::Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                if let Some(rp) = row.get("root_path").and_then(|v| v.as_str()) {
                                    root_map.insert(id_str, rp.to_string());
                                }
                            }
                        }
                    }
                }
            }

            for r in &mut results {
                r.project_root = root_map.get(&r.project_id).cloned().unwrap_or_default();
            }
        }

        if let Some(features) = &query.features {
            results.retain(|r| {
                features.iter().any(|f| {
                    r.get_features()
                        .iter()
                        .any(|rf| rf.to_lowercase().contains(&f.to_lowercase()))
                })
            });
        }

        if !query_tokens.is_empty() {
            results.retain(|r| {
                query_tokens.iter().any(|t| {
                    r.path.to_lowercase().contains(t)
                        || r.file_type.to_lowercase().contains(t)
                        || r.purpose.to_lowercase().contains(t)
                        || r.summary.to_lowercase().contains(t)
                        || r.get_features()
                            .iter()
                            .any(|f| f.to_lowercase().contains(t))
                        || r.tags.iter().any(|tag| tag.to_lowercase().contains(t))
                        || r.get_exported_functions()
                            .iter()
                            .any(|ef| ef.to_lowercase().contains(t))
                })
            });
        }

        if let Some(ref q) = query.query {
            for r in &mut results {
                r.score = Self::score_result(r, q, &query_tokens);
            }
            results.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        let limit = query.limit.unwrap_or(50);
        results.truncate(limit);

        debug!(
            elapsed_ms = start.elapsed().as_millis(),
            count = results.len(),
            "search complete"
        );
        Ok(results)
    }
}
