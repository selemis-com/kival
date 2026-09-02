//! HTTP helpers for exercising the real Kival router.

use std::net::{Ipv4Addr, SocketAddr};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{
        HeaderValue, Method, Request, Response, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE, COOKIE},
    },
};
use kival_sdk::{API_PREFIX, ApiErrorResponse};
use serde::{Serialize, de::DeserializeOwned};
use tower::ServiceExt;
use uuid::Uuid;

use crate::{TestKival, TestResult, db};

/// Authenticated test actor.
#[derive(Debug, Clone)]
pub struct TestActor {
    /// User ID.
    pub id: Uuid,

    /// Login username.
    pub username: String,

    /// Cookie header value containing session and CSRF cookies.
    pub cookie_header: HeaderValue,

    /// CSRF token header value for unsafe methods.
    pub csrf_token: HeaderValue,
}

/// Builds an authenticated test actor from raw browser-session material.
pub(crate) fn actor_from_session(session: db::SessionFixture) -> TestResult<TestActor> {
    let cookie_header = HeaderValue::from_str(&format!(
        "__Host-kival_session={}; __Host-kival_csrf={}",
        session.session_token, session.csrf_token
    ))?;
    let csrf_token = HeaderValue::from_str(&session.csrf_token)?;

    Ok(TestActor { id: session.user_id, username: session.username, cookie_header, csrf_token })
}

/// Useful decoded response data for assertions.
#[derive(Debug)]
pub struct TestJsonResponse<T> {
    /// Response status.
    pub status: StatusCode,

    /// Decoded JSON body.
    pub body: T,
}

/// Response assertion helpers.
pub trait TestResponseExt<T> {
    /// Requires a success status and returns the decoded body.
    ///
    /// # Errors
    ///
    /// Returns an error if the response status is not successful.
    fn into_success(self) -> TestResult<T>;
}

impl<T> TestResponseExt<T> for TestJsonResponse<T> {
    fn into_success(self) -> TestResult<T> {
        if self.status.is_success() {
            Ok(self.body)
        } else {
            Err(eyre::eyre!("expected success status, got {}", self.status))
        }
    }
}

/// Raw response assertion helpers.
pub trait TestRawResponseExt {
    /// Asserts that the raw response has the expected status.
    fn assert_status(&self, expected: StatusCode);
}

impl TestRawResponseExt for Response<Body> {
    fn assert_status(&self, expected: StatusCode) {
        assert_eq!(self.status(), expected);
    }
}

impl TestKival {
    /// Creates another browser session for an existing test user.
    ///
    /// # Errors
    ///
    /// Returns an error if session storage or HTTP header construction fails.
    pub async fn additional_session(&self, actor: &TestActor) -> TestResult<TestActor> {
        let session = db::insert_session(&self.pool, actor.id, actor.username.clone()).await?;
        actor_from_session(session)
    }

    /// Sends a byte-body request as an authenticated actor and decodes JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction, routing, or response decoding fails.
    pub async fn request_bytes_as<T>(
        &self,
        actor: &TestActor,
        method: Method,
        path: &str,
        bytes: Vec<u8>,
        content_type: Option<&str>,
    ) -> TestResult<TestJsonResponse<T>>
    where
        T: DeserializeOwned,
    {
        let response = self.request_bytes(Some(actor), method, path, bytes, content_type).await?;

        decode_json_response(response).await
    }

    /// Sends a JSON request as an authenticated actor.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction, routing, or response decoding fails.
    pub async fn request_json_as<B, T>(
        &self,
        actor: &TestActor,
        method: Method,
        path: &str,
        body: &B,
    ) -> TestResult<TestJsonResponse<T>>
    where
        B: Serialize + Sync + ?Sized,
        T: DeserializeOwned,
    {
        self.request_json(Some(actor), method, path, Some(serde_json::to_value(body)?)).await
    }

