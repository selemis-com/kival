//! Authorization and access-state projections.

use sqlx::PgPool;
use uuid::Uuid;

use crate::{MembershipRole, ObjectRole, Result, parse_optional_stored};

/// Existence and authorization result for a state capability query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    /// Whether the target exists in the lifecycle states accepted by the operation.
    pub exists: bool,
    /// Whether the actor is authorized for the requested capability.
    pub allowed: bool,
}

/// Existence and effective-role result for an object access query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleCapability {
    /// Whether the target exists in the lifecycle states accepted by the operation.
    pub exists: bool,
    /// Effective object role when authorization succeeds.
    pub role: Option<ObjectRole>,
}

/// Returns whether a user is an active global administrator.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read administrator state.
pub async fn is_global_admin(pool: &PgPool, user_id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (SELECT 1 FROM kival.global_admins WHERE user_id = $1 AND revoked_at IS NULL)
        "#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?)
}

/// Returns whether an active user can administer at least one active group.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot evaluate group state.
pub async fn can_manage_groups(pool: &PgPool, user_id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM kival.group_memberships gm
            JOIN kival.groups g
                ON g.id = gm.group_id
            WHERE gm.user_id = $1
                AND gm.group_role = 'admin'
                AND gm.revoked_at IS NULL
                AND g.archived_at IS NULL
        )
        "#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?)
}

