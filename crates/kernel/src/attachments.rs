//! Object attachment state bindings.

use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{KernelError, Result};

/// Stored object-attachment projection.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ObjectAttachmentRow {
    /// Row identifier.
    pub id: Uuid,
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// Object identifier.
    pub object_id: Uuid,
    /// Object-version identifier.
    pub version_id: Option<Uuid>,
    /// Content-addressed blob reference.
    pub blob_ref: String,
    /// Blob size in bytes.
    pub size_bytes: i64,
    /// Optional source attachment copied into this row.
    pub source_attachment_id: Option<Uuid>,
    /// Optional attachment name.
    pub name: Option<String>,
    /// Optional media type.
    pub media_type: Option<String>,
    /// Structured attachment metadata.
    pub metadata: Value,
    /// User that created the row, when retained.
    pub created_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

/// Content-addressing data required to serve an attachment.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ObjectAttachmentContentRow {
    /// Content-addressed blob reference.
    pub blob_ref: String,
    /// Blob size in bytes.
    pub size_bytes: i64,
    /// Optional media type.
    pub media_type: Option<String>,
}

/// Values used to insert an object attachment.
#[derive(Debug, Clone)]
pub struct CreateObjectAttachment<'a> {
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// Object identifier.
    pub object_id: Uuid,
    /// Object-version identifier.
    pub version_id: Option<Uuid>,
    /// Content-addressed blob reference.
    pub blob_ref: &'a str,
    /// Blob size in bytes.
    pub size_bytes: i64,
    /// Optional source attachment copied into this row.
    pub source_attachment_id: Option<Uuid>,
    /// Optional attachment name.
    pub name: Option<&'a str>,
    /// Optional media type.
    pub media_type: Option<&'a str>,
    /// Structured attachment metadata.
    pub metadata: Value,
    /// User that created the row, when retained.
    pub created_by: Uuid,
}

/// Values used to reuse an existing attachment on another object.
#[derive(Debug, Clone, Copy)]
pub struct ReuseObjectAttachment {
    /// Workspace containing both objects.
    pub workspace_id: Uuid,
    /// Object that owns the source attachment.
    pub source_object_id: Uuid,
    /// Existing attachment to reuse.
    pub source_attachment_id: Uuid,
    /// Target object receiving the reused attachment.
    pub target_object_id: Uuid,
    /// Optional version on the target object.
    pub target_version_id: Option<Uuid>,
    /// User creating the target attachment row.
    pub created_by: Uuid,
}

/// Lists attachments for an object in reverse creation order.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn list_object_attachments(
    pool: &PgPool,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    cursor_created_at: Option<DateTime<Utc>>,
    cursor_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<ObjectAttachmentRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT
            id, workspace_id, object_id, version_id, blob_ref, size_bytes, source_attachment_id,
            name, media_type, metadata, created_by, created_at
        FROM kival.object_attachments
        WHERE workspace_id = $1
            AND object_id = $2
            AND kival.user_can_read_object($1, $2, $3)
            AND ($4::timestamptz IS NULL OR (created_at, id) < ($4, $5))
        ORDER BY created_at DESC, id DESC
        LIMIT $6
        OFFSET CASE WHEN kival.require_read_object($1, $2, $3) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(actor_id)
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// Admits attachment reuse and returns the readable source attachment's owning object.
///
/// Source attachment identity/readability and target-editor access are evaluated in one
/// `PostgreSQL` statement snapshot. Missing and unreadable source attachments are deliberately
/// indistinguishable
/// and raise the kernel not-found capability error before target authorization is evaluated.
///
/// # Errors
///
/// Returns an error if the source attachment is unavailable, target edit access is unavailable, or
/// the underlying `PostgreSQL` operation fails.
pub async fn admit_attachment_reuse(
    pool: &PgPool,
    actor_id: Uuid,
    workspace_id: Uuid,
    source_attachment_id: Uuid,
    target_object_id: Uuid,
) -> Result<Uuid> {
    let source_object_id = sqlx::query_scalar::<_, Option<Uuid>>(
        r#"
        WITH source AS MATERIALIZED (
            SELECT attachment.object_id
            FROM kival.object_attachments attachment
            JOIN kival.objects object
                ON object.workspace_id = attachment.workspace_id
                AND object.id = attachment.object_id
                AND object.archived_at IS NULL
            JOIN kival.workspaces workspace
                ON workspace.id = object.workspace_id
                AND workspace.archived_at IS NULL
            WHERE attachment.workspace_id = $1
                AND attachment.id = $2
                AND kival.has_object_permission(
                    attachment.workspace_id,
                    attachment.object_id,
                    $3,
                    'viewer'::kival.object_role
                )
        )
        SELECT CASE
            WHEN kival.require_capability(EXISTS (SELECT 1 FROM source), TRUE)
            THEN CASE
                WHEN kival.require_access_active_object(
                    $1,
                    $4,
                    $3,
                    'editor'::kival.object_role
                )
                THEN (SELECT object_id FROM source)
                ELSE NULL::uuid
            END
            ELSE NULL::uuid
        END
        "#,
    )
    .bind(workspace_id)
    .bind(source_attachment_id)
    .bind(actor_id)
    .bind(target_object_id)
    .fetch_one(pool)
    .await?;

    source_object_id.ok_or(KernelError::ResourceNotFound)
}

