//! Personal workspace and object favorites.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    favorite_object, pin_object, pin_workspace, unfavorite_object, unpin_object, unpin_workspace,
};
use kival_sdk::{FavoriteState, PinState};
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        authz::{ensure_workspace_member, require_object_readable},
        error::ApiResult,
    },
};

/// Pins a visible workspace for the authenticated user.
pub(crate) async fn handle_pin_workspace(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<PinState>> {
    actor.require_session()?;
    ensure_workspace_member(state.db(), actor.id, workspace_id).await?;
    let pinned_at = pin_workspace(state.db(), actor.id, workspace_id).await?;
    Ok(Json(PinState { pinned: true, pinned_at: Some(pinned_at) }))
}

/// Removes a workspace pin without requiring current workspace access.
pub(crate) async fn handle_unpin_workspace(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<PinState>> {
    actor.require_session()?;
    unpin_workspace(state.db(), actor.id, workspace_id).await?;
    Ok(Json(PinState { pinned: false, pinned_at: None }))
}

/// Pins a readable object for the authenticated user.
pub(crate) async fn handle_pin_object(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<PinState>> {
    actor.require_session()?;
    require_object_readable(state.db(), actor.id, workspace_id, object_id).await?;
    let pinned_at = pin_object(state.db(), actor.id, workspace_id, object_id).await?;
    Ok(Json(PinState { pinned: true, pinned_at: Some(pinned_at) }))
}

/// Removes an object pin without requiring current object access.
pub(crate) async fn handle_unpin_object(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<PinState>> {
    actor.require_session()?;
    unpin_object(state.db(), actor.id, workspace_id, object_id).await?;
    Ok(Json(PinState { pinned: false, pinned_at: None }))
}

/// Favorites a readable object for the authenticated user.
pub(crate) async fn handle_favorite_object(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<FavoriteState>> {
    actor.require_session()?;
    require_object_readable(state.db(), actor.id, workspace_id, object_id).await?;
    favorite_object(state.db(), actor.id, workspace_id, object_id).await?;
    Ok(Json(FavoriteState { favorited: true }))
}

/// Removes an object favorite without requiring current object access.
pub(crate) async fn handle_unfavorite_object(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<FavoriteState>> {
    actor.require_session()?;
    unfavorite_object(state.db(), actor.id, workspace_id, object_id).await?;
    Ok(Json(FavoriteState { favorited: false }))
}
