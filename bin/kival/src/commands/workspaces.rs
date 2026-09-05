//! Workspace commands.

use std::collections::{HashMap, HashSet};

use argx::{Args, Subcommand, argx};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{
    CreateWorkspaceGroupRequest, CreateWorkspaceMembershipRequest, Event, EventListParams,
    EventOrder, ListResponse, MembershipRole, PatchField, UpdateWorkspaceMembershipRequest,
    UpdateWorkspaceRequest, Workspace, WorkspaceGraphEdge, WorkspaceGraphNode,
    WorkspaceGraphParams, WorkspaceGraphResponse, WorkspaceGroup, WorkspaceGroupListParams,
    WorkspaceListItem, WorkspaceListParams, WorkspaceMembership,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    commands::events::print_event_line,
    utils::{
        args::{CliArchiveListStatus, CliMembershipRole, DEFAULT_LIST_LIMIT, list_params},
        credentials::authenticated_client,
        error::{CommandError, command_error_codes, erase_command_error},
        input::{
            StructuredInputArgs, at_least_one_input_field, deserialize_optional_non_null,
            deserialize_optional_nullable, read_json_input, reject_conflicting_input,
        },
        output::{
            OutputMode, TREE_LAST, format_human_timestamp, print_empty_list, print_output,
            print_tree_none, quote_human_string, tree_connector,
        },
    },
};

command_error_codes! {
    pub(crate) enum WorkspaceGraphErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
    }
}

/// Error returned by the corresponding command handler.
type WorkspaceGraphError = CommandError<WorkspaceGraphErrorCode>;

command_error_codes! {
    pub(crate) enum WorkspaceListErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        InvalidCursor => ("invalid.cursor", InvalidCursor),
    }
}

/// Error returned by the corresponding command handler.
type WorkspaceListError = CommandError<WorkspaceListErrorCode>;

command_error_codes! {
    pub(crate) enum WorkspaceGetErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
    }
}

/// Error returned by the corresponding command handler.
type WorkspaceGetError = CommandError<WorkspaceGetErrorCode>;

command_error_codes! {
    pub(crate) enum WorkspaceUpdateErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
            ResourceConflict => ("resource.conflict", ResourceConflict),
        InputReadFailed => ("input.read_failed", InputReadFailed),
        InputInvalidJson => ("input.invalid_json", InputInvalidJson),
        InputInvalidValue => ("input.invalid_value", InputInvalidValue),
        InputConflictingSources => ("input.conflicting_sources", InputConflictingSources),
    }
}

/// Error returned by the corresponding command handler.
type WorkspaceUpdateError = CommandError<WorkspaceUpdateErrorCode>;

command_error_codes! {
    pub(crate) enum WorkspaceMutationErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
            ResourceConflict => ("resource.conflict", ResourceConflict),
    }
}

/// Error returned by the corresponding command handler.
type WorkspaceMutationError = CommandError<WorkspaceMutationErrorCode>;

command_error_codes! {
    pub(crate) enum WorkspaceEventsErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
        InvalidCursor => ("invalid.cursor", InvalidCursor),
    }
}

/// Error returned by the corresponding command handler.
type WorkspaceEventsError = CommandError<WorkspaceEventsErrorCode>;

command_error_codes! {
    pub(crate) enum WorkspaceMembershipListErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
        InvalidCursor => ("invalid.cursor", InvalidCursor),
    }
}

/// Error returned by the corresponding command handler.
type WorkspaceMembershipListError = CommandError<WorkspaceMembershipListErrorCode>;

command_error_codes! {
    pub(crate) enum WorkspaceMembershipMutationErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
            ResourceConflict => ("resource.conflict", ResourceConflict),
    }
}

/// Error returned by the corresponding command handler.
type WorkspaceMembershipMutationError = CommandError<WorkspaceMembershipMutationErrorCode>;

command_error_codes! {
    pub(crate) enum WorkspaceGroupListErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
        InvalidCursor => ("invalid.cursor", InvalidCursor),
    }
}

/// Error returned by the corresponding command handler.
type WorkspaceGroupListError = CommandError<WorkspaceGroupListErrorCode>;

command_error_codes! {
    pub(crate) enum WorkspaceGroupMutationErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
            ResourceConflict => ("resource.conflict", ResourceConflict),
    }
}

/// Error returned by the corresponding command handler.
type WorkspaceGroupMutationError = CommandError<WorkspaceGroupMutationErrorCode>;

