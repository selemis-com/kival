//! Small request validation helpers shared by API handlers.

use crate::api::error::{ApiError, ApiResult};

/// Trims a required string and rejects it when blank.
pub(crate) fn required_trimmed<'a>(value: &'a str, label: &str) -> ApiResult<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        Err(ApiError::bad_request(format!("{label} must not be blank")))
    } else {
        Ok(value)
    }
}

/// Trims an optional string and rejects it when present but blank.
pub(crate) fn optional_trimmed<'a>(
    value: Option<&'a str>,
    label: &str,
) -> ApiResult<Option<&'a str>> {
    value.map(|value| required_trimmed(value, label)).transpose()
}
