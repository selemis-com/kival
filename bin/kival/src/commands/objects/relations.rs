//! Object graph, backlink, and explicit-edge commands.

use std::collections::HashMap;

use argx::{Args, Subcommand, ValueEnum, argx};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{
    CreateObjectEdgeRequest, ListResponse, ObjectBacklink, ObjectBacklinkReference,
    ObjectBacklinksParams, ObjectBacklinksResponse, ObjectEdge, ObjectGraphDirection,
    ObjectGraphEdge, ObjectGraphNode, ObjectGraphParams, ObjectGraphResponse,
};
use uuid::Uuid;

use super::ObjectTargetArgs;
use crate::utils::{
    args::{DEFAULT_LIST_LIMIT, list_params},
    credentials::authenticated_client,
    error::CliError,
    output::{
        OutputMode, TREE_BRANCH, TREE_LAST, print_empty_list, print_indented_tree_none,
        print_output, print_tree_none, quote_human_string, tree_connector,
    },
};

/// Arguments for `kival objects graph`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectsGraphCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Maximum number of edge hops from the root to traverse.
    #[argx(long)]
    pub depth: Option<i32>,
    /// Traversal direction: both, incoming, or outgoing.
    #[argx(long, default = CliObjectGraphDirection::Both, value_enum)]
    pub direction: CliObjectGraphDirection,
    /// Maximum number of nodes to return.
    #[argx(long = "max-nodes", alias = "limit-nodes")]
    pub max_nodes: Option<i64>,
    /// Maximum number of edges to return.
    #[argx(long = "max-edges", alias = "limit-edges")]
    pub max_edges: Option<i64>,
    /// Exclude the root from the returned node set after traversal.
    #[argx(long)]
    pub no_root: bool,
}

/// CLI object graph traversal directions.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliObjectGraphDirection {
    /// Follow edges in both directions.
    Both,
    /// Follow edges from target to source.
    Incoming,
    /// Follow edges from source to target.
    Outgoing,
}

impl From<CliObjectGraphDirection> for ObjectGraphDirection {
    fn from(direction: CliObjectGraphDirection) -> Self {
        match direction {
            CliObjectGraphDirection::Both => Self::Both,
            CliObjectGraphDirection::Incoming => Self::Incoming,
            CliObjectGraphDirection::Outgoing => Self::Outgoing,
        }
    }
}
/// Arguments for kival objects backlinks.
#[derive(Debug, Args)]
pub struct ObjectsBacklinksCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Maximum number of backlinks to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,
    /// Opaque cursor for the next explicit-edge page.
    #[argx(long)]
    pub edge_cursor: Option<String>,
    /// Opaque cursor for the next textual-reference page.
    #[argx(long)]
    pub reference_cursor: Option<String>,
    /// Include archived source objects when authorized.
    #[argx(long)]
    pub include_archived: bool,
}

/// Arguments for `kival objects edges`.
#[derive(Debug, Args)]
#[argx(schema)]
pub struct ObjectEdgesCommand {
    /// The edge command to run.
    #[argx(subcommand)]
    pub command: ObjectEdgesSubcommand,
}

/// The available `kival objects edges` commands.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum ObjectEdgesSubcommand {
    /// List active incoming and outgoing edges attached to an object.
    ///
    /// An edge is returned when the selected object is either its source or target. Both endpoint
    /// objects must be active and visible to the current user.
    List(ObjectEdgesListCommand),
    /// Create a directed edge from a source object to a target object.
    ///
    /// `--source-object-id` is the origin of the relationship and `--target-object-id` is its
    /// destination.
    Create(ObjectEdgesCreateCommand),
    /// Get an object edge.
    Get(ObjectEdgesGetCommand),
    /// Revoke an active object edge without deleting its historical record.
    Revoke(ObjectEdgesRevokeCommand),
}

/// Arguments for `kival objects edges list`.
#[derive(Debug, Args)]
pub struct ObjectEdgesListCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Maximum number of edges to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,
    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[argx(long)]
    pub cursor: Option<String>,
}

/// Arguments for `kival objects edges create`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectEdgesCreateCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Source object ID.
    #[argx(long)]
    pub source_object_id: Uuid,
    /// Target object ID.
    #[argx(long)]
    pub target_object_id: Uuid,
}

/// Arguments for `kival objects edges get`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectEdgesGetCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Edge ID.
    pub edge_id: Uuid,
}

/// Arguments for `kival objects edges revoke`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectEdgesRevokeCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Edge ID.
    pub edge_id: Uuid,
}

#[argx(handler = run)]
impl ObjectsGraphCommand {
    /// Run `kival objects graph`.
    ///
    /// # Errors
    ///
    /// Returns an error if arguments, API-key resolution, or graph retrieval fail.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectGraphResponse, CliError> {
        let direction = ObjectGraphDirection::from(self.direction);
        let client = authenticated_client(&ctx)?;
        let graph = client
            .get_object_graph(
                self.target.workspace_id,
                self.target.object_id,
                &ObjectGraphParams {
                    depth: self.depth,
                    direction,
                    max_nodes: self.max_nodes,
                    max_edges: self.max_edges,
                    include_root: !self.no_root,
                },
            )
            .await?;

