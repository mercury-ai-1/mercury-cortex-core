//! Migration runner — applies pending migrations and verifies the result.

use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::types::Value;

use super::registry;

/// A single registered migration with its version number and human-readable
/// name.  The actual SQL is executed by the corresponding `run` function in
/// `versions/vXXX_*.rs`.
#[derive(Debug)]
pub struct Migration {
    pub version: u64,
    pub name: &'static str,
}

/// Report of which migrations were applied by a run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// Human-readable display names of the applied migrations.
    pub applied: Vec<String>,
}

/// Apply every migration that has not yet been recorded in the `_migrations`
/// tracking table without emitting progress text.
///
/// Bootstraps the `_migrations` table (with a `UNIQUE` index on `version`)
/// on first call.  Ordering follows the version numbers in
/// [`registry::all_migrations`].
pub async fn run_pending(db: &Surreal<Db>) -> Result<(), surrealdb::Error> {
    run_pending_with_report(db, |_| {}).await
}

/// Apply every pending migration and report each applied migration name.
///
/// This is intended for CLI commands that need user-visible progress. Runtime
/// and protocol entry points should use [`run_pending`] so stdout stays clean
/// for machine-readable transports.
pub async fn run_pending_with_report<F>(
    db: &Surreal<Db>,
    mut on_applied: F,
) -> Result<(), surrealdb::Error>
where
    F: FnMut(&str),
{
    db.query("DEFINE TABLE IF NOT EXISTS _migrations SCHEMAFULL")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS version ON TABLE _migrations TYPE int")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS name ON TABLE _migrations TYPE string")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS applied_at ON TABLE _migrations TYPE datetime")
        .await?;
    db.query(
        "DEFINE INDEX IF NOT EXISTS _migrations_version_unique \
         ON TABLE _migrations COLUMNS version UNIQUE",
    )
    .await?;

    let applied: Vec<i64> = db
        .query("SELECT VALUE version FROM _migrations ORDER BY version")
        .await?
        .take(0)?;

    let migrations = registry::all_migrations();
    for m in &migrations {
        let version_int = m.version as i64;
        if applied.contains(&version_int) {
            continue;
        }
        registry::run_migration(m, db).await?;
        db.query("CREATE _migrations SET version = $v, name = $n, applied_at = time::now()")
            .bind(("v", version_int))
            .bind(("n", m.name))
            .await?;
        let display_name = format_migration_name(m.name);
        on_applied(&display_name);
    }

    Ok(())
}

fn format_migration_name(name: &str) -> String {
    name.replace('_', " ")
        .split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Assert that all expected tables exist in the database.
///
/// Queries `INFO FOR DB` and checks the `tables` key for every name in
/// [`registry::expected_tables`].  Returns a `not_found` error listing any
/// missing tables, which is useful both as a post-setup sanity check and as
/// a diagnostic when a migration fails partway through.
pub async fn verify_schema(db: &Surreal<Db>) -> Result<(), surrealdb::Error> {
    let expected_tables = registry::expected_tables();

    let info = db.query("INFO FOR DB").await?.take::<Value>(0)?;

    let tables = match &info {
        Value::Object(root) => match root.get("tables") {
            Some(Value::Object(tbls)) => tbls,
            _ => {
                return Err(surrealdb::Error::not_found(
                    "INFO FOR DB returned no 'tables' key".into(),
                    None,
                ));
            }
        },
        _ => {
            return Err(surrealdb::Error::not_found(
                "INFO FOR DB returned unexpected format".into(),
                None,
            ));
        }
    };

    let missing: Vec<&str> = expected_tables
        .iter()
        .filter(|t| !tables.contains_key(**t))
        .copied()
        .collect();

    if missing.is_empty() {
        return Ok(());
    }

    Err(surrealdb::Error::not_found(
        format!(
            "Schema is incomplete – missing tables: {}",
            missing.join(", ")
        ),
        None,
    ))
}
