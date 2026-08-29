//! Stable CLI error response support.

use std::path::Path;

use argx::argx;
use eyre::Report;
use kival_sdk::{ApiErrorKind, ClientError};
use serde::Serialize;
use serde_json::Value;

use crate::utils::output::print_json;

/// Top-level stable CLI error response.
#[derive(Debug, Serialize)]
pub struct CliErrorResponse {
    /// Error payload.
    pub error: CliErrorBody,
}

/// Stable CLI error payload.
#[derive(Debug, Serialize)]
pub struct CliErrorBody {
    /// Stable machine-readable error code.
    pub code: CliErrorCode,
    /// Human-readable error message.
    pub message: String,
    /// Optional structured details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// Typed CLI error before it is rendered for humans or JSON.
#[derive(Debug, Clone, Serialize)]
#[argx(schema)]
pub struct CliError {
    /// Stable machine-readable error code.
    pub code: CliErrorCode,
    /// Human-readable error message.
    pub message: String,
    /// Optional structured details.
    pub details: Option<Value>,
}

impl CliError {
    /// Builds an invalid argument error.
    #[must_use]
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self { code: CliErrorCode::InvalidArgument, message: message.into(), details: None }
    }

    /// Builds an invalid output field error.
    #[must_use]
    pub fn invalid_field(message: impl Into<String>, details: Value) -> Self {
        Self { code: CliErrorCode::InvalidField, message: message.into(), details: Some(details) }
    }

    /// Builds an invalid output projection error.
    #[must_use]
    pub fn invalid_projection(message: impl Into<String>, field: Option<String>) -> Self {
        let details = field.map(|field| serde_json::json!({ "field": field }));
        Self { code: CliErrorCode::InvalidProjection, message: message.into(), details }
    }

    /// Builds an input read failure.
    #[must_use]
    pub fn input_read_failed(path: Option<&Path>) -> Self {
        let details = path.map(|path| serde_json::json!({ "path": path.display().to_string() }));
        Self {
            code: CliErrorCode::InputReadFailed,
            message: "Could not read input.".to_owned(),
            details,
        }
    }

    /// Builds an invalid structured JSON syntax error.
    #[must_use]
    pub fn input_invalid_json(details: Value) -> Self {
        Self {
            code: CliErrorCode::InputInvalidJson,
            message: "The structured input is not valid JSON.".to_owned(),
            details: Some(details),
        }
    }

    /// Builds an invalid structured input value error.
    #[must_use]
    pub fn input_invalid_value(details: Value) -> Self {
        Self {
            code: CliErrorCode::InputInvalidValue,
            message: "The structured input does not match the command schema.".to_owned(),
            details: Some(details),
        }
    }

    /// Builds a conflicting input source error.
    #[must_use]
    pub fn input_conflicting_sources(conflicts: &[Value]) -> Self {
        let details = (!conflicts.is_empty()).then(|| serde_json::json!({ "fields": conflicts }));
        Self {
            code: CliErrorCode::InputConflictingSources,
            message: "`--input` cannot be combined with command payload options.".to_owned(),
            details,
        }
    }

    /// Builds a stable CLI error from an SDK client error.
    #[must_use]
    pub fn from_client_error(error: &ClientError) -> Self {
        let body = CliErrorBody::from_client_error(error);
        Self { code: body.code, message: body.message, details: body.details }
    }

