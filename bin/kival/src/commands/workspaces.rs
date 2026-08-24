//! Workspace commands.

use std::collections::{HashMap, HashSet};

use clap::{Parser, Subcommand};
use clap_schema::{CommandSchema, schema_handler};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{
    CreateWorkspaceGroupRequest, CreateWorkspaceMembershipRequest, Event, ListResponse,
    MembershipRole, PatchField, UpdateWorkspaceRequest, Workspace, WorkspaceGraphEdge,
    WorkspaceGraphNode, WorkspaceGraphParams, WorkspaceGraphResponse, WorkspaceGroup,
    WorkspaceGroupListParams, WorkspaceListParams, WorkspaceMembership,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    commands::events::print_event_line,
    utils::{
        args::{
            CliArchiveListStatus, CliMembershipRole, DEFAULT_LIST_LIMIT, DEFAULT_LIST_LIMIT_HELP,
            event_params, list_params,
        },
        credentials::authenticated_client,
        error::CliError,
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

/// Arguments for `kival workspaces`.
#[derive(Debug, Parser, CommandSchema)]
pub struct WorkspacesCommand {
    /// The workspace command to run.
    #[command(subcommand)]
    pub command: WorkspacesSubcommand,
}

/// The available `kival workspaces` commands.
#[derive(Debug, Subcommand, CommandSchema)]
pub enum WorkspacesSubcommand {
    /// List visible workspaces, newest first.
    ///
    /// Active workspaces are returned by default. Use `--status` to select archived workspaces or
    /// both lifecycle states.
    #[command(name = "list")]
    List(WorkspacesListCommand),

    /// Get a workspace by ID.
    #[command(name = "get")]
    Get(WorkspacesGetCommand),

    /// Update a workspace.
    #[command(
        name = "update",
        after_help = "Examples:\n  kival workspaces update <WORKSPACE_ID> --name \"New name\"\n  kival workspaces update <WORKSPACE_ID> --clear-description\n  kival workspaces update <WORKSPACE_ID> --input update.json"
    )]
    Update(WorkspacesUpdateCommand),

    /// Archive a workspace while retaining its stored resources and history.
    #[command(name = "archive")]
    Archive(WorkspacesArchiveCommand),

    /// Restore an archived workspace to active status.
    #[command(name = "unarchive")]
    Unarchive(WorkspacesUnarchiveCommand),

    /// List workspace events in ascending global sequence order.
    ///
    /// `--after-sequence` is exclusive. When multiple filters are supplied, every filter must
    /// match.
    #[command(name = "events")]
    Events(WorkspacesEventsCommand),

    /// Get a bounded projection of visible active objects and active edges in a workspace.
    ///
    /// `--exclude-isolated` removes nodes with no relation. Returned
    /// limits report whether additional matching nodes or edges were omitted.
    #[command(name = "graph")]
    Graph(WorkspacesGraphCommand),

    /// Manage workspace memberships.
    #[command(name = "memberships")]
    Memberships(WorkspaceMembershipsCommand),

    /// Manage workspace group links.
    #[command(name = "groups")]
    Groups(WorkspaceGroupsCommand),
}

/// Arguments for `kival workspaces list`.
#[derive(Debug, Parser)]
pub struct WorkspacesListCommand {
    /// Archive status filter: active, archived, or all.
    #[arg(long, value_name = "STATUS", value_enum, default_value = "active")]
    pub status: CliArchiveListStatus,

    /// Maximum number of workspaces to return.
    #[arg(long, value_name = "N", default_value = DEFAULT_LIST_LIMIT_HELP)]
    pub limit: Option<i64>,

    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[arg(long, value_name = "CURSOR")]
    pub cursor: Option<String>,
}

/// Arguments for `kival workspaces get`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct WorkspacesGetCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
}

/// Semantic input for updating a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Parser)]
pub struct WorkspacesUpdateCommand {
    /// Structured input source.
    #[command(flatten)]
    pub input_source: StructuredInputArgs,
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,

