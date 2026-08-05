//! Migration registry — declares all known migrations.
use super::macros::declare_migrations;

declare_migrations! {
    (1, create_users_table, v001_create_users_table),
    (2, create_projects_table, v002_create_projects_table),
    (3, create_file_data_table, v003_create_file_data_table),
    (4, create_graph_relations, v004_create_graph_relations),
    (5, add_unique_root_path, v005_add_unique_root_path),
}

/// Return the set of table names expected to exist after all migrations have
/// been applied.  This is the source of truth for [`verify_schema`].
///
/// [`verify_schema`]: super::run::verify_schema
#[must_use]
pub fn expected_tables() -> Vec<&'static str> {
    vec![
        "users",
        "projects",
        "file_data",
        "owns",
        "contains",
        "imports",
        "calls",
        "depends_on",
        "part_of_pattern",
    ]
}