    /// Builds an internal CLI error without exposing implementation details.
    #[must_use]
    pub fn internal() -> Self {
        Self {
            code: CliErrorCode::Internal,
            message: "Internal error.".to_owned(),
            details: None,
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

impl From<ClientError> for CliError {
    fn from(error: ClientError) -> Self {
        Self::from_client_error(&error)
    }
}

impl From<Report> for CliError {
    fn from(error: Report) -> Self {
        if let Some(error) = error.downcast_ref::<Self>() {
            return error.clone();
        }
        if let Some(error) = error.downcast_ref::<ClientError>() {
            return Self::from_client_error(error);
        }
        Self::internal()
    }
}

impl From<std::io::Error> for CliError {
    fn from(_error: std::io::Error) -> Self {
        Self::internal()
    }
}

/// Stable machine-readable CLI error code.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
#[argx(schema)]
pub enum CliErrorCode {
    /// Authentication is required.
    #[serde(rename = "authentication.required")]
    AuthenticationRequired,
    /// The caller lacks permission.
    #[serde(rename = "permission.denied")]
    PermissionDenied,
    /// A command argument is invalid.
    #[serde(rename = "invalid.argument")]
    InvalidArgument,
    /// A resource was not found.
    #[serde(rename = "resource.not_found")]
    ResourceNotFound,
    /// An object was not found.
    #[serde(rename = "object.not_found")]
    ObjectNotFound,
    /// An object is archived.
    #[serde(rename = "object.archived")]
    ObjectArchived,
    /// Optimistic concurrency failed.
    #[serde(rename = "version.conflict")]
    VersionConflict,
    /// A relative object-version selector is outside the available history.
    #[serde(rename = "version.selector_out_of_range")]
    VersionSelectorOutOfRange,
    /// A pagination cursor is invalid.
    #[serde(rename = "invalid.cursor")]
    InvalidCursor,
    /// The server is unavailable.
    #[serde(rename = "server.unavailable")]
    ServerUnavailable,
    /// A request failed.
    #[serde(rename = "request.failed")]
    RequestFailed,
    /// An internal failure occurred.
    #[serde(rename = "internal")]
    Internal,
    /// A selected output field does not exist.
    #[serde(rename = "output.invalid_field")]
    InvalidField,
    /// A selected output projection cannot be applied.
    #[serde(rename = "output.invalid_projection")]
    InvalidProjection,
    /// Input from a file or stdin could not be read.
    #[serde(rename = "input.read_failed")]
    InputReadFailed,
    /// Structured input is not valid JSON.
    #[serde(rename = "input.invalid_json")]
    InputInvalidJson,
    /// Structured input does not match the command schema.
    #[serde(rename = "input.invalid_value")]
    InputInvalidValue,
    /// Structured input conflicts with CLI payload fields.
    #[serde(rename = "input.conflicting_sources")]
    InputConflictingSources,
}

impl CliErrorCode {
    /// Returns the public dotted-lowercase code string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthenticationRequired => "authentication.required",
            Self::PermissionDenied => "permission.denied",
            Self::InvalidArgument => "invalid.argument",
            Self::ResourceNotFound => "resource.not_found",
            Self::ObjectNotFound => "object.not_found",
            Self::ObjectArchived => "object.archived",
            Self::VersionConflict => "version.conflict",
            Self::VersionSelectorOutOfRange => "version.selector_out_of_range",
            Self::InvalidCursor => "invalid.cursor",
            Self::ServerUnavailable => "server.unavailable",
            Self::RequestFailed => "request.failed",
            Self::Internal => "internal",
            Self::InvalidField => "output.invalid_field",
            Self::InvalidProjection => "output.invalid_projection",
            Self::InputReadFailed => "input.read_failed",
            Self::InputInvalidJson => "input.invalid_json",
            Self::InputInvalidValue => "input.invalid_value",
            Self::InputConflictingSources => "input.conflicting_sources",
        }
    }
}

impl CliErrorBody {
    /// Builds an error body from a top-level report.
    #[must_use]
    pub fn from_report(error: &Report) -> Self {
        if let Some(cli_error) = error.downcast_ref::<CliError>() {
            return Self::from_cli_error(cli_error);
        }

        if let Some(client_error) = error.downcast_ref::<ClientError>() {
            return Self::from_client_error(client_error);
        }

        Self { code: CliErrorCode::Internal, message: "Internal error.".to_owned(), details: None }
    }

    /// Builds an error body from a typed CLI error.
    #[must_use]
    pub fn from_cli_error(error: &CliError) -> Self {
        Self { code: error.code, message: error.message.clone(), details: error.details.clone() }
    }

