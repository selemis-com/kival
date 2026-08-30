//! Stable Kival objects and their lifecycle transitions.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Acquire, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    ArchiveListStatus, ArchiveStatus, CreateObjectVersion, KernelError, MembershipRole,
    ObjectListOrder, ObjectRole, ObjectVersion, Result, create_object_version,
    parse_optional_stored, parse_stored,
};

/// Input for creating a complete object aggregate with its first immutable version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateInitialObject {
    /// Parent workspace identifier.
    pub workspace_id: Uuid,
    /// Initial object title.
    pub title: String,
    /// Initial object body.
    pub body: String,
    /// Initial object metadata.
    pub metadata: Value,
    /// User creating the object.
    pub created_by: Uuid,
}

/// Result of creating a complete initial object aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedObject {
    /// Stable object identifier.
    pub object_id: Uuid,
    /// Creator's initial administrator-grant identifier.
    pub creator_grant_id: Uuid,
    /// Initial immutable object version.
    pub version: ObjectVersion,
}

/// Stable object identity with lifecycle state and the title of its current version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// Object ID.
    pub id: Uuid,
    /// Parent workspace ID.
    pub workspace_id: Uuid,
    /// Current immutable version ID.
    pub current_version_id: Option<Uuid>,
    /// Title of the current object version.
    pub title: String,
    /// Object lifecycle status as stored in `PostgreSQL`.
    pub status: ArchiveStatus,
    /// User that created this object.
    pub created_by: Option<Uuid>,
    /// User that archived this object.
    pub archived_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last lifecycle or current-version update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Archive timestamp.
    pub archived_at: Option<DateTime<Utc>>,
}

/// Object returned from an actor-authorized read together with the actor's effective role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadableObject {
    /// Authorized object state.
    pub object: Object,
    /// Actor's current effective role for the object.
    pub effective_role: ObjectRole,
}

/// Active object row locked for a mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LockedActiveObject {
    /// Current immutable version ID, when initialized.
    pub current_version_id: Option<Uuid>,
}

/// Read projection for one object in a workspace list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectListEntry {
    /// Core object state.
    pub object: Object,
    /// Username that created the current object version.
    pub updated_by_username: Option<String>,
    /// Display name of the user that created the current object version.
    pub updated_by_display_name: Option<String>,
    /// Updater's active workspace role.
    pub updated_by_workspace_role: Option<MembershipRole>,
    /// Updater's effective access role for this object.
    pub updated_by_object_role: Option<ObjectRole>,
    /// Number of visible active object connections.
    pub connection_count: i64,
    /// Number of unresolved, unexpired commentary threads.
    pub unresolved_thread_count: i64,
    /// Whether the actor has favorited this object.
    pub favorited: bool,
    /// Whether the actor has pinned this object.
    pub pinned: bool,
    /// Time at which the actor pinned this object.
    pub pinned_at: Option<DateTime<Utc>>,
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
struct StoredObject {
    /// Stored row identifier.
    id: Uuid,
    /// Stored workspace identifier.
    workspace_id: Uuid,
    /// Stored current-version identifier.
    current_version_id: Option<Uuid>,
    /// Stored object title.
    title: String,
    /// Stored lifecycle status before typed parsing.
    status: String,
    /// Stored creator identifier, when retained.
    created_by: Option<Uuid>,
    /// Stored archiver identifier, when retained.
    archived_by: Option<Uuid>,
    /// Stored creation timestamp.
    created_at: DateTime<Utc>,
    /// Stored update timestamp.
    updated_at: DateTime<Utc>,
    /// Stored archive timestamp, when present.
    archived_at: Option<DateTime<Utc>>,
}

impl TryFrom<StoredObject> for Object {
    type Error = KernelError;

    fn try_from(row: StoredObject) -> Result<Self> {
        Ok(Self {
            id: row.id,
            workspace_id: row.workspace_id,
            current_version_id: row.current_version_id,
            title: row.title,
            status: parse_stored("object status", row.status)?,
            created_by: row.created_by,
            archived_by: row.archived_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
        })
    }
}

