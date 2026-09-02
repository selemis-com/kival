//! Object-command error types.

use argx::argx;
use eyre::Report;
use kival_sdk::ClientError;
use serde::Serialize;
use serde_json::Value;

use crate::utils::error::{CliFailure, CommandErrorCode, FailureCode};

/// Error-code mapping for commands scoped to an object.
pub(crate) trait ObjectScopedErrorCode: CommandErrorCode {
    /// Maps an object-specific failure when it is part of this command contract.
    fn from_object(code: ObjectErrorCode) -> Option<Self>;

    /// Maps a history-specific failure when it is part of this command contract.
    fn from_history(_code: ObjectHistoryErrorCode) -> Option<Self> {
        None
    }
}

/// Machine-readable error with a command-specific object-scoped code vocabulary.
#[derive(Debug, Clone, Serialize)]
#[argx(schema)]
pub(crate) struct ObjectCommandError<C> {
    /// Stable machine-readable error code.
    pub(crate) code: C,
    /// Human-readable error message.
    pub(crate) message: String,
    /// Optional structured details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) details: Option<Value>,
}

impl<C> ObjectCommandError<C>
where
    C: ObjectScopedErrorCode,
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

    /// Builds an invalid-argument failure.
    #[must_use]
    pub(crate) fn invalid_argument(message: impl Into<String>) -> Self {
        Self::from_failure(CliFailure::invalid_argument(message))
    }

    /// Converts an SDK client error while preserving object-specific public codes.
    #[must_use]
    pub(crate) fn from_client_error(error: &ClientError) -> Self {
        if let ClientError::Api(api_error) = error {
            let object_code = match api_error.code() {
                Some("object.not_found") => Some(ObjectErrorCode::NotFound),
                Some("object.archived") => Some(ObjectErrorCode::Archived),
                _ => None,
            };
            if let Some(object_code) = object_code
                && let Some(code) = C::from_object(object_code)
            {
                return Self { code, message: api_error.message().to_owned(), details: None };
            }
        }

        Self::from_failure(CliFailure::from_client_error(error))
    }

    /// Converts a legacy object error into this command's public contract.
    #[must_use]
    pub(crate) fn from_object_error(error: &ObjectError) -> Self {
        match error.code {
            ObjectCommandErrorCode::Common(code) => Self::from_failure(CliFailure {
                code,
                message: error.message.clone(),
                details: error.details.clone(),
            }),
            ObjectCommandErrorCode::Object(code) => {
                C::from_object(code).map_or_else(Self::internal, |code| Self {
                    code,
                    message: error.message.clone(),
                    details: error.details.clone(),
                })
            }
        }
    }

    /// Builds an internal error.
    #[must_use]
    pub(crate) fn internal() -> Self {
        Self { code: C::internal(), message: "Internal error.".to_owned(), details: None }
    }
}

impl<C> From<ObjectError> for ObjectCommandError<C>
where
    C: ObjectScopedErrorCode,
{
    fn from(error: ObjectError) -> Self {
        Self::from_object_error(&error)
    }
}

impl<C> From<ClientError> for ObjectCommandError<C>
where
    C: ObjectScopedErrorCode,
{
    fn from(error: ClientError) -> Self {
        Self::from_client_error(&error)
    }
}

impl<C> From<CliFailure> for ObjectCommandError<C>
where
    C: ObjectScopedErrorCode,
{
    fn from(error: CliFailure) -> Self {
        Self::from_failure(error)
    }
}

impl<C> From<Report> for ObjectCommandError<C>
where
    C: ObjectScopedErrorCode + std::fmt::Debug + Send + Sync + 'static,
{
    fn from(error: Report) -> Self {
        if let Some(error) = error.downcast_ref::<Self>() {
            return error.clone();
        }
        if let Some(error) = error.downcast_ref::<CliFailure>() {
            return Self::from_failure(error.clone());
        }
        if let Some(error) = error.downcast_ref::<ClientError>() {
            return Self::from_client_error(error);
        }
        if let Some(error) = error.downcast_ref::<ObjectError>() {
            return Self::from_object_error(error);
        }
        if let Some(error) = error.downcast_ref::<ObjectHistoryError>() {
            return match error.code {
                ObjectHistoryCommandErrorCode::Object(ObjectCommandErrorCode::Common(code)) => {
                    Self::from_failure(CliFailure {
                        code,
                        message: error.message.clone(),
                        details: error.details.clone(),
                    })
                }
                ObjectHistoryCommandErrorCode::Object(ObjectCommandErrorCode::Object(code)) => {
                    C::from_object(code).map_or_else(Self::internal, |code| Self {
                        code,
                        message: error.message.clone(),
                        details: error.details.clone(),
                    })
                }
                ObjectHistoryCommandErrorCode::History(code) => {
                    C::from_history(code).map_or_else(Self::internal, |code| Self {
                        code,
                        message: error.message.clone(),
                        details: error.details.clone(),
                    })
                }
            };
        }
        Self::internal()
    }
}