    /// Builds an error body from a client error.
    #[must_use]
    pub fn from_client_error(error: &ClientError) -> Self {
        match error {
            ClientError::ApiKeyRequired => Self {
                code: CliErrorCode::AuthenticationRequired,
                message: "An API key is required.".to_owned(),
                details: None,
            },
            ClientError::InvalidApiKey => Self {
                code: CliErrorCode::InvalidArgument,
                message: "API key must not be empty.".to_owned(),
                details: None,
            },
            ClientError::Api(error) => {
                let cli_code = code_for_api_error(error.kind(), error.code());
                let message = if error.code().is_some()
                    && !matches!(error.kind(), ApiErrorKind::ServerError)
                {
                    error.message().to_owned()
                } else {
                    generic_message_for_code(cli_code).to_owned()
                };

                Self { code: cli_code, message, details: None }
            }
            ClientError::Transport(error) if error.is_connect() || error.is_timeout() => Self {
                code: CliErrorCode::ServerUnavailable,
                message: "Kival server is unavailable.".to_owned(),
                details: None,
            },
            ClientError::Transport(_) => Self {
                code: CliErrorCode::RequestFailed,
                message: "Request failed.".to_owned(),
                details: None,
            },
            ClientError::Url(_) | ClientError::BaseUrl(_) => Self {
                code: CliErrorCode::InvalidArgument,
                message: "Invalid Kival server URL.".to_owned(),
                details: None,
            },
        }
    }
}

/// Prints a stable JSON error response.
pub fn print_json_error(body: &CliErrorBody) {
    let response = CliErrorResponse {
        error: CliErrorBody {
            code: body.code,
            message: body.message.clone(),
            details: body.details.clone(),
        },
    };

    if print_json(&response).is_err() {
        eprintln!("Error: failed to serialize JSON error response");
    }
}

/// Returns a generic public message for a stable CLI code.
const fn generic_message_for_code(code: CliErrorCode) -> &'static str {
    match code {
        CliErrorCode::AuthenticationRequired => "Authentication is required.",
        CliErrorCode::PermissionDenied => "Permission denied.",
        CliErrorCode::InvalidArgument => "Invalid request.",
        CliErrorCode::ResourceNotFound | CliErrorCode::ObjectNotFound => "Resource was not found.",
        CliErrorCode::ObjectArchived => "Object is archived.",
        CliErrorCode::VersionConflict => "Version conflict.",
        CliErrorCode::VersionSelectorOutOfRange => "Version selector out of range.",
        CliErrorCode::InvalidCursor => "Invalid cursor.",
        CliErrorCode::ServerUnavailable => "Kival server is unavailable.",
        CliErrorCode::RequestFailed => "Request failed.",
        CliErrorCode::Internal => "Internal error.",
        CliErrorCode::InvalidField => "Unknown output field.",
        CliErrorCode::InvalidProjection => "Invalid output projection.",
        CliErrorCode::InputReadFailed => "Could not read input.",
        CliErrorCode::InputInvalidJson => "The structured input is not valid JSON.",
        CliErrorCode::InputInvalidValue => {
            "The structured input does not match the command schema."
        }
        CliErrorCode::InputConflictingSources => "Conflicting input sources.",
    }
}

/// Maps a semantic API error kind and optional API code to a stable CLI code.
fn code_for_api_error(kind: ApiErrorKind, code: Option<&str>) -> CliErrorCode {
    if let Some(code) = code.and_then(code_from_api_code) {
        return code;
    }

    match kind {
        ApiErrorKind::Unauthorized => CliErrorCode::AuthenticationRequired,
        ApiErrorKind::Forbidden => CliErrorCode::PermissionDenied,
        ApiErrorKind::NotFound => CliErrorCode::ResourceNotFound,
        ApiErrorKind::Conflict => CliErrorCode::VersionConflict,
        ApiErrorKind::InvalidRequest => CliErrorCode::InvalidArgument,
        ApiErrorKind::ServerError => CliErrorCode::ServerUnavailable,
        _ => CliErrorCode::RequestFailed,
    }
}

/// Maps stable server API codes to stable CLI codes.
fn code_from_api_code(code: &str) -> Option<CliErrorCode> {
    match code {
        "authentication.required" | "auth.required" | "unauthorized" => {
            Some(CliErrorCode::AuthenticationRequired)
        }
        "permission.denied" | "forbidden" => Some(CliErrorCode::PermissionDenied),
        "object.not_found" => Some(CliErrorCode::ObjectNotFound),
        "object.archived" => Some(CliErrorCode::ObjectArchived),
        "resource.not_found" | "not_found" => Some(CliErrorCode::ResourceNotFound),
        "version.conflict" | "conflict" => Some(CliErrorCode::VersionConflict),
        "invalid.cursor" => Some(CliErrorCode::InvalidCursor),
        "invalid.argument" | "bad_request" => Some(CliErrorCode::InvalidArgument),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_cli_error_maps_to_stable_body() {
        let error = CliError::invalid_argument("description must not be empty");
        let body = CliErrorBody::from_cli_error(&error);

        assert_eq!(body.code, CliErrorCode::InvalidArgument);
        assert_eq!(body.message, "description must not be empty");
    }

}