        print_output(output, &graph, || {
            print_object_graph(&graph);
        })?;
        Ok(graph)
    }
}

/// Prints an object graph as source-grouped relations with node distances.
fn print_object_graph(graph: &ObjectGraphResponse) {
    let nodes_by_id = graph.nodes.iter().map(|node| (node.id, node)).collect::<HashMap<_, _>>();
    let root = nodes_by_id
        .get(&graph.root_object_id)
        .map_or_else(|| graph.root_object_id.to_string(), |node| node.title.clone());

    println!("Object graph {}", graph.root_object_id);
    println!("workspace={}", graph.workspace_id);
    println!("root={root}");
    println!("depth={} direction={}", graph.depth, graph.direction);
    println!(
        "{} {}, {} {}",
        graph.nodes.len(),
        pluralized(graph.nodes.len(), "node", "nodes"),
        graph.edges.len(),
        pluralized(graph.edges.len(), "edge", "edges")
    );
    println!(
        "truncated={} truncation.nodes={} truncation.edges={}",
        graph.truncated, graph.truncation.nodes, graph.truncation.edges
    );

    let mut edges_by_source: HashMap<Uuid, Vec<&ObjectGraphEdge>> = HashMap::new();
    let mut source_order = Vec::new();
    for edge in &graph.edges {
        if !edges_by_source.contains_key(&edge.source_object_id) {
            source_order.push(edge.source_object_id);
        }
        edges_by_source.entry(edge.source_object_id).or_default().push(edge);
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
                println!("{} distance={} id={}", source.title, source.distance, source.id);
            } else {
                println!("{source_id}");
            }
            println!("{TREE_LAST} outgoing relations");

            let Some(edges) = edges_by_source.get(&source_id) else {
                continue;
            };
            for (index, edge) in edges.iter().enumerate() {
                let connector = tree_connector(index, edges.len());
                let target = format_object_graph_target(
                    nodes_by_id.get(&edge.target_object_id).copied(),
                    edge.target_object_id,
                );
                println!("   {connector} target={target} edge={}", edge.id);
            }
        }
    }

    println!();
    println!("Distances");
    if graph.nodes.is_empty() {
        print_tree_none();
    } else {
        for node in &graph.nodes {
            println!("{}  {} id={}", node.distance, node.title, node.id);
        }
    }
}

/// Formats a target node label with distance, falling back to its ID.
fn format_object_graph_target(node: Option<&ObjectGraphNode>, node_id: Uuid) -> String {
    node.map_or_else(
        || node_id.to_string(),
        |node| format!("{} distance={}", node.title, node.distance),
    )
}

/// Selects a singular or plural label for a count.
const fn pluralized<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

#[argx(handler = run)]
impl ObjectsBacklinksCommand {
    /// Runs the object backlinks command.
    ///
    /// # Errors
    ///
    /// Returns an error if backlinks cannot be listed.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectBacklinksResponse, CliError> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .get_object_backlinks(
                self.target.workspace_id,
                self.target.object_id,
                &ObjectBacklinksParams {
                    limit: self.limit,
                    edge_cursor: self.edge_cursor,
                    reference_cursor: self.reference_cursor,
                    include_archived: self.include_archived,
                },
            )
            .await?;

        print_output(output, &response, || print_backlinks(&response))?;
        Ok(response)
    }
}

impl ObjectEdgesCommand {
    /// Run `kival objects edges`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected edge command fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            ObjectEdgesSubcommand::List(command) => {
                command.run(ctx, output).await?;
            }
            ObjectEdgesSubcommand::Create(command) => {
                command.run(ctx, output).await?;
            }
            ObjectEdgesSubcommand::Get(command) => {
                command.run(ctx, output).await?;
            }
            ObjectEdgesSubcommand::Revoke(command) => {
                command.run(ctx, output).await?;
            }
        }
        Ok(())
    }
}

#[argx(handler = run)]
impl ObjectEdgesListCommand {
    /// Run `kival objects edges list`.
    ///
    /// # Errors
    ///
    /// Returns an error if edges cannot be listed.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ListResponse<ObjectEdge>, CliError> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_object_edges(
                self.target.workspace_id,
                self.target.object_id,
                &list_params(self.limit, self.cursor),
            )
            .await?;
        print_output(output, &response, || {
            if response.items.is_empty() {
                print_empty_list("edges");
            } else {
                print_edge_neighborhood(self.target.object_id, &response.items);
            }
            if let Some(cursor) = &response.next_cursor {
                println!("\nNext cursor: {cursor}");
            }
        })?;
        Ok(response)
    }
}

