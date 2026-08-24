//! Object graph, backlink, and edge state bindings.

use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{ArchiveStatus, KernelError, ObjectGraphDirection, ObjectRole, Result, parse_stored};

/// Active explicit edge that points to an object.
#[derive(Debug, Clone)]
pub struct ObjectBacklinkRow {
    /// Explicit edge identifier.
    pub edge_id: Uuid,
    /// Source object identifier.
    pub source_object_id: Uuid,
    /// Current source-object title.
    pub source_title: String,
    /// Current source-object lifecycle status.
    pub source_status: ArchiveStatus,
    /// Target object identifier.
    pub target_object_id: Uuid,
    /// User that created the row, when retained.
    pub created_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: OffsetDateTime,
}

/// Resolved textual reference that points to an object.
#[derive(Debug, Clone)]
pub struct ObjectBacklinkReferenceRow {
    /// Object-reference identifier.
    pub reference_id: Uuid,
    /// Reference kind stored by the parser.
    pub reference_kind: String,
    /// Source object identifier.
    pub source_object_id: Uuid,
    /// Current source-object title.
    pub source_title: String,
    /// Current source-object lifecycle status.
    pub source_status: ArchiveStatus,
    /// Source object-version identifier.
    pub source_version_id: Uuid,
    /// Target object identifier.
    pub target_object_id: Uuid,
    /// Raw reference target captured from authored text.
    pub raw_target: String,
    /// Optional display text associated with the reference.
    pub display_text: Option<String>,
    /// UTF-8 byte offset where the reference starts.
    pub span_start: i32,
    /// UTF-8 byte offset where the reference ends.
    pub span_end: i32,
    /// Creation timestamp.
    pub created_at: OffsetDateTime,
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredObjectBacklinkRow {
    /// Stored edge identifier.
    edge_id: Uuid,
    /// Stored source-object identifier.
    source_object_id: Uuid,
    /// Stored `source_title` projection value.
    source_title: String,
    /// Stored source-object lifecycle status before typed parsing.
    source_status: String,
    /// Stored target-object identifier.
    target_object_id: Uuid,
    /// Stored creator identifier, when retained.
    created_by: Option<Uuid>,
    /// Stored creation timestamp.
    created_at: OffsetDateTime,
}

impl TryFrom<StoredObjectBacklinkRow> for ObjectBacklinkRow {
    type Error = KernelError;
    fn try_from(row: StoredObjectBacklinkRow) -> Result<Self> {
        Ok(Self {
            edge_id: row.edge_id,
            source_object_id: row.source_object_id,
            source_title: row.source_title,
            source_status: parse_stored("object status", row.source_status)?,
            target_object_id: row.target_object_id,
            created_by: row.created_by,
            created_at: row.created_at,
        })
    }
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredObjectBacklinkReferenceRow {
    /// Stored object-reference identifier.
    reference_id: Uuid,
    /// Stored reference kind.
    reference_kind: String,
    /// Stored source-object identifier.
    source_object_id: Uuid,
    /// Stored `source_title` projection value.
    source_title: String,
    /// Stored source-object lifecycle status before typed parsing.
    source_status: String,
    /// Stored source-version identifier.
    source_version_id: Uuid,
    /// Stored target-object identifier.
    target_object_id: Uuid,
    /// Stored raw reference target.
    raw_target: String,
    /// Stored optional reference display text.
    display_text: Option<String>,
    /// Stored reference start offset.
    span_start: i32,
    /// Stored reference end offset.
    span_end: i32,
    /// Stored creation timestamp.
    created_at: OffsetDateTime,
}

impl TryFrom<StoredObjectBacklinkReferenceRow> for ObjectBacklinkReferenceRow {
    type Error = KernelError;
    fn try_from(row: StoredObjectBacklinkReferenceRow) -> Result<Self> {
        Ok(Self {
            reference_id: row.reference_id,
            reference_kind: row.reference_kind,
            source_object_id: row.source_object_id,
            source_title: row.source_title,
            source_status: parse_stored("object status", row.source_status)?,
            source_version_id: row.source_version_id,
            target_object_id: row.target_object_id,
            raw_target: row.raw_target,
            display_text: row.display_text,
            span_start: row.span_start,
            span_end: row.span_end,
            created_at: row.created_at,
        })
    }
}

/// Stored explicit object-edge projection.
#[derive(Debug, Clone, Copy, sqlx::FromRow)]
pub struct ObjectEdgeRow {
    /// Row identifier.
    pub id: Uuid,
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// Source object identifier.
    pub source_object_id: Uuid,
    /// Target object identifier.
    pub target_object_id: Uuid,
    /// User that created the row, when retained.
    pub created_by: Option<Uuid>,
    /// User that revoked the row, when retained.
    pub revoked_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: OffsetDateTime,
    /// Last update timestamp.
    pub updated_at: OffsetDateTime,
    /// Revocation timestamp, when revoked.
    pub revoked_at: Option<OffsetDateTime>,
}

/// Object-graph node with distance and degree projections.
#[derive(Debug, Clone)]
pub struct ObjectGraphNodeRow {
    /// Row identifier.
    pub id: Uuid,
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// Current object-version identifier, when initialized.
    pub current_version_id: Option<Uuid>,
    /// Object title.
    pub title: String,
    /// Lifecycle status.
    pub status: ArchiveStatus,
    /// User that created the row, when retained.
    pub created_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: OffsetDateTime,
    /// Last update timestamp.
    pub updated_at: OffsetDateTime,
    /// Graph distance from the traversal root.
    pub distance: i32,
    /// Number of visible incoming edges.
    pub in_degree: i64,
    /// Number of visible outgoing edges.
    pub out_degree: i64,
}

/// Workspace graph node with degree projections.
#[derive(Debug, Clone)]
pub struct WorkspaceGraphNodeRow {
    /// Row identifier.
    pub id: Uuid,
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// Current object-version identifier, when initialized.
    pub current_version_id: Option<Uuid>,
    /// Object title.
    pub title: String,
    /// Lifecycle status.
    pub status: ArchiveStatus,
    /// User that created the row, when retained.
    pub created_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: OffsetDateTime,
    /// Last update timestamp.
    pub updated_at: OffsetDateTime,
    /// Number of visible incoming edges.
    pub in_degree: i64,
    /// Number of visible outgoing edges.
    pub out_degree: i64,
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredObjectGraphNodeRow {
    /// Stored row identifier.
    id: Uuid,
    /// Stored workspace identifier.
    workspace_id: Uuid,
    /// Stored current-version identifier.
    current_version_id: Option<Uuid>,
    /// Stored object title.
    title: String,
    /// Stored lifecycle status before typed parsing.
    status: String,
    /// Stored creator identifier, when retained.
    created_by: Option<Uuid>,
    /// Stored creation timestamp.
    created_at: OffsetDateTime,
    /// Stored update timestamp.
    updated_at: OffsetDateTime,
    /// Stored graph distance projection.
    distance: i32,
    /// Stored incoming-edge count.
    in_degree: i64,
    /// Stored outgoing-edge count.
    out_degree: i64,
}

impl TryFrom<StoredObjectGraphNodeRow> for ObjectGraphNodeRow {
    type Error = KernelError;
    fn try_from(row: StoredObjectGraphNodeRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            workspace_id: row.workspace_id,
            current_version_id: row.current_version_id,
            title: row.title,
            status: parse_stored("object status", row.status)?,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            distance: row.distance,
            in_degree: row.in_degree,
            out_degree: row.out_degree,
        })
    }
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredWorkspaceGraphNodeRow {
    /// Stored row identifier.
    id: Uuid,
    /// Stored workspace identifier.
    workspace_id: Uuid,
    /// Stored current-version identifier.
    current_version_id: Option<Uuid>,
    /// Stored object title.
    title: String,
    /// Stored lifecycle status before typed parsing.
    status: String,
    /// Stored creator identifier, when retained.
    created_by: Option<Uuid>,
    /// Stored creation timestamp.
    created_at: OffsetDateTime,
    /// Stored update timestamp.
    updated_at: OffsetDateTime,
    /// Stored incoming-edge count.
    in_degree: i64,
    /// Stored outgoing-edge count.
    out_degree: i64,
}

impl TryFrom<StoredWorkspaceGraphNodeRow> for WorkspaceGraphNodeRow {
    type Error = KernelError;
    fn try_from(row: StoredWorkspaceGraphNodeRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            workspace_id: row.workspace_id,
            current_version_id: row.current_version_id,
            title: row.title,
            status: parse_stored("object status", row.status)?,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            in_degree: row.in_degree,
            out_degree: row.out_degree,
        })
    }
}

