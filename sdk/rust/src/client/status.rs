//! Public API operations.

use reqwest::Method;

use crate::{ClientError, KivalClient, StatusResponse, Transport};

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Checks server health.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the server returns an error.
    pub async fn health(&self) -> Result<StatusResponse, ClientError> {
        let request = self.public_request(&Method::GET, "/healthz")?;
        self.send_json(request).await
    }

    /// Checks server readiness.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the server is not ready, or the response cannot be
    /// decoded.
    pub async fn ready(&self) -> Result<StatusResponse, ClientError> {
        let request = self.public_request(&Method::GET, "/readyz")?;
        self.send_json(request).await
    }
}