/// Arguments for `kival workspaces`.
#[derive(Debug, Args)]
#[argx(schema)]
pub struct WorkspacesCommand {
    /// The workspace command to run.
    #[argx(subcommand)]
    pub command: WorkspacesSubcommand,
}

/// The available `kival workspaces` commands.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum WorkspacesSubcommand {
    /// List visible workspaces, newest first.
    ///
    /// Active workspaces are returned by default. Use `--status` to select archived workspaces or
    /// both lifecycle states.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["workspaces:read"],
        })
    )]
    List(WorkspacesListCommand),

    /// Get a workspace by ID.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["workspaces:read"],
        })
    )]
    Get(WorkspacesGetCommand),

    /// Update a workspace.
    ///
    /// With `--input`, the JSON object accepts `name` (string) and `description` (string or null,
    /// where null clears it) and requires at least one of them. Unknown properties are rejected.
    ///
    /// Examples: `kival workspaces update <WORKSPACE_ID> --name "New name"`,
    /// `--clear-description`, or `--input update.json`.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["workspaces:write"],
        })
    )]
    Update(WorkspacesUpdateCommand),

    /// Archive a workspace while retaining its stored resources and history.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": true,
            "idempotent": false,
            "requiresConfirmation": true,
            "requiredScopes": ["workspaces:write"],
        })
    )]
    Archive(WorkspacesArchiveCommand),

    /// Restore an archived workspace to active status.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["workspaces:write"],
        })
    )]
    Unarchive(WorkspacesUnarchiveCommand),

    /// List workspace events.
    ///
    /// Sequence bounds are exclusive. Events are returned in ascending order by default. When
    /// multiple filters are supplied, every filter must match.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["events:read"],
        })
    )]
    Events(WorkspacesEventsCommand),

    /// Get a bounded projection of visible active objects and active edges in a workspace.
    ///
    /// `--exclude-isolated` removes nodes with no relation. Returned
    /// limits report whether additional matching nodes or edges were omitted.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["graph:read"],
        })
    )]
    Graph(WorkspacesGraphCommand),

    /// Manage workspace memberships.
    Memberships(WorkspaceMembershipsCommand),

    /// Manage workspace group links.
    Groups(WorkspaceGroupsCommand),
}

/// Arguments for `kival workspaces list`.
#[derive(Debug, Args)]
pub struct WorkspacesListCommand {
    /// Archive status filter: active, archived, or all.
    #[argx(long, value_enum, default = CliArchiveListStatus::Active)]
    pub status: CliArchiveListStatus,

    /// Case-insensitive workspace name search.
    #[argx(long)]
    pub query: Option<String>,

    /// Restrict by the authenticated user's personal pin state.
    #[argx(long)]
    pub pinned: Option<bool>,

    /// Maximum number of workspaces to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,

    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[argx(long)]
    pub cursor: Option<String>,
}

/// Arguments for `kival workspaces get`.
#[derive(Debug, Clone, Copy, Args)]
pub struct WorkspacesGetCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
}

/// Semantic input for updating a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateWorkspaceInput {
    /// New workspace name.
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    pub name: Option<String>,
    /// New workspace description, or null to clear it.
    #[serde(default, deserialize_with = "deserialize_optional_nullable")]
    pub description: Option<Option<String>>,
}

/// Arguments for `kival workspaces update`.
#[derive(Debug, Args)]
#[argx(any_of = ["input", "name", "description", "clear_description"])]
pub struct WorkspacesUpdateCommand {
    /// Structured input source.
    #[argx(flatten)]
    pub input_source: StructuredInputArgs,
    /// Workspace ID.
    pub workspace_id: Uuid,

    /// Set the workspace name.
    #[argx(long, conflicts = "input")]
    pub name: Option<String>,

    /// Set the workspace description.
    #[argx(long, conflicts = ["clear_description", "input"])]
    pub description: Option<String>,

    /// Clear the workspace description.
    #[argx(long, conflicts = "input")]
    pub clear_description: bool,
}

/// Arguments for `kival workspaces archive`.
#[derive(Debug, Clone, Copy, Args)]
pub struct WorkspacesArchiveCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
}

/// Arguments for `kival workspaces unarchive`.
#[derive(Debug, Clone, Copy, Args)]
pub struct WorkspacesUnarchiveCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
}

