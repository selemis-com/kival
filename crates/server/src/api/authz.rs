//! Authorization helpers.

use kival_kernel::{
    Capability, RoleCapability, active_group_admin_capability, active_object_role,
    active_object_role_pair, archived_object_admin_role, archived_workspace_admin_capability,
    is_global_admin, object_readable_role, workspace_membership_capability,
};
use kival_types::{MembershipRole, ObjectRole};
use uuid::Uuid;

use crate::api::error::{ApiError, ApiResult};

/// Returns the API error for an unsatisfied minimum object role.
const fn object_role_forbidden_message(role: ObjectRole) -> &'static str {
    match role {
        ObjectRole::Viewer => "object view access required",
        ObjectRole::Editor => "object edit access required",
        ObjectRole::Admin => "object admin access required",
    }
}

/// Converts a kernel capability check into API not-found or forbidden semantics.
fn ensure_capability(
    capability: Capability,
    not_found_message: &'static str,
    forbidden_message: &'static str,
) -> ApiResult<()> {
    if !capability.exists {
        Err(ApiError::not_found(not_found_message))
    } else if !capability.allowed {
        Err(ApiError::forbidden(forbidden_message))
    } else {
        Ok(())
    }
}

/// Ensures a user is an active global admin.
pub(crate) async fn ensure_global_admin(pool: &sqlx::PgPool, user_id: Uuid) -> ApiResult<()> {
    if is_global_admin(pool, user_id).await? {
        Ok(())
    } else {
        Err(ApiError::forbidden("global admin access required"))
    }
}

/// Ensures a user is an active member of a workspace.
///
/// Active global administrators satisfy this requirement.
pub(crate) async fn ensure_workspace_member(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> ApiResult<()> {
    ensure_capability(
        workspace_membership_capability(pool, user_id, workspace_id, MembershipRole::Member)
            .await?,
        "workspace not found",
        "workspace access required",
    )
}

/// Ensures a user is an active administrator of a workspace.
///
/// Active global administrators satisfy this requirement.
pub(crate) async fn ensure_workspace_admin(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> ApiResult<()> {
    ensure_capability(
        workspace_membership_capability(pool, user_id, workspace_id, MembershipRole::Admin).await?,
        "workspace not found",
        "workspace admin access required",
    )
}

/// Ensures a user is an administrator of an archived workspace.
pub(crate) async fn ensure_archived_workspace_admin(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> ApiResult<()> {
    ensure_capability(
        archived_workspace_admin_capability(pool, user_id, workspace_id).await?,
        "workspace not found",
        "workspace admin access required",
    )
}

/// Requires a global admin or group admin on an active group.
pub(crate) async fn require_active_group_admin(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    group_id: Uuid,
) -> ApiResult<()> {
    let capability = active_group_admin_capability(pool, user_id, group_id).await?;
    ensure_capability(capability, "group not found", "group admin access required")
}

/// Converts an active-object capability into the server's ordinary object error semantics.
fn require_projected_object_role(
    capability: RoleCapability,
    required_role: ObjectRole,
) -> ApiResult<ObjectRole> {
    if !capability.exists {
        return Err(ApiError::not_found("object not found"));
    }
    let forbidden_message = object_role_forbidden_message(required_role);
    capability.role.ok_or_else(|| ApiError::forbidden(forbidden_message))
}

/// Requires two active-object roles from one admission-statement snapshot.
pub(crate) async fn require_object_role_pair(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    first_object_id: Uuid,
    first_required_role: ObjectRole,
    second_object_id: Uuid,
    second_required_role: ObjectRole,
) -> ApiResult<(ObjectRole, ObjectRole)> {
    let (first, second) = active_object_role_pair(
        pool,
        user_id,
        workspace_id,
        first_object_id,
        first_required_role,
        second_object_id,
        second_required_role,
    )
    .await?;

    Ok((
        require_projected_object_role(first, first_required_role)?,
        require_projected_object_role(second, second_required_role)?,
    ))
}

/// Returns the actor's effective role while verifying an active-object permission.
pub(crate) async fn require_object_role(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    required_role: ObjectRole,
) -> ApiResult<ObjectRole> {
    let capability =
        active_object_role(pool, user_id, workspace_id, object_id, required_role).await?;
    require_projected_object_role(capability, required_role)
}

/// Ensures a user has the requested permission on an active object.
pub(crate) async fn ensure_object_permission(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    required_role: ObjectRole,
) -> ApiResult<()> {
    require_object_role(pool, user_id, workspace_id, object_id, required_role).await.map(|_| ())
}

/// Requires permission to read an active object or administratively read an archived object and
/// returns the effective role used for that authorization decision.
pub(crate) async fn require_object_readable_role(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> ApiResult<ObjectRole> {
    let capability = object_readable_role(pool, user_id, workspace_id, object_id).await?;
    if !capability.exists {
        return Err(ApiError::not_found("object not found"));
    }
    capability.role.ok_or_else(|| ApiError::forbidden("object view access required"))
}

/// Returns the actor's effective administrative role for an archived object.
pub(crate) async fn require_archived_object_admin_role(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> ApiResult<ObjectRole> {
    let capability = archived_object_admin_role(pool, user_id, workspace_id, object_id).await?;
    if !capability.exists {
        return Err(ApiError::not_found("object not found"));
    }
    capability.role.ok_or_else(|| ApiError::forbidden("object admin access required"))
}

/// Requires permission to read an active object or administratively read an archived object.
pub(crate) async fn require_object_readable(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> ApiResult<()> {
    require_object_readable_role(pool, user_id, workspace_id, object_id).await.map(|_| ())
}
