//! API error responses.

use std::{error::Error as StdError, sync::Once};

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use kival_kernel::KernelError;
use kival_metrics::{counter, describe_counter};
use kival_sdk::ApiErrorResponse;
use kival_storage::BlobStoreError;
use kival_tracing::error;

/// Result type returned by API handlers.
pub(crate) type ApiResult<T> = Result<T, ApiError>;

/// Ensures API error metric descriptions are emitted once.
static DESCRIBE_API_ERROR_METRICS: Once = Once::new();

/// Error returned by API handlers.
#[derive(Debug)]
pub(crate) struct ApiError {
    /// HTTP status code returned for this error.
    status: StatusCode,
    /// Stable machine-readable error code.
    code: &'static str,
    /// Bounded subsystem that produced the error.
    origin: &'static str,
    /// Human-readable error message returned to clients.
    message: String,
    /// Internal source retained for structured diagnostics.
    source: Option<Box<dyn StdError + Send + Sync>>,
    /// Retry delay advertised for rate-limited responses.
    retry_after_seconds: Option<u64>,
}

impl ApiError {
    /// Creates a `400 Bad Request` error.
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    /// Creates a `404 Not Found` error.
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    /// Creates a `401 Unauthorized` error.
    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }

    /// Creates a `403 Forbidden` error.
    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }

    /// Creates a `409 Conflict` error.
    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    /// Creates a `413 Payload Too Large` error.
    pub(crate) fn payload_too_large(message: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", message)
    }

    /// Creates a `429 Too Many Requests` error with a `Retry-After` header.
    pub(crate) fn too_many_requests(message: impl Into<String>, retry_after_seconds: u64) -> Self {
        let mut error = Self::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", message)
            .with_origin("rate_limit");
        error.retry_after_seconds = Some(retry_after_seconds);
        error
    }

    /// Creates a `503 Service Unavailable` error.
    pub(crate) fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable", message)
    }

    /// Creates a validation failure response.
    fn validation_failed(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "validation_failed", message)
    }

    /// Creates a `500 Internal Server Error` response.
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", message).with_origin("application")
    }

    /// Creates an API error from its response components.
    pub(crate) fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            origin: "request",
            message: message.into(),
            source: None,
            retry_after_seconds: None,
        }
    }

    /// Attributes this error to one bounded operational subsystem.
    pub(crate) const fn with_origin(mut self, origin: &'static str) -> Self {
        self.origin = origin;
        self
    }

    /// Retains an internal source without exposing it in the HTTP response.
    fn with_source(mut self, source: impl StdError + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        let response = match &error {
            sqlx::Error::RowNotFound => Self::not_found("resource not found"),
            sqlx::Error::Database(database_error) => match database_error.code().as_deref() {
                Some("23505") => Self::conflict("active resource already exists"),
                Some("23514") => Self::validation_failed("request violates a constraint"),
                Some("23503") => Self::bad_request("request references an unknown resource"),
                _ => Self::internal("database error"),
            },
            _ => Self::internal("database error"),
        };

        response.with_origin("database").with_source(error)
    }
}

impl From<steda::Error> for ApiError {
    fn from(error: steda::Error) -> Self {
        match error {
            steda::Error::Database(error) => Self::from(error),
            error => {
                Self::internal("durable task error").with_origin("durable_tasks").with_source(error)
            }
        }
    }
}

impl From<KernelError> for ApiError {
    fn from(error: KernelError) -> Self {
        match error {
            KernelError::Database(error) => Self::from(error),
            error @ KernelError::Migrate { .. } => Self::internal("database migration error")
                .with_origin("database")
                .with_source(error),
            error @ KernelError::InvalidStoredValue { .. } => {
                Self::internal("stored database value is invalid")
                    .with_origin("database")
                    .with_source(error)
            }
            KernelError::ResourceNotFound => Self::not_found("resource not found"),
            KernelError::CapabilityRequired => Self::forbidden("access required"),
            KernelError::InvalidAttachmentVersion => {
                Self::bad_request("version_id must belong to object")
            }
            KernelError::InvalidObjectGrantUserPrincipal => {
                Self::bad_request("user principal must reference an active workspace member")
            }
            KernelError::InvalidObjectGrantGroupPrincipal => {
                Self::bad_request("group principal must be linked to the workspace")
            }
            KernelError::ObjectHasNoCurrentVersion => {
                Self::conflict("object has no current version")
            }
            KernelError::ObjectVersionConflict => {
                Self::conflict("object changed since the expected version")
            }
            KernelError::ObjectMustRetainAdminGrant => {
                Self::conflict("object must retain at least one admin grant")
            }
        }
    }
}

impl From<BlobStoreError> for ApiError {
    fn from(error: BlobStoreError) -> Self {
        match error {
            BlobStoreError::SizeLimitExceeded { limit } => Self::payload_too_large(format!(
                "attachment exceeds the configured limit of {limit} bytes"
            ))
            .with_origin("blob_store"),
            error => {
                Self::internal("blob store error").with_origin("blob_store").with_source(error)
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        DESCRIBE_API_ERROR_METRICS.call_once(|| {
            describe_counter!("api.errors_total", "API errors returned to clients.");
        });
        counter!(
            "api.errors_total",
            "code" => self.code,
            "origin" => self.origin,
            "status" => self.status.as_u16().to_string(),
        )
        .increment(1);

        if self.status.is_server_error() {
            error!(
                target: "kival::server::api",
                status = self.status.as_u16(),
                code = self.code,
                origin = self.origin,
                message = %self.message,
                source = ?self.source,
                "API request failed",
            );
        }

        let mut response =
            (self.status, Json(ApiErrorResponse::new(self.code, self.message))).into_response();
        if let Some(retry_after_seconds) = self.retry_after_seconds
            && let Ok(value) = HeaderValue::from_str(&retry_after_seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}