/// Arguments for `kival workspaces events`.
#[derive(Debug, Args)]
pub struct WorkspacesEventsCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Maximum number of events to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,
    /// Return events with a global sequence number strictly greater than SEQUENCE.
    #[argx(long)]
    pub after_sequence: Option<i64>,
    /// Return events with a global sequence number strictly less than SEQUENCE.
    #[argx(long)]
    pub before_sequence: Option<i64>,
    /// Event sequence ordering.
    #[argx(long, default = EventOrder::Asc)]
    pub order: EventOrder,
    /// Filter by event kind.
    #[argx(long)]
    pub event_kind: Option<String>,
    /// Filter by actor user ID.
    #[argx(long)]
    pub actor_user_id: Option<Uuid>,
    /// Filter by target user ID.
    #[argx(long)]
    pub target_user_id: Option<Uuid>,
    /// Filter by object ID.
    #[argx(long)]
    pub object_id: Option<Uuid>,
    /// Filter by group ID.
    #[argx(long)]
    pub group_id: Option<Uuid>,
}

/// Arguments for `kival workspaces graph`.
#[derive(Debug, Clone, Copy, Args)]
pub struct WorkspacesGraphCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Maximum number of nodes to return.
    #[argx(long)]
    pub limit_nodes: Option<i64>,
    /// Maximum number of edges to return.
    #[argx(long)]
    pub limit_edges: Option<i64>,
    /// Exclude nodes with no relation.
    #[argx(long)]
    pub exclude_isolated: bool,
}

/// Arguments for `kival workspaces memberships`.
#[derive(Debug, Args)]
#[argx(schema)]
pub struct WorkspaceMembershipsCommand {
    /// The membership command to run.
    #[argx(subcommand)]
    pub command: WorkspaceMembershipsSubcommand,
}

/// The available `kival workspaces memberships` commands.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum WorkspaceMembershipsSubcommand {
    /// List active direct workspace memberships, newest first.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["access:manage"],
        })
    )]
    List(WorkspaceMembershipsListCommand),
    /// Add a user as a direct member or administrator of a workspace.
    ///
    /// This creates direct workspace membership and is distinct from access inherited through a
    /// group.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["access:manage"],
        })
    )]
    Create(WorkspaceMembershipsCreateCommand),
    /// Change an active direct workspace membership's role.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["access:manage"],
        })
    )]
    Update(WorkspaceMembershipsUpdateCommand),
    /// Revoke a direct workspace membership without deleting its historical record.
    ///
    /// This revokes only the selected direct membership; other access paths may still authorize the
    /// user.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": true,
            "idempotent": false,
            "requiresConfirmation": true,
            "requiredScopes": ["access:manage"],
        })
    )]
    Revoke(WorkspaceMembershipsRevokeCommand),
}

/// Arguments for `kival workspaces memberships list`.
#[derive(Debug, Args)]
pub struct WorkspaceMembershipsListCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Maximum number of memberships to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,
    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[argx(long)]
    pub cursor: Option<String>,
}

/// Arguments for `kival workspaces memberships create`.
#[derive(Debug, Clone, Args)]
#[argx(one_of = ["user_id", "username"])]
pub struct WorkspaceMembershipsCreateCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// User ID.
    #[argx(long)]
    pub user_id: Option<Uuid>,
    /// Username.
    #[argx(long)]
    pub username: Option<String>,
    /// Workspace role: member or admin.
    #[argx(long, value_enum)]
    pub role: CliMembershipRole,
}

/// Arguments for `kival workspaces memberships update`.
#[derive(Debug, Clone, Copy, Args)]
pub struct WorkspaceMembershipsUpdateCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Membership ID.
    pub membership_id: Uuid,
    /// New workspace role: member or admin.
    #[argx(long, value_enum)]
    pub role: CliMembershipRole,
}

/// Arguments for `kival workspaces memberships revoke`.
#[derive(Debug, Clone, Copy, Args)]
pub struct WorkspaceMembershipsRevokeCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Membership ID.
    pub membership_id: Uuid,
}

/// Arguments for `kival workspaces groups`.
#[derive(Debug, Args)]
#[argx(schema)]
pub struct WorkspaceGroupsCommand {
    /// The workspace group command to run.
    #[argx(subcommand)]
    pub command: WorkspaceGroupsSubcommand,
}

