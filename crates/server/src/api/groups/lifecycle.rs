//! Group creation, retrieval, and lifecycle transitions.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    EventKind, GroupRow, archive_group, create_group, fetch_group, list_groups, unarchive_group,
    update_group,
};
use kival_sdk::{
    CreateGroupRequest, Group, GroupListParams, GroupResponse, ListParams, ListResponse,
    UpdateGroupRequest,
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
        validate::{optional_trimmed, required_trimmed},
    },
};

/// Converts a kernel group row into its API representation.
fn group_into_wire(row: GroupRow) -> Group {
    Group {
        id: row.id,
        name: row.name,
        description: row.description,
        status: row.status,
        created_by: row.created_by,
        archived_by: row.archived_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        archived_at: row.archived_at,
    }
}

/// Lists groups by archive status.
pub(crate) async fn handle_list_groups(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    QueryParams(params): QueryParams<GroupListParams>,
) -> ApiResult<Json<ListResponse<Group>>> {
    let list_params = ListParams { limit: params.limit, cursor: params.cursor.clone() };
    let limit = list_params.checked_limit().map_err(ApiError::bad_request)?;
    let group_search = params.q.as_deref().map(str::trim).filter(|value| !value.is_empty());
    let cursor_kind = pagination::filtered_kind("groups", &(params.status.as_str(), group_search))?;
    let cursor = pagination::decode_created_at(&list_params, &cursor_kind, None)?;

    let groups = list_groups(
        state.db(),
        cursor.map(|cursor| cursor.created_at),
        cursor.map(|cursor| cursor.id),
        limit + 1,
        params.status,
        actor.id,
        group_search,
    )
    .await?;

    let groups = groups.into_iter().map(group_into_wire).collect::<Vec<_>>();

    Ok(Json(pagination::created_at_page(groups, limit, &cursor_kind, None, |group| {
        (group.created_at, group.id)
    })?))
}

/// Creates a group.
pub(crate) async fn handle_create_group(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    JsonBody(request): JsonBody<CreateGroupRequest>,
) -> ApiResult<Json<GroupResponse>> {
    ensure_global_admin(state.db(), actor.id).await?;

    let name = required_trimmed(&request.name, "group name")?;

    let mut tx = state.db().begin().await?;

    let group = group_into_wire(
        create_group(&mut tx, name, request.description.as_deref().map(str::trim), actor.id)
            .await?,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor.event(EventKind::GroupCreated, json!({ "group_id": group.id })).group(group.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(GroupResponse { group }))
}

/// Gets a group by ID.
pub(crate) async fn handle_get_group(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(group_id): Path<Uuid>,
) -> ApiResult<Json<GroupResponse>> {
    let group = group_into_wire(fetch_group(state.db(), actor.id, group_id).await?);

    Ok(Json(GroupResponse { group }))
}

/// Updates mutable group fields.
pub(crate) async fn handle_update_group(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(group_id): Path<Uuid>,
    JsonBody(request): JsonBody<UpdateGroupRequest>,
) -> ApiResult<Json<GroupResponse>> {
    ensure_global_admin(state.db(), actor.id).await?;
    if request.name.is_none() && request.description.is_missing() {
        return Err(ApiError::bad_request("at least one field must be provided"));
    }

    let name = optional_trimmed(request.name.as_deref(), "group name")?;
    let description_present = request.description.is_present();
    let description = request.description.into_trimmed_option();

    let mut tx = state.db().begin().await?;

    let group = group_into_wire(
        update_group(&mut tx, group_id, name, description_present, description).await?,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor.event(EventKind::GroupUpdated, json!({ "group_id": group.id })).group(group.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(GroupResponse { group }))
}

/// Archives a group.
pub(crate) async fn handle_archive_group(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(group_id): Path<Uuid>,
) -> ApiResult<Json<GroupResponse>> {
    ensure_global_admin(state.db(), actor.id).await?;

    let mut tx = state.db().begin().await?;

    let group = group_into_wire(archive_group(&mut tx, group_id, actor.id).await?);

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor.event(EventKind::GroupArchived, json!({ "group_id": group.id })).group(group.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(GroupResponse { group }))
}

/// Unarchives a group.
pub(crate) async fn handle_unarchive_group(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(group_id): Path<Uuid>,
) -> ApiResult<Json<GroupResponse>> {
    ensure_global_admin(state.db(), actor.id).await?;

    let mut tx = state.db().begin().await?;

    let group = group_into_wire(unarchive_group(&mut tx, group_id).await?);

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor.event(EventKind::GroupUnarchived, json!({ "group_id": group.id })).group(group.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(GroupResponse { group }))
}
