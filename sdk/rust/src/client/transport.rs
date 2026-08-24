//! HTTP transport abstraction and request helpers.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use reqwest::{Client, Method, Request, RequestBuilder, Response, StatusCode, header::RETRY_AFTER};
use serde::de::DeserializeOwned;
use tower::{Service, ServiceExt, util::BoxCloneSyncService};
use url::Url;

use crate::{
    API_PREFIX, ApiError, ApiErrorKind, ApiErrorResponse, ClientError, EventListParams, EventOrder,
    KivalClient, ListParams,
};

/// Maximum number of response-body bytes retained in an API error.
pub(super) const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// Future returned by Kival transport services.
pub type BoxTransportFuture =
    Pin<Box<dyn Future<Output = Result<Response, ClientError>> + Send + 'static>>;

/// Cloneable, type-erased Kival transport.
pub type BoxTransport = BoxCloneSyncService<Request, Response, ClientError>;

/// Tower service used by [`KivalClient`] to execute classified Kival requests.
pub trait Transport:
    Service<Request, Response = Response, Error = ClientError, Future: Send + 'static>
    + Clone
    + Send
    + Sync
    + 'static
{
}

impl<T> Transport for T where
    T: Service<Request, Response = Response, Error = ClientError, Future: Send + 'static>
        + Clone
        + Send
        + Sync
        + 'static
{
}

/// Reqwest-backed raw HTTP transport.
#[derive(Debug, Clone)]
pub struct HttpTransport {
    /// Shared underlying Reqwest client.
    http: Client,
}

impl HttpTransport {
    /// Creates a Reqwest-backed transport.
    #[must_use]
    pub const fn new(http: Client) -> Self {
        Self { http }
    }
}

impl Service<Request> for HttpTransport {
    type Response = Response;
    type Error = ClientError;
    type Future = BoxTransportFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let http = self.http.clone();
        Box::pin(async move { Ok(http.execute(request).await?) })
    }
}

/// Service that turns non-successful Kival HTTP responses into [`ClientError::Api`].
///
/// User-provided middleware is applied around this service, so retries, metrics, and circuit
/// breakers observe Kival API failures rather than raw successful transport responses.
#[derive(Debug, Clone)]
pub struct ResponseTransport<S> {
    /// Inner raw HTTP service.
    inner: S,
}

impl<S> ResponseTransport<S> {
    /// Wraps a raw HTTP service with Kival response classification.
    #[must_use]
    pub const fn new(inner: S) -> Self {
        Self { inner }
    }

    /// Returns the wrapped raw HTTP service.
    #[must_use]
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S> Service<Request> for ResponseTransport<S>
where
    S: Service<Request, Response = Response>,
    S::Error: Into<ClientError>,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = ClientError;
    type Future = BoxTransportFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.inner.poll_ready(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error.into())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let response = self.inner.call(request);
        Box::pin(async move {
            let response = response.await.map_err(Into::into)?;
            ensure_success(response).await
        })
    }
}

/// Service that normalizes a middleware stack's outer error into [`ClientError`].
#[derive(Debug, Clone)]
pub struct MapTransportError<S> {
    /// Inner middleware stack.
    inner: S,
}

impl<S> MapTransportError<S> {
    /// Creates an error-normalizing transport wrapper.
    #[must_use]
    pub const fn new(inner: S) -> Self {
        Self { inner }
    }

    /// Returns the wrapped middleware stack.
    #[must_use]
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S> Service<Request> for MapTransportError<S>
where
    S: Service<Request, Response = Response>,
    S::Error: Into<ClientError>,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = ClientError;
    type Future = BoxTransportFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.inner.poll_ready(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error.into())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let response = self.inner.call(request);
        Box::pin(async move { response.await.map_err(Into::into) })
    }
}

/// Appends standard list options to a URL query string.
pub fn append_list_params(url: &mut Url, params: &ListParams) {
    let mut pairs = url.query_pairs_mut();

    if let Some(limit) = params.limit {
        pairs.append_pair("limit", &limit.to_string());
    }

    if let Some(cursor) = &params.cursor {
        pairs.append_pair("cursor", cursor);
    }
}

