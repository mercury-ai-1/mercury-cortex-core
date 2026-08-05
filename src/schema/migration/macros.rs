//! Macro for declaring migrations from a single source of truth.

/// Generates module declarations, the migration registry list, and the
/// dispatch function from a single source of truth.
///
/// Usage:
/// ```ignore
/// declare_migrations! {
///     (1, create_users_table, v001_create_users_table),
///     (2, create_projects_table, v002_create_projects_table),
/// }
/// ```
macro_rules! declare_migrations {
    ($(($ver:expr, $name:ident, $mod:ident)),* $(,)?) => {
        $(
            mod $mod;
        )*

        pub fn all_migrations() -> Vec<super::run::Migration> {
            vec![
                $(super::run::Migration {
                    version: $ver,
                    name: stringify!($name),
                }),*
            ]
        }

        pub(crate) async fn run_migration(
            m: &super::run::Migration,
            db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
        ) -> Result<(), surrealdb::Error> {
            match m.version {
                $($ver => $mod::run(db).await,)*
                _ => unreachable!("unknown migration version {}", m.version),
            }
        }
    };
}

pub(crate) use declare_migrations;
