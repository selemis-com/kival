//! Object commentary API operations.

use reqwest::Method;
use uuid::Uuid;

use crate::{
    ClientError, Comment, CommentListResponse, CommentMentionCandidateListResponse,
    CommentMentionCandidateParams, CommentResponse, CommentThread, CommentThreadListResponse,
    CommentThreadResponse, CreateCommentRequest, KivalClient, ListParams, Transport,
    UpdateCommentRequest, client::transport::append_list_params,
};

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Lists commentary attached to an object.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured or the request fails.
    pub async fn list_object_commentary(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        params: &ListParams,
    ) -> Result<CommentThreadListResponse, ClientError> {
        self.require_api_key()?;

        let mut url =
            self.api_url(&format!("/workspaces/{workspace_id}/objects/{object_id}/commentary"))?;
        append_list_params(&mut url, params);

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Lists a page of comments in one commentary thread.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured or the request fails.
    pub async fn list_comment_thread_comments(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        thread_id: Uuid,
        params: &ListParams,
    ) -> Result<CommentListResponse, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url(&format!(
            "/workspaces/{workspace_id}/objects/{object_id}/commentary/{thread_id}/comments"
        ))?;
        append_list_params(&mut url, params);

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Lists active users who can currently view the object and may be mentioned.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured or the request fails.
    pub async fn list_comment_mention_candidates(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        params: &CommentMentionCandidateParams,
    ) -> Result<CommentMentionCandidateListResponse, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url(&format!(
            "/workspaces/{workspace_id}/objects/{object_id}/commentary/mention-candidates"
        ))?;
        {
            let mut pairs = url.query_pairs_mut();
            if !params.q.is_empty() {
                pairs.append_pair("q", &params.q);
            }
            if let Some(limit) = params.limit {
                pairs.append_pair("limit", &limit.to_string());
            }
        }

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Creates a top-level commentary thread.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured or the request fails.
    pub async fn create_comment_thread(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        request: &CreateCommentRequest,
    ) -> Result<CommentThread, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(
                &Method::POST,
                &format!("/workspaces/{workspace_id}/objects/{object_id}/commentary"),
            )?
            .json(request);
        let response = self.send_json::<CommentThreadResponse>(request).await?;
        Ok(response.thread)
    }

    /// Replies to an open commentary thread.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured or the request fails.
    pub async fn reply_to_comment_thread(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        thread_id: Uuid,
        request: &CreateCommentRequest,
    ) -> Result<Comment, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(
                &Method::POST,
                &format!(
                    "/workspaces/{workspace_id}/objects/{object_id}/commentary/{thread_id}/replies"
                ),
            )?
            .json(request);
        let response = self.send_json::<CommentResponse>(request).await?;
        Ok(response.comment)
    }

    /// Edits a comment authored by the current user in an open thread.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured or the request fails.
    pub async fn update_comment(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        comment_id: Uuid,
        request: &UpdateCommentRequest,
    ) -> Result<Comment, ClientError> {
        self.require_api_key()?;

        let request = self
            .request(
                &Method::PATCH,
                &format!(
                    "/workspaces/{workspace_id}/objects/{object_id}/commentary/comments/{comment_id}"
                ),
            )?
            .json(request);
        let response = self.send_json::<CommentResponse>(request).await?;
        Ok(response.comment)
    }

    /// Soft-deletes a comment authored by the current user or moderated by an object admin.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured or the request fails.
    pub async fn delete_comment(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        comment_id: Uuid,
    ) -> Result<Comment, ClientError> {
        self.require_api_key()?;

        let request = self.request(
            &Method::DELETE,
            &format!(
                "/workspaces/{workspace_id}/objects/{object_id}/commentary/comments/{comment_id}"
            ),
        )?;
        let response = self.send_json::<CommentResponse>(request).await?;
        Ok(response.comment)
    }

    /// Resolves an open commentary thread.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured or the request fails.
    pub async fn resolve_comment_thread(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        thread_id: Uuid,
    ) -> Result<CommentThread, ClientError> {
        self.set_comment_thread_resolution(workspace_id, object_id, thread_id, "resolve").await
    }

    /// Reopens a resolved commentary thread.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured or the request fails.
    pub async fn reopen_comment_thread(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        thread_id: Uuid,
    ) -> Result<CommentThread, ClientError> {
        self.set_comment_thread_resolution(workspace_id, object_id, thread_id, "reopen").await
    }

    /// Sets the resolution state of a commentary thread using the given API action.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured or the request fails.
    async fn set_comment_thread_resolution(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        thread_id: Uuid,
        action: &str,
    ) -> Result<CommentThread, ClientError> {
        self.require_api_key()?;

        let request = self.request(
            &Method::POST,
            &format!(
                "/workspaces/{workspace_id}/objects/{object_id}/commentary/{thread_id}/{action}"
            ),
        )?;
        let response = self.send_json::<CommentThreadResponse>(request).await?;
        Ok(response.thread)
    }
}
