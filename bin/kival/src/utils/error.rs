//! Stable machine-readable CLI error support.

use std::path::Path;

use argx::argx;
use eyre::Report;
use kival_sdk::{ApiErrorKind, ClientError};
use serde::Serialize;
use serde_json::Value;

use crate::utils::output::print_json;

/// Internal normalized CLI failure used to translate implementation errors into typed command
/// contracts.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CliFailure {
    /// Stable machine-readable error code.
    pub(crate) code: FailureCode,
    /// Human-readable error message.
    pub(crate) message: String,
    /// Optional structured details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) details: Option<Value>,
}

impl CliFailure {
    /// Builds an invalid argument error.
    #[must_use]
    pub(crate) fn invalid_argument(message: impl Into<String>) -> Self {
        Self { code: FailureCode::InvalidArgument, message: message.into(), details: None }
    }

    /// Builds an invalid output field error.
    #[must_use]
    pub(crate) fn invalid_field(message: impl Into<String>, details: Value) -> Self {
        Self { code: FailureCode::InvalidField, message: message.into(), details: Some(details) }
    }

    /// Builds an invalid output projection error.
    #[must_use]
    pub(crate) fn invalid_projection(message: impl Into<String>, field: Option<String>) -> Self {
        let details = field.map(|field| serde_json::json!({ "field": field }));
        Self { code: FailureCode::InvalidProjection, message: message.into(), details }
    }

    /// Builds an input read failure.
    #[must_use]
    pub(crate) fn input_read_failed(path: Option<&Path>) -> Self {
        let details = path.map(|path| serde_json::json!({ "path": path.display().to_string() }));
        Self {
            code: FailureCode::InputReadFailed,
            message: "Could not read input.".to_owned(),
            details,
        }
    }

    /// Builds an invalid structured JSON syntax error.
    #[must_use]
    pub(crate) fn input_invalid_json(details: Value) -> Self {
        Self {
            code: FailureCode::InputInvalidJson,
            message: "The structured input is not valid JSON.".to_owned(),
            details: Some(details),
        }
    }

    /// Builds an invalid structured input value error.
    #[must_use]
    pub(crate) fn input_invalid_value(details: Value) -> Self {
        Self {
            code: FailureCode::InputInvalidValue,
            message: "The structured input does not match the command schema.".to_owned(),
            details: Some(details),
        }
    }

    /// Builds a conflicting input source error.
    #[must_use]
    pub(crate) fn input_conflicting_sources(conflicts: &[Value]) -> Self {
        let details = (!conflicts.is_empty()).then(|| serde_json::json!({ "fields": conflicts }));
        Self {
            code: FailureCode::InputConflictingSources,
            message: "`--input` cannot be combined with command payload options.".to_owned(),
            details,
        }
    }

    /// Builds a common CLI error from an SDK client error.
    #[must_use]
    pub(crate) fn from_client_error(error: &ClientError) -> Self {
        match error {
            ClientError::ApiKeyRequired => Self {
                code: FailureCode::AuthenticationRequired,
                message: "An API key is required.".to_owned(),
                details: None,
            },
            ClientError::InvalidApiKey => Self {
                code: FailureCode::InvalidArgument,
                message: "API key must not be empty.".to_owned(),
                details: None,
            },
            ClientError::Api(error) => {
                let code = code_for_api_error(error.kind(), error.code());
                let message = if error.code().is_some()
                    && !matches!(error.kind(), ApiErrorKind::ServerError)
                {
                    error.message().to_owned()
                } else {
                    generic_message_for_code(code).to_owned()
                };
                Self { code, message, details: None }
            }
            ClientError::Transport(error) if error.is_connect() || error.is_timeout() => Self {
                code: FailureCode::ServerUnavailable,
                message: "Kival server is unavailable.".to_owned(),
                details: None,
            },
            ClientError::Transport(_) => Self {
                code: FailureCode::RequestFailed,
                message: "Request failed.".to_owned(),
                details: None,
            },
            ClientError::Url(_) | ClientError::BaseUrl(_) => Self {
                code: FailureCode::InvalidArgument,
                message: "Invalid Kival server URL.".to_owned(),
                details: None,
            },
        }
    }

    /// Builds an internal CLI error without exposing implementation details.
    #[must_use]
    pub(crate) fn internal() -> Self {
        Self { code: FailureCode::Internal, message: "Internal error.".to_owned(), details: None }
    }

    /// Builds a common CLI error from a top-level report.
    #[must_use]
    pub(crate) fn from_report(error: &Report) -> Self {
        if let Some(error) = error.downcast_ref::<Self>() {
            return error.clone();
        }
        if let Some(error) = error.downcast_ref::<ClientError>() {
            return Self::from_client_error(error);
        }
        Self::internal()
    }
}

impl std::fmt::Display for CliFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CliFailure {}

impl From<ClientError> for CliFailure {
    fn from(error: ClientError) -> Self {
        Self::from_client_error(&error)
    }
}