    /// Set the workspace name.
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    /// Set the workspace description.
    #[arg(long, value_name = "DESCRIPTION", conflicts_with = "clear_description")]
    pub description: Option<String>,

    /// Clear the workspace description.
    #[arg(long)]
    pub clear_description: bool,
}

/// Arguments for `kival workspaces archive`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct WorkspacesArchiveCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
}

/// Arguments for `kival workspaces unarchive`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct WorkspacesUnarchiveCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
}

/// Arguments for `kival workspaces events`.
#[derive(Debug, Parser)]
pub struct WorkspacesEventsCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
    /// Maximum number of events to return.
    #[arg(long, value_name = "N", default_value = DEFAULT_LIST_LIMIT_HELP)]
    pub limit: Option<i64>,
    /// Return events with a global sequence number strictly greater than SEQUENCE.
    #[arg(long, value_name = "SEQUENCE")]
    pub after_sequence: Option<i64>,
    /// Filter by event kind.
    #[arg(long, value_name = "KIND")]
    pub event_kind: Option<String>,
    /// Filter by actor user ID.
    #[arg(long, value_name = "USER_ID")]
    pub actor_user_id: Option<Uuid>,
    /// Filter by target user ID.
    #[arg(long, value_name = "USER_ID")]
    pub target_user_id: Option<Uuid>,
    /// Filter by object ID.
    #[arg(long, value_name = "OBJECT_ID")]
    pub object_id: Option<Uuid>,
    /// Filter by group ID.
    #[arg(long, value_name = "GROUP_ID")]
    pub group_id: Option<Uuid>,
}

/// Arguments for `kival workspaces graph`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct WorkspacesGraphCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
    /// Maximum number of nodes to return.
    #[arg(long, value_name = "N")]
    pub limit_nodes: Option<i64>,
    /// Maximum number of edges to return.
    #[arg(long, value_name = "N")]
    pub limit_edges: Option<i64>,
    /// Exclude nodes with no relation.
    #[arg(long)]
    pub exclude_isolated: bool,
}

/// Arguments for `kival workspaces memberships`.
#[derive(Debug, Parser, CommandSchema)]
pub struct WorkspaceMembershipsCommand {
    /// The membership command to run.
    #[command(subcommand)]
    pub command: WorkspaceMembershipsSubcommand,
}

/// The available `kival workspaces memberships` commands.
#[derive(Debug, Subcommand, CommandSchema)]
pub enum WorkspaceMembershipsSubcommand {
    /// List active direct workspace memberships, newest first.
    #[command(name = "list")]
    List(WorkspaceMembershipsListCommand),
    /// Add a user as a direct member or administrator of a workspace.
    ///
    /// This creates direct workspace membership and is distinct from access inherited through a
    /// group.
    #[command(name = "create")]
    Create(WorkspaceMembershipsCreateCommand),
    /// Revoke a direct workspace membership without deleting its historical record.
    ///
    /// This revokes only the selected direct membership; other access paths may still authorize the
    /// user.
    #[command(name = "revoke")]
    Revoke(WorkspaceMembershipsRevokeCommand),
}

/// Arguments for `kival workspaces memberships list`.
#[derive(Debug, Parser)]
pub struct WorkspaceMembershipsListCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
    /// Maximum number of memberships to return.
    #[arg(long, value_name = "N", default_value = DEFAULT_LIST_LIMIT_HELP)]
    pub limit: Option<i64>,
    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[arg(long, value_name = "CURSOR")]
    pub cursor: Option<String>,
}

/// Arguments for `kival workspaces memberships create`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct WorkspaceMembershipsCreateCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
    /// User ID.
    #[arg(long, value_name = "USER_ID")]
    pub user_id: Uuid,
    /// Workspace role: member or admin.
    #[arg(long, value_name = "ROLE", value_enum)]
    pub role: CliMembershipRole,
}