impl<C> From<std::io::Error> for ObjectCommandError<C>
where
    C: ObjectScopedErrorCode,
{
    fn from(_error: std::io::Error) -> Self {
        Self::internal()
    }
}

impl<C> std::fmt::Display for ObjectCommandError<C>
where
    C: ObjectScopedErrorCode,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl<C> std::error::Error for ObjectCommandError<C> where C: ObjectScopedErrorCode + std::fmt::Debug {}

/// Declares a finite object-scoped error vocabulary from normalized and object-specific failures.
macro_rules! object_error_codes {
    (
        $vis:vis enum $name:ident {
            failures { $( $fvariant:ident => ($fwire:literal, $failure:ident) ),* $(,)? }
            objects { $( $ovariant:ident => ($owire:literal, $object:ident) ),* $(,)? }
        }
    ) => {
        #[doc = concat!("Machine-readable error codes exposed by `", stringify!($name), "`.")]
        #[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize)]
        #[argx::argx(schema)]
        $vis enum $name {
            $(
                #[doc = concat!("The `", $fwire, "` error code.")]
                #[serde(rename = $fwire)]
                $fvariant,
            )*
            $(
                #[doc = concat!("The `", $owire, "` error code.")]
                #[serde(rename = $owire)]
                $ovariant,
            )*
        }

        impl $crate::utils::error::CommandErrorCode for $name {
            fn from_failure(code: $crate::utils::error::FailureCode) -> Option<Self> {
                match code {
                    $(
                        $crate::utils::error::FailureCode::$failure => Some(Self::$fvariant),
                    )*
                    _ => None,
                }
            }

            fn internal() -> Self {
                Self::Internal
            }
        }

        impl $crate::commands::objects::ObjectScopedErrorCode for $name {
            fn from_object(code: $crate::commands::objects::ObjectErrorCode) -> Option<Self> {
                let _ = code;
                $(
                    if code == $crate::commands::objects::ObjectErrorCode::$object {
                        return Some(Self::$ovariant);
                    }
                )*
                None
            }
        }
    };
}

pub(crate) use object_error_codes;

/// Machine-readable error returned by object commands.
#[derive(Debug, Clone, Serialize)]
#[argx(schema)]
pub(crate) struct ObjectError {
    /// Stable machine-readable error code.
    pub(super) code: ObjectCommandErrorCode,
    /// Human-readable error message.
    pub(super) message: String,
    /// Optional structured details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) details: Option<Value>,
}

/// Error-code vocabulary available to object commands.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
#[argx(schema)]
pub(crate) enum ObjectCommandErrorCode {
    /// A common CLI failure.
    Common(FailureCode),
    /// An object-specific failure.
    Object(ObjectErrorCode),
}

/// Error codes specific to object operations.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
#[argx(schema)]
pub(crate) enum ObjectErrorCode {
    /// The object was not found.
    #[serde(rename = "object.not_found")]
    NotFound,
    /// The object is archived.
    #[serde(rename = "object.archived")]
    Archived,
}

/// Machine-readable error returned by object history commands.
#[derive(Debug, Clone, Serialize)]
#[argx(schema)]
pub(crate) struct ObjectHistoryError {
    /// Stable machine-readable error code.
    pub(crate) code: ObjectHistoryCommandErrorCode,
    /// Human-readable error message.
    pub(crate) message: String,
    /// Optional structured details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) details: Option<Value>,
}

/// Error-code vocabulary available to object history commands.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
#[argx(schema)]
pub(crate) enum ObjectHistoryCommandErrorCode {
    /// An ordinary object-command failure.
    Object(ObjectCommandErrorCode),
    /// A history-specific failure.
    History(ObjectHistoryErrorCode),
}

/// Error codes specific to object history operations.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
#[argx(schema)]
pub(crate) enum ObjectHistoryErrorCode {
    /// A relative version selector is outside the available history.
    #[serde(rename = "version.selector_out_of_range")]
    VersionSelectorOutOfRange,
}

impl ObjectError {
    /// Builds an object error from a common error code.
    #[must_use]
    pub(crate) fn common(
        code: FailureCode,
        message: impl Into<String>,
        details: Option<Value>,
    ) -> Self {
        Self { code: ObjectCommandErrorCode::Common(code), message: message.into(), details }
    }

    /// Builds an object-specific error.
    #[must_use]
    pub(super) fn object(
        code: ObjectErrorCode,
        message: impl Into<String>,
        details: Option<Value>,
    ) -> Self {
        Self { code: ObjectCommandErrorCode::Object(code), message: message.into(), details }
    }

    /// Builds an invalid argument error.
    #[must_use]
    pub(crate) fn invalid_argument(message: impl Into<String>) -> Self {
        Self::from(CliFailure::invalid_argument(message))
    }