/// Deduplicated directed connection returned as part of a graph projection.
#[derive(Debug, Clone, Copy, sqlx::FromRow)]
pub struct WorkspaceGraphEdgeRow {
    /// Row identifier.
    pub id: Uuid,
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// Source object identifier.
    pub source_object_id: Uuid,
    /// Target object identifier.
    pub target_object_id: Uuid,
    /// Whether an active explicit relationship contributes this connection.
    pub has_relationship: bool,
    /// Whether a resolved current-version wikilink contributes this connection.
    pub has_wikilink: bool,
    /// User that created the representative relationship or source version, when retained.
    pub created_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: OffsetDateTime,
    /// Last update timestamp.
    pub updated_at: OffsetDateTime,
}

/// Parameters for listing inbound links to an object.
#[derive(Debug, Clone, Copy)]
pub struct ListObjectBacklinks {
    /// Workspace containing the target object.
    pub workspace_id: Uuid,
    /// Target object identifier.
    pub object_id: Uuid,
    /// Whether archived source objects may be returned.
    pub include_archived: bool,
    /// Creation timestamp from the pagination cursor, when present.
    pub cursor_created_at: Option<OffsetDateTime>,
    /// Row identifier from the pagination cursor, when present.
    pub cursor_id: Option<Uuid>,
    /// User whose object permissions constrain the result.
    pub user_id: Uuid,
    /// Whether this result class should be fetched for the current page.
    pub fetch: bool,
    /// Maximum number of rows to return.
    pub limit: i64,
}

