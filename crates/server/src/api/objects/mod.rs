//! Object and object version handlers.

use kival_kernel::{EventKind, Object, ReferenceReresolutionSummary};
use kival_sdk::ObjectResource;
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::api::{
    auth::AuthenticatedUser,
    emit::emit_event,
    error::{ApiError, ApiResult},
};

mod attachments;
mod lifecycle;
mod versions;

pub(crate) use attachments::{
    handle_get_attachment, handle_get_attachment_content, handle_list_attachments,
    handle_reuse_attachment, handle_upload_attachment,
};
pub(crate) use lifecycle::{
    handle_archive_object, handle_create_object, handle_get_object, handle_list_objects,
    handle_unarchive_object,
};
pub(crate) use versions::{
    handle_get_version, handle_get_version_wikilinks, handle_list_versions, handle_update_object,
};

/// Converts a kernel object into the HTTP wire representation.
fn api_object(object: Object) -> ObjectResource {
    ObjectResource {
        id: object.id,
        workspace_id: object.workspace_id,
        current_version_id: object.current_version_id,
        title: object.title,
        status: object.status,
        created_by: object.created_by,
        archived_by: object.archived_by,
        created_at: object.created_at,
        updated_at: object.updated_at,
        archived_at: object.archived_at,
    }
}

/// Emits a wikilink re-resolution event when references changed.
async fn emit_wikilink_reresolution_event(
    tx: &mut Transaction<'_, Postgres>,
    durable_queue: &steda::Queue,
    actor: &AuthenticatedUser,
    workspace_id: Uuid,
    triggering_object_id: Uuid,
    affected_titles: &[String],
    summary: ReferenceReresolutionSummary,
) -> ApiResult<()> {
    if !summary.changed() {
        return Ok(());
    }

    emit_event(
        tx,
        durable_queue,
        actor
            .event(
                EventKind::ObjectWikilinksReresolved,
                json!({
                    "affected_titles": affected_titles,
                    "updated_count": summary.updated_count,
                    "resolved_count": summary.resolved_count,
                    "unresolved_count": summary.unresolved_count,
                    "ambiguous_count": summary.ambiguous_count,
                }),
            )
            .workspace(workspace_id)
            .object(triggering_object_id),
    )
    .await
}

/// Validates metadata as a flat JSON object.
///
/// Metadata values may be JSON scalars or one-dimensional arrays of JSON
/// scalars. Nested objects and nested arrays are intentionally rejected so
/// metadata remains a compact set of attributes rather than a second document
/// structure.
fn validate_metadata(metadata: &Value) -> ApiResult<()> {
    let Some(metadata) = metadata.as_object() else {
        return Err(ApiError::bad_request("metadata must be a JSON object"));
    };

    for (key, value) in metadata {
        if !is_metadata_value(value) {
            return Err(ApiError::bad_request(format!(
                "metadata key {key:?} must be a JSON scalar or a one-dimensional array of JSON scalars"
            )));
        }
    }

    Ok(())
}

/// Returns whether a value is allowed as one top-level metadata member.
fn is_metadata_value(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
        Value::Array(values) => values.iter().all(is_metadata_scalar),
        Value::Object(_) => false,
    }
}

/// Returns whether a value is a JSON scalar permitted inside a metadata list.
const fn is_metadata_scalar(value: &Value) -> bool {
    matches!(value, Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::validate_metadata;

    #[test]
    fn flat_metadata_accepts_scalars_and_scalar_lists() {
        validate_metadata(&json!({
            "null": null,
            "boolean": true,
            "number": 2,
            "string": "value",
            "list": ["a", 2, true, null],
            "empty": [],
        }))
        .expect("flat metadata should be valid");
    }

    #[test]
    fn flat_metadata_rejects_nested_objects() {
        validate_metadata(&json!({
            "config": { "enabled": true }
        }))
        .expect_err("nested object metadata should be rejected");
    }

    #[test]
    fn flat_metadata_rejects_nested_lists() {
        validate_metadata(&json!({
            "matrix": [[1, 2], [3, 4]]
        }))
        .expect_err("nested list metadata should be rejected");
    }
}
