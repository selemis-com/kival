//! User handlers.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    EventKind, UserRow, can_manage_groups, disable_user, enable_user, fetch_active_user,
    fetch_user, is_global_admin, list_users, update_user_display_name,
};
use kival_sdk::{
    ListParams, ListResponse, UpdateUserRequest, User, UserListParams, UserResponse, WhoamiResponse,
};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        authz::ensure_global_admin,
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        pagination,
        query::QueryParams,
        validate::optional_trimmed,
    },
};

/// Converts a kernel user row into its API representation.
pub(crate) fn user_into_wire(row: UserRow) -> User {
    User {
        id: row.id,
        username: row.username,
        display_name: row.display_name,
        status: row.status,
        created_at: row.created_at,
        updated_at: row.updated_at,
        disabled_at: row.disabled_at,
        disabled_by: row.disabled_by,
    }
}

/// Gets the user associated with the current authentication credential.
pub(crate) async fn handle_get_current_user(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
) -> ApiResult<Json<WhoamiResponse>> {
    let user = user_into_wire(fetch_active_user(state.db(), actor.id).await?);

    let global_admin = is_global_admin(state.db(), actor.id).await?;
    let can_manage_groups = global_admin || can_manage_groups(state.db(), actor.id).await?;
    Ok(Json(WhoamiResponse {
        user,
        is_global_admin: global_admin,
        can_manage_groups,
        scopes: actor.api_key_scopes.clone(),
    }))
}

/// Lists active users.
pub(crate) async fn handle_list_users(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    QueryParams(params): QueryParams<UserListParams>,
) -> ApiResult<Json<ListResponse<User>>> {
    let list_params = ListParams { limit: params.limit, cursor: params.cursor.clone() };
    let limit = list_params.checked_limit().map_err(ApiError::bad_request)?;
    let user_search = params.q.as_deref().map(str::trim).filter(|value| !value.is_empty());
    let cursor_kind = pagination::filtered_kind("users", &(params.status.as_str(), user_search))?;
    let cursor = pagination::decode_created_at(&list_params, &cursor_kind, None)?;

    let rows = list_users(
        state.db(),
        actor.id,
        cursor.map(|cursor| cursor.created_at),
        cursor.map(|cursor| cursor.id),
        limit + 1,
        params.status,
        user_search,
    )
    .await?;

    let users = rows.into_iter().map(user_into_wire).collect::<Vec<_>>();

    Ok(Json(pagination::created_at_page(users, limit, &cursor_kind, None, |user| {
        (user.created_at, user.id)
    })?))
}

/// Gets a user by ID.
pub(crate) async fn handle_get_user(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(user_id): Path<Uuid>,
) -> ApiResult<Json<UserResponse>> {
    let user = user_into_wire(fetch_user(state.db(), actor.id, user_id).await?);

    Ok(Json(UserResponse { user, is_global_admin: None, can_manage_groups: None }))
}

/// Updates mutable user profile fields.
pub(crate) async fn handle_update_user(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(user_id): Path<Uuid>,
    JsonBody(request): JsonBody<UpdateUserRequest>,
) -> ApiResult<Json<UserResponse>> {
    ensure_global_admin(state.db(), actor.id).await?;
    if request.display_name.is_none() {
        return Err(ApiError::bad_request("at least one field must be provided"));
    }

    let display_name = optional_trimmed(request.display_name.as_deref(), "display_name")?;

    let mut tx = state.db().begin().await?;

    let user = user_into_wire(update_user_display_name(&mut tx, user_id, display_name).await?);

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor.event(EventKind::UserUpdated, json!({ "user_id": user.id })).target_user(user.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(UserResponse { user, is_global_admin: None, can_manage_groups: None }))
}

/// Disables a user.
pub(crate) async fn handle_disable_user(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(user_id): Path<Uuid>,
) -> ApiResult<Json<UserResponse>> {
    ensure_global_admin(state.db(), actor.id).await?;
    if actor.id == user_id {
        return Err(ApiError::conflict("cannot disable the current user"));
    }

    let mut tx = state.db().begin().await?;

    let user = user_into_wire(disable_user(&mut tx, user_id, actor.id).await?);

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor.event(EventKind::UserDisabled, json!({ "user_id": user.id })).target_user(user.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(UserResponse { user, is_global_admin: None, can_manage_groups: None }))
}

/// Enables a disabled user without changing credentials, access, or memberships.
pub(crate) async fn handle_enable_user(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(user_id): Path<Uuid>,
) -> ApiResult<Json<UserResponse>> {
    ensure_global_admin(state.db(), actor.id).await?;

    let mut tx = state.db().begin().await?;

    let user = user_into_wire(enable_user(&mut tx, user_id).await?);

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor.event(EventKind::UserEnabled, json!({ "user_id": user.id })).target_user(user.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(UserResponse { user, is_global_admin: None, can_manage_groups: None }))
}
