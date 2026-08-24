//! Focused tests for the client transport contract.

use std::{
    future::{Ready, ready},
    pin::Pin,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use http::Response as HttpResponse;
use reqwest::{Method, Request, Response, StatusCode, header::AUTHORIZATION};
use tower::{Layer, Service, timeout::TimeoutLayer};
use url::Url;
use uuid::Uuid;

use crate::{
    ApiErrorKind, ArchiveListStatus, ClientBuilder, ClientError, GroupListParams, KivalClient,
    ObjectListOrder, ObjectListParams, Transport, TransportError, UserListParams, UserListStatus,
    WorkspaceGroupListParams, WorkspaceListParams, client::transport::MAX_ERROR_BODY_BYTES,
};

/// Shared request capture used by test transports.
type Requests = Arc<Mutex<Vec<Request>>>;

/// Raw transport returning a fixed HTTP response.
#[derive(Debug, Clone)]
struct StubTransport {
    /// Requests observed by the transport.
    requests: Requests,
    /// Response status.
    status: StatusCode,
    /// Response headers.
    headers: Vec<(&'static str, &'static str)>,
    /// Response body.
    body: Arc<str>,
    /// Optional lifecycle log entry.
    log: Option<Arc<Mutex<Vec<&'static str>>>>,
}

impl StubTransport {
    /// Creates a successful JSON transport.
    fn json(body: impl Into<Arc<str>>) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            status: StatusCode::OK,
            headers: vec![("content-type", "application/json")],
            body: body.into(),
            log: None,
        }
    }

    /// Returns the shared captured requests.
    fn requests(&self) -> Requests {
        Arc::clone(&self.requests)
    }

    /// Builds a Reqwest response from the configured fixture.
    fn response(&self) -> Response {
        let mut response = HttpResponse::builder().status(self.status);
        for (name, value) in &self.headers {
            response = response.header(*name, *value);
        }
        response
            .body(self.body.to_string())
            .unwrap_or_else(|error| panic!("invalid test response: {error}"))
            .into()
    }
}

impl Service<Request> for StubTransport {
    type Response = Response;
    type Error = ClientError;
    type Future = Ready<Result<Response, ClientError>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request) -> Self::Future {
        lock(&self.requests).push(request);
        if let Some(log) = &self.log {
            lock(log).push("transport:request");
        }
        ready(Ok(self.response()))
    }
}

/// Layer that records request and response traversal.
#[derive(Debug, Clone)]
struct RecordLayer {
    /// Layer name.
    name: &'static str,
    /// Shared lifecycle log.
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl<S> Layer<S> for RecordLayer {
    type Service = RecordService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RecordService { inner, name: self.name, log: Arc::clone(&self.log) }
    }
}

/// Service produced by [`RecordLayer`].
#[derive(Debug, Clone)]
struct RecordService<S> {
    /// Wrapped service.
    inner: S,
    /// Layer name.
    name: &'static str,
    /// Shared lifecycle log.
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl<S> Service<Request> for RecordService<S>
where
    S: Service<Request, Response = Response, Error = ClientError>,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = ClientError;
    type Future = Pin<Box<dyn Future<Output = Result<Response, ClientError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let request_entry = match self.name {
            "a" => "a:request",
            "b" => "b:request",
            _ => "unknown:request",
        };
        let response_entry = match self.name {
            "a" => "a:response",
            "b" => "b:response",
            _ => "unknown:response",
        };
        lock(&self.log).push(request_entry);
        let response = self.inner.call(request);
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            let response = response.await;
            lock(&log).push(response_entry);
            response
        })
    }
}

/// Layer that records whether it observed a classified API error.
#[derive(Debug, Clone)]
struct ObserveApiErrorLayer(Arc<AtomicBool>);

impl<S> Layer<S> for ObserveApiErrorLayer {
    type Service = ObserveApiErrorService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ObserveApiErrorService { inner, observed: Arc::clone(&self.0) }
    }
}

/// Service produced by [`ObserveApiErrorLayer`].
#[derive(Debug, Clone)]
struct ObserveApiErrorService<S> {
    /// Wrapped service.
    inner: S,
    /// Whether an API error was observed.
    observed: Arc<AtomicBool>,
}

impl<S> Service<Request> for ObserveApiErrorService<S>
where
    S: Service<Request, Response = Response, Error = ClientError>,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = ClientError;
    type Future = Pin<Box<dyn Future<Output = Result<Response, ClientError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let response = self.inner.call(request);
        let observed = Arc::clone(&self.observed);
        Box::pin(async move {
            let response = response.await;
            if matches!(&response, Err(ClientError::Api(_))) {
                observed.store(true, Ordering::SeqCst);
            }
            response
        })
    }
}

/// Transport that never completes a request.
#[derive(Debug, Clone, Copy)]
struct PendingTransport;