/// The available `kival workspaces groups` commands.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum WorkspaceGroupsSubcommand {
    /// List groups linked to a workspace, newest links first.
    ///
    /// Active links are returned by default. Use `--status` to select archived links or both
    /// lifecycle states. A workspace-group link makes the group eligible for group-based object
    /// access in that workspace; it does not create or modify the group itself.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["access:manage"],
        })
    )]
    List(WorkspaceGroupsListCommand),
    /// Link an existing group to a workspace.
    ///
    /// The link makes the group eligible for group-based object access in this workspace. It does
    /// not change the group's own membership.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["access:manage"],
        })
    )]
    Create(WorkspaceGroupsCreateCommand),
    /// Disable a group's link to this workspace without archiving the group itself.
    ///
    /// Group memberships are unchanged. Group-based object grants in this workspace stop
    /// authorizing through the archived link until it is restored.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": true,
            "idempotent": false,
            "requiresConfirmation": true,
            "requiredScopes": ["access:manage"],
        })
    )]
    Archive(WorkspaceGroupsArchiveCommand),
    /// Restore an archived link between an active group and this workspace.
    ///
    /// Restoring the link can make existing group-based object grants effective again for active
    /// group members.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["access:manage"],
        })
    )]
    Unarchive(WorkspaceGroupsUnarchiveCommand),
}

/// Arguments for `kival workspaces groups list`.
#[derive(Debug, Args)]
pub struct WorkspaceGroupsListCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Archive status filter: active, archived, or all.
    #[argx(long, value_enum, default = CliArchiveListStatus::Active)]
    pub status: CliArchiveListStatus,
    /// Maximum number of group links to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,
    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[argx(long)]
    pub cursor: Option<String>,
}

/// Arguments for `kival workspaces groups create`.
#[derive(Debug, Clone, Copy, Args)]
pub struct WorkspaceGroupsCreateCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Group ID.
    #[argx(long)]
    pub group_id: Uuid,
}

/// Arguments for `kival workspaces groups archive`.
#[derive(Debug, Clone, Copy, Args)]
pub struct WorkspaceGroupsArchiveCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Group ID.
    pub group_id: Uuid,
}

/// Arguments for `kival workspaces groups unarchive`.
#[derive(Debug, Clone, Copy, Args)]
pub struct WorkspaceGroupsUnarchiveCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Group ID.
    pub group_id: Uuid,
}

