//! Workspace-to-group link state bindings.

use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{ArchiveListStatus, ArchiveStatus, KernelError, Result, parse_stored};

/// Workspace-to-group link projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceGroupRow {
    /// Link ID.
    pub id: Uuid,
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Group ID.
    pub group_id: Uuid,
    /// Group name.
    pub group_name: String,
    /// Group description.
    pub group_description: Option<String>,
    /// Link lifecycle status.
    pub status: ArchiveStatus,
    /// Creator.
    pub created_by: Option<Uuid>,
    /// Archiver.
    pub archived_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: OffsetDateTime,
    /// Update timestamp.
    pub updated_at: OffsetDateTime,
    /// Archive timestamp.
    pub archived_at: Option<OffsetDateTime>,
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredWorkspaceGroupRow {
    /// Stored row identifier.
    id: Uuid,
    /// Stored workspace identifier.
    workspace_id: Uuid,
    /// Stored group identifier.
    group_id: Uuid,
    /// Stored group name projection.
    group_name: String,
    /// Stored group description projection.
    group_description: Option<String>,
    /// Stored lifecycle status before typed parsing.
    status: String,
    /// Stored creator identifier, when retained.
    created_by: Option<Uuid>,
    /// Stored archiver identifier, when retained.
    archived_by: Option<Uuid>,
    /// Stored creation timestamp.
    created_at: OffsetDateTime,
    /// Stored update timestamp.
    updated_at: OffsetDateTime,
    /// Stored archive timestamp, when present.
    archived_at: Option<OffsetDateTime>,
}

impl TryFrom<StoredWorkspaceGroupRow> for WorkspaceGroupRow {
    type Error = KernelError;

    fn try_from(row: StoredWorkspaceGroupRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            workspace_id: row.workspace_id,
            group_id: row.group_id,
            group_name: row.group_name,
            group_description: row.group_description,
            status: parse_stored("workspace group status", row.status)?,
            created_by: row.created_by,
            archived_by: row.archived_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
        })
    }
}

/// Pins an active group-to-workspace link when it exists.
///
/// The object/workspace parent must already be pinned by the calling transition. The group and
/// link are lifecycle dependencies, so `FOR SHARE` prevents archival without serializing unrelated
/// references.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot perform the lock.
pub(crate) async fn lock_active_workspace_group(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    group_id: Uuid,
) -> Result<bool> {
    let group_exists = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM kival.groups
        WHERE id = $1
            AND archived_at IS NULL
        FOR SHARE
        "#,
    )
    .bind(group_id)
    .fetch_optional(&mut **tx)
    .await?
    .is_some();
    if !group_exists {
        return Ok(false);
    }

    Ok(sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM kival.workspace_groups
        WHERE workspace_id = $1
            AND group_id = $2
            AND archived_at IS NULL
        FOR SHARE
        "#,
    )
    .bind(workspace_id)
    .bind(group_id)
    .fetch_optional(&mut **tx)
    .await?
    .is_some())
}