/// Raw actor-authorized object projection including the actor's effective role.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
struct StoredReadableObject {
    /// Stored row identifier.
    id: Uuid,
    /// Stored workspace identifier.
    workspace_id: Uuid,
    /// Stored current-version identifier.
    current_version_id: Option<Uuid>,
    /// Stored object title.
    title: String,
    /// Stored lifecycle status before typed parsing.
    status: String,
    /// Stored creator identifier, when retained.
    created_by: Option<Uuid>,
    /// Stored archiver identifier, when retained.
    archived_by: Option<Uuid>,
    /// Stored creation timestamp.
    created_at: DateTime<Utc>,
    /// Stored update timestamp.
    updated_at: DateTime<Utc>,
    /// Stored archive timestamp, when present.
    archived_at: Option<DateTime<Utc>>,
    /// Actor's effective object role before typed parsing.
    effective_role: String,
}

impl TryFrom<StoredReadableObject> for ReadableObject {
    type Error = KernelError;

    fn try_from(row: StoredReadableObject) -> Result<Self> {
        let effective_role = parse_stored("object role", row.effective_role)?;
        let object = StoredObject {
            id: row.id,
            workspace_id: row.workspace_id,
            current_version_id: row.current_version_id,
            title: row.title,
            status: row.status,
            created_by: row.created_by,
            archived_by: row.archived_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
        }
        .try_into()?;

        Ok(Self { object, effective_role })
    }
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredObjectListEntry {
    /// Stored row identifier.
    id: Uuid,
    /// Stored workspace identifier.
    workspace_id: Uuid,
    /// Stored current-version identifier.
    current_version_id: Option<Uuid>,
    /// Stored object title.
    title: String,
    /// Stored lifecycle status before typed parsing.
    status: String,
    /// Stored creator identifier, when retained.
    created_by: Option<Uuid>,
    /// Stored archiver identifier, when retained.
    archived_by: Option<Uuid>,
    /// Stored creation timestamp.
    created_at: DateTime<Utc>,
    /// Stored update timestamp.
    updated_at: DateTime<Utc>,
    /// Stored archive timestamp, when present.
    archived_at: Option<DateTime<Utc>>,
    /// Stored updater username projection.
    updated_by_username: Option<String>,
    /// Stored updater display-name projection.
    updated_by_display_name: Option<String>,
    /// Stored updater workspace role before typed parsing.
    updated_by_workspace_role: Option<String>,
    /// Stored updater object role before typed parsing.
    updated_by_object_role: Option<String>,
    /// Stored visible connection count.
    connection_count: i64,
    /// Stored unresolved-thread count.
    unresolved_thread_count: i64,
    /// Stored actor-relative favorite projection.
    favorited: bool,
    /// Stored actor-relative pin projection.
    pinned: bool,
    /// Stored pin creation timestamp, when present.
    pinned_at: Option<DateTime<Utc>>,
}

impl TryFrom<StoredObjectListEntry> for ObjectListEntry {
    type Error = KernelError;
    fn try_from(row: StoredObjectListEntry) -> Result<Self> {
        Ok(Self {
            object: Object {
                id: row.id,
                workspace_id: row.workspace_id,
                current_version_id: row.current_version_id,
                title: row.title,
                status: parse_stored("object status", row.status)?,
                created_by: row.created_by,
                archived_by: row.archived_by,
                created_at: row.created_at,
                updated_at: row.updated_at,
                archived_at: row.archived_at,
            },
            updated_by_username: row.updated_by_username,
            updated_by_display_name: row.updated_by_display_name,
            updated_by_workspace_role: parse_optional_stored(
                "workspace membership role",
                row.updated_by_workspace_role,
            )?,
            updated_by_object_role: parse_optional_stored(
                "object role",
                row.updated_by_object_role,
            )?,
            connection_count: row.connection_count,
            unresolved_thread_count: row.unresolved_thread_count,
            favorited: row.favorited,
            pinned: row.pinned,
            pinned_at: row.pinned_at,
        })
    }
}

/// Parameters for the workspace object-list projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListObjects {
    /// Workspace whose objects are listed.
    pub workspace_id: Uuid,
    /// User whose access and personal state shape the projection.
    pub actor_id: Uuid,
    /// Timestamp component of the keyset cursor.
    pub cursor_at: Option<DateTime<Utc>>,
    /// Object ID component of the keyset cursor.
    pub cursor_id: Option<Uuid>,
    /// Maximum rows to return from `PostgreSQL`.
    pub limit: i64,
    /// Archive filter in `PostgreSQL` vocabulary: `active`, `archived`, or `all`.
    pub status: ArchiveListStatus,
    /// Sort field in `PostgreSQL` vocabulary: `created` or `updated`.
    pub order: ObjectListOrder,
    /// Optional actor-favorite filter.
    pub favorited: Option<bool>,
    /// Optional actor-pin filter.
    pub pinned: Option<bool>,
}

