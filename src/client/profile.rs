use crate::service::profile::{ProfileData, ProfileService, UpsertParams};

use super::{CoreClient, CoreError};

/// Profile operations, bound to a [`CoreClient`].
pub struct ProfileClient<'a> {
    pub(crate) client: &'a CoreClient,
}

impl ProfileClient<'_> {
    /// Get the first user profile, if any.
    pub async fn get(&self) -> Result<Option<ProfileData>, CoreError> {
        self.client.ensure_connected().await?;
        Ok(ProfileService::get(self.client.ctx()).await?)
    }

    /// Create or update a user profile.
    pub async fn upsert(&self, params: UpsertParams) -> Result<String, CoreError> {
        self.client.ensure_connected().await?;
        Ok(ProfileService::upsert(self.client.ctx(), params).await?)
    }

    /// Check whether an email is already in use (ignoring `current_email` as self).
    pub async fn email_exists(
        &self,
        email: &str,
        current_email: Option<&str>,
    ) -> Result<bool, CoreError> {
        self.client.ensure_connected().await?;
        Ok(ProfileService::email_exists(self.client.ctx(), email, current_email).await?)
    }
}
