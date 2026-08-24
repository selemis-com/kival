//! Workspace-to-group link management.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    EventKind, KernelError, WorkspaceGroupRow, archive_workspace_group, create_workspace_group,
    list_workspace_groups, unarchive_workspace_group,
};
use kival_sdk::{
    CreateWorkspaceGroupRequest, ListParams, ListResponse, WorkspaceGroup,
    WorkspaceGroupListParams, WorkspaceGroupResponse,
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
    },
};

/// Converts a kernel workspace-group row into its API representation.
fn workspace_group_into_wire(row: WorkspaceGroupRow) -> WorkspaceGroup {
    WorkspaceGroup {
        id: row.id,
        workspace_id: row.workspace_id,
        group_id: row.group_id,
        group_name: row.group_name,
        group_description: row.group_description,
        status: row.status,
        created_by: row.created_by,
        archived_by: row.archived_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        archived_at: row.archived_at,
    }
}

/// Lists groups linked to a workspace.
pub(crate) async fn handle_list_workspace_groups(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
    QueryParams(params): QueryParams<WorkspaceGroupListParams>,
) -> ApiResult<Json<ListResponse<WorkspaceGroup>>> {
    let list_params = ListParams { limit: params.limit, cursor: params.cursor.clone() };
    let limit = list_params.checked_limit().map_err(ApiError::bad_request)?;
    let cursor_kind = pagination::filtered_kind("workspace_groups", &params.status.as_str())?;
    let cursor = pagination::decode_created_at(&list_params, &cursor_kind, Some(workspace_id))?;

    let groups = list_workspace_groups(
        state.db(),
        workspace_id,
        actor.id,
        cursor.map(|cursor| cursor.created_at),
        cursor.map(|cursor| cursor.id),
        limit + 1,
        params.status,
    )
    .await?;

    let groups = groups.into_iter().map(workspace_group_into_wire).collect::<Vec<_>>();

    Ok(Json(pagination::created_at_page(
        groups,
        limit,
        &cursor_kind,
        Some(workspace_id),
        |group| (group.created_at, group.id),
    )?))
}

/// Links a group to a kival.
pub(crate) async fn handle_create_workspace_group(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
    JsonBody(request): JsonBody<CreateWorkspaceGroupRequest>,
) -> ApiResult<Json<WorkspaceGroupResponse>> {
    ensure_workspace_admin(state.db(), actor.id, workspace_id).await?;
    let mut tx = state.db().begin().await?;

    let workspace_group = workspace_group_into_wire(
        create_workspace_group(&mut tx, workspace_id, request.group_id, actor.id).await.map_err(
            |error| match error {
                KernelError::Database(sqlx::Error::RowNotFound) => {
                    ApiError::bad_request("group_id must reference an active group")
                }
                error => ApiError::from(error),
            },
        )?,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::WorkspaceGroupLinked,
                json!({ "workspace_group_id": workspace_group.id }),
            )
            .workspace(workspace_id)
            .group(workspace_group.group_id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(WorkspaceGroupResponse { workspace_group }))
}

/// Archives a workspace-group link.
pub(crate) async fn handle_archive_workspace_group(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, group_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<WorkspaceGroupResponse>> {
    ensure_workspace_admin(state.db(), actor.id, workspace_id).await?;
    let mut tx = state.db().begin().await?;

    let workspace_group = workspace_group_into_wire(
        archive_workspace_group(&mut tx, workspace_id, group_id, actor.id).await?,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::WorkspaceGroupArchived,
                json!({ "workspace_group_id": workspace_group.id }),
            )
            .workspace(workspace_id)
            .group(group_id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(WorkspaceGroupResponse { workspace_group }))
}

/// Unarchives a workspace-group link.
pub(crate) async fn handle_unarchive_workspace_group(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, group_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<WorkspaceGroupResponse>> {
    ensure_workspace_admin(state.db(), actor.id, workspace_id).await?;
    let mut tx = state.db().begin().await?;

    let workspace_group = workspace_group_into_wire(
        unarchive_workspace_group(&mut tx, workspace_id, group_id).await?,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::WorkspaceGroupUnarchived,
                json!({ "workspace_group_id": workspace_group.id }),
            )
            .workspace(workspace_id)
            .group(group_id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(WorkspaceGroupResponse { workspace_group }))
}
