//! Group API operations.

use reqwest::Method;
use uuid::Uuid;

use crate::{
    ClientError, CreateGroupMembershipRequest, CreateGroupRequest, Group, GroupListParams,
    GroupMembership, GroupMembershipResponse, GroupResponse, KivalClient, ListParams, ListResponse,
    Transport, UpdateGroupMembershipRequest, UpdateGroupRequest,
    client::transport::append_list_params,
};

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Lists groups.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_groups(
        &self,
        params: &GroupListParams,
    ) -> Result<ListResponse<Group>, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url("/groups")?;
        append_list_params(
            &mut url,
            &ListParams { limit: params.limit, cursor: params.cursor.clone() },
        );
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("status", params.status.as_str());
            if let Some(q) = &params.q {
                pairs.append_pair("q", q);
            }
        }

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Gets a group by ID.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn get_group(&self, group_id: Uuid) -> Result<Group, ClientError> {
        self.require_api_key()?;

        let request = self.request(&Method::GET, &format!("/groups/{group_id}"))?;
        let response = self.send_json::<GroupResponse>(request).await?;

        Ok(response.group)
    }

    /// Creates a group.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn create_group(&self, request: CreateGroupRequest) -> Result<Group, ClientError> {
        self.require_api_key()?;

        let request = self.request(&Method::POST, "/groups")?.json(&request);
        let response = self.send_json::<GroupResponse>(request).await?;

        Ok(response.group)
    }

    /// Updates a group.
    ///
    /// `description` is tri-state: `None` leaves it unchanged, `Some(None)` clears it, and
    /// `Some(Some(value))` sets it.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn update_group(
        &self,
        group_id: Uuid,
        request: UpdateGroupRequest,
    ) -> Result<Group, ClientError> {
        self.require_api_key()?;

        let request = self.request(&Method::PATCH, &format!("/groups/{group_id}"))?.json(&request);
        let response = self.send_json::<GroupResponse>(request).await?;

        Ok(response.group)
    }

    /// Archives a group.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn archive_group(&self, group_id: Uuid) -> Result<Group, ClientError> {
        self.require_api_key()?;

        let request = self.request(&Method::POST, &format!("/groups/{group_id}/archive"))?;
        let response = self.send_json::<GroupResponse>(request).await?;

        Ok(response.group)
    }

    /// Unarchives a group.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn unarchive_group(&self, group_id: Uuid) -> Result<Group, ClientError> {
        self.require_api_key()?;

        let request = self.request(&Method::POST, &format!("/groups/{group_id}/unarchive"))?;
        let response = self.send_json::<GroupResponse>(request).await?;

        Ok(response.group)
    }

    /// Lists active memberships in a group.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_group_memberships(
        &self,
        group_id: Uuid,
        params: &ListParams,
    ) -> Result<ListResponse<GroupMembership>, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url(&format!("/groups/{group_id}/memberships"))?;
        append_list_params(&mut url, params);

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Creates a group membership.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn create_group_membership(
        &self,
        group_id: Uuid,
        request: CreateGroupMembershipRequest,
    ) -> Result<GroupMembership, ClientError> {
        self.require_api_key()?;

        let request =
            self.request(&Method::POST, &format!("/groups/{group_id}/memberships"))?.json(&request);
        let response = self.send_json::<GroupMembershipResponse>(request).await?;

        Ok(response.membership)
    }

    /// Updates an active group membership's role.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn update_group_membership(
        &self,
        group_id: Uuid,
        membership_id: Uuid,
        request: UpdateGroupMembershipRequest,
    ) -> Result<GroupMembership, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(&Method::PATCH, &format!("/groups/{group_id}/memberships/{membership_id}"))?
            .json(&request);
        let response = self.send_json::<GroupMembershipResponse>(request).await?;

        Ok(response.membership)
    }

    /// Revokes a group membership.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn revoke_group_membership(
        &self,
        group_id: Uuid,
        membership_id: Uuid,
    ) -> Result<GroupMembership, ClientError> {
        self.require_api_key()?;

        let request = self.request(
            &Method::POST,
            &format!("/groups/{group_id}/memberships/{membership_id}/revoke"),
        )?;
        let response = self.send_json::<GroupMembershipResponse>(request).await?;

        Ok(response.membership)
    }
}
