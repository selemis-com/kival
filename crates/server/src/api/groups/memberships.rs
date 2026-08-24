//! Group membership management.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    EventKind, GroupMembershipRow, KernelError, create_group_membership, list_group_memberships,
    replace_group_membership, revoke_group_membership,
};
use kival_sdk::{
    CreateGroupMembershipRequest, GroupMembership, GroupMembershipResponse, ListParams,
    ListResponse, UpdateGroupMembershipRequest,
};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        authz::require_active_group_admin,
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        pagination,
        query::QueryParams,
        validate::required_trimmed,
    },
};

/// Converts a kernel group-membership row
fn group_membership_into_wire(row: GroupMembershipRow) -> GroupMembership {
    GroupMembership {
        id: row.id,
        group_id: row.group_id,
        user_id: row.user_id,
        user_username: row.user_username,
        user_display_name: row.user_display_name,
        group_role: row.group_role,
        created_by: row.created_by,
        revoked_by: row.revoked_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        revoked_at: row.revoked_at,
    }
}

/// Lists active group memberships.
pub(crate) async fn handle_list_group_memberships(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(group_id): Path<Uuid>,
    QueryParams(params): QueryParams<ListParams>,
) -> ApiResult<Json<ListResponse<GroupMembership>>> {
    let cursor = pagination::decode_created_at(&params, "group_memberships", Some(group_id))?;
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;

    let memberships = list_group_memberships(
        state.db(),
        group_id,
        actor.id,
        cursor.map(|cursor| cursor.created_at),
        cursor.map(|cursor| cursor.id),
        limit + 1,
    )
    .await?;

    let memberships = memberships.into_iter().map(group_membership_into_wire).collect::<Vec<_>>();

    Ok(Json(pagination::created_at_page(
        memberships,
        limit,
        "group_memberships",
        Some(group_id),
        |membership| (membership.created_at, membership.id),
    )?))
}

/// Adds a user to a group.
pub(crate) async fn handle_create_group_membership(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(group_id): Path<Uuid>,
    JsonBody(request): JsonBody<CreateGroupMembershipRequest>,
) -> ApiResult<Json<GroupMembershipResponse>> {
    require_active_group_admin(state.db(), actor.id, group_id).await?;

    let (requested_user_id, requested_username) =
        match (request.user_id, request.username.as_deref()) {
            (Some(user_id), None) => (Some(user_id), None),
            (None, Some(username)) => (None, Some(required_trimmed(username, "user username")?)),
            _ => {
                return Err(ApiError::bad_request(
                    "exactly one of user_id and username must be provided",
                ));
            }
        };

    let mut tx = state.db().begin().await?;
    let membership = group_membership_into_wire(
        create_group_membership(
            &mut tx,
            group_id,
            requested_user_id,
            requested_username,
            request.group_role,
            actor.id,
        )
        .await
        .map_err(|error| match error {
            KernelError::Database(sqlx::Error::RowNotFound) => {
                ApiError::not_found("active Kival user not found")
            }
            error => ApiError::from(error),
        })?,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::GroupMembershipCreated,
                json!({
                    "group_membership_id": membership.id,
                    "group_role": membership.group_role,
                }),
            )
            .group(group_id)
            .target_user(membership.user_id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(GroupMembershipResponse { membership }))
}

/// Replaces an active group membership with one carrying the requested role.
pub(crate) async fn handle_update_group_membership(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((group_id, membership_id)): Path<(Uuid, Uuid)>,
    JsonBody(request): JsonBody<UpdateGroupMembershipRequest>,
) -> ApiResult<Json<GroupMembershipResponse>> {
    require_active_group_admin(state.db(), actor.id, group_id).await?;

    let mut tx = state.db().begin().await?;
    let (previous_membership, membership) =
        replace_group_membership(&mut tx, group_id, membership_id, request.group_role, actor.id)
            .await?;
    let previous_membership = group_membership_into_wire(previous_membership);
    let membership = group_membership_into_wire(membership);

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::GroupMembershipUpdated,
                json!({
                    "group_membership_id": membership.id,
                    "previous_group_membership_id": previous_membership.id,
                    "previous_group_role": previous_membership.group_role,
                    "group_role": membership.group_role,
                }),
            )
            .group(group_id)
            .target_user(membership.user_id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(GroupMembershipResponse { membership }))
}

/// Revokes a group membership.
pub(crate) async fn handle_revoke_group_membership(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((group_id, membership_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<GroupMembershipResponse>> {
    require_active_group_admin(state.db(), actor.id, group_id).await?;

    let mut tx = state.db().begin().await?;
    let membership = group_membership_into_wire(
        revoke_group_membership(&mut tx, group_id, membership_id, actor.id).await?,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::GroupMembershipRevoked,
                json!({
                    "group_membership_id": membership.id,
                    "group_role": membership.group_role,
                }),
            )
            .group(group_id)
            .target_user(membership.user_id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(GroupMembershipResponse { membership }))
}