/// Lists visible objects in a workspace using Kival's access projection.
///
/// The typed request constrains filter and ordering vocabulary; `PostgreSQL` evaluates
/// object visibility and archived-object administration requirements.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot load the projection.
pub async fn list_objects(pool: &PgPool, request: ListObjects) -> Result<Vec<ObjectListEntry>> {
    sqlx::query_as::<_, StoredObjectListEntry>(
        r#"
        SELECT
            o.id,
            o.workspace_id,
            o.current_version_id,
            current_version.title,
            o.status,
            o.created_by,
            o.archived_by,
            o.created_at,
            o.updated_at,
            o.archived_at,
            updated_by.username AS updated_by_username,
            updated_by.display_name AS updated_by_display_name,
            CASE
                WHEN updated_by_global_admin.user_id IS NOT NULL THEN 'admin'
                ELSE updated_by_membership.workspace_role
            END AS updated_by_workspace_role,
            kival.object_access_role(
                o.workspace_id,
                o.id,
                current_version.created_by
            )::text AS updated_by_object_role,
            (
                SELECT COUNT(*)::bigint
                FROM kival.object_edges edge
                JOIN kival.objects related_object
                    ON related_object.workspace_id = edge.workspace_id
                    AND related_object.id = CASE
                     WHEN edge.source_object_id = o.id THEN edge.target_object_id
                     ELSE edge.source_object_id
                 END
                WHERE edge.workspace_id = o.workspace_id
                    AND edge.revoked_at IS NULL
                    AND (edge.source_object_id = o.id OR edge.target_object_id = o.id)
                    AND related_object.archived_at IS NULL
                    AND kival.has_object_permission(
                      related_object.workspace_id,
                      related_object.id,
                      $2,
                      'viewer'::kival.object_role
                  )
            ) AS connection_count,
            (
                SELECT COUNT(*)::bigint
                FROM kival.comment_threads thread
                WHERE thread.workspace_id = o.workspace_id
                    AND thread.object_id = o.id
                    AND thread.resolved_at IS NULL
                    AND (
                      thread.retention_expires_at IS NULL
                    OR thread.retention_expires_at > now()
                  )
            ) AS unresolved_thread_count,
            EXISTS (
                SELECT 1
                FROM kival.object_favorites favorite
                WHERE favorite.user_id = $2
                    AND favorite.workspace_id = o.workspace_id
                    AND favorite.object_id = o.id
            ) AS favorited,
            EXISTS (
                SELECT 1
                FROM kival.object_pins pin
                WHERE pin.user_id = $2
                    AND pin.workspace_id = o.workspace_id
                    AND pin.object_id = o.id
            ) AS pinned,
            (
                SELECT pin.created_at
                FROM kival.object_pins pin
                WHERE pin.user_id = $2
                    AND pin.workspace_id = o.workspace_id
                    AND pin.object_id = o.id
            ) AS pinned_at
        FROM kival.objects o
        JOIN kival.workspaces w
            ON w.id = o.workspace_id
            AND w.archived_at IS NULL
        JOIN kival.object_versions current_version
            ON current_version.object_id = o.id
            AND current_version.id = o.current_version_id
        LEFT JOIN kival.users updated_by
            ON updated_by.id = current_version.created_by
        LEFT JOIN kival.global_admins updated_by_global_admin
            ON updated_by_global_admin.user_id = current_version.created_by
            AND updated_by_global_admin.revoked_at IS NULL
        LEFT JOIN kival.workspace_memberships updated_by_membership
            ON updated_by_membership.workspace_id = o.workspace_id
            AND updated_by_membership.user_id = current_version.created_by
            AND updated_by_membership.revoked_at IS NULL
        WHERE o.workspace_id = $1
            AND (
              $6 = 'all'
            OR ($6 = 'active' AND o.archived_at IS NULL)
            OR ($6 = 'archived' AND o.archived_at IS NOT NULL)
          )
            AND (
              $7::text = 'created'
            AND (
                  $3::timestamptz IS NULL
            OR (o.created_at, o.id) < ($3, $4)
              )
            OR $7::text = 'updated'
            AND (
                  $3::timestamptz IS NULL
            OR (o.updated_at, o.id) < ($3, $4)
              )
          )
            AND kival.has_object_permission(
              $1,
              o.id,
              $2,
              CASE
                  WHEN o.archived_at IS NULL THEN 'viewer'::kival.object_role
                  ELSE 'admin'::kival.object_role
              END
          )
            AND (
              $8::boolean IS NULL
            OR EXISTS (
                  SELECT 1
                  FROM kival.object_favorites favorite_filter
                  WHERE favorite_filter.user_id = $2
                      AND favorite_filter.workspace_id = o.workspace_id
                      AND favorite_filter.object_id = o.id
              ) = $8
          )
                      AND (
              $9::boolean IS NULL
                      OR EXISTS (
                  SELECT 1
                  FROM kival.object_pins pin_filter
                  WHERE pin_filter.user_id = $2
                      AND pin_filter.workspace_id = o.workspace_id
                      AND pin_filter.object_id = o.id
              ) = $9
          )
        ORDER BY
            CASE WHEN $7::text = 'created' THEN o.created_at END DESC,
            CASE WHEN $7::text = 'updated' THEN o.updated_at END DESC,
            o.id DESC
        LIMIT $5
        OFFSET CASE WHEN kival.require_read_workspace($1, $2) THEN 0 ELSE 0 END
        "#,
    )
    .bind(request.workspace_id)
    .bind(request.actor_id)
    .bind(request.cursor_at)
    .bind(request.cursor_id)
    .bind(request.limit)
    .bind(request.status.as_str())
    .bind(request.order.as_str())
    .bind(request.favorited)
    .bind(request.pinned)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(TryInto::try_into)
    .collect()
}

