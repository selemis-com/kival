//! Client errors.

use std::{error::Error as StdError, time::Duration};

use reqwest::StatusCode;
use thiserror::Error;
use tower::BoxError;

/// Kival client error.
#[derive(Debug, Error)]
pub enum ClientError {
    /// Request transport or middleware failed.
    #[error(transparent)]
    Transport(TransportError),

    /// URL construction failed.
    #[error(transparent)]
    Url(UrlError),

    /// The configured server root URL is not supported.
    #[error(transparent)]
    BaseUrl(BaseUrlError),

    /// Kival API returned an error response.
    #[error(transparent)]
    Api(ApiError),

    /// Client operation requires an API key, but none is configured.
    #[error("API key required")]
    ApiKeyRequired,

    /// The configured API key is empty or contains only whitespace.
    #[error("API key must not be empty")]
    InvalidApiKey,
}

impl ClientError {
    /// Creates a transport error from a custom middleware or transport failure.
    pub fn transport<E>(kind: TransportErrorKind, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::Transport(TransportError::new(kind, source))
    }

    /// Returns the API error when this error came from a Kival API response.
    #[must_use]
    pub const fn api_error(&self) -> Option<&ApiError> {
        match self {
            Self::Api(error) => Some(error),
            _ => None,
        }
    }

    /// Returns the transport error when request execution or middleware failed.
    #[must_use]
    pub const fn transport_error(&self) -> Option<&TransportError> {
        match self {
            Self::Transport(error) => Some(error),
            _ => None,
        }
    }

    /// Returns whether the Kival API rejected the request as unauthorized.
    #[must_use]
    pub const fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Api(error) if matches!(error.kind, ApiErrorKind::Unauthorized))
    }
}

/// Broad semantic category for an error returned by the Kival API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ApiErrorKind {
    /// Authentication is missing or no longer valid.
    Unauthorized,
    /// The authenticated principal is not allowed to perform the operation.
    Forbidden,
    /// The requested resource does not exist or is not visible.
    NotFound,
    /// The request conflicts with the current resource state.
    Conflict,
    /// The caller has exceeded a server-side request limit.
    RateLimited,
    /// The request itself is invalid.
    InvalidRequest,
    /// Kival failed while processing an otherwise valid request.
    ServerError,
    /// An error category not otherwise classified by this client version.
    Other,
}

/// Structured error returned by the Kival API.
#[derive(Debug, Error)]
#[error("Kival API request failed with {status}: {message}")]
pub struct ApiError {
    /// HTTP status returned by Kival.
    status: StatusCode,

    /// Broad semantic category.
    kind: ApiErrorKind,

    /// Stable Kival API error code, when present.
    code: Option<String>,

    /// Human-readable error message.
    message: String,

    /// Raw `Retry-After` header value, when present.
    retry_after: Option<String>,
}

impl ApiError {
    /// Creates a structured API error.
    pub(crate) const fn new(
        status: StatusCode,
        kind: ApiErrorKind,
        code: Option<String>,
        message: String,
        retry_after: Option<String>,
    ) -> Self {
        Self { status, kind, code, message, retry_after }
    }

    /// Returns the HTTP status returned by Kival.
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns the broad semantic category of this API error.
    #[must_use]
    pub const fn kind(&self) -> ApiErrorKind {
        self.kind
    }

    /// Returns Kival's stable API error code, when one was provided.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// Returns the human-readable error message returned by Kival.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns Kival's raw `Retry-After` header value, when one was provided.
    #[must_use]
    pub fn retry_after(&self) -> Option<&str> {
        self.retry_after.as_deref()
    }

    /// Returns a delta-seconds `Retry-After` value as a duration.
    ///
    /// HTTP-date values remain available through [`Self::retry_after`].
    #[must_use]
    pub fn retry_after_duration(&self) -> Option<Duration> {
        self.retry_after()?.parse::<u64>().ok().map(Duration::from_secs)
    }
}