/// Lists workspace-to-group links.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read link state.
pub async fn list_workspace_groups(
    pool: &PgPool,
    workspace_id: Uuid,
    actor_id: Uuid,
    cursor_created_at: Option<OffsetDateTime>,
    cursor_id: Option<Uuid>,
    limit: i64,
    status: ArchiveListStatus,
) -> Result<Vec<WorkspaceGroupRow>> {
    sqlx::query_as::<_, StoredWorkspaceGroupRow>(
        r#"
        SELECT rg.id, rg.workspace_id, rg.group_id, g.name AS group_name,
            g.description AS group_description, rg.status, rg.created_by, rg.archived_by,
            rg.created_at, rg.updated_at, rg.archived_at
        FROM kival.workspace_groups rg
        JOIN kival.groups g
            ON g.id = rg.group_id
        WHERE rg.workspace_id = $1
            AND kival.user_can_read_workspace($1, $2)
            AND ($6 = 'all' OR ($6 = 'active' AND rg.archived_at IS NULL AND g.archived_at IS NULL)
            OR ($6 = 'archived' AND rg.archived_at IS NOT NULL))
            AND ($3::timestamptz IS NULL OR (rg.created_at, rg.id) < ($3, $4))
        ORDER BY rg.created_at DESC, rg.id DESC
        LIMIT $5
        OFFSET CASE WHEN kival.require_read_workspace($1, $2) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(actor_id)
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(limit)
    .bind(status.as_str())
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(TryInto::try_into)
    .collect::<Result<Vec<_>>>()
}

/// Links an active group to a workspace.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the link.
pub async fn create_workspace_group(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    group_id: Uuid,
    actor_id: Uuid,
) -> Result<WorkspaceGroupRow> {
    if !crate::workspaces::lock_active_workspace_for_child(tx, workspace_id).await? {
        return Err(KernelError::ResourceNotFound);
    }
    if !crate::groups::lock_active_group_for_reference(tx, group_id).await? {
        return Err(sqlx::Error::RowNotFound.into());
    }

    sqlx::query_as::<_, StoredWorkspaceGroupRow>(
        r#"
        WITH linked AS (
            INSERT INTO kival.workspace_groups (workspace_id, group_id, created_by)
            VALUES ($1, $2, $3)
            RETURNING
                id, workspace_id, group_id, status, created_by, archived_by,
                created_at, updated_at, archived_at
        )
        SELECT
            linked.id, linked.workspace_id, linked.group_id, g.name AS group_name,
            g.description AS group_description, linked.status, linked.created_by,
            linked.archived_by,
            linked.created_at, linked.updated_at, linked.archived_at
        FROM linked
        JOIN kival.groups g
            ON g.id = linked.group_id
        "#,
    )
    .bind(workspace_id)
    .bind(group_id)
    .bind(actor_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Archives an active workspace-to-group link.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub async fn archive_workspace_group(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    group_id: Uuid,
    actor_id: Uuid,
) -> Result<WorkspaceGroupRow> {
    sqlx::query_as::<_, StoredWorkspaceGroupRow>(
        r#"
        WITH active_workspace AS MATERIALIZED (
            SELECT id
            FROM kival.workspaces
            WHERE id = $1
                AND archived_at IS NULL
            FOR SHARE
        ),
        archived AS (
            UPDATE kival.workspace_groups link
            SET status = 'archived',
                archived_at = now(),
                archived_by = $3
            FROM active_workspace
            WHERE link.workspace_id = active_workspace.id
                AND link.group_id = $2
                AND link.archived_at IS NULL
            RETURNING
                link.id, link.workspace_id, link.group_id, link.status, link.created_by,
                link.archived_by, link.created_at, link.updated_at, link.archived_at
        )
        SELECT
            archived.id, archived.workspace_id, archived.group_id, g.name AS group_name,
            g.description AS group_description, archived.status, archived.created_by,
            archived.archived_by,
            archived.created_at, archived.updated_at, archived.archived_at
        FROM archived
        JOIN kival.groups g
            ON g.id = archived.group_id
        "#,
    )
    .bind(workspace_id)
    .bind(group_id)
    .bind(actor_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Unarchives a workspace-to-group link only when its group remains active.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub async fn unarchive_workspace_group(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    group_id: Uuid,
) -> Result<WorkspaceGroupRow> {
    if !crate::workspaces::lock_active_workspace_for_child(tx, workspace_id).await?
        || !crate::groups::lock_active_group_for_reference(tx, group_id).await?
    {
        return Err(sqlx::Error::RowNotFound.into());
    }

    sqlx::query_as::<_, StoredWorkspaceGroupRow>(
        r#"
        WITH unarchived AS (
            UPDATE kival.workspace_groups link
            SET status = 'active',
                archived_at = NULL,
                archived_by = NULL
            WHERE link.workspace_id = $1
                AND link.group_id = $2
                AND link.archived_at IS NOT NULL
            RETURNING link.id, link.workspace_id, link.group_id, link.status, link.created_by,
                link.archived_by, link.created_at, link.updated_at, link.archived_at
        )
        SELECT
            unarchived.id, unarchived.workspace_id, unarchived.group_id, g.name AS group_name,
            g.description AS group_description, unarchived.status, unarchived.created_by,
            unarchived.archived_by,
            unarchived.created_at, unarchived.updated_at, unarchived.archived_at
        FROM unarchived
        JOIN kival.groups g
            ON g.id = unarchived.group_id
        "#,
    )
    .bind(workspace_id)
    .bind(group_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}
