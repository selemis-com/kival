//! Shared CLI argument helpers.

use clap::ValueEnum;
use eyre::Result;
use kival_sdk::{
    ArchiveListStatus, EventListParams, EventOrder, GrantPrincipal, ListParams, MembershipRole,
    ObjectRole,
};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::utils::error::CliError;

/// Default page size used when the user does not pass `--limit`.
///
/// Sending an explicit limit keeps human and JSON output consistent across
/// endpoints whose server-side defaults may differ.
pub const DEFAULT_LIST_LIMIT: i64 = 50;
/// String form of the default page size for Clap help metadata.
pub const DEFAULT_LIST_LIMIT_HELP: &str = "50";

/// Builds standard list parameters.
#[must_use]
pub const fn list_params(limit: Option<i64>, cursor: Option<String>) -> ListParams {
    let limit = match limit {
        Some(limit) => limit,
        None => DEFAULT_LIST_LIMIT,
    };

    ListParams { limit: Some(limit), cursor }
}

/// Builds event list parameters from common event filter flags.
#[must_use]
pub const fn event_params(
    limit: Option<i64>,
    after_sequence: Option<i64>,
    event_kind: Option<String>,
    actor_user_id: Option<Uuid>,
    target_user_id: Option<Uuid>,
    object_id: Option<Uuid>,
    group_id: Option<Uuid>,
) -> EventListParams {
    EventListParams {
        limit,
        after_sequence,
        before_sequence: None,
        order: EventOrder::Asc,
        event_kind,
        actor_user_id,
        target_user_id,
        object_id,
        group_id,
    }
}

/// CLI archive status values accepted by list and search commands.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliArchiveListStatus {
    /// Only active resources.
    Active,
    /// Only archived resources.
    Archived,
    /// Active and archived resources.
    All,
}

impl From<CliArchiveListStatus> for ArchiveListStatus {
    fn from(status: CliArchiveListStatus) -> Self {
        match status {
            CliArchiveListStatus::Active => Self::Active,
            CliArchiveListStatus::Archived => Self::Archived,
            CliArchiveListStatus::All => Self::All,
        }
    }
}

/// CLI membership role values accepted by membership commands.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliMembershipRole {
    /// Regular member access.
    Member,
    /// Administrative access.
    Admin,
}

impl From<CliMembershipRole> for MembershipRole {
    fn from(role: CliMembershipRole) -> Self {
        match role {
            CliMembershipRole::Member => Self::Member,
            CliMembershipRole::Admin => Self::Admin,
        }
    }
}

/// CLI object role values accepted by object grant commands.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliObjectRole {
    /// Viewer access.
    Viewer,
    /// Editor access.
    Editor,
    /// Admin access.
    Admin,
}

impl From<CliObjectRole> for ObjectRole {
    fn from(role: CliObjectRole) -> Self {
        match role {
            CliObjectRole::Viewer => Self::Viewer,
            CliObjectRole::Editor => Self::Editor,
            CliObjectRole::Admin => Self::Admin,
        }
    }
}

/// Parses an object grant principal.
///
/// # Errors
///
/// Returns an error if zero or multiple principals are provided.
pub fn grant_principal(user_id: Option<Uuid>, group_id: Option<Uuid>) -> Result<GrantPrincipal> {
    match (user_id, group_id) {
        (Some(user_id), None) => Ok(GrantPrincipal::User(user_id)),
        (None, Some(group_id)) => Ok(GrantPrincipal::Group(group_id)),
        (None, None) => Err(CliError::invalid_argument("one principal must be provided").into()),
        (Some(_), Some(_)) => {
            Err(CliError::invalid_argument("only one principal may be provided").into())
        }
    }
}

/// Parses optional flat JSON metadata, defaulting to an empty object.
///
/// # Errors
///
/// Returns an error if the JSON string cannot be decoded or contains nested
/// objects or arrays.
pub fn metadata_value(metadata: Option<&str>) -> Result<Value> {
    let value = match metadata {
        Some(value) => serde_json::from_str(value).map_err(|error| {
            CliError::invalid_argument(format!("metadata must be valid JSON: {error}"))
        })?,
        None => Value::Object(Map::new()),
    };

    let Value::Object(metadata) = &value else {
        return Err(CliError::invalid_argument("metadata must be a JSON object").into());
    };
    validate_flat_metadata(metadata)?;

    Ok(value)
}

/// Validates metadata as top-level keys with scalar or scalar-list values.
///
/// # Errors
///
/// Returns an error when a metadata member contains an object or nested array.
pub fn validate_flat_metadata(metadata: &Map<String, Value>) -> Result<()> {
    for (key, value) in metadata {
        validate_flat_metadata_member(key, value)?;
    }
    Ok(())
}

/// Validates one top-level metadata member.
///
/// # Errors
///
/// Returns an error when the value is an object or contains a nested array/object.
pub fn validate_flat_metadata_member(key: &str, value: &Value) -> Result<()> {
    let valid = match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
        Value::Array(values) => values.iter().all(|value| {
            matches!(value, Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_))
        }),
        Value::Object(_) => false,
    };

    if valid {
        Ok(())
    } else {
        Err(CliError::invalid_argument(format!(
            "metadata key {key:?} must be a JSON scalar or a one-dimensional array of JSON scalars"
        ))
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::metadata_value;

    #[test]
    fn metadata_value_accepts_flat_values() {
        metadata_value(Some(r#"{"a":true,"b":2,"c":"x","d":null,"e":["x",2,true,null]}"#))
            .expect("flat metadata should be accepted");
    }

    #[test]
    fn metadata_value_rejects_nested_object() {
        assert!(metadata_value(Some(r#"{"config":{"enabled":true}}"#)).is_err());
    }

    #[test]
    fn metadata_value_rejects_nested_array() {
        assert!(metadata_value(Some(r#"{"matrix":[[1,2],[3,4]]}"#)).is_err());
    }
}
