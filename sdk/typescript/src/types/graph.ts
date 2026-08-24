import type { ArchiveStatus, Timestamp, UUID } from "./common.js";

/**
 * Object permission role.
 *
 * Viewers may read and participate in commentary. Editors may also edit canonical object
 * content, and admins may also manage access.
 */
export type ObjectRole = "viewer" | "editor" | "admin";

/** Traversal direction for an object-centered graph. */
export type ObjectGraphDirection = "outgoing" | "incoming" | "both";

/**
 * Query parameters for object backlinks.
 *
 * Omit both cursors on the initial request to fetch both backlink streams. On continuation
 * requests, only streams with a supplied cursor are fetched; an omitted cursor means that stream
 * is exhausted or is not being continued.
 */
export type ObjectBacklinksParams = {
  /** Maximum explicit edges and textual references to return per section. */
  limit?: number | null;
  /** Opaque cursor for the next explicit-edge page. */
  edge_cursor?: string | null;
  /** Opaque cursor for the next textual-reference page. */
  reference_cursor?: string | null;
  /** Include archived source objects when the actor may read them. */
  include_archived?: boolean;
};

/** Query parameters for a bounded object-centered graph. */
export type ObjectGraphParams = {
  /** Maximum traversal depth from the root. */
  depth?: number | null;
  /** Traversal direction. Defaults to both directions. */
  direction?: ObjectGraphDirection;
  /** Maximum number of nodes to return. */
  max_nodes?: number | null;
  /** Maximum number of edges to return. */
  max_edges?: number | null;
  /** Include the root node in the returned node set. Defaults to true. */
  include_root?: boolean;
};

/** Node in an object-centered graph. */
export type ObjectGraphNode = {
  /** Object ID. */
  id: UUID;
  /** Workspace ID. */
  workspace_id: UUID;
  /** Current object-version ID. */
  current_version_id: UUID | null;
  /** Title projected from the current immutable version. */
  title: string;
  /** Object lifecycle status. */
  status: ArchiveStatus;
  /** User that created the object. */
  created_by: UUID | null;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
  /** Shortest traversal distance from the root. */
  distance: number;
  /** Visible filtered incoming relationship count. */
  incoming_count: number;
  /** Visible filtered outgoing relationship count. */
  outgoing_count: number;
};

/** Edge in an object-centered graph. */
export type ObjectGraphEdge = {
  /** Edge ID. */
  id: UUID;
  /** Workspace ID. */
  workspace_id: UUID;
  /** Source-object ID. */
  source_object_id: UUID;
  /** Target-object ID. */
  target_object_id: UUID;
  /** User that created the edge. */
  created_by: UUID | null;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
};

/** Structured object-graph truncation details. */
export type ObjectGraphTruncation = {
  /** Whether traversal hit the node cap. */
  nodes: boolean;
  /** Whether the returned edge set hit the edge cap. */
  edges: boolean;
};

/** Bounded authorized graph neighborhood around an object. */
export type ObjectGraphResponse = {
  /** Workspace ID. */
  workspace_id: UUID;
  /** Root-object ID. */
  root_object_id: UUID;
  /** Applied traversal depth. */
  depth: number;
  /** Applied traversal direction. */
  direction: ObjectGraphDirection;
  /** Applied maximum node count. */
  max_nodes: number;
  /** Applied maximum edge count. */
  max_edges: number;
  /** Whether any truncation occurred. */
  truncated: boolean;
  /** Structured truncation details. */
  truncation: ObjectGraphTruncation;
  /** Visible local graph nodes. */
  nodes: ObjectGraphNode[];
  /** Visible edges between returned nodes. */
  edges: ObjectGraphEdge[];
};

/** Query parameters for a bounded workspace graph projection. */
export type WorkspaceGraphParams = {
  /** Maximum number of nodes to return. */
  limit_nodes?: number | null;
  /** Maximum number of edges to return. */
  limit_edges?: number | null;
  /** Exclude nodes with no visible filtered relation. */
  exclude_isolated?: boolean;
};

/** Node in a workspace graph projection. */
export type WorkspaceGraphNode = {
  /** Object ID. */
  id: UUID;
  /** Workspace ID. */
  workspace_id: UUID;
  /** Current object-version ID. */
  current_version_id: UUID | null;
  /** Title projected from the current immutable version. */
  title: string;
  /** Object lifecycle status. */
  status: ArchiveStatus;
  /** User that created the object. */
  created_by: UUID | null;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
  /** Incoming edges in the visible filtered graph before edge-response truncation. */
  in_degree: number;
  /** Outgoing edges in the visible filtered graph before edge-response truncation. */
  out_degree: number;
};

/** Edge in a workspace graph projection. */
export type WorkspaceGraphEdge = {
  /** Edge ID. */
  id: UUID;
  /** Workspace ID. */
  workspace_id: UUID;
  /** Source-object ID. */
  source_object_id: UUID;
  /** Target-object ID. */
  target_object_id: UUID;
  /** User that created the edge. */
  created_by: UUID | null;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
};

