//! Object graph and grant API operations.

use reqwest::Method;
use uuid::Uuid;

use crate::{
    ClientError, CreateObjectEdgeRequest, CreateObjectGrantRequest, KivalClient, ListParams,
    ListResponse, ObjectEdge, ObjectEdgeResponse, ObjectGrant, ObjectGrantResponse,
    ObjectGraphParams, ObjectGraphResponse, Transport, UpdateObjectGrantRequest,
    WorkspaceGraphParams, WorkspaceGraphResponse, client::transport::append_list_params,
};

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Gets a bounded authorized graph neighborhood around an object.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn get_object_graph(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        params: &ObjectGraphParams,
    ) -> Result<ObjectGraphResponse, ClientError> {
        self.require_api_key()?;

        let mut url =
            self.api_url(&format!("/workspaces/{workspace_id}/objects/{object_id}/graph"))?;
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(depth) = params.depth {
                pairs.append_pair("depth", &depth.to_string());
            }
            pairs.append_pair("direction", params.direction.as_str());
            if let Some(max_nodes) = params.max_nodes {
                pairs.append_pair("max_nodes", &max_nodes.to_string());
            }
            if let Some(max_edges) = params.max_edges {
                pairs.append_pair("max_edges", &max_edges.to_string());
            }
            if !params.include_root {
                pairs.append_pair("include_root", "false");
            }
        }

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Gets a bounded authorized workspace graph projection.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn get_workspace_graph(
        &self,
        workspace_id: Uuid,
        params: &WorkspaceGraphParams,
    ) -> Result<WorkspaceGraphResponse, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url(&format!("/workspaces/{workspace_id}/graph"))?;
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(limit) = params.limit_nodes {
                pairs.append_pair("limit_nodes", &limit.to_string());
            }
            if let Some(limit) = params.limit_edges {
                pairs.append_pair("limit_edges", &limit.to_string());
            }
            if params.exclude_isolated {
                pairs.append_pair("exclude_isolated", "true");
            }
        }

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Lists active edges attached to an object.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_object_edges(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        params: &ListParams,
    ) -> Result<ListResponse<ObjectEdge>, ClientError> {
        self.require_api_key()?;

        let mut url =
            self.api_url(&format!("/workspaces/{workspace_id}/objects/{object_id}/edges"))?;
        append_list_params(&mut url, params);

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Creates an object edge.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn create_object_edge(
        &self,
        workspace_id: Uuid,
        request: CreateObjectEdgeRequest,
    ) -> Result<ObjectEdge, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(&Method::POST, &format!("/workspaces/{workspace_id}/edges"))?
            .json(&request);
        let response = self.send_json::<ObjectEdgeResponse>(request).await?;

        Ok(response.edge)
    }

    /// Gets an object edge by ID.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn get_object_edge(
        &self,
        workspace_id: Uuid,
        edge_id: Uuid,
    ) -> Result<ObjectEdge, ClientError> {
        self.require_api_key()?;

        let request =
            self.request(&Method::GET, &format!("/workspaces/{workspace_id}/edges/{edge_id}"))?;
        let response = self.send_json::<ObjectEdgeResponse>(request).await?;

        Ok(response.edge)
    }

    /// Revokes an object edge.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn revoke_object_edge(
        &self,
        workspace_id: Uuid,
        edge_id: Uuid,
    ) -> Result<ObjectEdge, ClientError> {
        self.require_api_key()?;

        let request = self.request(
            &Method::POST,
            &format!("/workspaces/{workspace_id}/edges/{edge_id}/revoke"),
        )?;
        let response = self.send_json::<ObjectEdgeResponse>(request).await?;

        Ok(response.edge)
    }

    /// Lists active grants on an object.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_object_grants(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        params: &ListParams,
    ) -> Result<ListResponse<ObjectGrant>, ClientError> {
        self.require_api_key()?;

        let mut url =
            self.api_url(&format!("/workspaces/{workspace_id}/objects/{object_id}/grants"))?;
        append_list_params(&mut url, params);

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Creates an object grant.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn create_object_grant(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        request: CreateObjectGrantRequest,
    ) -> Result<ObjectGrant, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(
                &Method::POST,
                &format!("/workspaces/{workspace_id}/objects/{object_id}/grants"),
            )?
            .json(&request);
        let response = self.send_json::<ObjectGrantResponse>(request).await?;

        Ok(response.grant)
    }

    /// Updates an active object grant's role.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn update_object_grant(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        grant_id: Uuid,
        request: UpdateObjectGrantRequest,
    ) -> Result<ObjectGrant, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(
                &Method::PATCH,
                &format!("/workspaces/{workspace_id}/objects/{object_id}/grants/{grant_id}"),
            )?
            .json(&request);
        let response = self.send_json::<ObjectGrantResponse>(request).await?;

        Ok(response.grant)
    }

    /// Revokes an object grant.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn revoke_object_grant(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        grant_id: Uuid,
    ) -> Result<ObjectGrant, ClientError> {
        self.require_api_key()?;

        let request = self.request(
            &Method::POST,
            &format!("/workspaces/{workspace_id}/objects/{object_id}/grants/{grant_id}/revoke"),
        )?;
        let response = self.send_json::<ObjectGrantResponse>(request).await?;

        Ok(response.grant)
    }
}
