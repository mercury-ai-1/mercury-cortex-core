//! Create the `users` table for storing user profiles.
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

/// Create the `users` table for storing user profiles.
pub async fn run(db: &Surreal<Db>) -> Result<(), surrealdb::Error> {
    db.query("DEFINE TABLE IF NOT EXISTS users SCHEMAFULL")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS name ON TABLE users TYPE string")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS email ON TABLE users TYPE string")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS type ON TABLE users TYPE string")
        .await?;
    db.query(
        "DEFINE FIELD IF NOT EXISTS agent_name ON TABLE users TYPE string \
         ASSERT $value != NONE \
         AND string::matches($value, '^agent-[a-z0-9][a-z0-9-]*$')",
    )
    .await?;
    db.query("DEFINE FIELD IF NOT EXISTS created_at ON TABLE users TYPE datetime")
        .await?;
    db.query("DEFINE FIELD IF NOT EXISTS updated_at ON TABLE users TYPE datetime")
        .await?;
    db.query(
        "DEFINE INDEX IF NOT EXISTS unique_email ON TABLE users COLUMNS email UNIQUE",
    )
    .await?;
    db.query(
        "DEFINE INDEX IF NOT EXISTS unique_agent_name ON TABLE users COLUMNS agent_name UNIQUE",
    )
    .await?;
    Ok(())
}
