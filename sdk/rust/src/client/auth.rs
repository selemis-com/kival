//! Authentication and identity API operations.

use reqwest::Method;

use crate::{ClientError, KivalClient, Transport, User, UserResponse};

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Returns the user that owns the configured API key.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn whoami(&self) -> Result<User, ClientError> {
        self.require_api_key()?;

        let request = self.request(&Method::GET, "/auth/whoami")?;
        let response = self.send_json::<UserResponse>(request).await?;

        Ok(response.user)
    }
}