/// Creates an object, its creator administrator grant, and its first immutable version.
///
/// All pieces are created in the caller's transaction and the object is pointed at version one
/// before this function succeeds, so callers cannot accidentally expose a half-created object.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects any part of the initial object transition.
pub async fn create_initial_object(
    tx: &mut Transaction<'_, Postgres>,
    input: CreateInitialObject,
) -> Result<CreatedObject> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result = create_initial_object_in_savepoint(&mut savepoint, input).await;

    match result {
        Ok(created) => {
            savepoint.commit().await?;
            Ok(created)
        }
        Err(error) => {
            savepoint.rollback().await?;
            Err(error)
        }
    }
}

/// Applies initial-object creation inside a cancellation-safe savepoint.
async fn create_initial_object_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    input: CreateInitialObject,
) -> Result<CreatedObject> {
    if !crate::workspaces::lock_active_workspace_for_child(tx, input.workspace_id).await? {
        return Err(KernelError::ResourceNotFound);
    }

    let object_id = create_object(tx, input.workspace_id, input.created_by).await?;
    let creator_grant_id = crate::object_grants::create_creator_admin_grant(
        tx,
        input.workspace_id,
        object_id,
        input.created_by,
    )
    .await?;
    let version = create_object_version(
        tx,
        CreateObjectVersion {
            object_id,
            version_number: 1,
            title: input.title,
            body: input.body,
            metadata: input.metadata,
            created_by: Some(input.created_by),
        },
    )
    .await?;
    initialize_object_current_version(tx, object_id, version.id).await?;

    Ok(CreatedObject { object_id, creator_grant_id, version })
}

