//! Object attachment upload, reuse, metadata, and content delivery.

use std::sync::Arc;

use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use kival_kernel::{
    CreateObjectAttachment, EventKind, ObjectAttachmentRow, ReuseObjectAttachment,
    admit_attachment_reuse, create_object_attachment, fetch_object_attachment,
    fetch_object_attachment_content, list_object_attachments, object_version_belongs_to_object,
    reuse_object_attachment,
};
use kival_sdk::{
    ListParams, ListResponse, ObjectAttachment, ObjectAttachmentResponse,
    ReuseObjectAttachmentRequest, UploadObjectAttachmentParams,
};
use kival_storage::BlobRef;
use kival_types::ObjectRole;
use serde_json::json;
use uuid::Uuid;

use super::validate_metadata;
use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        authz::ensure_object_permission,
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        pagination,
        query::QueryParams,
        validate::optional_trimmed,
    },
};

/// Converts a kernel attachment row into its API representation.
fn attachment_into_wire(row: ObjectAttachmentRow) -> ObjectAttachment {
    ObjectAttachment {
        id: row.id,
        workspace_id: row.workspace_id,
        object_id: row.object_id,
        version_id: row.version_id,
        content_ref: row.blob_ref,
        size_bytes: row.size_bytes as u64,
        source_attachment_id: row.source_attachment_id,
        name: row.name,
        media_type: row.media_type,
        metadata: row.metadata,
        created_by: row.created_by,
        created_at: row.created_at,
    }
}

/// Lists object attachments.
pub(crate) async fn handle_list_attachments(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    QueryParams(params): QueryParams<ListParams>,
) -> ApiResult<Json<ListResponse<ObjectAttachment>>> {
    let cursor = pagination::decode_created_at(&params, "object_attachments", Some(object_id))?;
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;

    let attachments = list_object_attachments(
        state.db(),
        actor.id,
        workspace_id,
        object_id,
        cursor.map(|cursor| cursor.created_at),
        cursor.map(|cursor| cursor.id),
        limit + 1,
    )
    .await?;

    let attachments = attachments.into_iter().map(attachment_into_wire).collect();

    Ok(Json(pagination::created_at_page(
        attachments,
        limit,
        "object_attachments",
        Some(object_id),
        |attachment| (attachment.created_at, attachment.id),
    )?))
}

