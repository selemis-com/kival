//! Object graph and grant wire protocol types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, serde::rfc3339};
use uuid::Uuid;

use crate::{ArchiveStatus, DEFAULT_LIMIT, MAX_LIMIT};

/// Query parameters for object backlinks.
///
/// Omit both cursors on the initial request to fetch both backlink streams. On continuation
/// requests, only streams with a supplied cursor are fetched; an omitted cursor means that stream
/// is exhausted or is not being continued.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct ObjectBacklinksParams {
    /// Maximum number of explicit edges and textual references to return per section.
    pub limit: Option<i64>,
    /// Opaque cursor for the next explicit-edge page.
    pub edge_cursor: Option<String>,
    /// Opaque cursor for the next textual-reference page.
    pub reference_cursor: Option<String>,
    /// Include archived source objects when the actor may read them.
    #[serde(default)]
    pub include_archived: bool,
}

impl ObjectBacklinksParams {
    /// Returns the validated and capped result limit.
    ///
    /// # Errors
    ///
    /// Returns an error if the requested limit is less than one.
    pub fn checked_limit(&self) -> Result<i64, &'static str> {
        match self.limit {
            Some(limit) if limit < 1 => Err("limit must be at least 1"),
            Some(limit) => Ok(limit.min(MAX_LIMIT)),
            None => Ok(DEFAULT_LIMIT),
        }
    }
}

/// Source object summary included with an explicit backlink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BacklinkSourceObject {
    /// Source object ID.
    pub id: Uuid,
    /// Source object title.
    pub title: String,
    /// Source object lifecycle status.
    pub status: ArchiveStatus,
}

/// Explicit inbound edge in a backlinks response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectBacklink {
    /// Edge ID.
    pub edge_id: Uuid,
    /// Visible source object summary.
    pub source_object: BacklinkSourceObject,
    /// Requested target object ID.
    pub target_object_id: Uuid,
    /// User that created the edge.
    pub created_by: Option<Uuid>,
    /// Edge creation timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
}

/// Derived textual reference pointing to an object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectBacklinkReference {
    /// Derived reference row ID.
    pub reference_id: Uuid,
    /// Reference syntax kind.
    pub reference_kind: String,
    /// Visible source object summary.
    pub source_object: BacklinkSourceObject,
    /// Source object version containing the reference.
    pub source_version_id: Uuid,
    /// Resolved target object ID.
    pub target_object_id: Uuid,
    /// Raw target text found in source content.
    pub raw_target: String,
    /// Optional display text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_text: Option<String>,
    /// Inclusive UTF-8 byte offset in the source body.
    pub span_start: i32,
    /// Exclusive UTF-8 byte offset in the source body.
    pub span_end: i32,
    /// Reference creation timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
}

/// Object-centric inbound references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectBacklinksResponse {
    /// Requested target object ID.
    pub object_id: Uuid,
    /// Visible explicit inbound graph edges.
    pub incoming_edges: Vec<ObjectBacklink>,
    /// Opaque cursor for the next explicit-edge page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_edge_cursor: Option<String>,
    /// Visible resolved textual references from current source versions.
    pub incoming_references: Vec<ObjectBacklinkReference>,
    /// Opaque cursor for the next textual-reference page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_reference_cursor: Option<String>,
}

pub use kival_types::{GrantPrincipal, ObjectGraphDirection, ObjectRole};

/// Query parameters for a bounded object-centered graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectGraphParams {
    /// Maximum traversal depth from the root.
    pub depth: Option<i32>,
    /// Traversal direction.
    #[serde(default)]
    pub direction: ObjectGraphDirection,
    /// Maximum number of nodes to return.
    #[serde(alias = "limit_nodes")]
    pub max_nodes: Option<i64>,
    /// Maximum number of edges to return.
    #[serde(alias = "limit_edges")]
    pub max_edges: Option<i64>,
    /// Include the root node in the returned node set.
    #[serde(default = "default_true")]
    pub include_root: bool,
}

impl Default for ObjectGraphParams {
    fn default() -> Self {
        Self {
            depth: None,
            direction: ObjectGraphDirection::Both,
            max_nodes: None,
            max_edges: None,
            include_root: true,
        }
    }
}