/// Creates the stable identity for a new object.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects or cannot persist the object.
pub(crate) async fn create_object(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    created_by: Uuid,
) -> Result<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO kival.objects (workspace_id, created_by)
        VALUES ($1, $2)
        RETURNING id
        "#,
    )
    .bind(workspace_id)
    .bind(created_by)
    .fetch_one(&mut **tx)
    .await
    .map_err(Into::into)
}

/// Points a newly created object at its initial immutable version.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub(crate) async fn initialize_object_current_version(
    tx: &mut Transaction<'_, Postgres>,
    object_id: Uuid,
    version_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE kival.objects
        SET current_version_id = $2
        WHERE id = $1
        "#,
    )
    .bind(object_id)
    .bind(version_id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// Pins active object lifecycles in deterministic workspace/object-ID order.
///
/// The workspace is pinned first, then distinct object rows are acquired with `FOR SHARE` in UUID
/// order. This is the canonical lifecycle dependency lock for transitions that reference objects
/// without mutating the object rows themselves.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot perform the locks.
pub(crate) async fn lock_active_objects_for_reference(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_ids: &[Uuid],
) -> Result<bool> {
    if !crate::workspaces::lock_active_workspace_for_child(tx, workspace_id).await? {
        return Ok(false);
    }

    let mut object_ids = object_ids.to_vec();
    object_ids.sort_unstable();
    object_ids.dedup();
    for object_id in object_ids {
        let exists = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id
            FROM kival.objects
            WHERE workspace_id = $1
                AND id = $2
                AND archived_at IS NULL
            FOR SHARE
            "#,
        )
        .bind(workspace_id)
        .bind(object_id)
        .fetch_optional(&mut **tx)
        .await?
        .is_some();
        if !exists {
            return Ok(false);
        }
    }

    Ok(true)
}