impl From<Report> for CliFailure {
    fn from(error: Report) -> Self {
        Self::from_report(&error)
    }
}

impl From<std::io::Error> for CliFailure {
    fn from(_error: std::io::Error) -> Self {
        Self::internal()
    }
}

/// Internal normalized failure categories.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
#[argx(schema)]
pub(crate) enum FailureCode {
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
    /// The resource state conflicts with the requested operation.
    #[serde(rename = "resource.conflict")]
    ResourceConflict,
    /// Optimistic concurrency failed.
    #[serde(rename = "version.conflict")]
    VersionConflict,
    /// A pagination cursor is invalid.
    #[serde(rename = "invalid.cursor")]
    InvalidCursor,
    /// The server is unavailable.
    #[serde(rename = "server.unavailable")]
    ServerUnavailable,
    /// The server rate limit was exceeded.
    #[serde(rename = "rate_limited")]
    RateLimited,
    /// The request payload exceeds the server limit.
    #[serde(rename = "payload_too_large")]
    PayloadTooLarge,
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

/// Maps internal failures into the finite error-code vocabulary exposed by one command contract.
pub(crate) trait CommandErrorCode: Copy {
    /// Maps a normalized failure code when that failure is part of the command contract.
    fn from_failure(code: FailureCode) -> Option<Self>;

    /// Returns the command's internal-error code.
    fn internal() -> Self;
}

/// Machine-readable error whose code vocabulary is selected by the command handler type.
#[derive(Debug, Clone, Serialize)]
#[argx(schema)]
pub(crate) struct CommandError<C> {
    /// Stable machine-readable error code.
    pub(crate) code: C,
    /// Human-readable error message.
    pub(crate) message: String,
    /// Optional structured details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) details: Option<Value>,
}

impl<C> CommandError<C>
where
    C: CommandErrorCode,
{
    /// Converts a normalized implementation failure into this command's public contract.
    #[must_use]
    pub(crate) fn from_failure(error: CliFailure) -> Self {
        if let Some(code) = C::from_failure(error.code) {
            Self { code, message: error.message, details: error.details }
        } else {
            Self { code: C::internal(), message: "Internal error.".to_owned(), details: None }
        }
    }

    /// Builds an invalid-argument failure when supported by this contract.
    #[must_use]
    pub(crate) fn invalid_argument(message: impl Into<String>) -> Self {
        Self::from_failure(CliFailure::invalid_argument(message))
    }

    /// Builds an invalid structured input value failure when supported by this contract.
    #[must_use]
    pub(crate) fn input_invalid_value(details: Value) -> Self {
        Self::from_failure(CliFailure::input_invalid_value(details))
    }

    /// Builds an internal failure.
    #[must_use]
    pub(crate) fn internal() -> Self {
        Self { code: C::internal(), message: "Internal error.".to_owned(), details: None }
    }
}

impl<C> std::fmt::Display for CommandError<C>
where
    C: CommandErrorCode,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl<C> From<ClientError> for CommandError<C>
where
    C: CommandErrorCode,
{
    fn from(error: ClientError) -> Self {
        Self::from_failure(CliFailure::from_client_error(&error))
    }
}

impl<C> From<Report> for CommandError<C>
where
    C: CommandErrorCode + std::fmt::Debug + Send + Sync + 'static,
{
    fn from(error: Report) -> Self {
        if let Some(error) = error.downcast_ref::<Self>() {
            return error.clone();
        }
        if let Some(error) = error.downcast_ref::<CliFailure>() {
            return Self::from_failure(error.clone());
        }
        if let Some(error) = error.downcast_ref::<ClientError>() {
            return Self::from_failure(CliFailure::from_client_error(error));
        }
        Self::internal()
    }
}

impl<C> From<std::io::Error> for CommandError<C>
where
    C: CommandErrorCode,
{
    fn from(_error: std::io::Error) -> Self {
        Self::internal()
    }
}

impl<C> std::error::Error for CommandError<C> where C: CommandErrorCode + std::fmt::Debug {}

/// Declares the finite error-code vocabulary exposed by a command or command family.
macro_rules! command_error_codes {
    (
        $vis:vis enum $name:ident {
            $( $variant:ident => ($wire:literal, $failure:ident) ),+ $(,)?
        }
    ) => {
        #[doc = concat!("Machine-readable error codes exposed by `", stringify!($name), "`.")]
        #[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize)]
        #[argx::argx(schema)]
        $vis enum $name {
            $(
                #[doc = concat!("The `", $wire, "` error code.")]
                #[serde(rename = $wire)]
                $variant,
            )+
        }

        impl $crate::utils::error::CommandErrorCode for $name {
            fn from_failure(code: $crate::utils::error::FailureCode) -> Option<Self> {
                match code {
                    $(
                        $crate::utils::error::FailureCode::$failure => Some(Self::$variant),
                    )+
                    _ => None,
                }
            }

            fn internal() -> Self {
                Self::Internal
            }
        }
    };
}

pub(crate) use command_error_codes;

/// Type-erased command error retained only after a typed leaf handler crosses into process-level
/// dispatch.
#[derive(Debug)]
pub(crate) struct RenderedCommandError {
    /// Human-readable error message retained for text output.
    message: String,
    /// Already-serialized machine-readable error value retained for JSON output.
    value: Value,
}

impl std::fmt::Display for RenderedCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RenderedCommandError {}

impl RenderedCommandError {
    /// Returns the already-serialized machine error value.
    pub(crate) const fn value(&self) -> &Value {
        &self.value
    }
}

/// Erases a typed command error only after Argx has observed its concrete handler contract.
pub(crate) fn erase_command_error<E>(error: E) -> Report
where
    E: std::fmt::Display + Serialize,
{
    let message = error.to_string();
    let value = serde_json::to_value(&error).unwrap_or_else(
        |_| serde_json::json!({ "code": "internal", "message": "Internal error." }),
    );
    Report::new(RenderedCommandError { message, value })
}

/// Prints a machine-readable JSON error value.
pub fn print_json_error(error: &impl Serialize) {
    if print_json(error).is_err() {
        eprintln!("Error: failed to serialize JSON error response");
    }
}

/// Returns a generic public message for a stable common error code.
const fn generic_message_for_code(code: FailureCode) -> &'static str {
    match code {
        FailureCode::AuthenticationRequired => "Authentication is required.",
        FailureCode::PermissionDenied => "Permission denied.",
        FailureCode::InvalidArgument => "Invalid request.",
        FailureCode::ResourceNotFound => "Resource was not found.",
        FailureCode::ResourceConflict => "Resource state conflicts with the request.",
        FailureCode::VersionConflict => "Version conflict.",
        FailureCode::InvalidCursor => "Invalid cursor.",
        FailureCode::ServerUnavailable => "Kival server is unavailable.",
        FailureCode::RateLimited => "Kival server rate limit exceeded.",
        FailureCode::PayloadTooLarge => "Request payload is too large.",
        FailureCode::RequestFailed => "Request failed.",
        FailureCode::Internal => "Internal error.",
        FailureCode::InvalidField => "Unknown output field.",
        FailureCode::InvalidProjection => "Invalid output projection.",
        FailureCode::InputReadFailed => "Could not read input.",
        FailureCode::InputInvalidJson => "The structured input is not valid JSON.",
        FailureCode::InputInvalidValue => "The structured input does not match the command schema.",
        FailureCode::InputConflictingSources => "Conflicting input sources.",
    }
}