/** Applied workspace-graph limits and truncation indicators. */
export type WorkspaceGraphLimits = {
  /** Applied node limit. */
  limit_nodes: number;
  /** Applied edge limit. */
  limit_edges: number;
  /** Whether more visible nodes matched. */
  has_more_nodes: boolean;
  /** Whether more visible edges matched between the returned nodes. */
  has_more_edges: boolean;
};

/** Bounded workspace graph projection. */
export type WorkspaceGraphResponse = {
  /** Workspace ID. */
  workspace_id: UUID;
  /** Visible graph nodes. */
  nodes: WorkspaceGraphNode[];
  /** Visible edges between returned nodes. */
  edges: WorkspaceGraphEdge[];
  /** Applied limits and truncation indicators. */
  limits: WorkspaceGraphLimits;
};

/** Source-object summary included with a backlink. */
export type BacklinkSourceObject = {
  /** Source-object ID. */
  id: UUID;
  /** Source-object title. */
  title: string;
  /** Source-object lifecycle status. */
  status: ArchiveStatus;
};

/** Explicit inbound edge in a backlinks response. */
export type ObjectBacklink = {
  /** Edge ID. */
  edge_id: UUID;
  /** Visible source-object summary. */
  source_object: BacklinkSourceObject;
  /** Requested target-object ID. */
  target_object_id: UUID;
  /** User that created the edge. */
  created_by: UUID | null;
  /** Edge creation timestamp. */
  created_at: Timestamp;
};

/** Derived textual reference pointing to an object. */
export type ObjectBacklinkReference = {
  /** Derived reference-row ID. */
  reference_id: UUID;
  /** Reference syntax kind. */
  reference_kind: string;
  /** Visible source-object summary. */
  source_object: BacklinkSourceObject;
  /** Source-object version containing the reference. */
  source_version_id: UUID;
  /** Resolved target-object ID. */
  target_object_id: UUID;
  /** Raw target text found in source content. */
  raw_target: string;
  /** Optional display text. */
  display_text?: string;
  /** Inclusive UTF-8 byte offset in the source body. */
  span_start: number;
  /** Exclusive UTF-8 byte offset in the source body. */
  span_end: number;
  /** Reference creation timestamp. */
  created_at: Timestamp;
};

/** Object-centric inbound references. */
export type ObjectBacklinksResponse = {
  /** Requested target-object ID. */
  object_id: UUID;
  /** Visible explicit inbound graph edges. */
  incoming_edges: ObjectBacklink[];
  /** Opaque cursor for the next explicit-edge page. */
  next_edge_cursor?: string;
  /** Visible resolved textual references from current source versions. */
  incoming_references: ObjectBacklinkReference[];
  /** Opaque cursor for the next textual-reference page. */
  next_reference_cursor?: string;
};

/** Object-edge resource. */
export type ObjectEdge = {
  /** Object-edge ID. */
  id: UUID;
  /** Workspace ID. */
  workspace_id: UUID;
  /** Source-object ID. */
  source_object_id: UUID;
  /** Target-object ID. */
  target_object_id: UUID;
  /** User that created this edge. */
  created_by: UUID | null;
  /** User that revoked this edge. */
  revoked_by: UUID | null;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
  /** Revocation timestamp. */
  revoked_at: Timestamp | null;
};

/** Object-edge response envelope. */
export type ObjectEdgeResponse = { edge: ObjectEdge };

/** Request body for creating an object edge. */
export type CreateObjectEdgeRequest = {
  /** Source-object ID. */
  source_object_id: UUID;
  /** Target-object ID. */
  target_object_id: UUID;
};

/** Object-grant resource. */
export type ObjectGrant = {
  /** Object-grant ID. */
  id: UUID;
  /** Workspace ID. */
  workspace_id: UUID;
  /** Object ID. */
  object_id: UUID;
  /** Principal user ID for a user grant. */
  principal_user_id: UUID | null;
  /** Principal group ID for a group grant. */
  principal_group_id: UUID | null;
  /** Object role. */
  object_role: ObjectRole;
  /** User that created this grant. */
  created_by: UUID | null;
  /** User that revoked this grant. */
  revoked_by: UUID | null;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
  /** Revocation timestamp. */
  revoked_at: Timestamp | null;
};

/** Object-grant principal. */
export type GrantPrincipal = { type: "user"; id: UUID } | { type: "group"; id: UUID };

/** Request body for creating an object grant. */
export type CreateObjectGrantRequest = {
  /** User or group receiving access. */
  principal: GrantPrincipal;
  /** Object role to grant. */
  object_role: ObjectRole;
};

/** Request body for updating an object grant. */
export type UpdateObjectGrantRequest = { object_role: ObjectRole };

/** Object-grant response envelope. */
export type ObjectGrantResponse = { grant: ObjectGrant };
