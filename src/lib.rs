//! Crate root — re-exports top-level modules and type aliases.
pub mod client;
pub mod db;
pub mod engine;
pub mod runtime;
pub mod schema;
pub mod service;
pub mod util;

/// Convenience type alias for a local SurrealDB database handle.
pub type SurrealDb = surrealdb::Surreal<surrealdb::engine::local::Db>;
