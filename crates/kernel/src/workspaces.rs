//! Workspace state bindings.

use chrono::{DateTime, Utc};
use sqlx::{Acquire, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{ArchiveListStatus, ArchiveStatus, KernelError, MembershipRole, Result, parse_stored};

/// Stored workspace row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRow {
    /// Workspace ID.
    pub id: Uuid,
    /// Workspace name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Lifecycle status.
    pub status: ArchiveStatus,
    /// Creator user.
    pub created_by: Option<Uuid>,
    /// Archiving user.
    pub archived_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Archive timestamp.
    pub archived_at: Option<DateTime<Utc>>,
}

/// Actor-relative workspace directory row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleWorkspaceRow {
    /// Workspace ID.
    pub id: Uuid,
    /// Workspace name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Lifecycle status.
    pub status: ArchiveStatus,
    /// Effective workspace role for the actor.
    pub effective_role: MembershipRole,
    /// Creator user.
    pub created_by: Option<Uuid>,
    /// Archiving user.
    pub archived_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Archive timestamp.
    pub archived_at: Option<DateTime<Utc>>,
    /// Whether the actor pinned the workspace.
    pub pinned: bool,
    /// Workspace pin creation timestamp.
    pub pinned_at: Option<DateTime<Utc>>,
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredWorkspaceRow {
    /// Stored row identifier.
    id: Uuid,
    /// Stored name.
    name: String,
    /// Stored optional description.
    description: Option<String>,
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

impl TryFrom<StoredWorkspaceRow> for WorkspaceRow {
    type Error = KernelError;

    fn try_from(row: StoredWorkspaceRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            name: row.name,
            description: row.description,
            status: parse_stored("workspace status", row.status)?,
            created_by: row.created_by,
            archived_by: row.archived_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
        })
    }
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredVisibleWorkspaceRow {
    /// Stored row identifier.
    id: Uuid,
    /// Stored name.
    name: String,
    /// Stored optional description.
    description: Option<String>,
    /// Stored lifecycle status before typed parsing.
    status: String,
    /// Stored effective workspace role before typed parsing.
    effective_role: String,
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
    /// Stored actor-relative pin projection.
    pinned: bool,
    /// Stored pin creation timestamp, when present.
    pinned_at: Option<DateTime<Utc>>,
}

impl TryFrom<StoredVisibleWorkspaceRow> for VisibleWorkspaceRow {
    type Error = KernelError;

    fn try_from(row: StoredVisibleWorkspaceRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            name: row.name,
            description: row.description,
            status: parse_stored("workspace status", row.status)?,
            effective_role: parse_stored("workspace membership role", row.effective_role)?,
            created_by: row.created_by,
            archived_by: row.archived_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
            pinned: row.pinned,
            pinned_at: row.pinned_at,
        })
    }
}

/// Workspace-directory query parameters.
#[derive(Debug, Clone, Copy)]
pub struct ListVisibleWorkspaces<'a> {
    /// Cursor timestamp.
    pub cursor_created_at: Option<DateTime<Utc>>,
    /// Cursor ID.
    pub cursor_id: Option<Uuid>,
    /// Actor user ID.
    pub user_id: Uuid,
    /// Fetch limit including lookahead.
    pub limit: i64,
    /// Lifecycle filter.
    pub status: ArchiveListStatus,
    /// API key restriction, when authenticated by API key.
    pub api_key_id: Option<Uuid>,
    /// Optional name substring.
    pub query: Option<&'a str>,
    /// Optional pin filter.
    pub pinned: Option<bool>,
}