/// Node in an object-centered graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectGraphNode {
    /// Object ID.
    pub id: Uuid,
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Current object version ID.
    pub current_version_id: Option<Uuid>,
    /// Title projected from the current immutable version.
    pub title: String,
    /// Object lifecycle status.
    pub status: ArchiveStatus,
    /// User that created the object.
    pub created_by: Option<Uuid>,
    /// Creation timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    /// Last update timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
    /// Shortest traversal distance from the root.
    pub distance: i32,
    /// Visible filtered incoming explicit-relationship count.
    pub incoming_count: i64,
    /// Visible filtered outgoing explicit-relationship count.
    pub outgoing_count: i64,
}

/// Origin of a graph link projected into an object or workspace graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GraphEdgeKind {
    /// Explicit relationship stored in the object-edge model.
    Relationship,
    /// Resolved reference authored in the source object's current version.
    Reference,
    /// The same directed graph link exists both explicitly and as a reference.
    RelationshipAndReference,
}

/// Edge in an object-centered graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectGraphEdge {
    /// Representative relationship or reference row ID.
    pub id: Uuid,
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Source object ID.
    pub source_object_id: Uuid,
    /// Target object ID.
    pub target_object_id: Uuid,
    /// How this directed graph link is represented in Kival.
    pub kind: GraphEdgeKind,
    /// User that created the representative relationship or source version.
    pub created_by: Option<Uuid>,
    /// Creation timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    /// Last update timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// Structured object graph truncation details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectGraphTruncation {
    /// Whether traversal hit the node cap.
    pub nodes: bool,
    /// Whether the returned edge set hit the edge cap.
    pub edges: bool,
}

/// Bounded authorized graph neighborhood around an object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectGraphResponse {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Root object ID.
    pub root_object_id: Uuid,
    /// Applied traversal depth.
    pub depth: i32,
    /// Applied traversal direction.
    pub direction: ObjectGraphDirection,
    /// Applied maximum node count.
    pub max_nodes: i64,
    /// Applied maximum edge count.
    pub max_edges: i64,
    /// Whether any truncation occurred.
    pub truncated: bool,
    /// Structured truncation details.
    pub truncation: ObjectGraphTruncation,
    /// Visible local graph nodes.
    pub nodes: Vec<ObjectGraphNode>,
    /// Visible edges between returned nodes.
    pub edges: Vec<ObjectGraphEdge>,
}

/// Returns true for serde defaults.
const fn default_true() -> bool {
    true
}

/// Query parameters for a bounded workspace graph projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceGraphParams {
    /// Maximum number of nodes to return.
    pub limit_nodes: Option<i64>,
    /// Maximum number of edges to return.
    pub limit_edges: Option<i64>,
    /// Exclude nodes with no visible filtered relation.
    #[serde(default)]
    pub exclude_isolated: bool,
}

/// Node in a workspace graph projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceGraphNode {
    /// Object ID.
    pub id: Uuid,
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Current object version ID.
    pub current_version_id: Option<Uuid>,
    /// Title projected from the current immutable version.
    pub title: String,
    /// Object lifecycle status.
    pub status: ArchiveStatus,
    /// User that created the object.
    pub created_by: Option<Uuid>,
    /// Creation timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    /// Last update timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
    /// Number of incoming explicit relationships in the visible filtered graph before edge
    /// response truncation.
    pub in_degree: i64,
    /// Number of outgoing explicit relationships in the visible filtered graph before edge
    /// response truncation.
    pub out_degree: i64,
}

/// Edge in a workspace graph projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceGraphEdge {
    /// Representative relationship or reference row ID.
    pub id: Uuid,
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Source object ID.
    pub source_object_id: Uuid,
    /// Target object ID.
    pub target_object_id: Uuid,
    /// How this directed graph link is represented in Kival.
    pub kind: GraphEdgeKind,
    /// User that created the representative relationship or source version.
    pub created_by: Option<Uuid>,
    /// Creation timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    /// Last update timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// Applied workspace graph limits and truncation indicators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceGraphLimits {
    /// Applied node limit.
    pub limit_nodes: i64,
    /// Applied edge limit.
    pub limit_edges: i64,
    /// Whether more visible nodes matched.
    pub has_more_nodes: bool,
    /// Whether more visible edges matched between the returned nodes.
    pub has_more_edges: bool,
}