impl WorkspacesCommand {
    /// Run `kival workspaces`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected workspace command fails.
    pub(crate) async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            WorkspacesSubcommand::List(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            WorkspacesSubcommand::Get(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            WorkspacesSubcommand::Update(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            WorkspacesSubcommand::Archive(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            WorkspacesSubcommand::Unarchive(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            WorkspacesSubcommand::Events(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            WorkspacesSubcommand::Graph(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            WorkspacesSubcommand::Memberships(command) => command.run(ctx, output).await,
            WorkspacesSubcommand::Groups(command) => command.run(ctx, output).await,
        }
    }
}

#[argx(handler = run)]
impl WorkspacesGraphCommand {
    /// Run `kival workspaces graph`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the graph cannot be fetched.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<WorkspaceGraphResponse, WorkspaceGraphError> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .get_workspace_graph(
                self.workspace_id,
                &WorkspaceGraphParams {
                    limit_nodes: self.limit_nodes,
                    limit_edges: self.limit_edges,
                    exclude_isolated: self.exclude_isolated,
                },
            )
            .await?;

        print_output(&output, &response, || {
            print_workspace_graph(&response);
        })?;
        Ok(response)
    }
}

/// Prints a workspace graph as source-grouped outgoing relations.
fn print_workspace_graph(graph: &WorkspaceGraphResponse) {
    println!("Workspace graph {}", graph.workspace_id);
    println!(
        "{} {}, {} {}",
        graph.nodes.len(),
        pluralized(graph.nodes.len(), "node", "nodes"),
        graph.edges.len(),
        pluralized(graph.edges.len(), "edge", "edges")
    );
    println!(
        "has_more_nodes={} has_more_edges={}",
        graph.limits.has_more_nodes, graph.limits.has_more_edges
    );

    let nodes_by_id = graph.nodes.iter().map(|node| (node.id, node)).collect::<HashMap<_, _>>();
    let mut edges_by_source: HashMap<Uuid, Vec<&WorkspaceGraphEdge>> = HashMap::new();
    let mut source_order = Vec::new();
    let mut incident_node_ids = HashSet::new();

    for edge in &graph.edges {
        if !edges_by_source.contains_key(&edge.source_object_id) {
            source_order.push(edge.source_object_id);
        }
        edges_by_source.entry(edge.source_object_id).or_default().push(edge);
        incident_node_ids.insert(edge.source_object_id);
        incident_node_ids.insert(edge.target_object_id);
    }

    println!();
    println!("Relations");
    if graph.edges.is_empty() {
        print_tree_none();
    } else {
        for (source_index, source_id) in source_order.into_iter().enumerate() {
            if source_index != 0 {
                println!();
            }
            if let Some(source) = nodes_by_id.get(&source_id) {
                println!("{} id={}", source.title, source.id);
            } else {
                println!("{source_id}");
            }
            println!("{TREE_LAST} outgoing");

            let Some(edges) = edges_by_source.get(&source_id) else {
                continue;
            };
            for (index, edge) in edges.iter().enumerate() {
                let connector = tree_connector(index, edges.len());
                let target = nodes_by_id
                    .get(&edge.target_object_id)
                    .map_or_else(|| edge.target_object_id.to_string(), |node| node.title.clone());
                println!("   {connector} target={target} edge={}", edge.id);
            }
        }
    }

    println!();
    println!("Isolated");
    let isolated =
        graph.nodes.iter().filter(|node| !incident_node_ids.contains(&node.id)).collect::<Vec<_>>();
    if isolated.is_empty() {
        print_tree_none();
    } else {
        print_isolated_nodes(&isolated);
    }
}

/// Prints returned graph nodes with no returned incident edge.
fn print_isolated_nodes(nodes: &[&WorkspaceGraphNode]) {
    for (index, node) in nodes.iter().enumerate() {
        let connector = tree_connector(index, nodes.len());
        println!("{connector} {} id={}", node.title, node.id);
    }
}

/// Selects a singular or plural label for a count.
const fn pluralized<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

#[argx(handler = run)]
impl WorkspacesListCommand {
    /// Run `kival workspaces list`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or workspaces cannot be listed.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ListResponse<WorkspaceListItem>, WorkspaceListError> {
        let client = authenticated_client(&ctx)?;

        let response = client
            .list_workspaces(&WorkspaceListParams {
                limit: Some(self.limit.unwrap_or(DEFAULT_LIST_LIMIT)),
                cursor: self.cursor,
                status: self.status.into(),
                q: self.query,
                pinned: self.pinned,
            })
            .await?;

        print_output(&output, &response, || {
            if response.items.is_empty() {
                print_empty_list("workspaces");
            } else {
                for workspace in &response.items {
                    print_workspace_line(workspace, None);
                }
            }

            if let Some(cursor) = &response.next_cursor {
                println!();
                println!("Next cursor: {cursor}");
            }
        })?;
        Ok(response)
    }
}

#[argx(handler = run)]
impl WorkspacesGetCommand {
    /// Run `kival workspaces get`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the workspace cannot be fetched.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<Workspace, WorkspaceGetError> {
        let client = authenticated_client(&ctx)?;
        let workspace = client.get_workspace(self.workspace_id).await?;

        print_output(&output, &workspace, || {
            print_workspace_line(&workspace, None);
        })?;
        Ok(workspace)
    }
}

#[argx(handler = run)]
impl WorkspacesUpdateCommand {
    /// Run `kival workspaces update`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the workspace cannot be updated.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<Workspace, WorkspaceUpdateError> {
        let workspace_id = self.workspace_id;
        let input = self.into_input()?;
        let name = input.name.as_deref().map(str::trim);
        let description = input.description.as_ref().map(|value| value.as_deref().map(str::trim));

        if name.is_none() && input.description.is_none() {
            return Err(WorkspaceUpdateError::invalid_argument(
                "at least one field must be provided",
            ));
        }

        if matches!(name, Some("")) {
            return Err(WorkspaceUpdateError::invalid_argument("name must not be empty"));
        }

        if matches!(description, Some(Some(""))) {
            return Err(WorkspaceUpdateError::invalid_argument("description must not be empty"));
        }

        let client = authenticated_client(&ctx)?;
        let description = match description {
            None => PatchField::Missing,
            Some(None) => PatchField::Null,
            Some(Some(value)) => PatchField::Value(value.to_owned()),
        };
        let workspace = client
            .update_workspace(
                workspace_id,
                UpdateWorkspaceRequest { name: name.map(ToOwned::to_owned), description },
            )
            .await?;

        print_output(&output, &workspace, || {
            print_workspace_line(&workspace, Some("updated"));
        })?;
        Ok(workspace)
    }

