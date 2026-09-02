//! Event API operations.

use reqwest::Method;
use uuid::Uuid;

use crate::{
    ClientError, Event, EventListParams, KivalClient, ListResponse, Transport,
    client::transport::append_event_params,
};

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Lists global events.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_events(
        &self,
        params: &EventListParams,
    ) -> Result<ListResponse<Event>, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url("/events")?;
        append_event_params(&mut url, params);

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Lists events in a workspace.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_workspace_events(
        &self,
        workspace_id: Uuid,
        params: &EventListParams,
    ) -> Result<ListResponse<Event>, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url(&format!("/workspaces/{workspace_id}/events"))?;
        append_event_params(&mut url, params);

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }

    /// Lists events for an object.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn list_object_events(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        params: &EventListParams,
    ) -> Result<ListResponse<Event>, ClientError> {
        self.require_api_key()?;

        let mut url =
            self.api_url(&format!("/workspaces/{workspace_id}/objects/{object_id}/events"))?;
        append_event_params(&mut url, params);

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }
}
