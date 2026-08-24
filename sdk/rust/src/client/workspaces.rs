//! Workspace API operations.

use reqwest::Method;
use uuid::Uuid;

use crate::{
    ClientError, CreateWorkspaceGroupRequest, CreateWorkspaceMembershipRequest, KivalClient,
    ListParams, ListResponse, Transport, UpdateWorkspaceMembershipRequest, UpdateWorkspaceRequest,
    Workspace, WorkspaceGroup, WorkspaceGroupListParams, WorkspaceGroupResponse,
    WorkspaceListParams, WorkspaceMembership, WorkspaceMembershipResponse, WorkspaceResponse,
    client::transport::append_list_params,
};

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Lists workspaces visible to the authenticated user.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_workspaces(
        &self,
        params: &WorkspaceListParams,
    ) -> Result<ListResponse<Workspace>, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url("/workspaces")?;

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

            if let Some(pinned) = params.pinned {
                pairs.append_pair("pinned", if pinned { "true" } else { "false" });
            }
        }

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Gets a workspace by ID.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn get_workspace(&self, workspace_id: Uuid) -> Result<Workspace, ClientError> {
        self.require_api_key()?;

        let request = self.request(&Method::GET, &format!("/workspaces/{workspace_id}"))?;
        let response = self.send_json::<WorkspaceResponse>(request).await?;

        Ok(response.workspace)
    }

    /// Updates a workspace.
    ///
    /// `description` is tri-state:
    ///
    /// - `None`: leave unchanged
    /// - `Some(None)`: clear description
    /// - `Some(Some(value))`: set description
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn update_workspace(
        &self,
        workspace_id: Uuid,
        request: UpdateWorkspaceRequest,
    ) -> Result<Workspace, ClientError> {
        self.require_api_key()?;

        let request =
            self.request(&Method::PATCH, &format!("/workspaces/{workspace_id}"))?.json(&request);
        let response = self.send_json::<WorkspaceResponse>(request).await?;

        Ok(response.workspace)
    }

    /// Archives a workspace.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn archive_workspace(&self, workspace_id: Uuid) -> Result<Workspace, ClientError> {
        self.require_api_key()?;

        let request =
            self.request(&Method::POST, &format!("/workspaces/{workspace_id}/archive"))?;
        let response = self.send_json::<WorkspaceResponse>(request).await?;

        Ok(response.workspace)
    }

    /// Unarchives a workspace.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn unarchive_workspace(&self, workspace_id: Uuid) -> Result<Workspace, ClientError> {
        self.require_api_key()?;

        let request =
            self.request(&Method::POST, &format!("/workspaces/{workspace_id}/unarchive"))?;
        let response = self.send_json::<WorkspaceResponse>(request).await?;

        Ok(response.workspace)
    }

    /// Lists active workspace memberships.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_workspace_memberships(
        &self,
        workspace_id: Uuid,
        params: &ListParams,
    ) -> Result<ListResponse<WorkspaceMembership>, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url(&format!("/workspaces/{workspace_id}/memberships"))?;
        append_list_params(&mut url, params);

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Creates a workspace membership.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn create_workspace_membership(
        &self,
        workspace_id: Uuid,
        request: CreateWorkspaceMembershipRequest,
    ) -> Result<WorkspaceMembership, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(&Method::POST, &format!("/workspaces/{workspace_id}/memberships"))?
            .json(&request);
        let response = self.send_json::<WorkspaceMembershipResponse>(request).await?;

        Ok(response.membership)
    }

    /// Updates an active workspace membership's role.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn update_workspace_membership(
        &self,
        workspace_id: Uuid,
        membership_id: Uuid,
        request: UpdateWorkspaceMembershipRequest,
    ) -> Result<WorkspaceMembership, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(
                &Method::PATCH,
                &format!("/workspaces/{workspace_id}/memberships/{membership_id}"),
            )?
            .json(&request);
        let response = self.send_json::<WorkspaceMembershipResponse>(request).await?;

        Ok(response.membership)
    }

    /// Revokes a workspace membership.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn revoke_workspace_membership(
        &self,
        workspace_id: Uuid,
        membership_id: Uuid,
    ) -> Result<WorkspaceMembership, ClientError> {
        self.require_api_key()?;

        let request = self.request(
            &Method::POST,
            &format!("/workspaces/{workspace_id}/memberships/{membership_id}/revoke"),
        )?;
        let response = self.send_json::<WorkspaceMembershipResponse>(request).await?;

        Ok(response.membership)
    }

    /// Lists active workspace group links.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_workspace_groups(
        &self,
        workspace_id: Uuid,
        params: &WorkspaceGroupListParams,
    ) -> Result<ListResponse<WorkspaceGroup>, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url(&format!("/workspaces/{workspace_id}/groups"))?;
        append_list_params(
            &mut url,
            &ListParams { limit: params.limit, cursor: params.cursor.clone() },
        );
        url.query_pairs_mut().append_pair("status", params.status.as_str());

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Links a group to a workspace.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn create_workspace_group(
        &self,
        workspace_id: Uuid,
        request: CreateWorkspaceGroupRequest,
    ) -> Result<WorkspaceGroup, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(&Method::POST, &format!("/workspaces/{workspace_id}/groups"))?
            .json(&request);
        let response = self.send_json::<WorkspaceGroupResponse>(request).await?;

        Ok(response.workspace_group)
    }

    /// Archives a workspace group link.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn archive_workspace_group(
        &self,
        workspace_id: Uuid,
        group_id: Uuid,
    ) -> Result<WorkspaceGroup, ClientError> {
        self.require_api_key()?;

        let request = self.request(
            &Method::POST,
            &format!("/workspaces/{workspace_id}/groups/{group_id}/archive"),
        )?;
        let response = self.send_json::<WorkspaceGroupResponse>(request).await?;

        Ok(response.workspace_group)
    }

    /// Unarchives a workspace group link.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn unarchive_workspace_group(
        &self,
        workspace_id: Uuid,
        group_id: Uuid,
    ) -> Result<WorkspaceGroup, ClientError> {
        self.require_api_key()?;

        let request = self.request(
            &Method::POST,
            &format!("/workspaces/{workspace_id}/groups/{group_id}/unarchive"),
        )?;
        let response = self.send_json::<WorkspaceGroupResponse>(request).await?;

        Ok(response.workspace_group)
    }
}