/// Maps a semantic API error kind and optional API code to a stable common error code.
fn code_for_api_error(kind: ApiErrorKind, code: Option<&str>) -> FailureCode {
    if let Some(code) = code.and_then(code_from_api_code) {
        return code;
    }

    match kind {
        ApiErrorKind::Unauthorized => FailureCode::AuthenticationRequired,
        ApiErrorKind::Forbidden => FailureCode::PermissionDenied,
        ApiErrorKind::NotFound => FailureCode::ResourceNotFound,
        ApiErrorKind::Conflict => FailureCode::ResourceConflict,
        ApiErrorKind::RateLimited => FailureCode::RateLimited,
        ApiErrorKind::InvalidRequest => FailureCode::InvalidArgument,
        ApiErrorKind::ServerError => FailureCode::Internal,
        _ => FailureCode::RequestFailed,
    }
}

/// Maps stable server API codes to stable common CLI codes.
fn code_from_api_code(code: &str) -> Option<FailureCode> {
    match code {
        "authentication.required" | "auth.required" | "unauthorized" => {
            Some(FailureCode::AuthenticationRequired)
        }
        "permission.denied" | "forbidden" => Some(FailureCode::PermissionDenied),
        "resource.not_found" | "not_found" => Some(FailureCode::ResourceNotFound),
        "resource.conflict" | "conflict" => Some(FailureCode::ResourceConflict),
        "version.conflict" => Some(FailureCode::VersionConflict),
        "invalid.cursor" => Some(FailureCode::InvalidCursor),
        "service_unavailable" => Some(FailureCode::ServerUnavailable),
        "rate_limited" => Some(FailureCode::RateLimited),
        "payload_too_large" => Some(FailureCode::PayloadTooLarge),
        "internal" => Some(FailureCode::Internal),
        "invalid.argument" | "bad_request" | "validation_failed" => {
            Some(FailureCode::InvalidArgument)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_failure_serializes_as_public_error_value() {
        let error = CliFailure::invalid_argument("description must not be empty");

        assert_eq!(
            serde_json::to_value(error).expect("serialize common error"),
            serde_json::json!({
                "code": "invalid.argument",
                "message": "description must not be empty"
            })
        );
    }
}
