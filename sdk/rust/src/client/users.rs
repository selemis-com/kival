//! User API operations.

use reqwest::Method;
use uuid::Uuid;

use crate::{
    ClientError, KivalClient, ListResponse, Transport, UpdateUserRequest, User, UserListParams,
    UserResponse,
};

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Lists users.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_users(
        &self,
        params: &UserListParams,
    ) -> Result<ListResponse<User>, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url("/users")?;

        {
            let mut pairs = url.query_pairs_mut();

            if let Some(limit) = params.limit {
                pairs.append_pair("limit", &limit.to_string());
            }

            if let Some(cursor) = &params.cursor {
                pairs.append_pair("cursor", cursor);
            }

            pairs.append_pair("status", params.status.as_str());

            if let Some(q) = &params.q {
                pairs.append_pair("q", q);
            }
        }

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Gets a user by ID.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn get_user(&self, user_id: Uuid) -> Result<User, ClientError> {
        self.require_api_key()?;

        let request = self.request(&Method::GET, &format!("/users/{user_id}"))?;
        let response = self.send_json::<UserResponse>(request).await?;

        Ok(response.user)
    }

    /// Updates a user.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn update_user(
        &self,
        user_id: Uuid,
        request: UpdateUserRequest,
    ) -> Result<User, ClientError> {
        self.require_api_key()?;

        let request = self.request(&Method::PATCH, &format!("/users/{user_id}"))?.json(&request);
        let response = self.send_json::<UserResponse>(request).await?;

        Ok(response.user)
    }

    /// Disables a user.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn disable_user(&self, user_id: Uuid) -> Result<User, ClientError> {
        self.require_api_key()?;

        let request = self.request(&Method::POST, &format!("/users/{user_id}/disable"))?;
        let response = self.send_json::<UserResponse>(request).await?;

        Ok(response.user)
    }

    /// Enables a disabled user without changing their credentials or access.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn enable_user(&self, user_id: Uuid) -> Result<User, ClientError> {
        self.require_api_key()?;

        let request = self.request(&Method::POST, &format!("/users/{user_id}/enable"))?;
        let response = self.send_json::<UserResponse>(request).await?;

        Ok(response.user)
    }
}
