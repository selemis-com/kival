//! Direct workspace membership state bindings.

use sqlx::{Acquire, PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{KernelError, MembershipRole, Result, parse_stored, users::ActiveUserIdentity};

/// Workspace membership projection including user identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMembershipRow {
    /// Membership ID.
    pub id: Uuid,
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// User ID.
    pub user_id: Uuid,
    /// Username.
    pub user_username: String,
    /// Display name.
    pub user_display_name: String,
    /// Workspace role.
    pub workspace_role: MembershipRole,
    /// Creator.
    pub created_by: Option<Uuid>,
    /// Revoker.
    pub revoked_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: OffsetDateTime,
    /// Update timestamp.
    pub updated_at: OffsetDateTime,
    /// Revocation timestamp.
    pub revoked_at: Option<OffsetDateTime>,
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredWorkspaceMembershipRow {
    /// Stored row identifier.
    id: Uuid,
    /// Stored workspace identifier.
    workspace_id: Uuid,
    /// Stored user identifier.
    user_id: Uuid,
    /// Stored member username projection.
    user_username: String,
    /// Stored member display-name projection.
    user_display_name: String,
    /// Stored workspace role before typed parsing.
    workspace_role: String,
    /// Stored creator identifier, when retained.
    created_by: Option<Uuid>,
    /// Stored revoker identifier, when retained.
    revoked_by: Option<Uuid>,
    /// Stored creation timestamp.
    created_at: OffsetDateTime,
    /// Stored update timestamp.
    updated_at: OffsetDateTime,
    /// Stored revocation timestamp, when present.
    revoked_at: Option<OffsetDateTime>,
}

impl TryFrom<StoredWorkspaceMembershipRow> for WorkspaceMembershipRow {
    type Error = KernelError;

    fn try_from(row: StoredWorkspaceMembershipRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            workspace_id: row.workspace_id,
            user_id: row.user_id,
            user_username: row.user_username,
            user_display_name: row.user_display_name,
            workspace_role: parse_stored("workspace membership role", row.workspace_role)?,
            created_by: row.created_by,
            revoked_by: row.revoked_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            revoked_at: row.revoked_at,
        })
    }
}

/// Lists active direct workspace memberships.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read membership state.
pub async fn list_workspace_memberships(
    pool: &PgPool,
    workspace_id: Uuid,
    actor_id: Uuid,
    cursor_created_at: Option<OffsetDateTime>,
    cursor_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<WorkspaceMembershipRow>> {
    sqlx::query_as::<_, StoredWorkspaceMembershipRow>(
        r#"
        SELECT m.id, m.workspace_id, m.user_id, u.username AS user_username,
            u.display_name AS user_display_name, m.workspace_role, m.created_by,
            m.revoked_by, m.created_at, m.updated_at, m.revoked_at
        FROM kival.workspace_memberships m
        JOIN kival.users u
            ON u.id = m.user_id
        WHERE m.workspace_id = $1
            AND m.revoked_at IS NULL
            AND kival.user_can_read_workspace($1, $2)
            AND ($3::timestamptz IS NULL OR (m.created_at, m.id) < ($3, $4))
        ORDER BY m.created_at DESC, m.id DESC
        LIMIT $5
        OFFSET CASE WHEN kival.require_read_workspace($1, $2) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(actor_id)
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(limit)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(TryInto::try_into)
    .collect::<Result<Vec<_>>>()
}

/// Pins and validates an active workspace member referenced by another resource.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub(crate) async fn lock_active_workspace_member(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT u.id
        FROM kival.users u
        JOIN kival.workspace_memberships wm
            ON wm.user_id = u.id
            AND wm.workspace_id = $1
            AND wm.revoked_at IS NULL
        WHERE u.id = $2
            AND u.disabled_at IS NULL FOR SHARE OF u, wm
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    .is_some())
}

