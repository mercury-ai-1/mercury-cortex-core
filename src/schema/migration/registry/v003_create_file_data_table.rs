//! Create the `file_data` table for storing indexed file metadata.
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

/// Create the `file_data` table for storing indexed file metadata.
pub async fn run(db: &Surreal<Db>) -> Result<(), surrealdb::Error> {
    db.query("DEFINE TABLE IF NOT EXISTS file_data SCHEMAFULL")
        .await?;
    db.query(
        "DEFINE FIELD IF NOT EXISTS project_id ON TABLE file_data TYPE record<projects> \
         ASSERT $value != NONE",
    )
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS path ON TABLE file_data TYPE string")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS type ON TABLE file_data TYPE string DEFAULT ''")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS purpose ON TABLE file_data TYPE string DEFAULT ''")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS summary ON TABLE file_data TYPE string DEFAULT ''")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS features ON TABLE file_data TYPE array<string> DEFAULT []")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS tags ON TABLE file_data TYPE array<string> DEFAULT []")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS exported_functions ON TABLE file_data TYPE array<string> DEFAULT []")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS hash ON TABLE file_data TYPE string DEFAULT ''")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS indexed_at ON TABLE file_data TYPE datetime")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS updated_at ON TABLE file_data TYPE datetime")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS content ON TABLE file_data TYPE string DEFAULT ''")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS previous_content ON TABLE file_data TYPE string DEFAULT ''")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS content_digest ON TABLE file_data TYPE string DEFAULT ''")
        .await?;
    db.query(
        "DEFINE INDEX IF NOT EXISTS unique_path_per_project \
         ON TABLE file_data COLUMNS project_id, path UNIQUE",
    )
    .await?;
    db.query(
        "DEFINE INDEX IF NOT EXISTS file_data_type ON TABLE file_data COLUMNS type",
    )
    .await?;
    db.query(
        "DEFINE INDEX IF NOT EXISTS file_data_purpose ON TABLE file_data COLUMNS purpose",
    )
    .await?;
    db.query(
        "DEFINE INDEX IF NOT EXISTS file_data_hash ON TABLE file_data COLUMNS hash",
    )
    .await?;
    db.query(
        "DEFINE INDEX IF NOT EXISTS file_data_path ON TABLE file_data COLUMNS path",
    )
    .await?;
    Ok(())
}
