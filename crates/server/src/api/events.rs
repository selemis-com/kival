//! Event handlers.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{EventRow, ListEvents, list_events, list_object_events, list_workspace_events};
use kival_sdk::{Event, EventListParams, ListResponse};
use uuid::Uuid;

/// Converts a kernel event row into its API representation.
fn event_into_wire(row: EventRow) -> Event {
    Event {
        id: row.id,
        sequence_number: row.sequence_number,
        workspace_id: row.workspace_id,
        actor_user_id: row.actor_user_id,
        actor_username: row.actor_username,
        api_key_id: row.api_key_id,
        api_key_label: row.api_key_label,
        event_kind: row.event_kind,
        object_id: row.object_id,
        object_version_id: row.object_version_id,
        object_edge_id: row.object_edge_id,
        object_grant_id: row.object_grant_id,
        comment_thread_id: row.comment_thread_id,
        comment_id: row.comment_id,
        group_id: row.group_id,
        target_user_id: row.target_user_id,
        payload: row.payload,
        created_at: row.created_at,
    }
}

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        error::{ApiError, ApiResult},
        pagination,
        query::QueryParams,
    },
};

/// Lists global events.
pub(crate) async fn handle_list_events(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    QueryParams(params): QueryParams<EventListParams>,
) -> ApiResult<Json<ListResponse<Event>>> {
    let (after_sequence, before_sequence) = pagination::validated_event_bounds(&params)?;
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;

    let query = ListEvents {
        after_sequence,
        before_sequence,
        event_kind: params.event_kind.as_deref(),
        actor_user_id: params.actor_user_id,
        target_user_id: params.target_user_id,
        object_id: params.object_id,
        group_id: params.group_id,
        order: params.order,
        limit,
    };
    let events = list_events(state.db(), actor.id, query).await?;

    Ok(Json(ListResponse::new(events.into_iter().map(event_into_wire).collect())))
}

/// Lists events in a workspace.
pub(crate) async fn handle_list_workspace_events(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
    QueryParams(params): QueryParams<EventListParams>,
) -> ApiResult<Json<ListResponse<Event>>> {
    let (after_sequence, before_sequence) = pagination::validated_event_bounds(&params)?;
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;

    let query = ListEvents {
        after_sequence,
        before_sequence,
        event_kind: params.event_kind.as_deref(),
        actor_user_id: params.actor_user_id,
        target_user_id: params.target_user_id,
        object_id: params.object_id,
        group_id: params.group_id,
        order: params.order,
        limit,
    };
    let events = list_workspace_events(state.db(), workspace_id, actor.id, query).await?;

    Ok(Json(ListResponse::new(events.into_iter().map(event_into_wire).collect())))
}

/// Lists events for an object.
pub(crate) async fn handle_list_object_events(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    QueryParams(params): QueryParams<EventListParams>,
) -> ApiResult<Json<ListResponse<Event>>> {
    let (after_sequence, before_sequence) = pagination::validated_event_bounds(&params)?;
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;

    let query = ListEvents {
        after_sequence,
        before_sequence,
        event_kind: params.event_kind.as_deref(),
        actor_user_id: params.actor_user_id,
        target_user_id: params.target_user_id,
        object_id: params.object_id,
        group_id: params.group_id,
        order: params.order,
        limit,
    };
    let events = list_object_events(state.db(), workspace_id, object_id, actor.id, query).await?;

    Ok(Json(ListResponse::new(events.into_iter().map(event_into_wire).collect())))
}