/// Locks an active workspace and object for a mutation.
///
/// The workspace is held with `FOR SHARE` before the object is held with `FOR UPDATE`,
/// preventing workspace archival from committing while the object mutation is in flight.
/// Returns `None` when the workspace or object is not active.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot perform the lock.
pub(crate) async fn lock_active_object(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<Option<LockedActiveObject>> {
    sqlx::query_scalar::<_, Option<Uuid>>(
        r#"
        WITH active_workspace AS MATERIALIZED (
            SELECT id
            FROM kival.workspaces
            WHERE id = $1
                AND archived_at IS NULL
            FOR SHARE
        )
        SELECT object.current_version_id
        FROM kival.objects object
        JOIN active_workspace workspace
            ON workspace.id = object.workspace_id
        WHERE object.id = $2
            AND object.archived_at IS NULL
        FOR UPDATE OF object
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .fetch_optional(&mut **tx)
    .await
    .map(|row| row.map(|current_version_id| LockedActiveObject { current_version_id }))
    .map_err(Into::into)
}

/// Makes an immutable version current for an active object.
///
/// The caller is expected to hold the object's row lock for the surrounding
/// mutation transaction.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub(crate) async fn set_active_object_version(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    version_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE kival.objects
        SET current_version_id = $2
        WHERE workspace_id = $1
            AND id = $3
            AND archived_at IS NULL
        "#,
    )
    .bind(workspace_id)
    .bind(version_id)
    .bind(object_id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// Locks an active workspace and archived object for a lifecycle mutation.
///
/// The workspace is held with `FOR SHARE` before the object is held with `FOR UPDATE`,
/// preventing workspace archival from committing while the lifecycle mutation is in flight.
/// Returns `false` when the workspace is not active or the object is not archived.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot perform the lock.
pub(crate) async fn lock_archived_object(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        r#"
        WITH active_workspace AS MATERIALIZED (
            SELECT id
            FROM kival.workspaces
            WHERE id = $1
                AND archived_at IS NULL
            FOR SHARE
        )
        SELECT object.id
        FROM kival.objects object
        JOIN active_workspace workspace
            ON workspace.id = object.workspace_id
        WHERE object.id = $2
            AND object.archived_at IS NOT NULL
        FOR UPDATE OF object
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .fetch_optional(&mut **tx)
    .await?
    .is_some())
}

/// Archives an active object whose row is already locked by the caller.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub(crate) async fn archive_locked_object(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    archived_by: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE kival.objects
        SET status = 'archived',
            archived_at = now(),
            archived_by = $3
        WHERE workspace_id = $1
            AND id = $2
            AND archived_at IS NULL
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(archived_by)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// Unarchives an archived object whose row is already locked by the caller.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub(crate) async fn unarchive_locked_object(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE kival.objects
        SET status = 'active',
            archived_at = NULL,
            archived_by = NULL
        WHERE workspace_id = $1
            AND id = $2
            AND archived_at IS NOT NULL
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// Archives an active object while preserving workspace/object lifecycle ordering.
///
/// # Errors
///
/// Returns an error if the object is not active or `PostgreSQL` rejects the transition.
pub async fn archive_object(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    archived_by: Uuid,
) -> Result<()> {
    if lock_active_object(tx, workspace_id, object_id).await?.is_none() {
        return Err(KernelError::ResourceNotFound);
    }
    archive_locked_object(tx, workspace_id, object_id, archived_by).await
}

/// Unarchives an object while preserving workspace/object lifecycle ordering.
///
/// # Errors
///
/// Returns an error if the workspace/object lifecycle is incompatible with the transition or
/// `PostgreSQL` rejects it.
pub async fn unarchive_object(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<()> {
    if !lock_archived_object(tx, workspace_id, object_id).await? {
        return Err(KernelError::ResourceNotFound);
    }
    unarchive_locked_object(tx, workspace_id, object_id).await
}

/// Fetches a complete object from its current immutable version.
///
/// # Errors
///
/// Returns an error if the object cannot be loaded.
pub async fn fetch_object(
    pool: &PgPool,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<ReadableObject> {
    sqlx::query_as::<_, StoredReadableObject>(
        r#"
        SELECT
            object.id,
            object.workspace_id,
            object.current_version_id,
            current_version.title,
            object.status,
            object.created_by,
            object.archived_by,
            object.created_at,
            object.updated_at,
            object.archived_at,
            kival.object_access_role($1, $2, $3)::text AS effective_role
        FROM kival.objects object
        JOIN kival.object_versions current_version
            ON current_version.object_id = object.id
            AND current_version.id = object.current_version_id
        WHERE object.workspace_id = $1
            AND object.id = $2
            AND kival.user_can_read_object($1, $2, $3)
        OFFSET CASE WHEN kival.require_read_object($1, $2, $3) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(actor_id)
    .fetch_one(pool)
    .await?
    .try_into()
}

/// Fetches a complete object inside an existing mutation transaction.
///
/// # Errors
///
/// Returns an error if the object cannot be loaded.
pub async fn fetch_object_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<Object> {
    sqlx::query_as::<_, StoredObject>(OBJECT_SELECT)
        .bind(workspace_id)
        .bind(object_id)
        .fetch_one(&mut **tx)
        .await?
        .try_into()
}

/// Shared object projection used by pool and transaction reads.
const OBJECT_SELECT: &str = r#"
    SELECT
        object.id,
        object.workspace_id,
        object.current_version_id,
        current_version.title,
        object.status,
        object.created_by,
        object.archived_by,
        object.created_at,
        object.updated_at,
        object.archived_at
    FROM kival.objects object
    JOIN kival.object_versions current_version
        ON current_version.object_id = object.id
        AND current_version.id = object.current_version_id
    WHERE object.workspace_id = $1
        AND object.id = $2
"#;