/// Uploads bytes as a new object attachment.
pub(crate) async fn handle_upload_attachment(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    QueryParams(params): QueryParams<UploadObjectAttachmentParams>,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<Json<ObjectAttachmentResponse>> {
    ensure_object_permission(state.db(), actor.id, workspace_id, object_id, ObjectRole::Editor)
        .await?;

    let metadata = match params.metadata.as_deref() {
        Some(metadata) => serde_json::from_str(metadata)
            .map_err(|_error| ApiError::bad_request("metadata must be valid JSON"))?,
        None => json!({}),
    };
    validate_metadata(&metadata)?;
    let name = optional_trimmed(params.name.as_deref(), "name")?;
    let media_type = optional_trimmed(params.media_type.as_deref(), "media_type")?;

    if let Some(version_id) = params.version_id
        && !object_version_belongs_to_object(state.db(), object_id, version_id).await?
    {
        return Err(ApiError::bad_request("version_id must belong to object"));
    }

    let max_len = state.attachment_max_bytes();
    if let Some(content_length) = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        && content_length > max_len
    {
        return Err(ApiError::payload_too_large(format!(
            "attachment exceeds the configured limit of {max_len} bytes"
        )));
    }

    let stored = state.blob_store().put_stream(body.into_data_stream(), max_len).await?;
    let blob_ref = stored.reference.to_string();
    let size_bytes = i64::try_from(stored.len)
        .map_err(|_error| ApiError::internal("attachment size exceeds database range"))?;

    let mut tx = state.db().begin().await?;
    let attachment = attachment_into_wire(
        create_object_attachment(
            &mut tx,
            CreateObjectAttachment {
                workspace_id,
                object_id,
                version_id: params.version_id,
                blob_ref: &blob_ref,
                size_bytes,
                source_attachment_id: None,
                name,
                media_type,
                metadata,
                created_by: actor.id,
            },
        )
        .await?,
    );

    let mut event = actor
        .event(
            EventKind::ObjectAttachmentCreated,
            json!({
                "object_id": object_id,
                "attachment_id": attachment.id,
                "version_id": attachment.version_id,
            }),
        )
        .workspace(workspace_id)
        .object(object_id);

    if let Some(version_id) = attachment.version_id {
        event = event.object_version(version_id);
    }

    emit_event(&mut tx, state.durable_tasks().queue(), event).await?;

    tx.commit().await?;

    Ok(Json(ObjectAttachmentResponse { attachment }))
}

/// Reuses an authorized source attachment on a target object.
pub(crate) async fn handle_reuse_attachment(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    JsonBody(request): JsonBody<ReuseObjectAttachmentRequest>,
) -> ApiResult<Json<ObjectAttachmentResponse>> {
    // Resolve source identity only as part of the complete reuse admission statement. The source
    // remains non-disclosing, while target editor access is evaluated from the same snapshot.
    let source_object_id = admit_attachment_reuse(
        state.db(),
        actor.id,
        workspace_id,
        request.source_attachment_id,
        object_id,
    )
    .await?;

    if let Some(version_id) = request.version_id
        && !object_version_belongs_to_object(state.db(), object_id, version_id).await?
    {
        return Err(ApiError::bad_request("version_id must belong to object"));
    }

    let mut tx = state.db().begin().await?;
    let attachment = attachment_into_wire(
        reuse_object_attachment(
            &mut tx,
            ReuseObjectAttachment {
                workspace_id,
                source_object_id,
                source_attachment_id: request.source_attachment_id,
                target_object_id: object_id,
                target_version_id: request.version_id,
                created_by: actor.id,
            },
        )
        .await?,
    );
    validate_metadata(&attachment.metadata)?;

    let mut event = actor
        .event(
            EventKind::ObjectAttachmentCreated,
            json!({
                "object_id": object_id,
                "attachment_id": attachment.id,
                "version_id": attachment.version_id,
                "source_attachment_id": request.source_attachment_id,
                "name": attachment.name,
                "media_type": attachment.media_type,
            }),
        )
        .workspace(workspace_id)
        .object(object_id);

    if let Some(version_id) = attachment.version_id {
        event = event.object_version(version_id);
    }

    emit_event(&mut tx, state.durable_tasks().queue(), event).await?;
    tx.commit().await?;

    Ok(Json(ObjectAttachmentResponse { attachment }))
}

/// Gets an object attachment by ID.
pub(crate) async fn handle_get_attachment(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id, attachment_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<ObjectAttachmentResponse>> {
    let attachment = attachment_into_wire(
        fetch_object_attachment(state.db(), actor.id, workspace_id, object_id, attachment_id)
            .await?,
    );

    Ok(Json(ObjectAttachmentResponse { attachment }))
}

/// Gets the stored bytes for an object attachment by ID.
pub(crate) async fn handle_get_attachment_content(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id, attachment_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Response> {
    let attachment = fetch_object_attachment_content(
        state.db(),
        actor.id,
        workspace_id,
        object_id,
        attachment_id,
    )
    .await?;

    let blob_ref = attachment
        .blob_ref
        .parse::<BlobRef>()
        .map_err(|_error| ApiError::internal("invalid attachment blob reference"))?;
    let (stream, stored) = state
        .blob_store()
        .get(&blob_ref)
        .await?
        .ok_or_else(|| ApiError::internal("attachment blob is missing"))?;
    if attachment.size_bytes as u64 != stored.len {
        return Err(ApiError::internal("attachment blob length does not match metadata"));
    }

    let (content_type, inline) = attachment.media_type.as_deref().map_or_else(
        || (HeaderValue::from_static("application/octet-stream"), false),
        |media_type| {
            HeaderValue::from_str(media_type).map_or_else(
                |_error| (HeaderValue::from_static("application/octet-stream"), false),
                |header_value| (header_value, is_safe_inline_image_media_type(media_type)),
            )
        },
    );

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static(if inline { "inline" } else { "attachment" }),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&stored.len.to_string())
            .map_err(|_error| ApiError::internal("invalid attachment content length"))?,
    );
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&format!("\"{}\"", stored.reference))
            .map_err(|_error| ApiError::internal("invalid attachment ETag"))?,
    );

    let body = Body::from_stream(stream);
    Ok((headers, body).into_response())
}

/// Returns whether an attachment media type is safe to render inline as an image.
fn is_safe_inline_image_media_type(media_type: &str) -> bool {
    let media_type = media_type.split(';').next().unwrap_or(media_type).trim();

    ["image/gif", "image/jpeg", "image/png", "image/webp"]
        .iter()
        .any(|allowed| media_type.eq_ignore_ascii_case(allowed))
}
