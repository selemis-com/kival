//! Object edges, explicit backlinks, and derived textual backlinks.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    EventKind, ListObjectBacklinks, ListObjectEdges, ObjectBacklinkReferenceRow, ObjectBacklinkRow,
    ObjectEdgeRow, create_object_edge, fetch_object_edge, fetch_object_edge_for_revoke,
    list_object_backlink_edges, list_object_backlink_references, list_object_edges,
    revoke_object_edge,
};
use kival_sdk::{
    BacklinkSourceObject, CreateObjectEdgeRequest, ListParams, ListResponse, ObjectBacklink,
    ObjectBacklinkReference, ObjectBacklinksParams, ObjectBacklinksResponse, ObjectEdge,
    ObjectEdgeResponse,
};
use kival_types::ObjectRole;
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        authz::require_object_role_pair,
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        pagination,
        query::QueryParams,
    },
};

/// Converts an explicit backlink row into its API representation.
fn backlink_into_wire(row: ObjectBacklinkRow) -> ObjectBacklink {
    ObjectBacklink {
        edge_id: row.edge_id,
        source_object: BacklinkSourceObject {
            id: row.source_object_id,
            title: row.source_title,
            status: row.source_status,
        },
        target_object_id: row.target_object_id,
        created_by: row.created_by,
        created_at: row.created_at,
    }
}

/// Converts a resolved textual backlink row into its API representation.
fn backlink_reference_into_wire(row: ObjectBacklinkReferenceRow) -> ObjectBacklinkReference {
    ObjectBacklinkReference {
        reference_id: row.reference_id,
        reference_kind: row.reference_kind,
        source_object: BacklinkSourceObject {
            id: row.source_object_id,
            title: row.source_title,
            status: row.source_status,
        },
        source_version_id: row.source_version_id,
        target_object_id: row.target_object_id,
        raw_target: row.raw_target,
        display_text: row.display_text,
        span_start: row.span_start,
        span_end: row.span_end,
        created_at: row.created_at,
    }
}

/// Converts a kernel edge row into its API representation.
const fn edge_into_wire(row: ObjectEdgeRow) -> ObjectEdge {
    ObjectEdge {
        id: row.id,
        workspace_id: row.workspace_id,
        source_object_id: row.source_object_id,
        target_object_id: row.target_object_id,
        created_by: row.created_by,
        revoked_by: row.revoked_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        revoked_at: row.revoked_at,
    }
}

/// Lists visible inbound explicit edges and textual references for an object.
pub(crate) async fn handle_get_object_backlinks(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    QueryParams(params): QueryParams<ObjectBacklinksParams>,
) -> ApiResult<Json<ObjectBacklinksResponse>> {
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;
    let continuation = params.edge_cursor.is_some() || params.reference_cursor.is_some();
    let fetch_edges = !continuation || params.edge_cursor.is_some();
    let fetch_references = !continuation || params.reference_cursor.is_some();

    let edge_cursor_kind =
        pagination::filtered_kind("object_backlink_edges", &params.include_archived)?;
    let edge_cursor_params = ListParams { limit: params.limit, cursor: params.edge_cursor.clone() };
    let edge_cursor =
        pagination::decode_created_at(&edge_cursor_params, &edge_cursor_kind, Some(object_id))?;

    let rows = list_object_backlink_edges(
        state.db(),
        ListObjectBacklinks {
            workspace_id,
            object_id,
            include_archived: params.include_archived,
            cursor_created_at: edge_cursor.map(|cursor| cursor.created_at),
            cursor_id: edge_cursor.map(|cursor| cursor.id),
            user_id: actor.id,
            fetch: fetch_edges,
            limit: limit + 1,
        },
    )
    .await?;

    let incoming_edges = rows.into_iter().map(backlink_into_wire).collect::<Vec<_>>();
    let edge_page = pagination::created_at_page(
        incoming_edges,
        limit,
        &edge_cursor_kind,
        Some(object_id),
        |edge| (edge.created_at, edge.edge_id),
    )?;

    let reference_cursor_kind =
        pagination::filtered_kind("object_backlink_references", &params.include_archived)?;
    let reference_cursor_params =
        ListParams { limit: params.limit, cursor: params.reference_cursor.clone() };
    let reference_cursor = pagination::decode_created_at(
        &reference_cursor_params,
        &reference_cursor_kind,
        Some(object_id),
    )?;

    let reference_rows = list_object_backlink_references(
        state.db(),
        ListObjectBacklinks {
            workspace_id,
            object_id,
            include_archived: params.include_archived,
            cursor_created_at: reference_cursor.map(|cursor| cursor.created_at),
            cursor_id: reference_cursor.map(|cursor| cursor.id),
            user_id: actor.id,
            fetch: fetch_references,
            limit: limit + 1,
        },
    )
    .await?;

    let incoming_references =
        reference_rows.into_iter().map(backlink_reference_into_wire).collect::<Vec<_>>();
    let reference_page = pagination::created_at_page(
        incoming_references,
        limit,
        &reference_cursor_kind,
        Some(object_id),
        |reference| (reference.created_at, reference.reference_id),
    )?;

    Ok(Json(ObjectBacklinksResponse {
        object_id,
        incoming_edges: edge_page.items,
        next_edge_cursor: edge_page.next_cursor,
        incoming_references: reference_page.items,
        next_reference_cursor: reference_page.next_cursor,
    }))
}