impl Service<Request> for PendingTransport {
    type Response = Response;
    type Error = ClientError;
    type Future = Pin<Box<dyn Future<Output = Result<Response, ClientError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _request: Request) -> Self::Future {
        Box::pin(std::future::pending())
    }
}

#[tokio::test]
async fn layers_run_in_first_added_order() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut raw = StubTransport::json(r#"{"status":"ok"}"#);
    raw.log = Some(Arc::clone(&log));

    let client = ClientBuilder::new()
        .layer(RecordLayer { name: "a", log: Arc::clone(&log) })
        .layer(RecordLayer { name: "b", log: Arc::clone(&log) })
        .connect_with_transport(root_url(), raw)
        .unwrap_or_else(|error| panic!("client construction failed: {error}"));

    client.health().await.unwrap_or_else(|error| panic!("health failed: {error}"));

    assert_eq!(
        lock(&log).as_slice(),
        ["a:request", "b:request", "transport:request", "b:response", "a:response"]
    );
}

#[test]
fn bearer_header_is_sensitive_and_public_requests_omit_it() {
    let client = ClientBuilder::new()
        .with_api_key("secret")
        .connect_with_transport(root_url(), StubTransport::json(r#"{"status":"ok"}"#))
        .unwrap_or_else(|error| panic!("client construction failed: {error}"));

    let authenticated = client
        .request(&Method::GET, "/auth/whoami")
        .and_then(|request| request.build().map_err(ClientError::from))
        .unwrap_or_else(|error| panic!("authenticated request failed: {error}"));
    let authorization = authenticated
        .headers()
        .get(AUTHORIZATION)
        .unwrap_or_else(|| panic!("authorization header missing"));
    assert!(authorization.is_sensitive());
    assert_eq!(authorization, "Bearer secret");

    let public = client
        .public_request(&Method::GET, "/healthz")
        .and_then(|request| request.build().map_err(ClientError::from))
        .unwrap_or_else(|error| panic!("public request failed: {error}"));
    assert!(!public.headers().contains_key(AUTHORIZATION));
}

#[tokio::test]
async fn authenticated_operations_fail_before_transport_without_an_api_key() {
    let raw = StubTransport::json(r#"{"user":{}}"#);
    let requests = raw.requests();
    let client = ClientBuilder::new()
        .connect_with_transport(root_url(), raw)
        .unwrap_or_else(|error| panic!("client construction failed: {error}"));

    assert!(matches!(client.whoami().await, Err(ClientError::ApiKeyRequired)));
    assert!(lock(&requests).is_empty());
}

#[tokio::test]
async fn affected_list_queries_serialize_every_public_field() {
    let raw = StubTransport::json(r#"{"items":[],"next_cursor":null}"#);
    let requests = raw.requests();
    let client = ClientBuilder::new()
        .with_api_key("secret")
        .connect_with_transport(root_url(), raw)
        .unwrap_or_else(|error| panic!("client construction failed: {error}"));

    client
        .list_workspaces(&WorkspaceListParams {
            limit: Some(11),
            cursor: Some("workspace-cursor".into()),
            status: ArchiveListStatus::All,
            q: Some("workspace search".into()),
            pinned: Some(true),
        })
        .await
        .unwrap_or_else(|error| panic!("workspace list failed: {error}"));
    assert_query(
        &requests,
        &[
            ("limit", "11"),
            ("cursor", "workspace-cursor"),
            ("status", "all"),
            ("q", "workspace search"),
            ("pinned", "true"),
        ],
    );

    client
        .list_objects(
            Uuid::nil(),
            &ObjectListParams {
                limit: Some(12),
                cursor: Some("object-cursor".into()),
                status: ArchiveListStatus::All,
                order: ObjectListOrder::Updated,
                favorited: Some(true),
                pinned: Some(false),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("object list failed: {error}"));
    assert_query(
        &requests,
        &[
            ("limit", "12"),
            ("cursor", "object-cursor"),
            ("status", "all"),
            ("order", "updated"),
            ("favorited", "true"),
            ("pinned", "false"),
        ],
    );

    client
        .list_groups(&GroupListParams {
            limit: Some(12),
            cursor: Some("group-cursor".into()),
            status: ArchiveListStatus::Archived,
            q: Some("group search".into()),
        })
        .await
        .unwrap_or_else(|error| panic!("group list failed: {error}"));
    assert_query(
        &requests,
        &[
            ("limit", "12"),
            ("cursor", "group-cursor"),
            ("status", "archived"),
            ("q", "group search"),
        ],
    );

    client
        .list_users(&UserListParams {
            limit: Some(13),
            cursor: Some("user-cursor".into()),
            status: UserListStatus::All,
            q: Some("user search".into()),
        })
        .await
        .unwrap_or_else(|error| panic!("user list failed: {error}"));
    assert_query(
        &requests,
        &[("limit", "13"), ("cursor", "user-cursor"), ("status", "all"), ("q", "user search")],
    );

    client
        .list_workspace_groups(
            Uuid::nil(),
            &WorkspaceGroupListParams {
                limit: Some(14),
                cursor: Some("link-cursor".into()),
                status: ArchiveListStatus::All,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("workspace-group list failed: {error}"));
    assert_query(&requests, &[("limit", "14"), ("cursor", "link-cursor"), ("status", "all")]);
}

#[tokio::test]
async fn middleware_observes_classified_api_errors_with_status_and_retry_metadata() {
    let observed = Arc::new(AtomicBool::new(false));
    let raw = StubTransport {
        requests: Arc::new(Mutex::new(Vec::new())),
        status: StatusCode::TOO_MANY_REQUESTS,
        headers: vec![("content-type", "application/json"), ("retry-after", "30")],
        body: Arc::from(r#"{"error":{"code":"rate_limited","message":"slow down"}}"#),
        log: None,
    };
    let client = ClientBuilder::new()
        .layer(ObserveApiErrorLayer(Arc::clone(&observed)))
        .connect_with_transport(root_url(), raw)
        .unwrap_or_else(|error| panic!("client construction failed: {error}"));

    let error = client.health().await.unwrap_err();
    let api = error.api_error().unwrap_or_else(|| panic!("expected API error"));
    assert_eq!(api.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(api.kind(), ApiErrorKind::RateLimited);
    assert_eq!(api.code(), Some("rate_limited"));
    assert_eq!(api.message(), "slow down");
    assert_eq!(api.retry_after(), Some("30"));
    assert_eq!(api.retry_after_duration(), Some(Duration::from_secs(30)));
    assert!(observed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn timeout_layer_maps_to_a_timeout_client_error() {
    let client = ClientBuilder::new()
        .layer(TimeoutLayer::new(Duration::from_millis(1)))
        .connect_with_transport(root_url(), PendingTransport)
        .unwrap_or_else(|error| panic!("client construction failed: {error}"));

    let error = client.health().await.unwrap_err();
    assert!(error.transport_error().is_some_and(TransportError::is_timeout));
}

#[tokio::test]
async fn boxed_transport_remains_usable() {
    let client = ClientBuilder::new()
        .connect_with_transport(root_url(), StubTransport::json(r#"{"status":"ok"}"#))
        .unwrap_or_else(|error| panic!("client construction failed: {error}"))
        .boxed();

    client.health().await.unwrap_or_else(|error| panic!("health failed: {error}"));
    assert_transport(&client);
}

#[tokio::test]
async fn api_error_bodies_are_bounded() {
    let body = "x".repeat(MAX_ERROR_BODY_BYTES * 2);
    let raw = StubTransport {
        requests: Arc::new(Mutex::new(Vec::new())),
        status: StatusCode::INTERNAL_SERVER_ERROR,
        headers: vec![("content-type", "text/plain")],
        body: Arc::from(body),
        log: None,
    };
    let client = ClientBuilder::new()
        .connect_with_transport(root_url(), raw)
        .unwrap_or_else(|error| panic!("client construction failed: {error}"));

    let error = client.health().await.unwrap_err();
    let message = error.api_error().unwrap_or_else(|| panic!("expected API error")).message();
    assert!(message.len() <= MAX_ERROR_BODY_BYTES + 26);
    assert!(message.ends_with("[response body truncated]"));
}

#[test]
fn base_url_contract_is_enforced() {
    for value in [
        "ftp://kival.example",
        "https://user:secret@kival.example",
        "https://kival.example/prefix",
        "https://kival.example?query=yes",
        "https://kival.example#fragment",
    ] {
        let result = ClientBuilder::new().connect(value);
        assert!(matches!(result, Err(ClientError::BaseUrl(_))), "accepted invalid URL: {value}");
    }

    assert!(ClientBuilder::new().connect("https://kival.example").is_ok());
}

/// Returns a valid Kival server root URL.
fn root_url() -> Url {
    Url::parse("https://kival.example").unwrap_or_else(|error| panic!("invalid test URL: {error}"))
}

/// Returns a mutex guard even after a prior test panic poisoned the mutex.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Asserts the latest captured request has exactly the expected query pairs.
fn assert_query(requests: &Requests, expected: &[(&str, &str)]) {
    let actual = lock(requests)
        .last()
        .unwrap_or_else(|| panic!("no request captured"))
        .url()
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();

    let expected = expected
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect::<Vec<_>>();

    assert_eq!(actual, expected);
}

/// Compile-time assertion for the public transport trait.
fn assert_transport<S: Transport>(_client: &KivalClient<S>) {}