/// Parameters for listing explicit edges incident to an object.
#[derive(Debug, Clone, Copy)]
pub struct ListObjectEdges {
    /// Workspace containing the object.
    pub workspace_id: Uuid,
    /// Object whose incident edges are listed.
    pub object_id: Uuid,
    /// Creation timestamp from the pagination cursor, when present.
    pub cursor_created_at: Option<OffsetDateTime>,
    /// Edge identifier from the pagination cursor, when present.
    pub cursor_id: Option<Uuid>,
    /// User whose object permissions constrain the result.
    pub user_id: Uuid,
    /// Maximum number of rows to return.
    pub limit: i64,
}

/// Lists explicit active edges that point to an object.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn list_object_backlink_edges(
    pool: &PgPool,
    query: ListObjectBacklinks,
) -> Result<Vec<ObjectBacklinkRow>> {
    sqlx::query_as::<_, StoredObjectBacklinkRow>(
        r#"
        SELECT oe.id AS edge_id, source_object.id AS source_object_id,
               source_current_version.title AS source_title, source_object.status AS source_status,
               oe.target_object_id, oe.created_by, oe.created_at
        FROM kival.object_edges oe
        JOIN kival.objects source_object
            ON source_object.workspace_id = oe.workspace_id
            AND source_object.id = oe.source_object_id
        JOIN kival.object_versions source_current_version
            ON source_current_version.object_id = source_object.id
            AND source_current_version.id = source_object.current_version_id
        WHERE oe.workspace_id = $1
            AND oe.target_object_id = $2
            AND oe.revoked_at IS NULL
            AND (source_object.archived_at IS NULL OR $3)
            AND ($4::timestamptz IS NULL OR (oe.created_at, oe.id) < ($4, $5))
            AND kival.has_object_permission(
                source_object.workspace_id,
                source_object.id,
                $6,
                CASE
                    WHEN source_object.archived_at IS NULL THEN 'viewer'::kival.object_role
                    ELSE 'admin'::kival.object_role
                END
            )
            AND $7::bool
        ORDER BY oe.created_at DESC, oe.id DESC
        LIMIT $8
        OFFSET CASE WHEN kival.require_read_object($1, $2, $6) THEN 0 ELSE 0 END
        "#,
    )
    .bind(query.workspace_id)
    .bind(query.object_id)
    .bind(query.include_archived)
    .bind(query.cursor_created_at)
    .bind(query.cursor_id)
    .bind(query.user_id)
    .bind(query.fetch)
    .bind(query.limit)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(TryInto::try_into)
    .collect::<Result<Vec<_>>>()
}

/// Lists resolved textual references that point to an object.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn list_object_backlink_references(
    pool: &PgPool,
    query: ListObjectBacklinks,
) -> Result<Vec<ObjectBacklinkReferenceRow>> {
    sqlx::query_as::<_, StoredObjectBacklinkReferenceRow>(
        r#"
        SELECT
            object_reference.id AS reference_id,
            object_reference.reference_kind,
            source_object.id AS source_object_id,
            source_version.title AS source_title,
            source_object.status AS source_status,
            object_reference.source_version_id,
            object_reference.target_object_id,
            object_reference.raw_target,
            object_reference.display_text,
            object_reference.span_start,
            object_reference.span_end,
            object_reference.created_at
        FROM kival.object_references object_reference
        JOIN kival.objects source_object
            ON source_object.workspace_id = object_reference.workspace_id
            AND source_object.id = object_reference.source_object_id
            AND source_object.current_version_id = object_reference.source_version_id
        JOIN kival.object_versions source_version
            ON source_version.object_id = source_object.id
            AND source_version.id = object_reference.source_version_id
        WHERE object_reference.workspace_id = $1
            AND object_reference.target_object_id = $2
            AND object_reference.status = 'resolved'
            AND (source_object.archived_at IS NULL OR $3)
            AND (
                $4::timestamptz IS NULL
                OR (object_reference.created_at, object_reference.id) < ($4, $5)
            )
            AND kival.has_object_permission(
                source_object.workspace_id,
                source_object.id,
                $6,
                CASE
                    WHEN source_object.archived_at IS NULL THEN 'viewer'::kival.object_role
                    ELSE 'admin'::kival.object_role
                END
            )
            AND $7::bool
        ORDER BY object_reference.created_at DESC, object_reference.id DESC
        LIMIT $8
        OFFSET CASE WHEN kival.require_read_object($1, $2, $6) THEN 0 ELSE 0 END
        "#,
    )
    .bind(query.workspace_id)
    .bind(query.object_id)
    .bind(query.include_archived)
    .bind(query.cursor_created_at)
    .bind(query.cursor_id)
    .bind(query.user_id)
    .bind(query.fetch)
    .bind(query.limit)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(TryInto::try_into)
    .collect::<Result<Vec<_>>>()
}

