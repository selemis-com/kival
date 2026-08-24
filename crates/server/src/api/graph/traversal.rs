//! Bounded object and workspace graph projections.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    ObjectGraphNodeRow, ObjectGraphQuery, WorkspaceGraphEdgeRow, WorkspaceGraphNodeRow,
    object_graph_edges_for_nodes, object_graph_nodes, workspace_graph_edges_for_nodes,
    workspace_graph_nodes,
};
use kival_sdk::{
    ObjectGraphEdge, ObjectGraphNode, ObjectGraphParams, ObjectGraphResponse,
    ObjectGraphTruncation, WorkspaceGraphEdge, WorkspaceGraphLimits, WorkspaceGraphNode,
    WorkspaceGraphParams, WorkspaceGraphResponse,
};
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        error::{ApiError, ApiResult},
        metrics::GraphMetrics,
        query::QueryParams,
    },
};

/// Default maximum number of workspace graph nodes.
const DEFAULT_GRAPH_NODE_LIMIT: i64 = 200;
/// Hard maximum number of workspace graph nodes.
const MAX_GRAPH_NODE_LIMIT: i64 = 1000;
/// Default maximum number of workspace graph edges.
const DEFAULT_GRAPH_EDGE_LIMIT: i64 = 500;
/// Hard maximum number of workspace graph edges.
const MAX_GRAPH_EDGE_LIMIT: i64 = 3000;
/// Default object graph traversal depth.
const DEFAULT_OBJECT_GRAPH_DEPTH: i32 = 1;
/// Hard maximum object graph traversal depth.
const MAX_OBJECT_GRAPH_DEPTH: i32 = 3;
/// Default maximum number of object graph nodes.
const DEFAULT_OBJECT_GRAPH_NODE_LIMIT: i64 = 100;
/// Hard maximum number of object graph nodes.
const MAX_OBJECT_GRAPH_NODE_LIMIT: i64 = 500;
/// Default maximum number of object graph edges.
const DEFAULT_OBJECT_GRAPH_EDGE_LIMIT: i64 = 250;
/// Hard maximum number of object graph edges.
const MAX_OBJECT_GRAPH_EDGE_LIMIT: i64 = 1500;

/// Validates and caps a positive graph limit.
fn validated_graph_limit(
    value: Option<i64>,
    default: i64,
    maximum: i64,
    field: &'static str,
) -> ApiResult<i64> {
    match value {
        Some(value) if value < 1 => {
            Err(ApiError::bad_request(format!("{field} must be at least 1")))
        }
        Some(value) => Ok(value.min(maximum)),
        None => Ok(default),
    }
}

/// Validates and caps a non-negative graph traversal depth.
fn validated_graph_depth(value: Option<i32>) -> ApiResult<i32> {
    match value {
        Some(value) if value < 0 => Err(ApiError::bad_request("depth must be at least 0")),
        Some(value) => Ok(value.min(MAX_OBJECT_GRAPH_DEPTH)),
        None => Ok(DEFAULT_OBJECT_GRAPH_DEPTH),
    }
}

/// Converts a kernel graph node row into its API representation.
fn object_graph_node_into_wire(row: ObjectGraphNodeRow) -> ObjectGraphNode {
    ObjectGraphNode {
        id: row.id,
        workspace_id: row.workspace_id,
        current_version_id: row.current_version_id,
        title: row.title,
        status: row.status,
        created_by: row.created_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        distance: row.distance,
        incoming_count: row.in_degree,
        outgoing_count: row.out_degree,
    }
}

/// Converts a kernel graph node row into its API representation.
fn workspace_graph_node_into_wire(row: WorkspaceGraphNodeRow) -> WorkspaceGraphNode {
    WorkspaceGraphNode {
        id: row.id,
        workspace_id: row.workspace_id,
        current_version_id: row.current_version_id,
        title: row.title,
        status: row.status,
        created_by: row.created_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        in_degree: row.in_degree,
        out_degree: row.out_degree,
    }
}