/// Appends event list query parameters to a URL query string.
pub fn append_event_params(url: &mut Url, params: &EventListParams) {
    let mut pairs = url.query_pairs_mut();

    if let Some(limit) = params.limit {
        pairs.append_pair("limit", &limit.to_string());
    }

    if let Some(after_sequence) = params.after_sequence {
        pairs.append_pair("after_sequence", &after_sequence.to_string());
    }

    if let Some(before_sequence) = params.before_sequence {
        pairs.append_pair("before_sequence", &before_sequence.to_string());
    }

    if params.order == EventOrder::Desc {
        pairs.append_pair("order", params.order.as_str());
    }

    if let Some(event_kind) = &params.event_kind {
        pairs.append_pair("event_kind", event_kind);
    }

    if let Some(actor_user_id) = params.actor_user_id {
        pairs.append_pair("actor_user_id", &actor_user_id.to_string());
    }

    if let Some(target_user_id) = params.target_user_id {
        pairs.append_pair("target_user_id", &target_user_id.to_string());
    }

    if let Some(object_id) = params.object_id {
        pairs.append_pair("object_id", &object_id.to_string());
    }

    if let Some(group_id) = params.group_id {
        pairs.append_pair("group_id", &group_id.to_string());
    }
}

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Builds an absolute API URL for a path relative to [`API_PREFIX`].
    ///
    /// # Errors
    ///
    /// Returns an error if the final URL cannot be constructed.
    pub(crate) fn api_url(&self, path: &str) -> Result<Url, ClientError> {
        let path = format!("{API_PREFIX}/{}", path.trim_start_matches('/'));
        Ok(self.base_url.join(&path)?)
    }

    /// Builds an authenticated HTTP request for a Kival API path.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured or the API URL cannot be constructed.
    pub(crate) fn request(
        &self,
        method: &Method,
        path: &str,
    ) -> Result<RequestBuilder, ClientError> {
        self.request_url(method, self.api_url(path)?)
    }

    /// Builds an authenticated HTTP request for an already constructed Kival API URL.
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is configured.
    pub(crate) fn request_url(
        &self,
        method: &Method,
        url: Url,
    ) -> Result<RequestBuilder, ClientError> {
        let api_key = self.api_key.as_ref().ok_or(ClientError::ApiKeyRequired)?;
        if api_key.0.trim().is_empty() {
            return Err(ClientError::InvalidApiKey);
        }
        Ok(self.http.request(method.clone(), url).bearer_auth(&api_key.0))
    }

    /// Builds a public HTTP request without attaching credentials.
    pub(crate) fn public_request(
        &self,
        method: &Method,
        path: &str,
    ) -> Result<RequestBuilder, ClientError> {
        Ok(self.http.request(method.clone(), self.api_url(path)?))
    }

    /// Sends a request and decodes a successful JSON response body.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction, middleware, transport, status classification, or
    /// JSON decoding fails.
    pub(crate) async fn send_json<T>(&self, request: RequestBuilder) -> Result<T, ClientError>
    where
        T: DeserializeOwned,
    {
        let response = self.send(request).await?;
        decode_json(response).await
    }

    /// Sends a request and returns a successful response.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction, middleware, transport, or status classification
    /// fails.
    pub(crate) async fn send(&self, request: RequestBuilder) -> Result<Response, ClientError> {
        let request = request.build()?;
        self.transport.clone().oneshot(request).await
    }

    /// Ensures an API key is attached.
    pub(crate) const fn require_api_key(&self) -> Result<(), ClientError> {
        if self.api_key.is_some() { Ok(()) } else { Err(ClientError::ApiKeyRequired) }
    }
}

/// Converts a response into success or [`ClientError::Api`].
async fn ensure_success(response: Response) -> Result<Response, ClientError> {
    let status = response.status();

    if status.is_success() {
        return Ok(response);
    }

    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let error = api_error_body(response).await;

    let kind = match status {
        StatusCode::UNAUTHORIZED => ApiErrorKind::Unauthorized,
        StatusCode::FORBIDDEN => ApiErrorKind::Forbidden,
        StatusCode::NOT_FOUND => ApiErrorKind::NotFound,
        StatusCode::CONFLICT => ApiErrorKind::Conflict,
        StatusCode::TOO_MANY_REQUESTS => ApiErrorKind::RateLimited,
        status if status.is_client_error() => ApiErrorKind::InvalidRequest,
        status if status.is_server_error() => ApiErrorKind::ServerError,
        _ => ApiErrorKind::Other,
    };

    Err(ClientError::Api(ApiError::new(status, kind, error.code, error.message, retry_after)))
}

/// Decodes a successful JSON response.
async fn decode_json<T>(response: Response) -> Result<T, ClientError>
where
    T: DeserializeOwned,
{
    Ok(response.json::<T>().await?)
}

/// Structured API error data retained by the client.
#[derive(Debug)]
struct ParsedApiError {
    /// Stable API error code, when present.
    code: Option<String>,
    /// Human-readable API error message.
    message: String,
}

/// Extracts useful structured data from an API error response.
async fn api_error_body(response: Response) -> ParsedApiError {
    match read_limited_body(response).await {
        Ok((body, truncated)) => {
            let mut error = format_api_error_body(&body);
            if truncated {
                error.message.push_str(" [response body truncated]");
            }
            error
        }
        Err(error) => ParsedApiError {
            code: None,
            message: format!("failed to read error response body: {error}"),
        },
    }
}

/// Reads at most [`MAX_ERROR_BODY_BYTES`] from a response body.
async fn read_limited_body(mut response: Response) -> Result<(String, bool), reqwest::Error> {
    let mut body = Vec::new();
    let mut truncated = false;

    while let Some(chunk) = response.chunk().await? {
        let remaining = MAX_ERROR_BODY_BYTES.saturating_sub(body.len());
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }

        body.extend_from_slice(&chunk);
        if body.len() == MAX_ERROR_BODY_BYTES {
            truncated = response.chunk().await?.is_some();
            break;
        }
    }

    Ok((String::from_utf8_lossy(&body).into_owned(), truncated))
}

/// Formats an API error response body for display.
fn format_api_error_body(body: &str) -> ParsedApiError {
    let body = body.trim();

    if body.is_empty() {
        return ParsedApiError { code: None, message: "empty error response".to_owned() };
    }

    if let Ok(response) = serde_json::from_str::<ApiErrorResponse>(body) {
        return ParsedApiError { code: Some(response.error.code), message: response.error.message };
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        for field in ["message", "error", "detail"] {
            if let Some(message) = json_string_field(&value, field) {
                return ParsedApiError { code: None, message: message.to_owned() };
            }
        }

        if let Some(error) = value.get("error") {
            if let Some(message) = json_string_field(error, "message") {
                return ParsedApiError { code: None, message: message.to_owned() };
            }

            if let Some(error) = error.as_str() {
                return ParsedApiError { code: None, message: error.to_owned() };
            }
        }
    }

    ParsedApiError { code: None, message: body.to_owned() }
}

/// Returns a string field from a JSON object.
fn json_string_field<'a>(value: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    value.as_object()?.get(field)?.as_str()
}