/// Lists active explicit edges incident to an object.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn list_object_edges(
    pool: &PgPool,
    query: ListObjectEdges,
) -> Result<Vec<ObjectEdgeRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT oe.id, oe.workspace_id, oe.source_object_id, oe.target_object_id, oe.created_by,
               oe.revoked_by, oe.created_at, oe.updated_at, oe.revoked_at
        FROM kival.object_edges oe
        JOIN kival.objects source_object
            ON source_object.workspace_id = oe.workspace_id
            AND source_object.id = oe.source_object_id
            AND source_object.archived_at IS NULL
        JOIN kival.objects target_object
            ON target_object.workspace_id = oe.workspace_id
            AND target_object.id = oe.target_object_id
            AND target_object.archived_at IS NULL
        WHERE oe.workspace_id = $1
            AND (oe.source_object_id = $2 OR oe.target_object_id = $2)
            AND oe.revoked_at IS NULL
            AND ($3::timestamptz IS NULL OR (oe.created_at, oe.id) < ($3, $4))
            AND kival.has_object_permission(
                oe.workspace_id,
                oe.source_object_id,
                $5,
                'viewer'::kival.object_role
            )
            AND kival.has_object_permission(
                oe.workspace_id,
                oe.target_object_id,
                $5,
                'viewer'::kival.object_role
            )
        ORDER BY oe.created_at DESC, oe.id DESC
        LIMIT $6
        OFFSET CASE
            WHEN kival.require_access_active_object(
                $1,
                $2,
                $5,
                'viewer'::kival.object_role
            )
            THEN 0
            ELSE 0
        END
        "#,
    )
    .bind(query.workspace_id)
    .bind(query.object_id)
    .bind(query.cursor_created_at)
    .bind(query.cursor_id)
    .bind(query.user_id)
    .bind(query.limit)
    .fetch_all(pool)
    .await?)
}

/// Creates an explicit directed edge between two objects.
///
/// The transition pins the workspace and both endpoint lifecycles in deterministic object-ID
/// order before publishing the edge.
///
/// # Errors
///
/// Returns an error if either endpoint is inactive or the underlying `PostgreSQL` operation fails.
pub async fn create_object_edge(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    source_object_id: Uuid,
    target_object_id: Uuid,
    created_by: Uuid,
) -> Result<ObjectEdgeRow> {
    if !lock_active_object_edge_endpoints(tx, workspace_id, source_object_id, target_object_id)
        .await?
    {
        return Err(sqlx::Error::RowNotFound.into());
    }

    Ok(sqlx::query_as(
        r#"
        INSERT INTO kival.object_edges (
            workspace_id, source_object_id, target_object_id, created_by
        )
        VALUES ($1, $2, $3, $4)
        RETURNING id, workspace_id, source_object_id, target_object_id, created_by,
                  revoked_by, created_at, updated_at, revoked_at
        "#,
    )
    .bind(workspace_id)
    .bind(source_object_id)
    .bind(target_object_id)
    .bind(created_by)
    .fetch_one(&mut **tx)
    .await?)
}

/// Revokes an explicit object edge.
///
/// The transition reloads the edge inside the mutation transaction and pins both endpoint
/// lifecycles in the same deterministic order used by edge creation.
///
/// # Errors
///
/// Returns an error if the edge/endpoints are inactive or the underlying `PostgreSQL` operation
/// fails.
pub async fn revoke_object_edge(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    edge_id: Uuid,
    revoked_by: Uuid,
) -> Result<ObjectEdgeRow> {
    let edge = fetch_object_edge_for_transition(tx, workspace_id, edge_id).await?;
    if !lock_active_object_edge_endpoints(
        tx,
        workspace_id,
        edge.source_object_id,
        edge.target_object_id,
    )
    .await?
    {
        return Err(sqlx::Error::RowNotFound.into());
    }

    Ok(sqlx::query_as(
        r#"
        UPDATE kival.object_edges
        SET revoked_at = now(),
            revoked_by = $3
        WHERE workspace_id = $1
            AND id = $2
            AND revoked_at IS NULL
        RETURNING id, workspace_id, source_object_id, target_object_id, created_by,
                  revoked_by, created_at, updated_at, revoked_at
        "#,
    )
    .bind(workspace_id)
    .bind(edge_id)
    .bind(revoked_by)
    .fetch_one(&mut **tx)
    .await?)
}

