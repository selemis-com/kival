//! Group membership state bindings.

use chrono::{DateTime, Utc};
use sqlx::{Acquire, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{KernelError, MembershipRole, Result, parse_stored, users::ActiveUserIdentity};

/// Group membership projection including user identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMembershipRow {
    /// Membership ID.
    pub id: Uuid,
    /// Group ID.
    pub group_id: Uuid,
    /// User ID.
    pub user_id: Uuid,
    /// Username.
    pub user_username: String,
    /// Display name.
    pub user_display_name: String,
    /// Group role.
    pub group_role: MembershipRole,
    /// Creator.
    pub created_by: Option<Uuid>,
    /// Revoker.
    pub revoked_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Revocation timestamp.
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredGroupMembershipRow {
    /// Stored row identifier.
    id: Uuid,
    /// Stored group identifier.
    group_id: Uuid,
    /// Stored user identifier.
    user_id: Uuid,
    /// Stored member username projection.
    user_username: String,
    /// Stored member display-name projection.
    user_display_name: String,
    /// Stored group role before typed parsing.
    group_role: String,
    /// Stored creator identifier, when retained.
    created_by: Option<Uuid>,
    /// Stored revoker identifier, when retained.
    revoked_by: Option<Uuid>,
    /// Stored creation timestamp.
    created_at: DateTime<Utc>,
    /// Stored update timestamp.
    updated_at: DateTime<Utc>,
    /// Stored revocation timestamp, when present.
    revoked_at: Option<DateTime<Utc>>,
}

impl TryFrom<StoredGroupMembershipRow> for GroupMembershipRow {
    type Error = KernelError;

    fn try_from(row: StoredGroupMembershipRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            group_id: row.group_id,
            user_id: row.user_id,
            user_username: row.user_username,
            user_display_name: row.user_display_name,
            group_role: parse_stored("group membership role", row.group_role)?,
            created_by: row.created_by,
            revoked_by: row.revoked_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            revoked_at: row.revoked_at,
        })
    }
}

/// Lists active memberships for a group.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read group membership state.
pub async fn list_group_memberships(
    pool: &PgPool,
    group_id: Uuid,
    actor_id: Uuid,
    cursor_created_at: Option<DateTime<Utc>>,
    cursor_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<GroupMembershipRow>> {
    sqlx::query_as::<_, StoredGroupMembershipRow>(
        r#"
        SELECT
            m.id, m.group_id, m.user_id, u.username AS user_username,
            u.display_name AS user_display_name,
            m.group_role, m.created_by, m.revoked_by, m.created_at, m.updated_at, m.revoked_at
        FROM kival.group_memberships m
        JOIN kival.users u
            ON u.id = m.user_id
        WHERE m.group_id = $1
            AND m.revoked_at IS NULL
            AND kival.user_can_admin_active_group($1, $2)
            AND ($3::timestamptz IS NULL OR (m.created_at, m.id) < ($3, $4))
        ORDER BY m.created_at DESC, m.id DESC
        LIMIT $5
        OFFSET CASE WHEN kival.require_admin_active_group($1, $2) THEN 0 ELSE 0 END
        "#,
    )
    .bind(group_id)
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

/// Creates a membership while the parent group remains active.
///
/// The kernel owns the parent/reference lock order: group lifecycle first, then the referenced
/// active user.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the membership or either referenced resource is not
/// active.
pub async fn create_group_membership(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    user_id: Option<Uuid>,
    username: Option<&str>,
    group_role: MembershipRole,
    actor_id: Uuid,
) -> Result<GroupMembershipRow> {
    if !crate::groups::lock_active_group_for_reference(tx, group_id).await? {
        return Err(KernelError::ResourceNotFound);
    }
    let user = crate::users::lock_active_user_for_reference(tx, user_id, username).await?;
    create_group_membership_unchecked(tx, group_id, &user, group_role, actor_id).await
}

/// Inserts a group membership after the parent and referenced user are pinned.
async fn create_group_membership_unchecked(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    user: &ActiveUserIdentity,
    group_role: MembershipRole,
    actor_id: Uuid,
) -> Result<GroupMembershipRow> {
    sqlx::query_as::<_, StoredGroupMembershipRow>(
        r#"
        INSERT INTO kival.group_memberships (group_id, user_id, group_role, created_by)
        VALUES ($1, $2, $3, $4)
        RETURNING id, group_id, user_id, $5::text AS user_username, $6::text AS user_display_name,
            group_role, created_by, revoked_by, created_at, updated_at, revoked_at
        "#,
    )
    .bind(group_id)
    .bind(user.id)
    .bind(group_role.as_str())
    .bind(actor_id)
    .bind(&user.username)
    .bind(&user.display_name)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Revokes an active membership while the parent group remains active.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub async fn revoke_group_membership(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    membership_id: Uuid,
    actor_id: Uuid,
) -> Result<GroupMembershipRow> {
    if !crate::groups::lock_active_group_for_reference(tx, group_id).await? {
        return Err(KernelError::ResourceNotFound);
    }

    sqlx::query_as::<_, StoredGroupMembershipRow>(
        r#"
        WITH revoked AS (
            UPDATE kival.group_memberships membership
            SET revoked_at = now(),
                revoked_by = $3
            WHERE membership.group_id = $1
                AND membership.id = $2
                AND membership.revoked_at IS NULL
            RETURNING
                membership.id, membership.group_id, membership.user_id, membership.group_role,
                membership.created_by, membership.revoked_by, membership.created_at,
                membership.updated_at, membership.revoked_at
        )
        SELECT
            revoked.id, revoked.group_id, revoked.user_id, u.username AS user_username,
            u.display_name AS user_display_name, revoked.group_role, revoked.created_by,
            revoked.revoked_by,
            revoked.created_at, revoked.updated_at, revoked.revoked_at
        FROM revoked
        JOIN kival.users u
            ON u.id = revoked.user_id
        "#,
    )
    .bind(group_id)
    .bind(membership_id)
    .bind(actor_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Replaces an active group membership with a new role-bearing membership.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects either transition.
pub async fn replace_group_membership(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    membership_id: Uuid,
    group_role: MembershipRole,
    actor_id: Uuid,
) -> Result<(GroupMembershipRow, GroupMembershipRow)> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result = replace_group_membership_in_savepoint(
        &mut savepoint,
        group_id,
        membership_id,
        group_role,
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

/// Applies group-membership replacement inside a cancellation-safe savepoint.
async fn replace_group_membership_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    membership_id: Uuid,
    group_role: MembershipRole,
    actor_id: Uuid,
) -> Result<(GroupMembershipRow, GroupMembershipRow)> {
    let previous = revoke_group_membership(tx, group_id, membership_id, actor_id).await?;
    let user = ActiveUserIdentity {
        id: previous.user_id,
        username: previous.user_username.clone(),
        display_name: previous.user_display_name.clone(),
    };
    let _user_lock = crate::users::lock_active_user_for_reference(tx, Some(user.id), None).await?;
    let current =
        create_group_membership_unchecked(tx, group_id, &user, group_role, actor_id).await?;
    Ok((previous, current))
}
