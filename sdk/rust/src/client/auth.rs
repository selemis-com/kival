//! Authentication and identity API operations.

use reqwest::Method;

use crate::{ClientError, KivalClient, Transport, WhoamiResponse};

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Returns the authenticated user, effective capabilities, and delegated API-key scopes.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn whoami(&self) -> Result<WhoamiResponse, ClientError> {
        self.require_api_key()?;

        let request = self.request(&Method::GET, "/auth/whoami")?;
        self.send_json::<WhoamiResponse>(request).await
    }
}