/// Converts a kernel graph edge into a workspace-graph edge.
const fn workspace_graph_edge_into_wire(row: WorkspaceGraphEdgeRow) -> WorkspaceGraphEdge {
    WorkspaceGraphEdge {
        id: row.id,
        workspace_id: row.workspace_id,
        source_object_id: row.source_object_id,
        target_object_id: row.target_object_id,
        created_by: row.created_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// Converts a kernel graph edge into an object-graph edge.
const fn object_graph_edge_into_wire(row: WorkspaceGraphEdgeRow) -> ObjectGraphEdge {
    ObjectGraphEdge {
        id: row.id,
        workspace_id: row.workspace_id,
        source_object_id: row.source_object_id,
        target_object_id: row.target_object_id,
        created_by: row.created_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// Returns a bounded authorized graph neighborhood around an active object.
pub(crate) async fn handle_get_object_graph(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    QueryParams(params): QueryParams<ObjectGraphParams>,
) -> ApiResult<Json<ObjectGraphResponse>> {
    let depth = validated_graph_depth(params.depth)?;
    let max_nodes = validated_graph_limit(
        params.max_nodes,
        DEFAULT_OBJECT_GRAPH_NODE_LIMIT,
        MAX_OBJECT_GRAPH_NODE_LIMIT,
        "max_nodes",
    )?;
    let max_edges = validated_graph_limit(
        params.max_edges,
        DEFAULT_OBJECT_GRAPH_EDGE_LIMIT,
        MAX_OBJECT_GRAPH_EDGE_LIMIT,
        "max_edges",
    )?;
    let mut metrics = GraphMetrics::start("object");

    let mut node_rows = object_graph_nodes(
        state.db(),
        ObjectGraphQuery {
            workspace_id,
            user_id: actor.id,
            root_object_id: object_id,
            depth,
            direction: params.direction,
            include_root: params.include_root,
            limit: max_nodes + 1,
        },
    )
    .await?;

    let truncated_nodes = node_rows.len() > max_nodes as usize;
    node_rows.truncate(max_nodes as usize);
    let node_ids: Vec<Uuid> = node_rows.iter().map(|node| node.id).collect();

    let mut edge_rows = object_graph_edges_for_nodes(
        state.db(),
        workspace_id,
        object_id,
        actor.id,
        &node_ids,
        max_edges + 1,
    )
    .await?;

    let truncated_edges = edge_rows.len() > max_edges as usize;
    edge_rows.truncate(max_edges as usize);
    let edges: Vec<ObjectGraphEdge> =
        edge_rows.into_iter().map(object_graph_edge_into_wire).collect();
    let nodes: Vec<ObjectGraphNode> =
        node_rows.into_iter().map(object_graph_node_into_wire).collect::<Vec<_>>();
    let truncation = ObjectGraphTruncation { nodes: truncated_nodes, edges: truncated_edges };
    let truncated = truncation.nodes || truncation.edges;
    metrics.complete(nodes.len(), edges.len(), truncated_nodes, truncated_edges);

    Ok(Json(ObjectGraphResponse {
        workspace_id,
        root_object_id: object_id,
        depth,
        direction: params.direction,
        max_nodes,
        max_edges,
        truncated,
        truncation,
        nodes,
        edges,
    }))
}

/// Returns a bounded authorized graph projection for an active workspace.
pub(crate) async fn handle_get_workspace_graph(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
    QueryParams(params): QueryParams<WorkspaceGraphParams>,
) -> ApiResult<Json<WorkspaceGraphResponse>> {
    let limit_nodes = validated_graph_limit(
        params.limit_nodes,
        DEFAULT_GRAPH_NODE_LIMIT,
        MAX_GRAPH_NODE_LIMIT,
        "limit_nodes",
    )?;
    let limit_edges = validated_graph_limit(
        params.limit_edges,
        DEFAULT_GRAPH_EDGE_LIMIT,
        MAX_GRAPH_EDGE_LIMIT,
        "limit_edges",
    )?;
    let mut metrics = GraphMetrics::start("workspace");

    let mut node_rows = workspace_graph_nodes(
        state.db(),
        workspace_id,
        actor.id,
        params.exclude_isolated,
        limit_nodes + 1,
    )
    .await?;

    let has_more_nodes = node_rows.len() > limit_nodes as usize;
    node_rows.truncate(limit_nodes as usize);
    let node_ids: Vec<Uuid> = node_rows.iter().map(|node| node.id).collect();

    let mut edge_rows = workspace_graph_edges_for_nodes(
        state.db(),
        workspace_id,
        actor.id,
        &node_ids,
        limit_edges + 1,
    )
    .await?;

    let has_more_edges = edge_rows.len() > limit_edges as usize;
    edge_rows.truncate(limit_edges as usize);
    let edges: Vec<WorkspaceGraphEdge> =
        edge_rows.into_iter().map(workspace_graph_edge_into_wire).collect();
    let nodes: Vec<WorkspaceGraphNode> =
        node_rows.into_iter().map(workspace_graph_node_into_wire).collect::<Vec<_>>();
    metrics.complete(nodes.len(), edges.len(), has_more_nodes, has_more_edges);

    Ok(Json(WorkspaceGraphResponse {
        workspace_id,
        nodes,
        edges,
        limits: WorkspaceGraphLimits { limit_nodes, limit_edges, has_more_nodes, has_more_edges },
    }))
}