#[argx(handler = run)]
impl ObjectEdgesCreateCommand {
    /// Run `kival objects edges create`.
    ///
    /// # Errors
    ///
    /// Returns an error if the edge cannot be created.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectEdge, CliError> {
        let client = authenticated_client(&ctx)?;
        let edge = client
            .create_object_edge(
                self.workspace_id,
                CreateObjectEdgeRequest {
                    source_object_id: self.source_object_id,
                    target_object_id: self.target_object_id,
                },
            )
            .await?;
        print_output(output, &edge, || print_edge_line(&edge, Some("created")))?;
        Ok(edge)
    }
}

#[argx(handler = run)]
impl ObjectEdgesGetCommand {
    /// Run `kival objects edges get`.
    ///
    /// # Errors
    ///
    /// Returns an error if the edge cannot be fetched.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectEdge, CliError> {
        let client = authenticated_client(&ctx)?;
        let edge = client.get_object_edge(self.workspace_id, self.edge_id).await?;
        print_output(output, &edge, || print_edge_line(&edge, None))?;
        Ok(edge)
    }
}

#[argx(handler = run)]
impl ObjectEdgesRevokeCommand {
    /// Run `kival objects edges revoke`.
    ///
    /// # Errors
    ///
    /// Returns an error if the edge cannot be revoked.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectEdge, CliError> {
        let client = authenticated_client(&ctx)?;
        let edge = client.revoke_object_edge(self.workspace_id, self.edge_id).await?;
        print_output(output, &edge, || print_edge_line(&edge, Some("revoked")))?;
        Ok(edge)
    }
}

/// Prints an object backlinks response.
fn print_backlinks(response: &ObjectBacklinksResponse) {
    println!("Backlinks for {}", response.object_id);
    println!();
    println!("Explicit edges");

    if response.incoming_edges.is_empty() {
        println!("none");
    } else {
        for backlink in &response.incoming_edges {
            print_backlink_line(backlink);
        }
    }
    if let Some(cursor) = &response.next_edge_cursor {
        println!("Next edge cursor: {cursor}");
    }

    println!();
    println!("Textual references");

    if response.incoming_references.is_empty() {
        println!("none");
    } else {
        for reference in &response.incoming_references {
            print_backlink_reference_line(reference);
        }
    }
    if let Some(cursor) = &response.next_reference_cursor {
        println!("Next reference cursor: {cursor}");
    }
}

/// Prints a readable textual backlink.
fn print_backlink_reference_line(reference: &ObjectBacklinkReference) {
    let mut fields = vec![
        format!("reference={}", reference.reference_id),
        format!("kind={}", reference.reference_kind),
        format!("source={}", reference.source_object.id),
        format!("title={}", quote_human_string(&reference.source_object.title)),
        format!("target={}", reference.target_object_id),
        format!("version={}", reference.source_version_id),
    ];

    if reference.reference_kind != "kival_object_link" {
        fields.push(format!("raw={}", quote_human_string(&reference.raw_target)));
    }

    if let Some(display_text) = &reference.display_text {
        fields.push(format!("display={}", quote_human_string(display_text)));
    }

    println!("{}", fields.join(" "));
}

/// Prints a readable explicit backlink.
fn print_backlink_line(backlink: &ObjectBacklink) {
    println!(
        "edge={} source={} title={} target={}",
        backlink.edge_id,
        backlink.source_object.id,
        quote_human_string(&backlink.source_object.title),
        backlink.target_object_id,
    );
}

/// Prints a compact edge line.
fn print_edge_line(edge: &ObjectEdge, action: Option<&str>) {
    let mut fields = vec![edge.id.to_string()];
    if let Some(action) = action {
        fields.push(format!("action={action}"));
    }
    fields.extend([
        format!("source={}", edge.source_object_id),
        format!("target={}", edge.target_object_id),
    ]);
    println!("{}", fields.join(" "));
}

/// Prints edges attached to an object as a one-hop graph neighborhood.
fn print_edge_neighborhood(object_id: Uuid, edges: &[ObjectEdge]) {
    let outgoing =
        edges.iter().filter(|edge| edge.source_object_id == object_id).collect::<Vec<_>>();
    let incoming =
        edges.iter().filter(|edge| edge.target_object_id == object_id).collect::<Vec<_>>();

    println!("{object_id}");
    print_outgoing_edges(&outgoing);
    print_incoming_edges(&incoming);
}

/// Prints outgoing edges from the selected object.
fn print_outgoing_edges(edges: &[&ObjectEdge]) {
    println!("{TREE_BRANCH} outgoing");

    if edges.is_empty() {
        print_indented_tree_none("│  ");
        return;
    }

    for (index, edge) in edges.iter().enumerate() {
        let connector = tree_connector(index, edges.len());
        println!("│  {} target={} edge={}", connector, edge.target_object_id, edge.id);
    }
}

/// Prints incoming edges to the selected object.
fn print_incoming_edges(edges: &[&ObjectEdge]) {
    println!("{TREE_LAST} incoming");

    if edges.is_empty() {
        print_indented_tree_none("   ");
        return;
    }

    for (index, edge) in edges.iter().enumerate() {
        let connector = tree_connector(index, edges.len());
        println!("   {} source={} edge={}", connector, edge.source_object_id, edge.id);
    }
}