/// Broad transport failure category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransportErrorKind {
    /// Failed while connecting to the server.
    Connect,

    /// Request or middleware operation timed out.
    Timeout,

    /// Other transport or middleware failure.
    Other,
}

/// Transport or middleware failure reported by the client.
#[derive(Debug, Error)]
#[error("request failed: {source}")]
pub struct TransportError {
    /// Broad transport failure category.
    kind: TransportErrorKind,

    /// Underlying transport or middleware error.
    #[source]
    source: BoxError,
}

impl TransportError {
    /// Creates a transport error from a custom failure.
    pub fn new<E>(kind: TransportErrorKind, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self { kind, source: Box::new(source) }
    }

    /// Creates a transport error from an already boxed failure.
    #[must_use]
    pub fn from_boxed(kind: TransportErrorKind, source: BoxError) -> Self {
        Self { kind, source }
    }

    /// Returns the broad transport failure category.
    #[must_use]
    pub const fn kind(&self) -> TransportErrorKind {
        self.kind
    }

    /// Returns whether the request failed while connecting to the server.
    #[must_use]
    pub const fn is_connect(&self) -> bool {
        matches!(self.kind, TransportErrorKind::Connect)
    }

    /// Returns whether the request failed because it timed out.
    #[must_use]
    pub const fn is_timeout(&self) -> bool {
        matches!(self.kind, TransportErrorKind::Timeout)
    }
}

impl From<reqwest::Error> for ClientError {
    fn from(source: reqwest::Error) -> Self {
        let kind = if source.is_connect() {
            TransportErrorKind::Connect
        } else if source.is_timeout() {
            TransportErrorKind::Timeout
        } else {
            TransportErrorKind::Other
        };
        Self::Transport(TransportError::new(kind, source))
    }
}

impl From<BoxError> for ClientError {
    fn from(source: BoxError) -> Self {
        if source.is::<Self>() {
            return match source.downcast::<Self>() {
                Ok(error) => *error,
                Err(source) => {
                    Self::Transport(TransportError::from_boxed(TransportErrorKind::Other, source))
                }
            };
        }

        let kind = source.downcast_ref::<reqwest::Error>().map_or_else(
            || {
                if source.is::<tower::timeout::error::Elapsed>() {
                    TransportErrorKind::Timeout
                } else {
                    TransportErrorKind::Other
                }
            },
            |error| {
                if error.is_connect() {
                    TransportErrorKind::Connect
                } else if error.is_timeout() {
                    TransportErrorKind::Timeout
                } else {
                    TransportErrorKind::Other
                }
            },
        );

        Self::Transport(TransportError::from_boxed(kind, source))
    }
}

/// Invalid URL reported by the client.
#[derive(Debug, Clone, Copy, Error)]
#[error("invalid URL")]
pub struct UrlError {
    /// Underlying URL parsing error.
    #[source]
    source: url::ParseError,
}

impl From<url::ParseError> for ClientError {
    fn from(source: url::ParseError) -> Self {
        Self::Url(UrlError { source })
    }
}

/// Invalid Kival server root URL.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum BaseUrlError {
    /// Kival HTTP clients support only HTTP and HTTPS URLs.
    #[error("Kival server URL must use http or https, not {0}")]
    UnsupportedScheme(String),

    /// Kival server URLs must identify a host.
    #[error("Kival server URL must include a host")]
    MissingHost,

    /// Embedded credentials are not accepted.
    #[error("Kival server URL must not contain embedded credentials")]
    Credentials,

    /// The SDK currently requires an origin-root URL.
    #[error("Kival server URL must not contain a path prefix")]
    PathPrefix,

    /// Query parameters do not belong in the server root URL.
    #[error("Kival server URL must not contain a query string")]
    Query,

    /// Fragments do not belong in the server root URL.
    #[error("Kival server URL must not contain a fragment")]
    Fragment,
}