/// Pins both active edge endpoints in the kernel's canonical object-reference order.
async fn lock_active_object_edge_endpoints(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    source_object_id: Uuid,
    target_object_id: Uuid,
) -> Result<bool> {
    crate::objects::lock_active_objects_for_reference(
        tx,
        workspace_id,
        &[source_object_id, target_object_id],
    )
    .await
}

/// Loads one active edge while a kernel transition is already in progress.
async fn fetch_object_edge_for_transition(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    edge_id: Uuid,
) -> Result<ObjectEdgeRow> {
    Ok(sqlx::query_as(
        r#"
        SELECT id, workspace_id, source_object_id, target_object_id, created_by,
               revoked_by, created_at, updated_at, revoked_at
        FROM kival.object_edges
        WHERE workspace_id = $1
            AND id = $2
            AND revoked_at IS NULL
        "#,
    )
    .bind(workspace_id)
    .bind(edge_id)
    .fetch_one(&mut **tx)
    .await?)
}

/// Loads one active explicit edge while enforcing endpoint roles in the admission statement.
async fn fetch_object_edge_with_roles(
    pool: &PgPool,
    workspace_id: Uuid,
    edge_id: Uuid,
    user_id: Uuid,
    source_role: ObjectRole,
    target_role: ObjectRole,
) -> Result<ObjectEdgeRow> {
    Ok(sqlx::query_as(
        r#"
        WITH edge AS MATERIALIZED (
            SELECT id, workspace_id, source_object_id, target_object_id, created_by,
                   revoked_by, created_at, updated_at, revoked_at
            FROM kival.object_edges
            WHERE workspace_id = $1
                AND id = $2
                AND revoked_at IS NULL
        )
        SELECT id, workspace_id, source_object_id, target_object_id, created_by,
               revoked_by, created_at, updated_at, revoked_at
        FROM edge
        OFFSET CASE
            WHEN kival.require_capability(EXISTS (SELECT 1 FROM edge), TRUE)
            THEN CASE
                WHEN kival.require_access_active_object(
                    $1,
                    (SELECT source_object_id FROM edge),
                    $3,
                    $4::text::kival.object_role
                )
                THEN CASE
                    WHEN kival.require_access_active_object(
                        $1,
                        (SELECT target_object_id FROM edge),
                        $3,
                        $5::text::kival.object_role
                    )
                    THEN 0
                    ELSE 0
                END
                ELSE 0
            END
            ELSE 0
        END
        "#,
    )
    .bind(workspace_id)
    .bind(edge_id)
    .bind(user_id)
    .bind(source_role.as_str())
    .bind(target_role.as_str())
    .fetch_one(pool)
    .await?)
}

/// Loads one visible explicit object edge whose endpoints are active.
///
/// Authorization preserves source-then-target error ordering: the edge must exist, then the source
/// must be readable, then the target must be readable. All predicates and the returned edge use one
/// `PostgreSQL` statement snapshot.
///
/// # Errors
///
/// Returns an error if the edge is missing, either endpoint is not an active readable object, or
/// the underlying `PostgreSQL` operation fails.
pub async fn fetch_object_edge(
    pool: &PgPool,
    workspace_id: Uuid,
    edge_id: Uuid,
    user_id: Uuid,
) -> Result<ObjectEdgeRow> {
    fetch_object_edge_with_roles(
        pool,
        workspace_id,
        edge_id,
        user_id,
        ObjectRole::Viewer,
        ObjectRole::Viewer,
    )
    .await
}

/// Loads one active explicit edge while admitting a revoke request.
///
/// Edge identity, source-editor access and target-viewer access are evaluated in one `PostgreSQL`
/// statement snapshot. No unprotected edge lookup precedes this admission projection.
///
/// # Errors
///
/// Returns an error if the edge is missing, the actor lacks the required endpoint role, or the
/// underlying `PostgreSQL` operation fails.
pub async fn fetch_object_edge_for_revoke(
    pool: &PgPool,
    workspace_id: Uuid,
    edge_id: Uuid,
    user_id: Uuid,
) -> Result<ObjectEdgeRow> {
    fetch_object_edge_with_roles(
        pool,
        workspace_id,
        edge_id,
        user_id,
        ObjectRole::Editor,
        ObjectRole::Viewer,
    )
    .await
}

/// Parameters for traversing the graph around one object.
#[derive(Debug, Clone, Copy)]
pub struct ObjectGraphQuery {
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// User identifier.
    pub user_id: Uuid,
    /// Root object identifier for the traversal.
    pub root_object_id: Uuid,
    /// Maximum traversal depth.
    pub depth: i32,
    /// Traversal direction selector.
    pub direction: ObjectGraphDirection,
    /// Whether to include the root object in results.
    pub include_root: bool,
    /// Maximum number of rows to return.
    pub limit: i64,
}