    /// Resolves semantic update input from either `--input` or CLI payload fields.
    fn into_input(self) -> Result<UpdateWorkspaceInput> {
        reject_conflicting_input(
            &self.input_source.input,
            &[
                ("name", self.name.is_some()),
                ("description", self.description.is_some()),
                ("clear_description", self.clear_description),
            ],
        )?;

        if let Some(input) = self.input_source.input {
            let input: UpdateWorkspaceInput = read_json_input(input)?;
            if input.name.is_none() && input.description.is_none() {
                return Err(WorkspaceUpdateError::input_invalid_value(at_least_one_input_field(
                    &["name", "description"],
                ))
                .into());
            }
            return Ok(input);
        }

        Ok(UpdateWorkspaceInput {
            name: self.name,
            description: if self.clear_description {
                Some(None)
            } else {
                self.description.map(Some)
            },
        })
    }
}

#[argx(handler = run)]
impl WorkspacesArchiveCommand {
    /// Run `kival workspaces archive`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the workspace cannot be archived.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<Workspace, WorkspaceMutationError> {
        let client = authenticated_client(&ctx)?;
        let workspace = client.archive_workspace(self.workspace_id).await?;

        print_output(&output, &workspace, || {
            print_workspace_line(&workspace, Some("archived"));
        })?;
        Ok(workspace)
    }
}

#[argx(handler = run)]
impl WorkspacesUnarchiveCommand {
    /// Run `kival workspaces unarchive`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the workspace cannot be unarchived.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<Workspace, WorkspaceMutationError> {
        let client = authenticated_client(&ctx)?;
        let workspace = client.unarchive_workspace(self.workspace_id).await?;

        print_output(&output, &workspace, || {
            print_workspace_line(&workspace, Some("unarchived"));
        })?;
        Ok(workspace)
    }
}

#[argx(handler = run)]
impl WorkspacesEventsCommand {
    /// Run `kival workspaces events`.
    ///
    /// # Errors
    ///
    /// Returns an error if events cannot be listed.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ListResponse<Event>, WorkspaceEventsError> {
        let params = EventListParams {
            limit: self.limit,
            after_sequence: self.after_sequence,
            before_sequence: self.before_sequence,
            order: self.order,
            event_kind: self.event_kind,
            actor_user_id: self.actor_user_id,
            target_user_id: self.target_user_id,
            object_id: self.object_id,
            group_id: self.group_id,
        };
        let client = authenticated_client(&ctx)?;
        let response = client.list_workspace_events(self.workspace_id, &params).await?;

        print_output(&output, &response, || {
            if response.items.is_empty() {
                print_empty_list("events");
            } else {
                for event in &response.items {
                    print_event_line(event);
                }
            }
        })?;
        Ok(response)
    }
}

impl WorkspaceMembershipsCommand {
    /// Run `kival workspaces memberships`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected membership command fails.
    pub(crate) async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            WorkspaceMembershipsSubcommand::List(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            WorkspaceMembershipsSubcommand::Create(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            WorkspaceMembershipsSubcommand::Update(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            WorkspaceMembershipsSubcommand::Revoke(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
        }
        Ok(())
    }
}

#[argx(handler = run)]
impl WorkspaceMembershipsListCommand {
    /// Run `kival workspaces memberships list`.
    ///
    /// # Errors
    ///
    /// Returns an error if memberships cannot be listed.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ListResponse<WorkspaceMembership>, WorkspaceMembershipListError> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_workspace_memberships(self.workspace_id, &list_params(self.limit, self.cursor))
            .await?;
        print_output(&output, &response, || {
            if response.items.is_empty() {
                print_empty_list("memberships");
            } else {
                for membership in &response.items {
                    print_membership_line(membership, None);
                }
            }
            if let Some(cursor) = &response.next_cursor {
                println!("\nNext cursor: {cursor}");
            }
        })?;
        Ok(response)
    }
}

#[argx(handler = run)]
impl WorkspaceMembershipsCreateCommand {
    /// Run `kival workspaces memberships create`.
    ///
    /// # Errors
    ///
    /// Returns an error if the membership cannot be created.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<WorkspaceMembership, WorkspaceMembershipMutationError> {
        let role = MembershipRole::from(self.role);
        if self.user_id.is_none() && self.username.is_none() {
            return Err(WorkspaceMembershipMutationError::invalid_argument(
                "either --user-id or --username is required",
            ));
        }
        let client = authenticated_client(&ctx)?;
        let membership = client
            .create_workspace_membership(
                self.workspace_id,
                CreateWorkspaceMembershipRequest {
                    user_id: self.user_id,
                    username: self.username,
                    workspace_role: role,
                },
            )
            .await?;
        print_output(&output, &membership, || {
            print_membership_line(&membership, Some("created"));
        })?;
        Ok(membership)
    }
}

