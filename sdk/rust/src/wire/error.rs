//! API error response wire protocol types.

use serde::{Deserialize, Serialize};

/// Top-level JSON API error response body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    /// Error payload.
    pub error: ApiErrorBody,
}

impl ApiErrorResponse {
    /// Creates a serialized error response.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { error: ApiErrorBody { code: code.into(), message: message.into() } }
    }
}

/// Inner JSON API error object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiErrorBody {
    /// Stable machine-readable error code returned to clients.
    pub code: String,
    /// Human-readable error message returned to clients.
    pub message: String,
}
