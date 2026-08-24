//! Real-HTTP clients used by integration and stateful tests.

use std::sync::{Arc, Mutex};

use reqwest::{
    Client, Method, RequestBuilder, Url,
    cookie::{CookieStore, Jar},
};
use thiserror::Error;
use uuid::Uuid;

use crate::passkeys::{
    PasskeyFixtureError, TestPasskey, authenticate_shared_as, fresh_authenticate_shared,
};

/// Raw HTTP response returned by a real-HTTP test client.
pub type HttpResponse = reqwest::Response;

/// Browser-like test client using a real cookie jar, CSRF, and passkey authenticator.
#[derive(Clone)]
pub struct BrowserClient {
    /// Server base URL used for relative API targets and authentication ceremonies.
    base_url: String,
    /// Browser origin asserted by the deterministic authenticator.
    origin: String,
    /// User authenticated by this browser.
    user_id: Uuid,
    /// Canonical username authenticated by this browser.
    username: String,
    /// Cookie-enabled real HTTP client.
    http: Client,
    /// Cookie jar shared with the HTTP client so CSRF follows session rotation.
    cookies: Arc<Jar>,
    /// Deterministic authenticator shared by independent sessions for this user.
    passkey: Arc<Mutex<TestPasskey>>,
}

impl std::fmt::Debug for BrowserClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BrowserClient")
            .field("base_url", &self.base_url)
            .field("origin", &self.origin)
            .field("user_id", &self.user_id)
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

impl BrowserClient {
    /// Creates a browser client from an already-authenticated cookie jar.
    pub(crate) fn authenticated(
        base_url: String,
        origin: String,
        user_id: Uuid,
        username: String,
        http: Client,
        cookies: Arc<Jar>,
        passkey: TestPasskey,
    ) -> Self {
        Self {
            base_url,
            origin,
            user_id,
            username,
            http,
            cookies,
            passkey: Arc::new(Mutex::new(passkey)),
        }
    }

    /// Builds a GET request using this browser's cookies.
    pub fn get(&self, target: impl Into<String>) -> RequestBuilder {
        self.request(Method::GET, target.into())
    }

    /// Builds a POST request using this browser's cookies and current CSRF token.
    pub fn post(&self, target: impl Into<String>) -> RequestBuilder {
        self.request(Method::POST, target.into())
    }

    /// Builds a PATCH request using this browser's cookies and current CSRF token.
    pub fn patch(&self, target: impl Into<String>) -> RequestBuilder {
        self.request(Method::PATCH, target.into())
    }

    /// Builds a DELETE request using this browser's cookies and current CSRF token.
    pub fn delete(&self, target: impl Into<String>) -> RequestBuilder {
        self.request(Method::DELETE, target.into())
    }

    /// Performs real fresh passkey authentication, allowing the server to rotate this session.
    ///
    /// # Errors
    ///
    /// Returns an error when the browser has no CSRF cookie or the passkey ceremony fails.
    pub async fn fresh_authenticate(&self) -> Result<(), TestClientError> {
        let api = format!("{}/api/v1", self.base_url.trim_end_matches('/'));
        let csrf = self
            .csrf_token(&format!("{api}/auth/passkeys/fresh/options"))
            .ok_or(TestClientError::MissingCsrfCookie)?;
        fresh_authenticate_shared(
            &self.passkey,
            &self.http,
            &self.base_url,
            self.user_id,
            &self.origin,
            &csrf,
        )
        .await?;
        Ok(())
    }

    /// Authenticates the same passkey into an independent browser session.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP client cannot be built, the passkey ceremony fails, or the
    /// server authenticates a different user.
    pub async fn new_session(&self) -> Result<Self, TestClientError> {
        let cookies = Arc::new(Jar::default());
        let http = Client::builder().cookie_provider(cookies.clone()).build()?;
        let session = authenticate_shared_as(
            &self.passkey,
            &http,
            &self.base_url,
            &self.username,
            self.user_id,
            &self.origin,
        )
        .await?;
        if session.user.id != self.user_id || session.user.username != self.username {
            return Err(TestClientError::UnexpectedAuthenticatedUser {
                expected_user_id: self.user_id,
                actual_user_id: session.user.id,
                actual_username: session.user.username,
            });
        }

        Ok(Self {
            base_url: self.base_url.clone(),
            origin: self.origin.clone(),
            user_id: self.user_id,
            username: self.username.clone(),
            http,
            cookies,
            passkey: self.passkey.clone(),
        })
    }