/// Lists active edges attached to an object.
pub(crate) async fn handle_list_object_edges(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    QueryParams(params): QueryParams<ListParams>,
) -> ApiResult<Json<ListResponse<ObjectEdge>>> {
    let cursor = pagination::decode_created_at(&params, "object_edges", Some(object_id))?;
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;
    let edges = list_object_edges(
        state.db(),
        ListObjectEdges {
            workspace_id,
            object_id,
            cursor_created_at: cursor.map(|cursor| cursor.created_at),
            cursor_id: cursor.map(|cursor| cursor.id),
            user_id: actor.id,
            limit: limit + 1,
        },
    )
    .await?;

    let edges = edges.into_iter().map(edge_into_wire).collect();

    Ok(Json(pagination::created_at_page(edges, limit, "object_edges", Some(object_id), |edge| {
        (edge.created_at, edge.id)
    })?))
}

/// Creates an object edge.
pub(crate) async fn handle_create_edge(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
    JsonBody(request): JsonBody<CreateObjectEdgeRequest>,
) -> ApiResult<Json<ObjectEdgeResponse>> {
    require_object_role_pair(
        state.db(),
        actor.id,
        workspace_id,
        request.source_object_id,
        ObjectRole::Editor,
        request.target_object_id,
        ObjectRole::Viewer,
    )
    .await?;

    let mut tx = state.db().begin().await?;

    let edge = edge_into_wire(
        create_object_edge(
            &mut tx,
            workspace_id,
            request.source_object_id,
            request.target_object_id,
            actor.id,
        )
        .await?,
    );

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::ObjectEdgeCreated,
                json!({
                    "object_edge_id": edge.id,
                    "source_object_id": edge.source_object_id,
                    "target_object_id": edge.target_object_id
                }),
            )
            .workspace(workspace_id)
            .object(edge.source_object_id)
            .object_edge(edge.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(ObjectEdgeResponse { edge }))
}

/// Gets an object edge.
pub(crate) async fn handle_get_edge(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, edge_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ObjectEdgeResponse>> {
    let edge =
        edge_into_wire(fetch_object_edge(state.db(), workspace_id, edge_id, actor.id).await?);

    Ok(Json(ObjectEdgeResponse { edge }))
}

/// Revokes an object edge.
pub(crate) async fn handle_revoke_edge(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, edge_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ObjectEdgeResponse>> {
    fetch_object_edge_for_revoke(state.db(), workspace_id, edge_id, actor.id).await?;
    let mut tx = state.db().begin().await?;

    let edge = edge_into_wire(revoke_object_edge(&mut tx, workspace_id, edge_id, actor.id).await?);

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(EventKind::ObjectEdgeRevoked, json!({ "object_edge_id": edge.id }))
            .workspace(workspace_id)
            .object(edge.source_object_id)
            .object_edge(edge.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(ObjectEdgeResponse { edge }))
}