/// Lists workspaces visible to an actor.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read the directory projection.
pub async fn list_visible_workspaces(
    pool: &PgPool,
    query: ListVisibleWorkspaces<'_>,
) -> Result<Vec<VisibleWorkspaceRow>> {
    sqlx::query_as::<_, StoredVisibleWorkspaceRow>(
        r#"
        SELECT
            id,
            name,
            description,
            status,
            CASE
                WHEN EXISTS (
                    SELECT 1 FROM kival.global_admins ga
                    WHERE ga.user_id = $3::uuid AND ga.revoked_at IS NULL
                ) THEN 'admin'
                ELSE (
                    SELECT rm.workspace_role::text
                    FROM kival.workspace_memberships rm
                    WHERE rm.workspace_id = workspaces.id
                        AND rm.user_id = $3::uuid
                        AND rm.revoked_at IS NULL
                )
            END AS effective_role,
            created_by,
            archived_by,
            created_at,
            updated_at,
            archived_at,
            EXISTS (
                SELECT 1
                FROM kival.workspace_pins pin
                WHERE pin.user_id = $3::uuid
                    AND pin.workspace_id = workspaces.id
            ) AS pinned,
            (
                SELECT pin.created_at
                FROM kival.workspace_pins pin
                WHERE pin.user_id = $3::uuid
                    AND pin.workspace_id = workspaces.id
            ) AS pinned_at
        FROM kival.workspaces
        WHERE (
            $5 = 'all'
            OR ($5 = 'active' AND archived_at IS NULL)
            OR ($5 = 'archived' AND archived_at IS NOT NULL)
        )
            AND ($1::timestamptz IS NULL OR (created_at, id) < ($1, $2))
            AND (
                EXISTS (
                    SELECT 1 FROM kival.global_admins ga
                    WHERE ga.user_id = $3::uuid AND ga.revoked_at IS NULL
                )
                OR EXISTS (
                    SELECT 1
                    FROM kival.workspace_memberships rm
                    WHERE rm.workspace_id = workspaces.id
                        AND rm.user_id = $3::uuid
                        AND rm.revoked_at IS NULL
                )
            )
            AND (
                $6::uuid IS NULL
                OR EXISTS (
                    SELECT 1
                    FROM kival.api_key_workspaces akw
                    WHERE akw.api_key_id = $6::uuid
                        AND akw.workspace_id = workspaces.id
                )
            )
            AND ($7::text IS NULL OR strpos(lower(name), lower($7)) > 0)
            AND (
                $8::boolean IS NULL
                OR EXISTS (
                    SELECT 1
                    FROM kival.workspace_pins pin_filter
                    WHERE pin_filter.user_id = $3::uuid
                        AND pin_filter.workspace_id = workspaces.id
                ) = $8
            )
        ORDER BY created_at DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(query.cursor_created_at)
    .bind(query.cursor_id)
    .bind(query.user_id)
    .bind(query.limit)
    .bind(query.status.as_str())
    .bind(query.api_key_id)
    .bind(query.query)
    .bind(query.pinned)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(TryInto::try_into)
    .collect::<Result<Vec<_>>>()
}

/// Creates a workspace and its creator administrator membership atomically within a transaction.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects either transition.
pub async fn create_workspace(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
    description: Option<&str>,
    actor_id: Uuid,
) -> Result<WorkspaceRow> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result = create_workspace_in_savepoint(&mut savepoint, name, description, actor_id).await;

    match result {
        Ok(workspace) => {
            savepoint.commit().await?;
            Ok(workspace)
        }
        Err(error) => {
            savepoint.rollback().await?;
            Err(error)
        }
    }
}