/// Traverses visible graph nodes around one root object.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn object_graph_nodes(
    pool: &PgPool,
    input: ObjectGraphQuery,
) -> Result<Vec<ObjectGraphNodeRow>> {
    sqlx::query_as::<_, StoredObjectGraphNodeRow>(
        r#"
        WITH RECURSIVE visible_nodes AS MATERIALIZED (
            SELECT o.id, o.workspace_id, o.current_version_id, current_version.title, o.status,
                   o.created_by, o.created_at, o.updated_at
            FROM kival.objects o
            JOIN kival.object_versions current_version
                ON current_version.object_id = o.id
                AND current_version.id = o.current_version_id
            WHERE o.workspace_id = $1
                AND o.archived_at IS NULL
                AND kival.has_object_permission(
                    o.workspace_id,
                    o.id,
                    $2,
                    'viewer'::kival.object_role
                )
        ),
        visible_connections AS MATERIALIZED (
            SELECT edge.source_object_id, edge.target_object_id
            FROM kival.object_edges edge
            JOIN visible_nodes source_node
                ON source_node.id = edge.source_object_id
            JOIN visible_nodes target_node
                ON target_node.id = edge.target_object_id
            WHERE edge.workspace_id = $1
                AND edge.revoked_at IS NULL
            UNION
            SELECT reference.source_object_id, reference.target_object_id
            FROM kival.object_references reference
            JOIN visible_nodes source_node
                ON source_node.id = reference.source_object_id
                AND source_node.current_version_id = reference.source_version_id
            JOIN visible_nodes target_node
                ON target_node.id = reference.target_object_id
            WHERE reference.workspace_id = $1
                AND reference.reference_kind = 'wikilink'
                AND reference.status = 'resolved'
                AND reference.source_object_id <> reference.target_object_id
        ),
        walk(object_id, distance) AS (
            SELECT $3::uuid, 0::integer
            UNION
            SELECT CASE
                WHEN edge.source_object_id = walk.object_id THEN edge.target_object_id
                ELSE edge.source_object_id
            END,
                walk.distance + 1
            FROM walk
            JOIN visible_connections edge
                ON (
                ($5::text IN ('outgoing', 'both') AND edge.source_object_id = walk.object_id)
                OR ($5::text IN ('incoming', 'both') AND edge.target_object_id = walk.object_id))
            WHERE walk.distance < $4
        ),
        reached AS MATERIALIZED (
            SELECT walk.object_id, MIN(walk.distance)::integer AS distance
            FROM walk
            GROUP BY walk.object_id
        ),
        degrees AS (
            SELECT endpoints.object_id,
                   SUM(endpoints.in_degree)::bigint AS in_degree,
                   SUM(endpoints.out_degree)::bigint AS out_degree
            FROM (
                SELECT
                    target_object_id AS object_id,
                    1::bigint AS in_degree,
                    0::bigint AS out_degree
                FROM visible_connections
                UNION ALL
                SELECT
                    source_object_id AS object_id,
                    0::bigint AS in_degree,
                    1::bigint AS out_degree
                FROM visible_connections
            ) endpoints
            GROUP BY endpoints.object_id
        )
        SELECT node.id, node.workspace_id, node.current_version_id, node.title, node.status,
               node.created_by, node.created_at, node.updated_at, reached.distance,
               COALESCE(degrees.in_degree, 0)::bigint AS in_degree,
               COALESCE(degrees.out_degree, 0)::bigint AS out_degree
        FROM reached
        JOIN visible_nodes node
            ON node.id = reached.object_id
        LEFT JOIN degrees
            ON degrees.object_id = node.id
        WHERE $6
            OR node.id <> $3
        ORDER BY
            CASE WHEN node.id = $3 THEN 0 ELSE 1 END,
            reached.distance ASC,
            node.title ASC,
            node.id ASC
        LIMIT $7
        OFFSET CASE
            WHEN kival.require_access_active_object(
                $1,
                $3,
                $2,
                'viewer'::kival.object_role
            )
            THEN 0
            ELSE 0
        END
        "#,
    )
    .bind(input.workspace_id)
    .bind(input.user_id)
    .bind(input.root_object_id)
    .bind(input.depth)
    .bind(input.direction.as_str())
    .bind(input.include_root)
    .bind(input.limit)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(TryInto::try_into)
    .collect::<Result<Vec<_>>>()
}

