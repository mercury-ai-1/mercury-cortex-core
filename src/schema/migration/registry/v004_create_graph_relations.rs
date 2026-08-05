//! Define graph relations between entities in the knowledge graph.
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

/// Define graph relations between entities in the knowledge graph.
pub async fn run(db: &Surreal<Db>) -> Result<(), surrealdb::Error> {
    db.query("DEFINE TABLE IF NOT EXISTS owns TYPE RELATION FROM users TO projects")
        .await?;
    db.query("DEFINE TABLE IF NOT EXISTS contains TYPE RELATION FROM projects TO file_data")
        .await?;
    db.query("DEFINE TABLE IF NOT EXISTS imports TYPE RELATION FROM file_data TO file_data")
        .await?;
    db.query("DEFINE TABLE IF NOT EXISTS calls TYPE RELATION FROM file_data TO file_data")
        .await?;
    db.query("DEFINE TABLE IF NOT EXISTS depends_on TYPE RELATION FROM file_data TO file_data")
        .await?;
    db.query("DEFINE TABLE IF NOT EXISTS part_of_pattern TYPE RELATION FROM file_data TO file_data")
        .await?;
    Ok(())
}