/// Applies workspace creation inside a cancellation-safe savepoint.
async fn create_workspace_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
    description: Option<&str>,
    actor_id: Uuid,
) -> Result<WorkspaceRow> {
    let workspace: WorkspaceRow = sqlx::query_as::<_, StoredWorkspaceRow>(
        r#"
        INSERT INTO kival.workspaces (name, description, created_by)
        VALUES ($1, $2, $3)
        RETURNING
            id, name, description, status, created_by, archived_by,
            created_at, updated_at, archived_at
        "#,
    )
    .bind(name)
    .bind(description)
    .bind(actor_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()?;
    sqlx::query(
        r#"
        INSERT INTO kival.workspace_memberships (workspace_id, user_id, workspace_role, created_by)
        VALUES ($1, $2, 'admin', $2)
        "#,
    )
    .bind(workspace.id)
    .bind(actor_id)
    .execute(&mut **tx)
    .await?;
    Ok(workspace)
}

/// Reads an actor-visible workspace.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read workspace state.
pub async fn fetch_visible_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<VisibleWorkspaceRow>> {
    sqlx::query_as::<_, StoredVisibleWorkspaceRow>(
        r#"
        SELECT
            id,
            name,
            description,
            status,
            CASE
                WHEN EXISTS (
                    SELECT 1
                    FROM kival.global_admins
                    WHERE user_id = $2
                        AND revoked_at IS NULL
                ) THEN 'admin'
                ELSE (
                    SELECT rm.workspace_role::text
                    FROM kival.workspace_memberships rm
                    WHERE rm.workspace_id = workspaces.id
                        AND rm.user_id = $2
                        AND rm.revoked_at IS NULL
                )
            END AS effective_role,
            created_by,
            archived_by,
            created_at,
            updated_at,
            archived_at,
            EXISTS (
                SELECT 1
                FROM kival.workspace_pins pin
                WHERE pin.user_id = $2
                    AND pin.workspace_id = workspaces.id
            ) AS pinned,
            (
                SELECT pin.created_at
                FROM kival.workspace_pins pin
                WHERE pin.user_id = $2
                    AND pin.workspace_id = workspaces.id
            ) AS pinned_at
        FROM kival.workspaces
        WHERE id = $1
            AND (
                EXISTS (
                    SELECT 1
                    FROM kival.global_admins
                    WHERE user_id = $2
                        AND revoked_at IS NULL
                )
                OR EXISTS (
                    SELECT 1
                    FROM kival.workspace_memberships rm
                    WHERE rm.workspace_id = workspaces.id
                        AND rm.user_id = $2
                        AND rm.revoked_at IS NULL
                )
            )
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .map(TryInto::try_into)
    .transpose()
}

/// Returns whether a workspace exists in any lifecycle state.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read workspace state.
pub async fn workspace_exists(pool: &PgPool, workspace_id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM kival.workspaces
            WHERE id = $1
        )
        "#,
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await?)
}

/// Pins an active workspace lifecycle while a child-resource transition is in flight.
///
/// `FOR SHARE` prevents archival without excluding unrelated child mutations. Kernel transitions
/// acquire this parent lock before referenced users, groups, or objects.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot perform the lock.
pub(crate) async fn lock_active_workspace_for_child(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM kival.workspaces
        WHERE id = $1
            AND archived_at IS NULL
        FOR SHARE
        "#,
    )
    .bind(workspace_id)
    .fetch_optional(&mut **tx)
    .await?
    .is_some())
}

/// Updates mutable fields of an active workspace.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition or the workspace is not active.
pub async fn update_workspace(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    name: Option<&str>,
    description_present: bool,
    description: Option<String>,
) -> Result<WorkspaceRow> {
    sqlx::query_as::<_, StoredWorkspaceRow>(
        r#"
        UPDATE kival.workspaces
        SET name = COALESCE($2, name),

            description = CASE WHEN $3 THEN $4 ELSE description END
        WHERE id = $1
            AND archived_at IS NULL
        RETURNING
            id, name, description, status, created_by, archived_by,
            created_at, updated_at, archived_at
        "#,
    )
    .bind(workspace_id)
    .bind(name)
    .bind(description_present)
    .bind(description)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Archives an active workspace.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition or the workspace is not active.
pub async fn archive_workspace(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    actor_id: Uuid,
) -> Result<WorkspaceRow> {
    sqlx::query_as::<_, StoredWorkspaceRow>(
        r#"
        UPDATE kival.workspaces
        SET status = 'archived',
            archived_at = now(),
            archived_by = $2
        WHERE id = $1
            AND archived_at IS NULL
        RETURNING
            id, name, description, status, created_by, archived_by,
            created_at, updated_at, archived_at
        "#,
    )
    .bind(workspace_id)
    .bind(actor_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Unarchives a workspace.
///
/// Authorization is an early server admission decision; this transition owns only lifecycle
/// serialization and state validity.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition or the workspace is not archived.
pub async fn unarchive_workspace(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
) -> Result<WorkspaceRow> {
    sqlx::query_as::<_, StoredWorkspaceRow>(
        r#"
        UPDATE kival.workspaces
        SET status = 'active',
            archived_at = NULL,
            archived_by = NULL
        WHERE id = $1
            AND archived_at IS NOT NULL
        RETURNING
            id, name, description, status, created_by, archived_by,
            created_at, updated_at, archived_at
        "#,
    )
    .bind(workspace_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}
