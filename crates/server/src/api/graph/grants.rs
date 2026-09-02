//! Object grant HTTP orchestration.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    EventKind, ObjectGrantRow, create_object_grant, list_object_grants, replace_object_grant,
    revoke_object_grant,
};
use kival_sdk::{
    CreateObjectGrantRequest, ListParams, ListResponse, ObjectGrant, ObjectGrantResponse,
    UpdateObjectGrantRequest,
};
use kival_types::{GrantPrincipal, ObjectRole};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        authz::require_object_role,
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        pagination,
        query::QueryParams,
    },
};

/// Converts a kernel object-grant row into its API representation.
const fn grant_into_wire(row: &ObjectGrantRow) -> ObjectGrant {
    let (principal_user_id, principal_group_id) = match row.principal {
        GrantPrincipal::User(user_id) => (Some(user_id), None),
        GrantPrincipal::Group(group_id) => (None, Some(group_id)),
    };

    ObjectGrant {
        id: row.id,
        workspace_id: row.workspace_id,
        object_id: row.object_id,
        principal_user_id,
        principal_group_id,
        object_role: row.object_role,
        created_by: row.created_by,
        revoked_by: row.revoked_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        revoked_at: row.revoked_at,
    }
}

/// Lists active grants on an object.
pub(crate) async fn handle_list_object_grants(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    QueryParams(params): QueryParams<ListParams>,
) -> ApiResult<Json<ListResponse<ObjectGrant>>> {
    let cursor = pagination::decode_created_at(&params, "object_grants", Some(object_id))?;
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;

    let grants = list_object_grants(
        state.db(),
        workspace_id,
        object_id,
        actor.id,
        cursor.map(|c| c.created_at),
        cursor.map(|c| c.id),
        limit + 1,
    )
    .await?;

    let grants = grants.iter().map(grant_into_wire).collect::<Vec<_>>();

    Ok(Json(pagination::created_at_page(
        grants,
        limit,
        "object_grants",
        Some(object_id),
        |grant| (grant.created_at, grant.id),
    )?))
}

/// Creates an object grant.
pub(crate) async fn handle_create_object_grant(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    JsonBody(request): JsonBody<CreateObjectGrantRequest>,
) -> ApiResult<Json<ObjectGrantResponse>> {
    let principal = request.principal;
    require_object_role(state.db(), actor.id, workspace_id, object_id, ObjectRole::Admin).await?;

    let mut tx = state.db().begin().await?;
    let grant_row = create_object_grant(
        &mut tx,
        workspace_id,
        object_id,
        principal,
        request.object_role,
        actor.id,
    )
    .await?;
    let grant = grant_into_wire(&grant_row);

    let mut event = actor
        .event(
            EventKind::ObjectGrantCreated,
            json!({
                "object_grant_id": grant.id,
                "object_role": grant.object_role,
            }),
        )
        .workspace(workspace_id)
        .object(object_id)
        .object_grant(grant.id);
    if let Some(user_id) = grant.principal_user_id {
        event = event.target_user(user_id);
    }
    if let Some(group_id) = grant.principal_group_id {
        event = event.group(group_id);
    }
    emit_event(&mut tx, state.durable_tasks().queue(), event).await?;

    tx.commit().await?;

    Ok(Json(ObjectGrantResponse { grant }))
}

/// Replaces an active object grant with one carrying the requested role.
pub(crate) async fn handle_update_object_grant(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id, grant_id)): Path<(Uuid, Uuid, Uuid)>,
    JsonBody(request): JsonBody<UpdateObjectGrantRequest>,
) -> ApiResult<Json<ObjectGrantResponse>> {
    require_object_role(state.db(), actor.id, workspace_id, object_id, ObjectRole::Admin).await?;

    let mut tx = state.db().begin().await?;

    let (previous_grant_row, grant_row) = replace_object_grant(
        &mut tx,
        workspace_id,
        object_id,
        grant_id,
        request.object_role,
        actor.id,
    )
    .await?;
    let previous_grant = grant_into_wire(&previous_grant_row);
    let grant = grant_into_wire(&grant_row);

    let mut event = actor
        .event(
            EventKind::ObjectGrantUpdated,
            json!({
                "object_grant_id": grant.id,
                "previous_object_grant_id": previous_grant.id,
                "previous_object_role": previous_grant.object_role,
                "object_role": grant.object_role,
            }),
        )
        .workspace(workspace_id)
        .object(object_id)
        .object_grant(grant.id);
    if let Some(user_id) = grant.principal_user_id {
        event = event.target_user(user_id);
    }
    if let Some(group_id) = grant.principal_group_id {
        event = event.group(group_id);
    }
    emit_event(&mut tx, state.durable_tasks().queue(), event).await?;

    tx.commit().await?;

    Ok(Json(ObjectGrantResponse { grant }))
}

/// Revokes an object grant.
pub(crate) async fn handle_revoke_object_grant(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id, grant_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<ObjectGrantResponse>> {
    require_object_role(state.db(), actor.id, workspace_id, object_id, ObjectRole::Admin).await?;

    let mut tx = state.db().begin().await?;

    let grant_row =
        revoke_object_grant(&mut tx, workspace_id, object_id, grant_id, actor.id).await?;
    let grant = grant_into_wire(&grant_row);

    let mut event = actor
        .event(
            EventKind::ObjectGrantRevoked,
            json!({
                "object_grant_id": grant.id,
                "object_role": grant.object_role,
            }),
        )
        .workspace(workspace_id)
        .object(object_id)
        .object_grant(grant.id);
    if let Some(user_id) = grant.principal_user_id {
        event = event.target_user(user_id);
    }
    if let Some(group_id) = grant.principal_group_id {
        event = event.group(group_id);
    }
    emit_event(&mut tx, state.durable_tasks().queue(), event).await?;

    tx.commit().await?;

    Ok(Json(ObjectGrantResponse { grant }))
}