/// Bounded workspace graph projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceGraphResponse {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Visible graph nodes.
    pub nodes: Vec<WorkspaceGraphNode>,
    /// Visible edges between returned nodes.
    pub edges: Vec<WorkspaceGraphEdge>,
    /// Applied limits and truncation indicators.
    pub limits: WorkspaceGraphLimits,
}

/// Request body for creating an object edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CreateObjectEdgeRequest {
    /// Source object ID.
    pub source_object_id: Uuid,

    /// Target object ID.
    pub target_object_id: Uuid,
}

/// Object edge resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectEdge {
    /// Object edge ID.
    pub id: Uuid,

    /// Workspace ID.
    pub workspace_id: Uuid,

    /// Source object ID.
    pub source_object_id: Uuid,

    /// Target object ID.
    pub target_object_id: Uuid,

    /// User that created this edge.
    pub created_by: Option<Uuid>,

    /// User that revoked this edge.
    pub revoked_by: Option<Uuid>,

    /// Creation timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,

    /// Last update timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,

    /// Revocation timestamp.
    #[schemars(with = "Option<String>", extend("format" = "date-time"))]
    #[serde(with = "rfc3339::option")]
    pub revoked_at: Option<OffsetDateTime>,
}

/// Object edge response envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectEdgeResponse {
    /// Object edge resource.
    pub edge: ObjectEdge,
}

/// Request body for creating an object grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CreateObjectGrantRequest {
    /// Grant principal.
    pub principal: GrantPrincipal,

    /// Object role.
    pub object_role: ObjectRole,
}

/// Request body for updating an object grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UpdateObjectGrantRequest {
    /// New object role.
    pub object_role: ObjectRole,
}

/// Object grant resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectGrant {
    /// Object grant ID.
    pub id: Uuid,

    /// Workspace ID.
    pub workspace_id: Uuid,

    /// Object ID.
    pub object_id: Uuid,

    /// Principal user ID.
    pub principal_user_id: Option<Uuid>,

    /// Principal group ID.
    pub principal_group_id: Option<Uuid>,

    /// Object role.
    pub object_role: ObjectRole,

    /// User that created this grant.
    pub created_by: Option<Uuid>,

    /// User that revoked this grant.
    pub revoked_by: Option<Uuid>,

    /// Creation timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,

    /// Last update timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,

    /// Revocation timestamp.
    #[schemars(with = "Option<String>", extend("format" = "date-time"))]
    #[serde(with = "rfc3339::option")]
    pub revoked_at: Option<OffsetDateTime>,
}

/// Object grant response envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectGrantResponse {
    /// Object grant resource.
    pub grant: ObjectGrant,
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::{DEFAULT_LIMIT, MAX_LIMIT, ObjectBacklinksParams, ObjectBacklinksResponse};

    #[test]
    fn backlinks_response_serializes_empty_references() {
        let response = ObjectBacklinksResponse {
            object_id: Uuid::from_u128(1),
            incoming_edges: Vec::new(),
            next_edge_cursor: None,
            incoming_references: Vec::new(),
            next_reference_cursor: None,
        };

        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["incoming_references"], serde_json::json!([]));
    }

    #[test]
    fn backlinks_query_defaults_and_caps_limit() {
        assert_eq!(ObjectBacklinksParams::default().checked_limit(), Ok(DEFAULT_LIMIT));
        assert_eq!(
            ObjectBacklinksParams {
                limit: Some(MAX_LIMIT + 1),
                ..ObjectBacklinksParams::default()
            }
            .checked_limit(),
            Ok(MAX_LIMIT)
        );
        assert_eq!(
            ObjectBacklinksParams { limit: Some(0), ..ObjectBacklinksParams::default() }
                .checked_limit(),
            Err("limit must be at least 1")
        );
    }
}