    /// Builds one browser request and mirrors the CSRF header from the current cookie jar.
    fn request(&self, method: Method, target: String) -> RequestBuilder {
        let url = self.target(target);
        let requires_csrf = !matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS");
        let mut request = self.http.request(method, &url);
        if requires_csrf && let Some(csrf) = self.csrf_token(&url) {
            request = request.header("x-csrf-token", csrf);
        }
        request
    }

    /// Returns the concrete URL used for one absolute or API-relative target.
    fn target(&self, target: String) -> String {
        resolve_target(&self.base_url, target)
    }

    /// Reads the browser's current CSRF cookie for the target URL.
    fn csrf_token(&self, target: &str) -> Option<String> {
        let url = Url::parse(target).ok()?;
        let cookies = self.cookies.cookies(&url)?;
        let cookies = cookies.to_str().ok()?;
        cookies
            .split(';')
            .map(str::trim)
            .find_map(|cookie| cookie.strip_prefix("__Host-kival_csrf=").map(str::to_owned))
    }
}

/// API-key test client using a real bearer-authenticated HTTP connection.
#[derive(Clone)]
pub struct ApiKeyClient {
    /// Server base URL used for relative API targets.
    base_url: String,
    /// Bearer token attached to every request.
    token: String,
    /// Real HTTP client used for API-key traffic.
    http: Client,
}

impl std::fmt::Debug for ApiKeyClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApiKeyClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl ApiKeyClient {
    /// Creates a real HTTP client for one API-key token.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP client cannot be constructed.
    pub fn new(
        base_url: impl Into<String>,
        token: impl Into<String>,
    ) -> Result<Self, TestClientError> {
        Ok(Self {
            base_url: base_url.into(),
            token: token.into(),
            http: Client::builder().build()?,
        })
    }

    /// Builds a GET request carrying this API key as a bearer token.
    pub fn get(&self, target: impl Into<String>) -> RequestBuilder {
        self.request(Method::GET, target.into())
    }

    /// Builds a POST request carrying this API key as a bearer token.
    pub fn post(&self, target: impl Into<String>) -> RequestBuilder {
        self.request(Method::POST, target.into())
    }

    /// Builds a PATCH request carrying this API key as a bearer token.
    pub fn patch(&self, target: impl Into<String>) -> RequestBuilder {
        self.request(Method::PATCH, target.into())
    }

    /// Builds a DELETE request carrying this API key as a bearer token.
    pub fn delete(&self, target: impl Into<String>) -> RequestBuilder {
        self.request(Method::DELETE, target.into())
    }

    /// Builds one request carrying this client's bearer credential.
    fn request(&self, method: Method, target: String) -> RequestBuilder {
        self.http.request(method, resolve_target(&self.base_url, target)).bearer_auth(&self.token)
    }
}

/// Resolves an absolute target or an API-relative path against one test server.
fn resolve_target(base_url: &str, target: String) -> String {
    if target.starts_with("http://") || target.starts_with("https://") {
        target
    } else {
        format!("{}/api/v1/{}", base_url.trim_end_matches('/'), target.trim_start_matches('/'))
    }
}

/// Failure while constructing or authenticating a real-HTTP test client.
#[derive(Debug, Error)]
pub enum TestClientError {
    /// Constructing or using the underlying HTTP client failed.
    #[error("HTTP error")]
    Http(#[from] reqwest::Error),
    /// A deterministic passkey ceremony failed.
    #[error(transparent)]
    Passkey(#[from] PasskeyFixtureError),
    /// The browser cookie jar does not contain the CSRF cookie required for an unsafe request.
    #[error("authenticated browser is missing its CSRF cookie")]
    MissingCsrfCookie,
    /// Authentication completed for a user other than the expected browser identity.
    #[error(
        "authenticated as unexpected user {actual_username} ({actual_user_id}); expected {expected_user_id}"
    )]
    UnexpectedAuthenticatedUser {
        /// User expected for this browser.
        expected_user_id: Uuid,
        /// User returned by the server.
        actual_user_id: Uuid,
        /// Username returned by the server.
        actual_username: String,
    },
}
