use std::path::Path;

use crate::db::DB_FILENAME;
use crate::db::backup;
use crate::db::backup::{BackupList, BackupResult, RestoreResult};
use crate::db::export::{self, ExportFilter, ExportSummary};
use crate::db::reset::{self, ResetMode, ResetSummary};
use crate::schema::migration::run::MigrationReport;
use crate::schema::{run_pending_with_report, verify_schema};

use super::{CoreClient, CoreError};

/// Database maintenance, bound to a [`CoreClient`].
pub struct DatabaseClient<'a> {
    pub(crate) client: &'a CoreClient,
}

impl DatabaseClient<'_> {
    /// Whether the database lock is currently held by a live process.
    pub fn lock_is_held(&self) -> Result<bool, CoreError> {
        let db_path = self.client.data_dir().join(DB_FILENAME);
        Ok(crate::db::lock_is_held(&db_path)?)
    }

    /// Create a timestamped backup of the database directory.
    pub fn backup(&self) -> Result<BackupResult, CoreError> {
        Ok(backup::backup(self.client.data_dir())?)
    }

    /// List the available timestamped backups.
    pub fn list_backups(&self) -> Result<BackupList, CoreError> {
        Ok(backup::list_backups(self.client.data_dir())?)
    }

    /// Restore the database from a backup directory.
    pub fn restore(&self, backup_path: &Path) -> Result<RestoreResult, CoreError> {
        Ok(backup::restore(self.client.data_dir(), backup_path)?)
    }

    /// List the schema tables that currently exist and can be reset.
    pub async fn list_resettable_tables(&self) -> Result<Vec<String>, CoreError> {
        self.client.ensure_connected().await?;
        let db = self.client.ctx().database()?;
        Ok(reset::list_resettable_tables(&db).await?)
    }

    /// Count records in each of the given tables.
    pub async fn table_counts(&self, tables: &[String]) -> Result<Vec<(String, u64)>, CoreError> {
        self.client.ensure_connected().await?;
        let db = self.client.ctx().database()?;
        Ok(reset::table_counts(&db, tables).await?)
    }

    /// Reset tables according to `mode`.
    pub async fn reset(&self, mode: ResetMode) -> Result<ResetSummary, CoreError> {
        self.client.ensure_connected().await?;
        let db = self.client.ctx().database()?;
        Ok(reset::reset(&db, mode).await?)
    }

    /// Apply pending schema migrations, reporting each applied migration.
    pub async fn migrate<F>(&self, mut on_applied: F) -> Result<MigrationReport, CoreError>
    where
        F: FnMut(&str),
    {
        self.client.ensure_connected().await?;
        let db = self.client.ctx().database()?;
        let mut applied = Vec::new();
        run_pending_with_report(&db, |name| {
            on_applied(name);
            applied.push(name.to_string());
        })
        .await?;
        Ok(MigrationReport { applied })
    }

    /// Verify the schema has all expected tables.
    pub async fn verify_schema(&self) -> Result<(), CoreError> {
        self.client.ensure_connected().await?;
        let db = self.client.ctx().database()?;
        Ok(verify_schema(&db).await?)
    }

    /// List the tables present in the database, excluding `_`-prefixed
    /// internal tables, sorted alphabetically.
    pub async fn list_tables(&self) -> Result<Vec<String>, CoreError> {
        self.client.ensure_connected().await?;
        let db = self.client.ctx().database()?;
        Ok(export::list_tables(&db).await?)
    }

    /// Export the given tables to `<table>.json` files in `out_dir`.
    pub async fn export(
        &self,
        tables: &[String],
        filters: &[ExportFilter],
        out_dir: &Path,
    ) -> Result<ExportSummary, CoreError> {
        self.client.ensure_connected().await?;
        let db = self.client.ctx().database()?;
        Ok(export::export_tables(&db, tables, filters, out_dir).await?)
    }
}