/// Arguments for `kival workspaces memberships revoke`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct WorkspaceMembershipsRevokeCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
    /// Membership ID.
    #[arg(value_name = "MEMBERSHIP_ID")]
    pub membership_id: Uuid,
}

/// Arguments for `kival workspaces groups`.
#[derive(Debug, Parser, CommandSchema)]
pub struct WorkspaceGroupsCommand {
    /// The workspace group command to run.
    #[command(subcommand)]
    pub command: WorkspaceGroupsSubcommand,
}

/// The available `kival workspaces groups` commands.
#[derive(Debug, Subcommand, CommandSchema)]
pub enum WorkspaceGroupsSubcommand {
    /// List active groups linked to a workspace, newest links first.
    ///
    /// A workspace-group link makes the group eligible for group-based object access in that
    /// workspace; it does not create or modify the group itself. Archived links and archived groups
    /// are excluded.
    #[command(name = "list")]
    List(WorkspaceGroupsListCommand),
    /// Link an existing group to a workspace.
    ///
    /// The link makes the group eligible for group-based object access in this workspace. It does
    /// not change the group's own membership.
    #[command(name = "create")]
    Create(WorkspaceGroupsCreateCommand),
    /// Disable a group's link to this workspace without archiving the group itself.
    ///
    /// Group memberships are unchanged. Group-based object grants in this workspace stop
    /// authorizing through the archived link until it is restored.
    #[command(name = "archive")]
    Archive(WorkspaceGroupsArchiveCommand),
    /// Restore an archived link between an active group and this workspace.
    ///
    /// Restoring the link can make existing group-based object grants effective again for active
    /// group members.
    #[command(name = "unarchive")]
    Unarchive(WorkspaceGroupsUnarchiveCommand),
}

/// Arguments for `kival workspaces groups list`.
#[derive(Debug, Parser)]
pub struct WorkspaceGroupsListCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
    /// Maximum number of group links to return.
    #[arg(long, value_name = "N", default_value = DEFAULT_LIST_LIMIT_HELP)]
    pub limit: Option<i64>,
    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[arg(long, value_name = "CURSOR")]
    pub cursor: Option<String>,
}

/// Arguments for `kival workspaces groups create`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct WorkspaceGroupsCreateCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
    /// Group ID.
    #[arg(long, value_name = "GROUP_ID")]
    pub group_id: Uuid,
}

/// Arguments for `kival workspaces groups archive`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct WorkspaceGroupsArchiveCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
    /// Group ID.
    #[arg(value_name = "GROUP_ID")]
    pub group_id: Uuid,
}

/// Arguments for `kival workspaces groups unarchive`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct WorkspaceGroupsUnarchiveCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
    /// Group ID.
    #[arg(value_name = "GROUP_ID")]
    pub group_id: Uuid,
}

impl WorkspacesCommand {
    /// Run `kival workspaces`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected workspace command fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            WorkspacesSubcommand::List(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            WorkspacesSubcommand::Get(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            WorkspacesSubcommand::Update(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            WorkspacesSubcommand::Archive(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            WorkspacesSubcommand::Unarchive(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            WorkspacesSubcommand::Events(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            WorkspacesSubcommand::Graph(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            WorkspacesSubcommand::Memberships(command) => command.run(ctx, output).await,
            WorkspacesSubcommand::Groups(command) => command.run(ctx, output).await,
        }
    }
}

#[schema_handler(run)]
impl WorkspacesGraphCommand {
    /// Run `kival workspaces graph`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the graph cannot be fetched.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<WorkspaceGraphResponse> {
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