/// Returns active workspace membership or administration capability.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot evaluate workspace access.
pub async fn workspace_membership_capability(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    required_role: MembershipRole,
) -> Result<Capability> {
    let (exists, allowed) = sqlx::query_as::<_, (bool, bool)>(
        r#"
        WITH resource AS (
            SELECT EXISTS (
                SELECT 1
                FROM kival.workspaces
                WHERE id = $1
                    AND archived_at IS NULL
            ) AS exists
        )
        SELECT
            resource.exists,
            resource.exists AND (
                EXISTS (
                    SELECT 1
                    FROM kival.global_admins
                    WHERE user_id = $2
                        AND revoked_at IS NULL
                )
                OR EXISTS (
                    SELECT 1
                    FROM kival.workspace_memberships
                    WHERE workspace_id = $1
                        AND user_id = $2
                        AND revoked_at IS NULL
                        AND ($3 = 'member' OR workspace_role = 'admin')
                )
            ) AS allowed
        FROM resource
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(required_role.as_str())
    .fetch_one(pool)
    .await?;
    Ok(Capability { exists, allowed })
}

/// Returns administration capability for an archived workspace.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot evaluate workspace access.
pub async fn archived_workspace_admin_capability(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Capability> {
    let (exists, allowed) = sqlx::query_as::<_, (bool, bool)>(
        r#"
        WITH resource AS (
            SELECT EXISTS (
                SELECT 1
                FROM kival.workspaces
                WHERE id = $1
                    AND archived_at IS NOT NULL
            ) AS exists
        )
        SELECT
            resource.exists,
            resource.exists AND (
                EXISTS (
                    SELECT 1
                    FROM kival.global_admins
                    WHERE user_id = $2
                        AND revoked_at IS NULL
                )
                OR EXISTS (
                    SELECT 1
                    FROM kival.workspace_memberships
                    WHERE workspace_id = $1
                        AND user_id = $2
                        AND workspace_role = 'admin'
                        AND revoked_at IS NULL
                )
            ) AS allowed
        FROM resource
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(Capability { exists, allowed })
}

/// Returns administration capability for an active group.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot evaluate group access.
pub async fn active_group_admin_capability(
    pool: &PgPool,
    user_id: Uuid,
    group_id: Uuid,
) -> Result<Capability> {
    let (exists, allowed) = sqlx::query_as::<_, (bool, bool)>(
        r#"
        WITH resource AS (
            SELECT EXISTS (
                SELECT 1
                FROM kival.groups
                WHERE id = $1
                    AND archived_at IS NULL
            ) AS exists
        )
        SELECT
            resource.exists,
            resource.exists AND (
                EXISTS (
                    SELECT 1
                    FROM kival.global_admins
                    WHERE user_id = $2
                        AND revoked_at IS NULL
                )
                OR EXISTS (
                    SELECT 1
                    FROM kival.group_memberships
                    WHERE group_id = $1
                        AND user_id = $2
                        AND group_role = 'admin'
                        AND revoked_at IS NULL
                )
            ) AS allowed
        FROM resource
        "#,
    )
    .bind(group_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(Capability { exists, allowed })
}

/// Returns the actor's effective role while evaluating a minimum role against an active object.
///
/// This is Kival's canonical active-object admission query. Callers that only need a yes/no
/// decision can discard the returned role rather than using a second authorization path.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot evaluate object access.
pub async fn active_object_role(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    required_role: ObjectRole,
) -> Result<RoleCapability> {
    let (exists, role) = sqlx::query_as::<_, (bool, Option<String>)>(
        r#"
        WITH resource AS (
            SELECT EXISTS (
                SELECT 1
                FROM kival.objects o
                JOIN kival.workspaces r
                    ON r.id = o.workspace_id
                WHERE o.workspace_id = $1
                    AND o.id = $3
                    AND o.archived_at IS NULL
                    AND r.archived_at IS NULL
            ) AS exists
        )
        SELECT
            resource.exists,
            CASE
                WHEN resource.exists AND kival.has_object_permission(
                    $1,
                    $3,
                    $2,
                    $4::text::kival.object_role
                ) THEN kival.object_access_role($1, $3, $2)::text
                ELSE NULL
            END AS role
        FROM resource
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(object_id)
    .bind(required_role.as_str())
    .fetch_one(pool)
    .await?;
    role_capability(exists, role)
}

/// Returns two active-object role capabilities from one admission-statement snapshot.
///
/// Multi-resource mutations use this projection so every actor-authorization predicate that admits
/// the mutation observes the same `PostgreSQL` statement snapshot. The caller remains responsible
/// for applying operation-specific not-found and forbidden semantics in the requested order.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot evaluate object access.
pub async fn active_object_role_pair(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    first_object_id: Uuid,
    first_required_role: ObjectRole,
    second_object_id: Uuid,
    second_required_role: ObjectRole,
) -> Result<(RoleCapability, RoleCapability)> {
    let (first_exists, first_role, second_exists, second_role) =
        sqlx::query_as::<_, (bool, Option<String>, bool, Option<String>)>(
            r#"
            WITH resources AS (
                SELECT
                    EXISTS (
                        SELECT 1
                        FROM kival.objects o
                        JOIN kival.workspaces w
                            ON w.id = o.workspace_id
                        WHERE o.workspace_id = $1
                            AND o.id = $3
                            AND o.archived_at IS NULL
                            AND w.archived_at IS NULL
                    ) AS first_exists,
                    EXISTS (
                        SELECT 1
                        FROM kival.objects o
                        JOIN kival.workspaces w
                            ON w.id = o.workspace_id
                        WHERE o.workspace_id = $1
                            AND o.id = $5
                            AND o.archived_at IS NULL
                            AND w.archived_at IS NULL
                    ) AS second_exists
            )
            SELECT
                resources.first_exists,
                CASE
                    WHEN resources.first_exists AND kival.has_object_permission(
                        $1,
                        $3,
                        $2,
                        $4::text::kival.object_role
                    ) THEN kival.object_access_role($1, $3, $2)::text
                    ELSE NULL
                END AS first_role,
                resources.second_exists,
                CASE
                    WHEN resources.second_exists AND kival.has_object_permission(
                        $1,
                        $5,
                        $2,
                        $6::text::kival.object_role
                    ) THEN kival.object_access_role($1, $5, $2)::text
                    ELSE NULL
                END AS second_role
            FROM resources
            "#,
        )
        .bind(workspace_id)
        .bind(user_id)
        .bind(first_object_id)
        .bind(first_required_role.as_str())
        .bind(second_object_id)
        .bind(second_required_role.as_str())
        .fetch_one(pool)
        .await?;

    Ok((role_capability(first_exists, first_role)?, role_capability(second_exists, second_role)?))
}

/// Returns the effective role for reading an active object or administratively reading an archived
/// object.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot evaluate object access.
pub async fn object_readable_role(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<RoleCapability> {
    let (exists, role) = sqlx::query_as::<_, (bool, Option<String>)>(
        r#"
        WITH resource AS (
            SELECT o.archived_at IS NOT NULL AS archived
            FROM kival.objects o
            JOIN kival.workspaces w
                ON w.id = o.workspace_id
            WHERE o.workspace_id = $1
                AND o.id = $3
                AND w.archived_at IS NULL
        )
        SELECT
            EXISTS (SELECT 1 FROM resource) AS exists,
            (
                SELECT CASE
                    WHEN (
                        NOT resource.archived
                        AND kival.has_object_permission(
                            $1,
                            $3,
                            $2,
                            'viewer'::kival.object_role
                        )
                    ) OR (
                        resource.archived
                        AND kival.has_object_permission(
                            $1,
                            $3,
                            $2,
                            'admin'::kival.object_role
                        )
                    ) THEN kival.object_access_role($1, $3, $2)::text
                    ELSE NULL
                END
                FROM resource
            ) AS role
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(object_id)
    .fetch_one(pool)
    .await?;
    role_capability(exists, role)
}

/// Returns the effective administrative role for an archived object.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot evaluate object access.
pub async fn archived_object_admin_role(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<RoleCapability> {
    let (exists, role) = sqlx::query_as::<_, (bool, Option<String>)>(
        r#"
        WITH resource AS (
            SELECT EXISTS (
                SELECT 1
                FROM kival.objects o
                JOIN kival.workspaces w
                    ON w.id = o.workspace_id
                WHERE o.workspace_id = $1
                    AND o.id = $3
                    AND o.archived_at IS NOT NULL
                    AND w.archived_at IS NULL
            ) AS exists
        )
        SELECT
            resource.exists,
            CASE
                WHEN resource.exists AND kival.has_object_permission(
                    $1,
                    $3,
                    $2,
                    'admin'::kival.object_role
                ) THEN kival.object_access_role($1, $3, $2)::text
                ELSE NULL
            END AS role
        FROM resource
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(object_id)
    .fetch_one(pool)
    .await?;
    role_capability(exists, role)
}

/// Converts a database role projection into the typed kernel vocabulary.
fn role_capability(exists: bool, role: Option<String>) -> Result<RoleCapability> {
    Ok(RoleCapability { exists, role: parse_optional_stored("object role", role)? })
}
