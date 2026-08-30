//! Object grant state bindings.

use sqlx::{Acquire, PgPool, Postgres, Transaction};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{GrantPrincipal, KernelError, ObjectRole, Result, parse_stored};

/// Stored object-grant projection.
#[derive(Debug, Clone, Copy)]
pub struct ObjectGrantRow {
    /// Row identifier.
    pub id: Uuid,
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// Object identifier.
    pub object_id: Uuid,
    /// Principal receiving the grant.
    pub principal: GrantPrincipal,
    /// Object role granted to the principal.
    pub object_role: ObjectRole,
    /// User that created the row, when retained.
    pub created_by: Option<Uuid>,
    /// User that revoked the row, when retained.
    pub revoked_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Revocation timestamp, when revoked.
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredObjectGrantRow {
    /// Stored row identifier.
    id: Uuid,
    /// Stored workspace identifier.
    workspace_id: Uuid,
    /// Stored object identifier.
    object_id: Uuid,
    /// Stored user-principal identifier, when present.
    principal_user_id: Option<Uuid>,
    /// Stored group-principal identifier, when present.
    principal_group_id: Option<Uuid>,
    /// Stored object role before typed parsing.
    object_role: String,
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

impl TryFrom<StoredObjectGrantRow> for ObjectGrantRow {
    type Error = KernelError;

    fn try_from(row: StoredObjectGrantRow) -> Result<Self> {
        let principal = match (row.principal_user_id, row.principal_group_id) {
            (Some(user_id), None) => GrantPrincipal::User(user_id),
            (None, Some(group_id)) => GrantPrincipal::Group(group_id),
            _ => {
                return Err(KernelError::InvalidStoredValue {
                    kind: "object grant principal",
                    value: format!(
                        "user={:?}, group={:?}",
                        row.principal_user_id, row.principal_group_id
                    ),
                });
            }
        };

        Ok(Self {
            id: row.id,
            workspace_id: row.workspace_id,
            object_id: row.object_id,
            principal,
            object_role: parse_stored("object role", row.object_role)?,
            created_by: row.created_by,
            revoked_by: row.revoked_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            revoked_at: row.revoked_at,
        })
    }
}

/// Creates the initial administrator grant for an object's creator.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects or cannot persist the grant.
pub(crate) async fn create_creator_admin_grant(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    creator_user_id: Uuid,
) -> Result<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO kival.object_grants (
            workspace_id,
            object_id,
            principal_user_id,
            object_role,
            created_by
        )
        VALUES ($1, $2, $3, 'admin', $3)
        RETURNING id
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(creator_user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(Into::into)
}

/// Lists active grants for an object in reverse creation order.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn list_object_grants(
    pool: &PgPool,
    workspace_id: Uuid,
    object_id: Uuid,
    actor_id: Uuid,
    cursor_created_at: Option<DateTime<Utc>>,
    cursor_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<ObjectGrantRow>> {
    sqlx::query_as::<_, StoredObjectGrantRow>(
        r#"
        SELECT
            id, workspace_id, object_id, principal_user_id, principal_group_id, object_role,
            created_by, revoked_by, created_at, updated_at, revoked_at
        FROM kival.object_grants
        WHERE workspace_id = $1
            AND object_id = $2
            AND revoked_at IS NULL
            AND kival.user_can_access_active_object(
                $1, $2, $3, 'admin'::kival.object_role
            )
            AND ($4::timestamptz IS NULL OR (created_at, id) < ($4, $5))
        ORDER BY created_at DESC, id DESC
        LIMIT $6
        OFFSET CASE
            WHEN kival.require_access_active_object(
                $1, $2, $3, 'admin'::kival.object_role
            ) THEN 0
            ELSE 0
        END
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
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

/// Creates an object grant for a user or group principal.
///
/// The transition owns object-before-principal lifecycle ordering so grant creation cannot race
/// object archival or principal removal into an invalid stored reference.
///
/// # Errors
///
/// Returns an error if the object or principal is not active or `PostgreSQL` rejects the grant.
pub async fn create_object_grant(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    principal: GrantPrincipal,
    object_role: ObjectRole,
    created_by: Uuid,
) -> Result<ObjectGrantRow> {
    if !crate::objects::lock_active_objects_for_reference(tx, workspace_id, &[object_id]).await? {
        return Err(KernelError::ResourceNotFound);
    }

    match principal {
        GrantPrincipal::User(user_id) => {
            if !crate::workspace_memberships::lock_active_workspace_member(
                tx,
                workspace_id,
                user_id,
            )
            .await?
            {
                return Err(KernelError::InvalidObjectGrantUserPrincipal);
            }
        }
        GrantPrincipal::Group(group_id) => {
            if !crate::workspace_groups::lock_active_workspace_group(tx, workspace_id, group_id)
                .await?
            {
                return Err(KernelError::InvalidObjectGrantGroupPrincipal);
            }
        }
    }

    create_object_grant_unchecked(tx, workspace_id, object_id, principal, object_role, created_by)
        .await
}

/// Inserts a grant after its object/principal lifecycle dependencies are pinned.
async fn create_object_grant_unchecked(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    principal: GrantPrincipal,
    object_role: ObjectRole,
    created_by: Uuid,
) -> Result<ObjectGrantRow> {
    let (principal_user_id, principal_group_id) = match principal {
        GrantPrincipal::User(user_id) => (Some(user_id), None),
        GrantPrincipal::Group(group_id) => (None, Some(group_id)),
    };

    sqlx::query_as::<_, StoredObjectGrantRow>(
        r#"
        INSERT INTO kival.object_grants (
            workspace_id, object_id, principal_user_id, principal_group_id, object_role, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING
            id, workspace_id, object_id, principal_user_id, principal_group_id, object_role,
            created_by, revoked_by, created_at, updated_at, revoked_at
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(principal_user_id)
    .bind(principal_group_id)
    .bind(object_role.as_str())
    .bind(created_by)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Locks an active workspace and object before changing its grants.
///
/// The workspace is held with `FOR SHARE` before the object is held with `FOR UPDATE`,
/// preventing workspace archival from committing while grant state changes.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub(crate) async fn lock_object_for_grant_changes(
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
            AND object.archived_at IS NULL
        FOR UPDATE OF object
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .fetch_optional(&mut **tx)
    .await?
    .is_some())
}

/// Loads and locks an active grant while changing object authorization.
async fn lock_active_object_grant(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    grant_id: Uuid,
) -> Result<ObjectGrantRow> {
    let row = sqlx::query_as::<_, StoredObjectGrantRow>(
        r#"
        SELECT
            id, workspace_id, object_id, principal_user_id, principal_group_id, object_role,
            created_by, revoked_by, created_at, updated_at, revoked_at
        FROM kival.object_grants
        WHERE workspace_id = $1
            AND object_id = $2
            AND id = $3
            AND revoked_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(grant_id)
    .fetch_one(&mut **tx)
    .await?;

    row.try_into()
}

/// Counts active administrator grants on an object.
async fn object_admin_grant_count(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<i64> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM kival.object_grants
        WHERE workspace_id = $1
            AND object_id = $2
            AND object_role = 'admin'
            AND revoked_at IS NULL
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .fetch_one(&mut **tx)
    .await?;

    Ok(count)
}

/// Rejects removing the object's final active administrator grant.
async fn ensure_admin_grant_retained(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    current_role: ObjectRole,
    replacement_role: Option<ObjectRole>,
) -> Result<()> {
    if current_role != ObjectRole::Admin || replacement_role == Some(ObjectRole::Admin) {
        return Ok(());
    }

    if object_admin_grant_count(tx, workspace_id, object_id).await? <= 1 {
        return Err(KernelError::ObjectMustRetainAdminGrant);
    }

    Ok(())
}

/// Revokes a locked active object grant without checking aggregate invariants.
async fn revoke_object_grant_unchecked(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    grant_id: Uuid,
    revoked_by: Uuid,
) -> Result<ObjectGrantRow> {
    let row = sqlx::query_as::<_, StoredObjectGrantRow>(
        r#"
        UPDATE kival.object_grants
        SET revoked_at = now(),
            revoked_by = $4
        WHERE workspace_id = $1
            AND object_id = $2
            AND id = $3
            AND revoked_at IS NULL
        RETURNING
            id, workspace_id, object_id, principal_user_id, principal_group_id, object_role,
            created_by, revoked_by, created_at, updated_at, revoked_at
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(grant_id)
    .bind(revoked_by)
    .fetch_one(&mut **tx)
    .await?;

    row.try_into()
}

/// Replaces an active object grant with one carrying a new role.
///
/// The transition serializes grant changes for the object and refuses to demote its final active
/// administrator grant.
///
/// # Errors
///
/// Returns an error if the object or grant does not exist, `PostgreSQL` rejects the transition, or
/// the change would leave the object without an active administrator grant.
pub async fn replace_object_grant(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    grant_id: Uuid,
    object_role: ObjectRole,
    actor_id: Uuid,
) -> Result<(ObjectGrantRow, ObjectGrantRow)> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result = replace_object_grant_in_savepoint(
        &mut savepoint,
        workspace_id,
        object_id,
        grant_id,
        object_role,
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

/// Applies grant replacement inside a cancellation-safe savepoint.
async fn replace_object_grant_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    grant_id: Uuid,
    object_role: ObjectRole,
    actor_id: Uuid,
) -> Result<(ObjectGrantRow, ObjectGrantRow)> {
    if !lock_object_for_grant_changes(tx, workspace_id, object_id).await? {
        return Err(sqlx::Error::RowNotFound.into());
    }

    let previous = lock_active_object_grant(tx, workspace_id, object_id, grant_id).await?;
    ensure_admin_grant_retained(
        tx,
        workspace_id,
        object_id,
        previous.object_role,
        Some(object_role),
    )
    .await?;

    let principal = previous.principal;
    let previous =
        revoke_object_grant_unchecked(tx, workspace_id, object_id, grant_id, actor_id).await?;
    let replacement = create_object_grant_unchecked(
        tx,
        workspace_id,
        object_id,
        principal,
        object_role,
        actor_id,
    )
    .await?;

    Ok((previous, replacement))
}

/// Revokes an active object grant while preserving the object's administrator invariant.
///
/// # Errors
///
/// Returns an error if the object or grant does not exist, `PostgreSQL` rejects the transition, or
/// revocation would remove the object's final active administrator grant.
pub async fn revoke_object_grant(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    grant_id: Uuid,
    revoked_by: Uuid,
) -> Result<ObjectGrantRow> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result = revoke_object_grant_in_savepoint(
        &mut savepoint,
        workspace_id,
        object_id,
        grant_id,
        revoked_by,
    )
    .await;

    match result {
        Ok(revoked) => {
            savepoint.commit().await?;
            Ok(revoked)
        }
        Err(error) => {
            savepoint.rollback().await?;
            Err(error)
        }
    }
}

/// Applies grant revocation inside a cancellation-safe savepoint.
async fn revoke_object_grant_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    grant_id: Uuid,
    revoked_by: Uuid,
) -> Result<ObjectGrantRow> {
    if !lock_object_for_grant_changes(tx, workspace_id, object_id).await? {
        return Err(sqlx::Error::RowNotFound.into());
    }

    let grant = lock_active_object_grant(tx, workspace_id, object_id, grant_id).await?;
    ensure_admin_grant_retained(tx, workspace_id, object_id, grant.object_role, None).await?;

    revoke_object_grant_unchecked(tx, workspace_id, object_id, grant_id, revoked_by).await
}