#[argx(handler = run)]
impl WorkspaceMembershipsUpdateCommand {
    /// Run `kival workspaces memberships update`.
    ///
    /// # Errors
    ///
    /// Returns an error if the active membership role cannot be updated.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<WorkspaceMembership, WorkspaceMembershipMutationError> {
        let client = authenticated_client(&ctx)?;
        let membership = client
            .update_workspace_membership(
                self.workspace_id,
                self.membership_id,
                UpdateWorkspaceMembershipRequest { workspace_role: self.role.into() },
            )
            .await?;
        print_output(&output, &membership, || {
            print_membership_line(&membership, Some("updated"));
        })?;
        Ok(membership)
    }
}

#[argx(handler = run)]
impl WorkspaceMembershipsRevokeCommand {
    /// Run `kival workspaces memberships revoke`.
    ///
    /// # Errors
    ///
    /// Returns an error if the membership cannot be revoked.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<WorkspaceMembership, WorkspaceMembershipMutationError> {
        let client = authenticated_client(&ctx)?;
        let membership =
            client.revoke_workspace_membership(self.workspace_id, self.membership_id).await?;
        print_output(&output, &membership, || {
            print_membership_line(&membership, Some("revoked"));
        })?;
        Ok(membership)
    }
}

impl WorkspaceGroupsCommand {
    /// Run `kival workspaces groups`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected workspace group command fails.
    pub(crate) async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            WorkspaceGroupsSubcommand::List(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            WorkspaceGroupsSubcommand::Create(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            WorkspaceGroupsSubcommand::Archive(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            WorkspaceGroupsSubcommand::Unarchive(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
        }
        Ok(())
    }
}

#[argx(handler = run)]
impl WorkspaceGroupsListCommand {
    /// Run `kival workspaces groups list`.
    ///
    /// # Errors
    ///
    /// Returns an error if workspace group links cannot be listed.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ListResponse<WorkspaceGroup>, WorkspaceGroupListError> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_workspace_groups(
                self.workspace_id,
                &WorkspaceGroupListParams {
                    limit: self.limit,
                    cursor: self.cursor,
                    status: self.status.into(),
                },
            )
            .await?;
        print_output(&output, &response, || {
            if response.items.is_empty() {
                print_empty_list("workspace group links");
            } else {
                for workspace_group in &response.items {
                    print_workspace_group_line(workspace_group, None);
                }
            }
            if let Some(cursor) = &response.next_cursor {
                println!("\nNext cursor: {cursor}");
            }
        })?;
        Ok(response)
    }
}

#[argx(handler = run)]
impl WorkspaceGroupsCreateCommand {
    /// Run `kival workspaces groups create`.
    ///
    /// # Errors
    ///
    /// Returns an error if the group cannot be linked.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<WorkspaceGroup, WorkspaceGroupMutationError> {
        let client = authenticated_client(&ctx)?;
        let workspace_group = client
            .create_workspace_group(
                self.workspace_id,
                CreateWorkspaceGroupRequest { group_id: self.group_id },
            )
            .await?;
        print_output(&output, &workspace_group, || {
            print_workspace_group_line(&workspace_group, Some("linked"));
        })?;
        Ok(workspace_group)
    }
}

#[argx(handler = run)]
impl WorkspaceGroupsArchiveCommand {
    /// Run `kival workspaces groups archive`.
    ///
    /// # Errors
    ///
    /// Returns an error if the group link cannot be archived.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<WorkspaceGroup, WorkspaceGroupMutationError> {
        let client = authenticated_client(&ctx)?;
        let workspace_group =
            client.archive_workspace_group(self.workspace_id, self.group_id).await?;
        print_output(&output, &workspace_group, || {
            print_workspace_group_line(&workspace_group, Some("archived"));
        })?;
        Ok(workspace_group)
    }
}

#[argx(handler = run)]
impl WorkspaceGroupsUnarchiveCommand {
    /// Run `kival workspaces groups unarchive`.
    ///
    /// # Errors
    ///
    /// Returns an error if the group link cannot be unarchived.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<WorkspaceGroup, WorkspaceGroupMutationError> {
        let client = authenticated_client(&ctx)?;
        let workspace_group =
            client.unarchive_workspace_group(self.workspace_id, self.group_id).await?;
        print_output(&output, &workspace_group, || {
            print_workspace_group_line(&workspace_group, Some("unarchived"));
        })?;
        Ok(workspace_group)
    }
}

