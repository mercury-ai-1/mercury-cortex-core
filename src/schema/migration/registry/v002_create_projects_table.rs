//! Create the `projects` table for storing registered project metadata.
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

/// Create the `projects` table for storing registered project metadata.
pub async fn run(db: &Surreal<Db>) -> Result<(), surrealdb::Error> {
    db.query("DEFINE TABLE IF NOT EXISTS projects SCHEMAFULL")
        .await?;
    db.query(
        "DEFINE FIELD IF NOT EXISTS owner_id ON TABLE projects TYPE record<users> \
         ASSERT $value != NONE",
    )
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS name ON TABLE projects TYPE string")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS slug ON TABLE projects TYPE string")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS root_path ON TABLE projects TYPE string")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS framework ON TABLE projects TYPE string DEFAULT ''")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS language ON TABLE projects TYPE string DEFAULT ''")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS created_at ON TABLE projects TYPE datetime")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS updated_at ON TABLE projects TYPE datetime")
        .await?;
    db.query(
        "DEFINE INDEX IF NOT EXISTS unique_slug_per_owner \
         ON TABLE projects COLUMNS owner_id, slug UNIQUE",
    )
    .await?;
    db.query(
        "DEFINE INDEX IF NOT EXISTS projects_owner_id ON TABLE projects COLUMNS owner_id",
    )
    .await?;
    Ok(())
}