    /// Builds an invalid structured input value error.
    #[must_use]
    pub(crate) fn input_invalid_value(details: Value) -> Self {
        Self::from(CliFailure::input_invalid_value(details))
    }

    /// Builds an internal error without exposing implementation details.
    #[must_use]
    pub(crate) fn internal() -> Self {
        Self::from(CliFailure::internal())
    }

    /// Builds an object error from an SDK client error.
    #[must_use]
    pub(crate) fn from_client_error(error: &ClientError) -> Self {
        if let ClientError::Api(api_error) = error {
            let code = match api_error.code() {
                Some("object.not_found") => Some(ObjectErrorCode::NotFound),
                Some("object.archived") => Some(ObjectErrorCode::Archived),
                _ => None,
            };
            if let Some(code) = code {
                return Self::object(code, api_error.message(), None);
            }
        }

        Self::from(CliFailure::from_client_error(error))
    }
}

impl From<CliFailure> for ObjectError {
    fn from(error: CliFailure) -> Self {
        Self::common(error.code, error.message, error.details)
    }
}

impl From<ClientError> for ObjectError {
    fn from(error: ClientError) -> Self {
        Self::from_client_error(&error)
    }
}

impl From<Report> for ObjectError {
    fn from(error: Report) -> Self {
        if let Some(error) = error.downcast_ref::<Self>() {
            return error.clone();
        }
        if let Some(error) = error.downcast_ref::<CliFailure>() {
            return Self::from(error.clone());
        }
        if let Some(error) = error.downcast_ref::<ClientError>() {
            return Self::from_client_error(error);
        }
        Self::internal()
    }
}

impl From<std::io::Error> for ObjectError {
    fn from(_error: std::io::Error) -> Self {
        Self::internal()
    }
}

impl std::fmt::Display for ObjectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ObjectError {}

impl PartialEq<FailureCode> for ObjectCommandErrorCode {
    fn eq(&self, other: &FailureCode) -> bool {
        matches!(self, Self::Common(code) if code == other)
    }
}

impl PartialEq<ObjectErrorCode> for ObjectCommandErrorCode {
    fn eq(&self, other: &ObjectErrorCode) -> bool {
        matches!(self, Self::Object(code) if code == other)
    }
}

impl ObjectHistoryError {
    /// Builds a history-specific error.
    #[must_use]
    pub(crate) fn history(
        code: ObjectHistoryErrorCode,
        message: impl Into<String>,
        details: Option<Value>,
    ) -> Self {
        Self {
            code: ObjectHistoryCommandErrorCode::History(code),
            message: message.into(),
            details,
        }
    }
}

impl From<ObjectError> for ObjectHistoryError {
    fn from(error: ObjectError) -> Self {
        Self {
            code: ObjectHistoryCommandErrorCode::Object(error.code),
            message: error.message,
            details: error.details,
        }
    }
}

impl From<CliFailure> for ObjectHistoryError {
    fn from(error: CliFailure) -> Self {
        Self::from(ObjectError::from(error))
    }
}

impl From<ClientError> for ObjectHistoryError {
    fn from(error: ClientError) -> Self {
        Self::from(ObjectError::from(error))
    }
}

impl From<Report> for ObjectHistoryError {
    fn from(error: Report) -> Self {
        if let Some(error) = error.downcast_ref::<Self>() {
            return error.clone();
        }
        if let Some(error) = error.downcast_ref::<ObjectError>() {
            return Self::from(error.clone());
        }
        if let Some(error) = error.downcast_ref::<CliFailure>() {
            return Self::from(error.clone());
        }
        if let Some(error) = error.downcast_ref::<ClientError>() {
            return Self::from(ObjectError::from_client_error(error));
        }
        Self::from(ObjectError::internal())
    }
}

impl From<std::io::Error> for ObjectHistoryError {
    fn from(error: std::io::Error) -> Self {
        Self::from(ObjectError::from(error))
    }
}

impl std::fmt::Display for ObjectHistoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ObjectHistoryError {}

impl PartialEq<ObjectErrorCode> for ObjectHistoryCommandErrorCode {
    fn eq(&self, other: &ObjectErrorCode) -> bool {
        matches!(self, Self::Object(code) if code == other)
    }
}

impl PartialEq<FailureCode> for ObjectHistoryCommandErrorCode {
    fn eq(&self, other: &FailureCode) -> bool {
        matches!(self, Self::Object(code) if code == other)
    }
}

impl PartialEq<ObjectHistoryErrorCode> for ObjectHistoryCommandErrorCode {
    fn eq(&self, other: &ObjectHistoryErrorCode) -> bool {
        matches!(self, Self::History(code) if code == other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_error_serializes_as_public_error_value() {
        let error = ObjectError::object(ObjectErrorCode::Archived, "Object is archived.", None);

        assert_eq!(
            serde_json::to_value(error).expect("serialize object error"),
            serde_json::json!({
                "code": "object.archived",
                "message": "Object is archived."
            })
        );
    }
}