/// Loads an active attachment inside an existing transaction.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
async fn fetch_active_attachment_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    attachment_id: Uuid,
) -> Result<Option<ObjectAttachmentRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT
            a.id, a.workspace_id, a.object_id, a.version_id, a.blob_ref, a.size_bytes,
            a.source_attachment_id, a.name, a.media_type, a.metadata, a.created_by, a.created_at
        FROM kival.object_attachments a
        JOIN kival.objects o
            ON o.workspace_id = a.workspace_id
            AND o.id = a.object_id
            AND o.archived_at IS NULL
        WHERE a.workspace_id = $1
            AND a.object_id = $2
            AND a.id = $3
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(attachment_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Loads one attachment belonging to an object.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn fetch_object_attachment(
    pool: &PgPool,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    attachment_id: Uuid,
) -> Result<ObjectAttachmentRow> {
    Ok(sqlx::query_as(
        r#"
        SELECT
            id, workspace_id, object_id, version_id, blob_ref, size_bytes, source_attachment_id,
            name, media_type, metadata, created_by, created_at
        FROM kival.object_attachments
        WHERE workspace_id = $1
            AND object_id = $2
            AND id = $3
            AND kival.user_can_read_object($1, $2, $4)
        OFFSET CASE WHEN kival.require_read_object($1, $2, $4) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(attachment_id)
    .bind(actor_id)
    .fetch_one(pool)
    .await?)
}

/// Loads the content-addressing fields required to serve an attachment.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn fetch_object_attachment_content(
    pool: &PgPool,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    attachment_id: Uuid,
) -> Result<ObjectAttachmentContentRow> {
    Ok(sqlx::query_as(
        r#"
        SELECT blob_ref, size_bytes, media_type
        FROM kival.object_attachments
                   WHERE workspace_id = $1
                       AND object_id = $2
                       AND id = $3
                       AND kival.user_can_read_object($1, $2, $4)
        OFFSET CASE WHEN kival.require_read_object($1, $2, $4) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(attachment_id)
    .bind(actor_id)
    .fetch_one(pool)
    .await?)
}

/// Creates an attachment row for an already-authorized object mutation.
///
/// The transition pins the target object lifecycle and validates any target version immediately
/// before publishing attachment metadata.
///
/// # Errors
///
/// Returns an error if the target lifecycle/version is invalid or `PostgreSQL` rejects the row.
pub async fn create_object_attachment(
    tx: &mut Transaction<'_, Postgres>,
    attachment: CreateObjectAttachment<'_>,
) -> Result<ObjectAttachmentRow> {
    if !crate::objects::lock_active_objects_for_reference(
        tx,
        attachment.workspace_id,
        &[attachment.object_id],
    )
    .await?
    {
        return Err(KernelError::ResourceNotFound);
    }
    if let Some(version_id) = attachment.version_id
        && !object_version_belongs_to_object_in_tx(tx, attachment.object_id, version_id).await?
    {
        return Err(KernelError::InvalidAttachmentVersion);
    }

    create_object_attachment_unchecked(tx, attachment).await
}

/// Reuses one active attachment after pinning both object lifecycles in canonical order.
///
/// # Errors
///
/// Returns an error if either object, the source attachment, or target version is invalid.
pub async fn reuse_object_attachment(
    tx: &mut Transaction<'_, Postgres>,
    request: ReuseObjectAttachment,
) -> Result<ObjectAttachmentRow> {
    if !crate::objects::lock_active_objects_for_reference(
        tx,
        request.workspace_id,
        &[request.source_object_id, request.target_object_id],
    )
    .await?
    {
        return Err(KernelError::ResourceNotFound);
    }

    let source = fetch_active_attachment_in_tx(
        tx,
        request.workspace_id,
        request.source_object_id,
        request.source_attachment_id,
    )
    .await?
    .ok_or(KernelError::ResourceNotFound)?;

    if let Some(version_id) = request.target_version_id
        && !object_version_belongs_to_object_in_tx(tx, request.target_object_id, version_id).await?
    {
        return Err(KernelError::InvalidAttachmentVersion);
    }

    create_object_attachment_unchecked(
        tx,
        CreateObjectAttachment {
            workspace_id: request.workspace_id,
            object_id: request.target_object_id,
            version_id: request.target_version_id,
            blob_ref: &source.blob_ref,
            size_bytes: source.size_bytes,
            source_attachment_id: Some(request.source_attachment_id),
            name: source.name.as_deref(),
            media_type: source.media_type.as_deref(),
            metadata: source.metadata,
            created_by: request.created_by,
        },
    )
    .await
}

/// Inserts attachment metadata after lifecycle dependencies have been validated.
async fn create_object_attachment_unchecked(
    tx: &mut Transaction<'_, Postgres>,
    attachment: CreateObjectAttachment<'_>,
) -> Result<ObjectAttachmentRow> {
    Ok(sqlx::query_as(
        r#"
        INSERT INTO kival.object_attachments (
            workspace_id, object_id, version_id, blob_ref, size_bytes, source_attachment_id,
            name, media_type, metadata, created_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        RETURNING
            id, workspace_id, object_id, version_id, blob_ref, size_bytes, source_attachment_id,
            name, media_type, metadata, created_by, created_at
        "#,
    )
    .bind(attachment.workspace_id)
    .bind(attachment.object_id)
    .bind(attachment.version_id)
    .bind(attachment.blob_ref)
    .bind(attachment.size_bytes)
    .bind(attachment.source_attachment_id)
    .bind(attachment.name)
    .bind(attachment.media_type)
    .bind(attachment.metadata)
    .bind(attachment.created_by)
    .fetch_one(&mut **tx)
    .await?)
}

/// Returns whether a version belongs to the requested object before expensive external work.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot perform the lookup.
pub async fn object_version_belongs_to_object(
    pool: &PgPool,
    object_id: Uuid,
    version_id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM kival.object_versions WHERE object_id = $1 AND id = $2",
    )
    .bind(object_id)
    .bind(version_id)
    .fetch_optional(pool)
    .await?
    .is_some())
}

/// Returns whether a version belongs to the requested object inside a transition.
async fn object_version_belongs_to_object_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    object_id: Uuid,
    version_id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM kival.object_versions WHERE object_id = $1 AND id = $2",
    )
    .bind(object_id)
    .bind(version_id)
    .fetch_optional(&mut **tx)
    .await?
    .is_some())
}
