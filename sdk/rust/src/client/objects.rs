//! Object and object-version API operations.

use reqwest::{Body, Method, Response};
use uuid::Uuid;

use crate::{
    ClientError, CreateObjectRequest, KivalClient, ListParams, ListResponse, ObjectAttachment,
    ObjectAttachmentResponse, ObjectBacklinksParams, ObjectBacklinksResponse, ObjectListItem,
    ObjectListParams, ObjectResponse, ObjectVersion, ObjectVersionResponse,
    ReuseObjectAttachmentRequest, Transport, UpdateObjectRequest, UploadObjectAttachmentParams,
    client::transport::append_list_params,
};

/// Identifier accepted by the object-version read endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectVersionIdentifier {
    /// Immutable object-version UUID.
    Id(Uuid),
    /// Monotonic object version number.
    Number(i64),
}

impl From<Uuid> for ObjectVersionIdentifier {
    fn from(value: Uuid) -> Self {
        Self::Id(value)
    }
}

impl From<i64> for ObjectVersionIdentifier {
    fn from(value: i64) -> Self {
        Self::Number(value)
    }
}

impl std::fmt::Display for ObjectVersionIdentifier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Id(version_id) => version_id.fmt(formatter),
            Self::Number(version_number) => version_number.fmt(formatter),
        }
    }
}

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Lists objects in a workspace.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_objects(
        &self,
        workspace_id: Uuid,
        params: &ObjectListParams,
    ) -> Result<ListResponse<ObjectListItem>, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url(&format!("/workspaces/{workspace_id}/objects"))?;
        append_list_params(
            &mut url,
            &ListParams { limit: params.limit, cursor: params.cursor.clone() },
        );
        {
            let mut pairs = url.query_pairs_mut();
            pairs
                .append_pair("status", params.status.as_str())
                .append_pair("order", params.order.as_str());

            if let Some(favorited) = params.favorited {
                pairs.append_pair("favorited", if favorited { "true" } else { "false" });
            }

            if let Some(pinned) = params.pinned {
                pairs.append_pair("pinned", if pinned { "true" } else { "false" });
            }
        }

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Creates an object and its initial version.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn create_object(
        &self,
        workspace_id: Uuid,
        request: CreateObjectRequest,
    ) -> Result<ObjectResponse, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(&Method::POST, &format!("/workspaces/{workspace_id}/objects"))?
            .json(&request);

        self.send_json(request).await
    }

    /// Gets an object by ID.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn get_object(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> Result<ObjectResponse, ClientError> {
        self.require_api_key()?;

        let request =
            self.request(&Method::GET, &format!("/workspaces/{workspace_id}/objects/{object_id}"))?;

        self.send_json(request).await
    }

    /// Updates an object, creating a new current version only when state changes.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn update_object(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        request: UpdateObjectRequest,
    ) -> Result<ObjectResponse, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(&Method::PATCH, &format!("/workspaces/{workspace_id}/objects/{object_id}"))?
            .json(&request);

        self.send_json(request).await
    }

    /// Archives an object.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn archive_object(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> Result<ObjectResponse, ClientError> {
        self.require_api_key()?;

        let request = self.request(
            &Method::POST,
            &format!("/workspaces/{workspace_id}/objects/{object_id}/archive"),
        )?;

        self.send_json(request).await
    }

    /// Unarchives an object.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn unarchive_object(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> Result<ObjectResponse, ClientError> {
        self.require_api_key()?;

        let request = self.request(
            &Method::POST,
            &format!("/workspaces/{workspace_id}/objects/{object_id}/unarchive"),
        )?;

        self.send_json(request).await
    }

    /// Lists visible inbound explicit edges and textual references for an object.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured or the request fails.
    pub async fn get_object_backlinks(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        params: &ObjectBacklinksParams,
    ) -> Result<ObjectBacklinksResponse, ClientError> {
        self.require_api_key()?;

        let mut url =
            self.api_url(&format!("/workspaces/{workspace_id}/objects/{object_id}/backlinks"))?;
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(limit) = params.limit {
                pairs.append_pair("limit", &limit.to_string());
            }
            if let Some(edge_cursor) = &params.edge_cursor {
                pairs.append_pair("edge_cursor", edge_cursor);
            }
            if let Some(reference_cursor) = &params.reference_cursor {
                pairs.append_pair("reference_cursor", reference_cursor);
            }
            if params.include_archived {
                pairs.append_pair("include_archived", "true");
            }
        }

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Lists object attachments.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_object_attachments(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        params: &ListParams,
    ) -> Result<ListResponse<ObjectAttachment>, ClientError> {
        self.require_api_key()?;

        let mut url =
            self.api_url(&format!("/workspaces/{workspace_id}/objects/{object_id}/attachments"))?;
        append_list_params(&mut url, params);

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Uploads in-memory bytes and creates an object attachment.
    ///
    /// Use [`Self::upload_object_attachment_body`] for streaming or otherwise non-buffered bodies.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn upload_object_attachment(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        params: &UploadObjectAttachmentParams,
        bytes: Vec<u8>,
    ) -> Result<ObjectAttachment, ClientError> {
        self.upload_object_attachment_body(workspace_id, object_id, params, Body::from(bytes)).await
    }

    /// Uploads a Reqwest body and creates an object attachment.
    ///
    /// A streaming body can be created with [`Body::wrap_stream`].
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn upload_object_attachment_body(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        params: &UploadObjectAttachmentParams,
        body: Body,
    ) -> Result<ObjectAttachment, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url(&format!(
            "/workspaces/{workspace_id}/objects/{object_id}/attachments/upload"
        ))?;
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(version_id) = params.version_id {
                pairs.append_pair("version_id", &version_id.to_string());
            }
            if let Some(name) = &params.name {
                pairs.append_pair("name", name);
            }
            if let Some(media_type) = &params.media_type {
                pairs.append_pair("media_type", media_type);
            }
            if let Some(metadata) = &params.metadata {
                pairs.append_pair("metadata", metadata);
            }
        }

        let request = self.request_url(&Method::POST, url)?.body(body);
        let response = self.send_json::<ObjectAttachmentResponse>(request).await?;
        Ok(response.attachment)
    }

    /// Creates an attachment by reusing an authorized source attachment.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn reuse_object_attachment(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        request: ReuseObjectAttachmentRequest,
    ) -> Result<ObjectAttachment, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(
                &Method::POST,
                &format!("/workspaces/{workspace_id}/objects/{object_id}/attachments/reuse"),
            )?
            .json(&request);
        let response = self.send_json::<ObjectAttachmentResponse>(request).await?;

        Ok(response.attachment)
    }

    /// Fetches object attachment content bytes by ID.
    ///
    /// This convenience method buffers the complete response. Use
    /// [`Self::get_object_attachment_content_response`] to consume large bodies incrementally.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn get_object_attachment_content(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        attachment_id: Uuid,
    ) -> Result<Vec<u8>, ClientError> {
        let response = self
            .get_object_attachment_content_response(workspace_id, object_id, attachment_id)
            .await?;
        Ok(response.bytes().await?.to_vec())
    }

    /// Fetches an object attachment response without buffering its body.
    ///
    /// Callers can consume the response incrementally with [`Response::chunk`] or
    /// [`Response::bytes_stream`].
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn get_object_attachment_content_response(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        attachment_id: Uuid,
    ) -> Result<Response, ClientError> {
        self.require_api_key()?;

        let request = self.request(
            &Method::GET,
            &format!(
                "/workspaces/{workspace_id}/objects/{object_id}/attachments/{attachment_id}/content"
            ),
        )?;
        self.send(request).await
    }

    /// Gets an object attachment by ID.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn get_object_attachment(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        attachment_id: Uuid,
    ) -> Result<ObjectAttachment, ClientError> {
        self.require_api_key()?;

        let request = self.request(
            &Method::GET,
            &format!("/workspaces/{workspace_id}/objects/{object_id}/attachments/{attachment_id}"),
        )?;
        let response = self.send_json::<ObjectAttachmentResponse>(request).await?;

        Ok(response.attachment)
    }

    /// Lists object versions.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_object_versions(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        params: &ListParams,
    ) -> Result<ListResponse<ObjectVersion>, ClientError> {
        self.require_api_key()?;

        let mut url =
            self.api_url(&format!("/workspaces/{workspace_id}/objects/{object_id}/versions"))?;
        append_list_params(&mut url, params);

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Gets an object version by immutable ID or monotonic version number.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured. Numeric identifiers
    /// smaller than one are rejected by the server.
    pub async fn get_object_version(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        version: impl Into<ObjectVersionIdentifier>,
    ) -> Result<ObjectVersion, ClientError> {
        self.require_api_key()?;

        let version = version.into();
        let request = self.request(
            &Method::GET,
            &format!("/workspaces/{workspace_id}/objects/{object_id}/versions/{version}"),
        )?;
        let response = self.send_json::<ObjectVersionResponse>(request).await?;

        Ok(response.version)
    }
}