/// Loads currently visible active edges for an object-graph node set.
///
/// The root authorization is re-checked in this statement because object-graph responses load
/// nodes and edges separately.
///
/// # Errors
///
/// Returns an error if the root object is no longer readable or the underlying `PostgreSQL`
/// operation fails.
pub async fn object_graph_edges_for_nodes(
    pool: &PgPool,
    workspace_id: Uuid,
    root_object_id: Uuid,
    user_id: Uuid,
    node_ids: &[Uuid],
    limit: i64,
) -> Result<Vec<WorkspaceGraphEdgeRow>> {
    Ok(sqlx::query_as(
        r#"
        WITH visible_nodes AS MATERIALIZED (
            SELECT o.id, o.current_version_id
            FROM kival.objects o
            WHERE o.workspace_id = $1
                AND o.archived_at IS NULL
                AND kival.has_object_permission(
                    o.workspace_id,
                    o.id,
                    $3,
                    'viewer'::kival.object_role
                )
        ),
        connections AS MATERIALIZED (
            SELECT
                edge.id, edge.workspace_id, edge.source_object_id, edge.target_object_id,
                TRUE AS has_relationship, FALSE AS has_wikilink, edge.created_by,
                edge.created_at, edge.updated_at
            FROM kival.object_edges edge
            JOIN visible_nodes source_node
                ON source_node.id = edge.source_object_id
            JOIN visible_nodes target_node
                ON target_node.id = edge.target_object_id
            WHERE edge.workspace_id = $1
                AND edge.revoked_at IS NULL
            UNION ALL
            SELECT
                reference.id, reference.workspace_id, reference.source_object_id,
                reference.target_object_id, FALSE AS has_relationship, TRUE AS has_wikilink,
                source_version.created_by, reference.created_at, reference.updated_at
            FROM kival.object_references reference
            JOIN visible_nodes source_node
                ON source_node.id = reference.source_object_id
                AND source_node.current_version_id = reference.source_version_id
            JOIN visible_nodes target_node
                ON target_node.id = reference.target_object_id
            JOIN kival.object_versions source_version
                ON source_version.object_id = reference.source_object_id
                AND source_version.id = reference.source_version_id
            WHERE reference.workspace_id = $1
                AND reference.reference_kind = 'wikilink'
                AND reference.status = 'resolved'
                AND reference.source_object_id <> reference.target_object_id
        ),
        ranked AS (
            SELECT
                connection.*,
                BOOL_OR(connection.has_relationship) OVER connection_pair AS pair_has_relationship,
                BOOL_OR(connection.has_wikilink) OVER connection_pair AS pair_has_wikilink,
                ROW_NUMBER() OVER (
                    PARTITION BY connection.source_object_id, connection.target_object_id
                    ORDER BY
                        connection.has_relationship DESC,
                        connection.created_at DESC,
                        connection.id DESC
                ) AS row_number
            FROM connections connection
            WINDOW connection_pair AS (
                PARTITION BY connection.source_object_id, connection.target_object_id
            )
        )
        SELECT
            ranked.id, ranked.workspace_id, ranked.source_object_id, ranked.target_object_id,
            ranked.pair_has_relationship AS has_relationship,
            ranked.pair_has_wikilink AS has_wikilink, ranked.created_by, ranked.created_at,
            ranked.updated_at
        FROM ranked
        WHERE ranked.row_number = 1
            AND ranked.source_object_id = ANY($4)
            AND ranked.target_object_id = ANY($4)
        ORDER BY ranked.source_object_id ASC, ranked.target_object_id ASC, ranked.id ASC
        LIMIT $5
        OFFSET CASE
            WHEN kival.require_access_active_object(
                $1,
                $2,
                $3,
                'viewer'::kival.object_role
            )
            THEN 0
            ELSE 0
        END
        "#,
    )
    .bind(workspace_id)
    .bind(root_object_id)
    .bind(user_id)
    .bind(node_ids)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// Loads currently visible active edges for a workspace-graph node set.
///
/// Workspace authorization is re-checked in this statement because workspace-graph responses
/// load nodes and edges separately.
///
/// # Errors
///
/// Returns an error if the workspace is no longer readable or the underlying `PostgreSQL`
/// operation fails.
pub async fn workspace_graph_edges_for_nodes(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    node_ids: &[Uuid],
    limit: i64,
) -> Result<Vec<WorkspaceGraphEdgeRow>> {
    Ok(sqlx::query_as(
        r#"
        WITH visible_nodes AS MATERIALIZED (
            SELECT o.id, o.current_version_id
            FROM kival.objects o
            WHERE o.workspace_id = $1
                AND o.archived_at IS NULL
                AND kival.has_object_permission(
                    o.workspace_id,
                    o.id,
                    $2,
                    'viewer'::kival.object_role
                )
        ),
        connections AS MATERIALIZED (
            SELECT
                edge.id, edge.workspace_id, edge.source_object_id, edge.target_object_id,
                TRUE AS has_relationship, FALSE AS has_wikilink, edge.created_by,
                edge.created_at, edge.updated_at
            FROM kival.object_edges edge
            JOIN visible_nodes source_node
                ON source_node.id = edge.source_object_id
            JOIN visible_nodes target_node
                ON target_node.id = edge.target_object_id
            WHERE edge.workspace_id = $1
                AND edge.revoked_at IS NULL
            UNION ALL
            SELECT
                reference.id, reference.workspace_id, reference.source_object_id,
                reference.target_object_id, FALSE AS has_relationship, TRUE AS has_wikilink,
                source_version.created_by, reference.created_at, reference.updated_at
            FROM kival.object_references reference
            JOIN visible_nodes source_node
                ON source_node.id = reference.source_object_id
                AND source_node.current_version_id = reference.source_version_id
            JOIN visible_nodes target_node
                ON target_node.id = reference.target_object_id
            JOIN kival.object_versions source_version
                ON source_version.object_id = reference.source_object_id
                AND source_version.id = reference.source_version_id
            WHERE reference.workspace_id = $1
                AND reference.reference_kind = 'wikilink'
                AND reference.status = 'resolved'
                AND reference.source_object_id <> reference.target_object_id
        ),
        ranked AS (
            SELECT
                connection.*,
                BOOL_OR(connection.has_relationship) OVER connection_pair AS pair_has_relationship,
                BOOL_OR(connection.has_wikilink) OVER connection_pair AS pair_has_wikilink,
                ROW_NUMBER() OVER (
                    PARTITION BY connection.source_object_id, connection.target_object_id
                    ORDER BY
                        connection.has_relationship DESC,
                        connection.created_at DESC,
                        connection.id DESC
                ) AS row_number
            FROM connections connection
            WINDOW connection_pair AS (
                PARTITION BY connection.source_object_id, connection.target_object_id
            )
        )
        SELECT
            ranked.id, ranked.workspace_id, ranked.source_object_id, ranked.target_object_id,
            ranked.pair_has_relationship AS has_relationship,
            ranked.pair_has_wikilink AS has_wikilink, ranked.created_by, ranked.created_at,
            ranked.updated_at
        FROM ranked
        WHERE ranked.row_number = 1
            AND ranked.source_object_id = ANY($3)
            AND ranked.target_object_id = ANY($3)
        ORDER BY ranked.created_at DESC, ranked.id DESC
        LIMIT $4
        OFFSET CASE WHEN kival.require_read_workspace($1, $2) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(node_ids)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// Lists visible graph nodes for a workspace.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn workspace_graph_nodes(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    exclude_isolated: bool,
    limit: i64,
) -> Result<Vec<WorkspaceGraphNodeRow>> {
    sqlx::query_as::<_, StoredWorkspaceGraphNodeRow>(
        r#"
        WITH visible_nodes AS MATERIALIZED (
            SELECT o.id, o.workspace_id, o.current_version_id, current_version.title, o.status,
                   o.created_by, o.created_at, o.updated_at
            FROM kival.objects o
            JOIN kival.object_versions current_version
                ON current_version.object_id = o.id
                AND current_version.id = o.current_version_id
            WHERE o.workspace_id = $1
                AND o.archived_at IS NULL
                AND kival.has_object_permission(
                    o.workspace_id,
                    o.id,
                    $2,
                    'viewer'::kival.object_role
                )
        ),
        visible_connections AS MATERIALIZED (
            SELECT edge.source_object_id, edge.target_object_id
            FROM kival.object_edges edge
            JOIN visible_nodes source_node
                ON source_node.id = edge.source_object_id
            JOIN visible_nodes target_node
                ON target_node.id = edge.target_object_id
            WHERE edge.workspace_id = $1
                AND edge.revoked_at IS NULL
            UNION
            SELECT reference.source_object_id, reference.target_object_id
            FROM kival.object_references reference
            JOIN visible_nodes source_node
                ON source_node.id = reference.source_object_id
                AND source_node.current_version_id = reference.source_version_id
            JOIN visible_nodes target_node
                ON target_node.id = reference.target_object_id
            WHERE reference.workspace_id = $1
                AND reference.reference_kind = 'wikilink'
                AND reference.status = 'resolved'
                AND reference.source_object_id <> reference.target_object_id
        ),
        degrees AS (
            SELECT endpoints.object_id,
                   SUM(endpoints.in_degree)::bigint AS in_degree,
                   SUM(endpoints.out_degree)::bigint AS out_degree
            FROM (
                SELECT
                    target_object_id AS object_id,
                    1::bigint AS in_degree,
                    0::bigint AS out_degree
                FROM visible_connections
                UNION ALL
                SELECT
                    source_object_id AS object_id,
                    0::bigint AS in_degree,
                    1::bigint AS out_degree
                FROM visible_connections
            ) endpoints
            GROUP BY endpoints.object_id
        )
        SELECT node.id, node.workspace_id, node.current_version_id, node.title, node.status,
               node.created_by, node.created_at, node.updated_at,
               COALESCE(degrees.in_degree, 0)::bigint AS in_degree,
               COALESCE(degrees.out_degree, 0)::bigint AS out_degree
        FROM visible_nodes node
        LEFT JOIN degrees
            ON degrees.object_id = node.id
        WHERE NOT $3
            OR degrees.object_id IS NOT NULL
        ORDER BY node.updated_at DESC, node.id DESC
        LIMIT $4
        OFFSET CASE WHEN kival.require_read_workspace($1, $2) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(exclude_isolated)
    .bind(limit)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(TryInto::try_into)
    .collect::<Result<Vec<_>>>()
}