        print_output(output, &response, || {
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

#[schema_handler(run)]
impl WorkspacesListCommand {
    /// Run `kival workspaces list`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or workspaces cannot be listed.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<ListResponse<Workspace>> {
        let client = authenticated_client(&ctx)?;

        let response = client
            .list_workspaces(&WorkspaceListParams {
                limit: Some(self.limit.unwrap_or(DEFAULT_LIST_LIMIT)),
                cursor: self.cursor,
                status: self.status.into(),
                q: None,
                pinned: None,
            })
            .await?;

        print_output(output, &response, || {
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

#[schema_handler(run)]
impl WorkspacesGetCommand {
    /// Run `kival workspaces get`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the workspace cannot be fetched.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<Workspace> {
        let client = authenticated_client(&ctx)?;
        let workspace = client.get_workspace(self.workspace_id).await?;

        print_output(output, &workspace, || {
            print_workspace_line(&workspace, None);
        })?;
        Ok(workspace)
    }
}

#[schema_handler(run)]
impl WorkspacesUpdateCommand {
    /// Run `kival workspaces update`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the workspace cannot be updated.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<Workspace> {
        let workspace_id = self.workspace_id;
        let input = self.into_input()?;
        let name = input.name.as_deref().map(str::trim);
        let description = input.description.as_ref().map(|value| value.as_deref().map(str::trim));

        if name.is_none() && input.description.is_none() {
            return Err(CliError::invalid_argument("at least one field must be provided").into());
        }

        if matches!(name, Some("")) {
            return Err(CliError::invalid_argument("name must not be empty").into());
        }

        if matches!(description, Some(Some(""))) {
            return Err(CliError::invalid_argument("description must not be empty").into());
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

        print_output(output, &workspace, || {
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
                return Err(CliError::input_invalid_value(at_least_one_input_field(&[
                    "name",
                    "description",
                ]))
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

#[schema_handler(run)]
impl WorkspacesArchiveCommand {
    /// Run `kival workspaces archive`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the workspace cannot be archived.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<Workspace> {
        let client = authenticated_client(&ctx)?;
        let workspace = client.archive_workspace(self.workspace_id).await?;

        print_output(output, &workspace, || {
            print_workspace_line(&workspace, Some("archived"));
        })?;
        Ok(workspace)
    }
}

#[schema_handler(run)]
impl WorkspacesUnarchiveCommand {
    /// Run `kival workspaces unarchive`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the workspace cannot be unarchived.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<Workspace> {
        let client = authenticated_client(&ctx)?;
        let workspace = client.unarchive_workspace(self.workspace_id).await?;

        print_output(output, &workspace, || {
            print_workspace_line(&workspace, Some("unarchived"));
        })?;
        Ok(workspace)
    }
}

#[schema_handler(run)]
impl WorkspacesEventsCommand {
    /// Run `kival workspaces events`.
    ///
    /// # Errors
    ///
    /// Returns an error if events cannot be listed.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<ListResponse<Event>> {
        let params = event_params(
            self.limit,
            self.after_sequence,
            self.event_kind,
            self.actor_user_id,
            self.target_user_id,
            self.object_id,
            self.group_id,
        );
        let client = authenticated_client(&ctx)?;
        let response = client.list_workspace_events(self.workspace_id, &params).await?;

        print_output(output, &response, || {
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
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            WorkspaceMembershipsSubcommand::List(command) => {
                command.run(ctx, output).await?;
            }
            WorkspaceMembershipsSubcommand::Create(command) => {
                command.run(ctx, output).await?;
            }
            WorkspaceMembershipsSubcommand::Revoke(command) => {
                command.run(ctx, output).await?;
            }
        }
        Ok(())
    }
}

#[schema_handler(run)]
impl WorkspaceMembershipsListCommand {
    /// Run `kival workspaces memberships list`.
    ///
    /// # Errors
    ///
    /// Returns an error if memberships cannot be listed.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> Result<ListResponse<WorkspaceMembership>> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_workspace_memberships(self.workspace_id, &list_params(self.limit, self.cursor))
            .await?;
        print_output(output, &response, || {
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

#[schema_handler(run)]
impl WorkspaceMembershipsCreateCommand {
    /// Run `kival workspaces memberships create`.
    ///
    /// # Errors
    ///
    /// Returns an error if the membership cannot be created.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<WorkspaceMembership> {
        let role = MembershipRole::from(self.role);
        let client = authenticated_client(&ctx)?;
        let membership = client
            .create_workspace_membership(
                self.workspace_id,
                CreateWorkspaceMembershipRequest {
                    user_id: Some(self.user_id),
                    username: None,
                    workspace_role: role,
                },
            )
            .await?;
        print_output(output, &membership, || {
            print_membership_line(&membership, Some("created"));
        })?;
        Ok(membership)
    }
}

#[schema_handler(run)]
impl WorkspaceMembershipsRevokeCommand {
    /// Run `kival workspaces memberships revoke`.
    ///
    /// # Errors
    ///
    /// Returns an error if the membership cannot be revoked.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<WorkspaceMembership> {
        let client = authenticated_client(&ctx)?;
        let membership =
            client.revoke_workspace_membership(self.workspace_id, self.membership_id).await?;
        print_output(output, &membership, || {
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
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            WorkspaceGroupsSubcommand::List(command) => {
                command.run(ctx, output).await?;
            }
            WorkspaceGroupsSubcommand::Create(command) => {
                command.run(ctx, output).await?;
            }
            WorkspaceGroupsSubcommand::Archive(command) => {
                command.run(ctx, output).await?;
            }
            WorkspaceGroupsSubcommand::Unarchive(command) => {
                command.run(ctx, output).await?;
            }
        }
        Ok(())
    }
}

#[schema_handler(run)]
impl WorkspaceGroupsListCommand {
    /// Run `kival workspaces groups list`.
    ///
    /// # Errors
    ///
    /// Returns an error if workspace group links cannot be listed.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> Result<ListResponse<WorkspaceGroup>> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_workspace_groups(
                self.workspace_id,
                &WorkspaceGroupListParams {
                    limit: self.limit,
                    cursor: self.cursor,
                    ..WorkspaceGroupListParams::default()
                },
            )
            .await?;
        print_output(output, &response, || {
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

#[schema_handler(run)]
impl WorkspaceGroupsCreateCommand {
    /// Run `kival workspaces groups create`.
    ///
    /// # Errors
    ///
    /// Returns an error if the group cannot be linked.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<WorkspaceGroup> {
        let client = authenticated_client(&ctx)?;
        let workspace_group = client
            .create_workspace_group(
                self.workspace_id,
                CreateWorkspaceGroupRequest { group_id: self.group_id },
            )
            .await?;
        print_output(output, &workspace_group, || {
            print_workspace_group_line(&workspace_group, Some("linked"));
        })?;
        Ok(workspace_group)
    }
}

#[schema_handler(run)]
impl WorkspaceGroupsArchiveCommand {
    /// Run `kival workspaces groups archive`.
    ///
    /// # Errors
    ///
    /// Returns an error if the group link cannot be archived.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<WorkspaceGroup> {
        let client = authenticated_client(&ctx)?;
        let workspace_group =
            client.archive_workspace_group(self.workspace_id, self.group_id).await?;
        print_output(output, &workspace_group, || {
            print_workspace_group_line(&workspace_group, Some("archived"));
        })?;
        Ok(workspace_group)
    }
}

#[schema_handler(run)]
impl WorkspaceGroupsUnarchiveCommand {
    /// Run `kival workspaces groups unarchive`.
    ///
    /// # Errors
    ///
    /// Returns an error if the group link cannot be unarchived.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<WorkspaceGroup> {
        let client = authenticated_client(&ctx)?;
        let workspace_group =
            client.unarchive_workspace_group(self.workspace_id, self.group_id).await?;
        print_output(output, &workspace_group, || {
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
    use super::*;

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