/// Creates a direct workspace membership.
///
/// The kernel owns the parent/reference lock order: workspace lifecycle first, then the referenced
/// active user.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the membership or either referenced resource is not
/// active.
pub async fn create_workspace_membership(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    user_id: Option<Uuid>,
    username: Option<&str>,
    workspace_role: MembershipRole,
    actor_id: Uuid,
) -> Result<WorkspaceMembershipRow> {
    if !crate::workspaces::lock_active_workspace_for_child(tx, workspace_id).await? {
        return Err(KernelError::ResourceNotFound);
    }
    let user = crate::users::lock_active_user_for_reference(tx, user_id, username).await?;
    create_workspace_membership_unchecked(tx, workspace_id, &user, workspace_role, actor_id).await
}

/// Inserts a workspace membership after the parent and referenced user are pinned.
async fn create_workspace_membership_unchecked(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    user: &ActiveUserIdentity,
    workspace_role: MembershipRole,
    actor_id: Uuid,
) -> Result<WorkspaceMembershipRow> {
    sqlx::query_as::<_, StoredWorkspaceMembershipRow>(
        r#"
        INSERT INTO kival.workspace_memberships (workspace_id, user_id, workspace_role, created_by)
        VALUES ($1, $2, $3, $4)
        RETURNING id, workspace_id, user_id, $5::text AS user_username,
            $6::text AS user_display_name, workspace_role, created_by, revoked_by,
            created_at, updated_at, revoked_at
        "#,
    )
    .bind(workspace_id)
    .bind(user.id)
    .bind(workspace_role.as_str())
    .bind(actor_id)
    .bind(&user.username)
    .bind(&user.display_name)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Revokes an active direct workspace membership and returns its historical row.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition or the membership is not active.
pub async fn revoke_workspace_membership(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    membership_id: Uuid,
    actor_id: Uuid,
) -> Result<WorkspaceMembershipRow> {
    sqlx::query_as::<_, StoredWorkspaceMembershipRow>(
        r#"
        WITH active_workspace AS MATERIALIZED (
            SELECT id
            FROM kival.workspaces
            WHERE id = $1
                AND archived_at IS NULL
            FOR SHARE
        ),
        revoked AS (
            UPDATE kival.workspace_memberships membership
            SET revoked_at = now(),
                revoked_by = $3
            FROM active_workspace
            WHERE membership.workspace_id = active_workspace.id
                AND membership.id = $2
                AND membership.revoked_at IS NULL
            RETURNING membership.*
        )
        SELECT r.id, r.workspace_id, r.user_id, u.username AS user_username,
            u.display_name AS user_display_name, r.workspace_role, r.created_by, r.revoked_by,
            r.created_at, r.updated_at, r.revoked_at
        FROM revoked r
        JOIN kival.users u
            ON u.id = r.user_id
        "#,
    )
    .bind(workspace_id)
    .bind(membership_id)
    .bind(actor_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Replaces an active workspace membership with a new role-bearing membership.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects either transition.
pub async fn replace_workspace_membership(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    membership_id: Uuid,
    workspace_role: MembershipRole,
    actor_id: Uuid,
) -> Result<(WorkspaceMembershipRow, WorkspaceMembershipRow)> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result = replace_workspace_membership_in_savepoint(
        &mut savepoint,
        workspace_id,
        membership_id,
        workspace_role,
        actor_id,
    )
    .await;

    match result {
        Ok(replacement) => {
            savepoint.commit().await?;
            Ok(replacement)
        }
        Err(error) => {
            savepoint.rollback().await?;
            Err(error)
        }
    }
}

/// Applies workspace-membership replacement inside a cancellation-safe savepoint.
async fn replace_workspace_membership_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    membership_id: Uuid,
    workspace_role: MembershipRole,
    actor_id: Uuid,
) -> Result<(WorkspaceMembershipRow, WorkspaceMembershipRow)> {
    let previous = revoke_workspace_membership(tx, workspace_id, membership_id, actor_id).await?;
    let user = ActiveUserIdentity {
        id: previous.user_id,
        username: previous.user_username.clone(),
        display_name: previous.user_display_name.clone(),
    };
    let _user_lock = crate::users::lock_active_user_for_reference(tx, Some(user.id), None).await?;
    let current =
        create_workspace_membership_unchecked(tx, workspace_id, &user, workspace_role, actor_id)
            .await?;
    Ok((previous, current))
}
