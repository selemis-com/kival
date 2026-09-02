//! Public health and readiness wire protocol types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Represents the status of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The server is operating normally.
    Ok,
    /// The server encountered an error.
    Error,
}

impl Status {
    /// Returns the API representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
        }
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// JSON response indicating server status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StatusResponse {
    /// The status of the server.
    pub status: Status,
    /// Optional message providing additional information about the status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl StatusResponse {
    /// Response: `{ "status": "ok" }`
    #[must_use]
    pub const fn ok() -> Self {
        Self { status: Status::Ok, message: None }
    }

    /// Response: `{ "status": "error" }`
    #[must_use]
    pub const fn error() -> Self {
        Self { status: Status::Error, message: None }
    }
}