    /// Sends a JSON request as an authenticated actor and returns the raw response.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction or routing fails.
    pub async fn request_json_raw_as<B>(
        &self,
        actor: &TestActor,
        method: Method,
        path: &str,
        body: &B,
    ) -> TestResult<Response<Body>>
    where
        B: Serialize + Sync + ?Sized,
    {
        self.request(Some(actor), method, path, Some(serde_json::to_value(body)?)).await
    }

    /// Sends an empty request as an authenticated actor and decodes JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction, routing, or response decoding fails.
    pub async fn empty_json_as<T>(
        &self,
        actor: &TestActor,
        method: Method,
        path: &str,
    ) -> TestResult<TestJsonResponse<T>>
    where
        T: DeserializeOwned,
    {
        self.request_json(Some(actor), method, path, None).await
    }

    /// Sends a GET request as an authenticated actor and decodes JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction, routing, or response decoding fails.
    pub async fn get_json_as<T>(
        &self,
        actor: &TestActor,
        path: &str,
    ) -> TestResult<TestJsonResponse<T>>
    where
        T: DeserializeOwned,
    {
        self.empty_json_as(actor, Method::GET, path).await
    }

    /// Sends a request and returns the raw response.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction or routing fails.
    pub async fn request(
        &self,
        actor: Option<&TestActor>,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> TestResult<Response<Body>> {
        request_with_app(self.app.clone(), actor, method, path, body).await
    }

    /// Sends a byte-body request and returns the raw response.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction or routing fails.
    pub async fn request_bytes(
        &self,
        actor: Option<&TestActor>,
        method: Method,
        path: &str,
        bytes: Vec<u8>,
        content_type: Option<&str>,
    ) -> TestResult<Response<Body>> {
        request_bytes_with_app(self.app.clone(), actor, method, path, bytes, content_type).await
    }

    /// Sends a request authenticated with a bearer API key.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction or routing fails.
    pub async fn request_with_api_key(
        &self,
        api_key: &str,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> TestResult<Response<Body>> {
        request_with_api_key(self.app.clone(), api_key, method, path, body).await
    }

    /// Sends a request as an authenticated actor with an explicit Authorization header.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction or routing fails.
    pub async fn request_with_authorization_as(
        &self,
        actor: &TestActor,
        authorization: &str,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> TestResult<Response<Body>> {
        request_with_app_and_authorization(
            self.app.clone(),
            Some(actor),
            Some(authorization),
            method,
            path,
            body,
        )
        .await
    }

    /// Sends a JSON request authenticated with a bearer API key and decodes JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction, routing, or response decoding fails.
    pub async fn request_json_with_api_key<T>(
        &self,
        api_key: &str,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> TestResult<TestJsonResponse<T>>
    where
        T: DeserializeOwned,
    {
        let response = self.request_with_api_key(api_key, method, path, body).await?;
        decode_json_response(response).await
    }

    /// Sends a request and decodes the JSON response body.
    ///
    /// # Errors
    ///
    /// Returns an error if request construction, routing, or response decoding fails.
    pub async fn request_json<T>(
        &self,
        actor: Option<&TestActor>,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> TestResult<TestJsonResponse<T>>
    where
        T: DeserializeOwned,
    {
        let response = self.request(actor, method, path, body).await?;
        decode_json_response(response).await
    }
}

/// Sends a request through an Axum app using bearer authentication.
async fn request_with_api_key(
    app: Router,
    api_key: &str,
    method: Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> TestResult<Response<Body>> {
    let uri = format!("{API_PREFIX}{path}");
    let mut builder = Request::builder().method(method).uri(uri);

    {
        let headers = builder
            .headers_mut()
            .ok_or_else(|| eyre::eyre!("request builder headers unavailable"))?;
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {api_key}"))?);
        if body.is_some() {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }
    }

    let body = match body {
        Some(body) => Body::from(serde_json::to_vec(&body)?),
        None => Body::empty(),
    };
    let mut request = builder.body(body)?;
    request.extensions_mut().insert(ConnectInfo(SocketAddr::from((Ipv4Addr::LOCALHOST, 31000))));

    app.oneshot(request).await.map_err(|error| eyre::eyre!("request failed: {error}"))
}

/// Sends a request through an Axum app.
///
/// # Errors
///
/// Returns an error if request construction or routing fails.
async fn request_with_app(
    app: Router,
    actor: Option<&TestActor>,
    method: Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> TestResult<Response<Body>> {
    request_with_app_and_authorization(app, actor, None, method, path, body).await
}

/// Sends a request through an Axum app with optional cookie and Authorization credentials.
async fn request_with_app_and_authorization(
    app: Router,
    actor: Option<&TestActor>,
    authorization: Option<&str>,
    method: Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> TestResult<Response<Body>> {
    let uri = format!("{API_PREFIX}{path}");
    let mut builder = Request::builder().method(method.clone()).uri(uri);

    {
        let headers = builder
            .headers_mut()
            .ok_or_else(|| eyre::eyre!("request builder headers unavailable"))?;

        if body.is_some() {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }

        if let Some(actor) = actor {
            headers.insert(COOKIE, actor.cookie_header.clone());

            if is_unsafe_method(&method) {
                headers.insert("x-csrf-token", actor.csrf_token.clone());
            }
        }

        if let Some(authorization) = authorization {
            headers.insert(AUTHORIZATION, HeaderValue::from_str(authorization)?);
        }
    }

    let body = match body {
        Some(body) => Body::from(serde_json::to_vec(&body)?),
        None => Body::empty(),
    };

    let mut request = builder.body(body)?;
    request.extensions_mut().insert(ConnectInfo(SocketAddr::from((Ipv4Addr::LOCALHOST, 31000))));

    app.oneshot(request).await.map_err(|error| eyre::eyre!("request failed: {error}"))
}

/// Sends a byte-body request through an Axum app.
///
/// # Errors
///
/// Returns an error if request construction or routing fails.
async fn request_bytes_with_app(
    app: Router,
    actor: Option<&TestActor>,
    method: Method,
    path: &str,
    bytes: Vec<u8>,
    content_type: Option<&str>,
) -> TestResult<Response<Body>> {
    let uri = format!("{API_PREFIX}{path}");
    let mut builder = Request::builder().method(method.clone()).uri(uri);

    {
        let headers = builder
            .headers_mut()
            .ok_or_else(|| eyre::eyre!("request builder headers unavailable"))?;

        if let Some(content_type) = content_type {
            headers.insert(CONTENT_TYPE, HeaderValue::from_str(content_type)?);
        }

        if let Some(actor) = actor {
            headers.insert(COOKIE, actor.cookie_header.clone());

            if is_unsafe_method(&method) {
                headers.insert("x-csrf-token", actor.csrf_token.clone());
            }
        }
    }

    let mut request = builder.body(Body::from(bytes))?;
    request.extensions_mut().insert(ConnectInfo(SocketAddr::from((Ipv4Addr::LOCALHOST, 31000))));

    app.oneshot(request).await.map_err(|error| eyre::eyre!("request failed: {error}"))
}

/// Decodes a JSON response body.
///
/// # Errors
///
/// Returns an error if the body cannot be read or decoded.
async fn decode_json_response<T>(response: Response<Body>) -> TestResult<TestJsonResponse<T>>
where
    T: DeserializeOwned,
{
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;

    if status.is_success() {
        let body = serde_json::from_slice::<T>(&bytes)?;
        return Ok(TestJsonResponse { status, body });
    }

    if let Ok(error) = serde_json::from_slice::<ApiErrorResponse>(&bytes) {
        return Err(eyre::eyre!("request failed with status {status}: {}", error.error.message));
    }

    let body = serde_json::from_slice::<T>(&bytes)?;
    Ok(TestJsonResponse { status, body })
}

/// Returns whether an HTTP method needs CSRF protection.
const fn is_unsafe_method(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE)
}
