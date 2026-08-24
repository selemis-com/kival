//! Workspace creation, retrieval, and lifecycle transitions.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    EventKind, ListVisibleWorkspaces, VisibleWorkspaceRow, WorkspaceRow, archive_workspace,
    create_workspace, fetch_visible_workspace, list_visible_workspaces, unarchive_workspace,
    update_workspace, workspace_exists,
};
use kival_sdk::{
    CreateWorkspaceRequest, ListParams, ListResponse, UpdateWorkspaceRequest, Workspace,
    WorkspaceListItem, WorkspaceListParams, WorkspaceResponse,
};
use kival_types::MembershipRole;
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        authz::{ensure_archived_workspace_admin, ensure_workspace_admin},
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        pagination,
        query::QueryParams,
        validate::{optional_trimmed, required_trimmed},
    },
};

/// Converts a kernel workspace row into its API representation.
fn workspace_into_wire(row: WorkspaceRow, effective_role: MembershipRole) -> Workspace {
    Workspace {
        id: row.id,
        name: row.name,
        description: row.description,
        status: row.status,
        effective_role,
        created_by: row.created_by,
        archived_by: row.archived_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        archived_at: row.archived_at,
    }
}

/// Converts a visible-workspace projection into a list item.
fn visible_workspace_into_wire(row: VisibleWorkspaceRow) -> WorkspaceListItem {
    let workspace = workspace_into_wire(
        WorkspaceRow {
            id: row.id,
            name: row.name,
            description: row.description,
            status: row.status,
            created_by: row.created_by,
            archived_by: row.archived_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
        },
        row.effective_role,
    );
    WorkspaceListItem { workspace, pinned: row.pinned, pinned_at: row.pinned_at }
}

/// Lists workspaces visible to the authenticated user.
pub(crate) async fn handle_list_workspaces(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    QueryParams(params): QueryParams<WorkspaceListParams>,
) -> ApiResult<Json<ListResponse<WorkspaceListItem>>> {
    let list_params = ListParams { limit: params.limit, cursor: params.cursor.clone() };
    let cursor_subject = actor.api_key_id().unwrap_or(actor.id);
    let limit = list_params.checked_limit().map_err(ApiError::bad_request)?;
    let workspace_search = params.q.as_deref().map(str::trim).filter(|value| !value.is_empty());
    let cursor_kind = pagination::filtered_kind(
        "workspaces",
        &(params.status.as_str(), workspace_search, params.pinned),
    )?;
    let cursor = pagination::decode_created_at(&list_params, &cursor_kind, Some(cursor_subject))?;

    let rows = list_visible_workspaces(
        state.db(),
        ListVisibleWorkspaces {
            cursor_created_at: cursor.map(|cursor| cursor.created_at),
            cursor_id: cursor.map(|cursor| cursor.id),
            user_id: actor.id,
            limit: limit + 1,
            status: params.status,
            api_key_id: actor.api_key_id(),
            query: workspace_search,
            pinned: params.pinned,
        },
    )
    .await?;

    let workspaces = rows.into_iter().map(visible_workspace_into_wire).collect::<Vec<_>>();

    Ok(Json(pagination::created_at_page(
        workspaces,
        limit,
        &cursor_kind,
        Some(cursor_subject),
        |workspace| (workspace.created_at, workspace.id),
    )?))
}

/// Creates a workspace.
pub(crate) async fn handle_create_workspace(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    JsonBody(request): JsonBody<CreateWorkspaceRequest>,
) -> ApiResult<Json<WorkspaceResponse>> {
    actor.require_session()?;
    let name = required_trimmed(&request.name, "workspace name")?;

    let mut tx = state.db().begin().await?;

    let workspace = workspace_into_wire(
        create_workspace(&mut tx, name, request.description.as_deref().map(str::trim), actor.id)
            .await?,
        MembershipRole::Admin,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(EventKind::WorkspaceCreated, json!({ "workspace_id": workspace.id }))
            .workspace(workspace.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(WorkspaceResponse { workspace }))
}

/// Gets a workspace by ID.
pub(crate) async fn handle_get_workspace(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<WorkspaceResponse>> {
    let workspace = fetch_visible_workspace(state.db(), workspace_id, actor.id).await?;

    let Some(workspace) = workspace else {
        let exists = workspace_exists(state.db(), workspace_id).await?;

        return Err(if exists {
            ApiError::forbidden("workspace access required")
        } else {
            ApiError::not_found("workspace not found")
        });
    };

    Ok(Json(WorkspaceResponse { workspace: visible_workspace_into_wire(workspace).workspace }))
}

/// Updates mutable workspace fields.
pub(crate) async fn handle_update_workspace(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
    JsonBody(request): JsonBody<UpdateWorkspaceRequest>,
) -> ApiResult<Json<WorkspaceResponse>> {
    ensure_workspace_admin(state.db(), actor.id, workspace_id).await?;

    if request.name.is_none() && request.description.is_missing() {
        return Err(ApiError::bad_request("at least one field must be provided"));
    }

    let name = optional_trimmed(request.name.as_deref(), "workspace name")?;
    let description_present = request.description.is_present();
    let description = request.description.into_trimmed_option();

    let mut tx = state.db().begin().await?;

    let workspace = workspace_into_wire(
        update_workspace(&mut tx, workspace_id, name, description_present, description).await?,
        MembershipRole::Admin,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(EventKind::WorkspaceUpdated, json!({ "workspace_id": workspace.id }))
            .workspace(workspace.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(WorkspaceResponse { workspace }))
}

/// Archives a workspace.
pub(crate) async fn handle_archive_workspace(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<WorkspaceResponse>> {
    ensure_workspace_admin(state.db(), actor.id, workspace_id).await?;

    let mut tx = state.db().begin().await?;

    let workspace = workspace_into_wire(
        archive_workspace(&mut tx, workspace_id, actor.id).await?,
        MembershipRole::Admin,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(EventKind::WorkspaceArchived, json!({ "workspace_id": workspace.id }))
            .workspace(workspace.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(WorkspaceResponse { workspace }))
}

/// Unarchives a workspace.
pub(crate) async fn handle_unarchive_workspace(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<WorkspaceResponse>> {
    ensure_archived_workspace_admin(state.db(), actor.id, workspace_id).await?;

    let mut tx = state.db().begin().await?;

    let workspace = workspace_into_wire(
        unarchive_workspace(&mut tx, workspace_id).await?,
        MembershipRole::Admin,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(EventKind::WorkspaceUnarchived, json!({ "workspace_id": workspace.id }))
            .workspace(workspace.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(WorkspaceResponse { workspace }))
}