/// Prints a compact workspace line.
fn print_workspace_line(workspace: &Workspace, action: Option<&str>) {
    let mut fields = vec![workspace.id.to_string()];
    if let Some(action) = action {
        fields.push(format!("action={action}"));
    }
    fields.extend([
        format!("created={}", format_human_timestamp(workspace.created_at)),
        format!("updated={}", format_human_timestamp(workspace.updated_at)),
        format!("status={}", workspace.status),
        format!("name={}", quote_human_string(&workspace.name)),
    ]);
    if let Some(description) = &workspace.description {
        fields.push(format!("description={}", quote_human_string(description)));
    }
    println!("{}", fields.join(" "));
}

/// Prints a compact workspace membership line.
fn print_membership_line(membership: &WorkspaceMembership, action: Option<&str>) {
    let mut fields = vec![membership.id.to_string()];
    if let Some(action) = action {
        fields.push(format!("action={action}"));
    }
    fields.extend([
        format!("workspace={}", membership.workspace_id),
        format!("user={}", membership.user_id),
        format!("role={}", membership.workspace_role),
    ]);
    println!("{}", fields.join(" "));
}

/// Prints a compact workspace group link line.
fn print_workspace_group_line(workspace_group: &WorkspaceGroup, action: Option<&str>) {
    let mut fields = vec![workspace_group.id.to_string()];
    if let Some(action) = action {
        fields.push(format!("action={action}"));
    }
    fields.extend([
        format!("workspace={}", workspace_group.workspace_id),
        format!("group={}", workspace_group.group_id),
        format!("status={}", workspace_group.status),
    ]);
    println!("{}", fields.join(" "));
}

#[cfg(test)]
mod tests {
    use argx::Parser as _;

    use super::*;

    #[derive(Debug, argx::Parser)]
    struct MembershipParser {
        #[argx(subcommand)]
        command: WorkspaceMembershipsSubcommand,
    }

    #[test]
    fn membership_create_accepts_user_id_or_username() {
        let workspace_id = Uuid::from_u128(1).to_string();
        let user_id = Uuid::from_u128(2).to_string();

        let by_id = MembershipParser::try_parse_from([
            "memberships",
            "create",
            workspace_id.as_str(),
            "--user-id",
            user_id.as_str(),
            "--role",
            "member",
        ])
        .unwrap();
        let WorkspaceMembershipsSubcommand::Create(by_id) = by_id.command else {
            panic!("expected create command");
        };
        assert_eq!(by_id.user_id, Some(Uuid::from_u128(2)));
        assert_eq!(by_id.username, None);

        let by_username = MembershipParser::try_parse_from([
            "memberships",
            "create",
            workspace_id.as_str(),
            "--username",
            "importer",
            "--role",
            "member",
        ])
        .unwrap();
        let WorkspaceMembershipsSubcommand::Create(by_username) = by_username.command else {
            panic!("expected create command");
        };
        assert_eq!(by_username.user_id, None);
        assert_eq!(by_username.username.as_deref(), Some("importer"));
    }

    #[test]
    fn membership_create_requires_a_user_selector() {
        let resource_id = Uuid::from_u128(1).to_string();
        assert!(
            MembershipParser::try_parse_from([
                "memberships",
                "create",
                resource_id.as_str(),
                "--role",
                "member",
            ])
            .is_err()
        );
    }

    #[test]
    fn membership_create_rejects_multiple_user_selectors() {
        let workspace_id = Uuid::from_u128(1).to_string();
        let user_id = Uuid::from_u128(2).to_string();
        assert!(
            MembershipParser::try_parse_from([
                "memberships",
                "create",
                workspace_id.as_str(),
                "--user-id",
                user_id.as_str(),
                "--username",
                "importer",
                "--role",
                "member",
            ])
            .is_err()
        );
    }

    #[test]
    fn update_workspace_input_preserves_description_null_state() {
        let omitted: UpdateWorkspaceInput = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(omitted.description, None);

        let cleared: UpdateWorkspaceInput =
            serde_json::from_str(r#"{"description":null}"#).unwrap();
        assert_eq!(cleared.description, Some(None));

        let set: UpdateWorkspaceInput = serde_json::from_str(r#"{"description":"New"}"#).unwrap();
        assert_eq!(set.description, Some(Some("New".to_owned())));

        let null_name = serde_json::from_str::<UpdateWorkspaceInput>(r#"{"name":null}"#)
            .expect_err("name null should be rejected during deserialization");
        assert_eq!(null_name.classify(), serde_json::error::Category::Data);
    }
}
