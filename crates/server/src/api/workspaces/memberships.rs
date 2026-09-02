//! Direct workspace membership management.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    EventKind, KernelError, WorkspaceMembershipRow, create_workspace_membership,
    list_workspace_memberships, replace_workspace_membership, revoke_workspace_membership,
};
use kival_sdk::{
    CreateWorkspaceMembershipRequest, ListParams, ListResponse, UpdateWorkspaceMembershipRequest,
    WorkspaceMembership, WorkspaceMembershipResponse,
};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        authz::ensure_workspace_admin,
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        pagination,
        query::QueryParams,
        validate::required_trimmed,
    },
};

/// Converts a kernel workspace-membership row into its API representation.
fn workspace_membership_into_wire(row: WorkspaceMembershipRow) -> WorkspaceMembership {
    WorkspaceMembership {
        id: row.id,
        workspace_id: row.workspace_id,
        user_id: row.user_id,
        user_username: row.user_username,
        user_display_name: row.user_display_name,
        workspace_role: row.workspace_role,
        created_by: row.created_by,
        revoked_by: row.revoked_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        revoked_at: row.revoked_at,
    }
}

/// Lists active direct workspace memberships.
pub(crate) async fn handle_list_workspace_memberships(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
    QueryParams(params): QueryParams<ListParams>,
) -> ApiResult<Json<ListResponse<WorkspaceMembership>>> {
    let cursor =
        pagination::decode_created_at(&params, "workspace_memberships", Some(workspace_id))?;
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;

    let memberships = list_workspace_memberships(
        state.db(),
        workspace_id,
        actor.id,
        cursor.map(|cursor| cursor.created_at),
        cursor.map(|cursor| cursor.id),
        limit + 1,
    )
    .await?;

    let memberships =
        memberships.into_iter().map(workspace_membership_into_wire).collect::<Vec<_>>();

    Ok(Json(pagination::created_at_page(
        memberships,
        limit,
        "workspace_memberships",
        Some(workspace_id),
        |membership| (membership.created_at, membership.id),
    )?))
}

/// Adds an existing active user to a workspace.
pub(crate) async fn handle_create_workspace_membership(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
    JsonBody(request): JsonBody<CreateWorkspaceMembershipRequest>,
) -> ApiResult<Json<WorkspaceMembershipResponse>> {
    ensure_workspace_admin(state.db(), actor.id, workspace_id).await?;

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

    let membership = workspace_membership_into_wire(
        create_workspace_membership(
            &mut tx,
            workspace_id,
            requested_user_id,
            requested_username,
            request.workspace_role,
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
                EventKind::WorkspaceMembershipCreated,
                json!({
                    "workspace_membership_id": membership.id,
                    "workspace_role": membership.workspace_role,
                }),
            )
            .workspace(workspace_id)
            .target_user(membership.user_id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(WorkspaceMembershipResponse { membership }))
}

/// Replaces an active workspace membership with one carrying the requested role.
pub(crate) async fn handle_update_workspace_membership(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, membership_id)): Path<(Uuid, Uuid)>,
    JsonBody(request): JsonBody<UpdateWorkspaceMembershipRequest>,
) -> ApiResult<Json<WorkspaceMembershipResponse>> {
    ensure_workspace_admin(state.db(), actor.id, workspace_id).await?;

    let mut tx = state.db().begin().await?;

    let (previous_membership, membership) = replace_workspace_membership(
        &mut tx,
        workspace_id,
        membership_id,
        request.workspace_role,
        actor.id,
    )
    .await?;
    let previous_membership = workspace_membership_into_wire(previous_membership);
    let membership = workspace_membership_into_wire(membership);

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::WorkspaceMembershipUpdated,
                json!({
                    "workspace_membership_id": membership.id,
                    "previous_workspace_membership_id": previous_membership.id,
                    "previous_workspace_role": previous_membership.workspace_role,
                    "workspace_role": membership.workspace_role,
                }),
            )
            .workspace(workspace_id)
            .target_user(membership.user_id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(WorkspaceMembershipResponse { membership }))
}

/// Revokes a direct workspace membership.
pub(crate) async fn handle_revoke_workspace_membership(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, membership_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<WorkspaceMembershipResponse>> {
    ensure_workspace_admin(state.db(), actor.id, workspace_id).await?;
    let mut tx = state.db().begin().await?;

    let membership = workspace_membership_into_wire(
        revoke_workspace_membership(&mut tx, workspace_id, membership_id, actor.id).await?,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::WorkspaceMembershipRevoked,
                json!({
                    "workspace_membership_id": membership.id,
                    "workspace_role": membership.workspace_role,
                }),
            )
            .workspace(workspace_id)
            .target_user(membership.user_id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(WorkspaceMembershipResponse { membership }))
}
