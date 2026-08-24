//! Graph API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        GraphEdgeKind, GrantPrincipal, MembershipRole, ObjectGraphEdge, ObjectGraphNode, ObjectRole,
        WorkspaceGraphEdge, WorkspaceGraphNode,
    };
    use kival_tests::{TestFixtureExt, TestKival, TestRawResponseExt, object_metadata, test_body};
    use uuid::Uuid;

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn graph_queries_reject_invalid_low_bounds(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("graph invalid bounds").await?;
        let object = r
            .create_object(
                workspace.id,
                "Graph Invalid Bounds Object",
                &test_body("Graph Invalid Bounds Object", "Body."),
                object_metadata("graph-invalid-bounds-object"),
            )
            .await?;

        for path in [
            format!("/workspaces/{}/objects/{}/graph?depth=-1", workspace.id, object.id,),
            format!("/workspaces/{}/objects/{}/graph?max_nodes=0", workspace.id, object.id,),
            format!("/workspaces/{}/objects/{}/graph?max_edges=-1", workspace.id, object.id,),
            format!("/workspaces/{}/graph?limit_nodes=0", workspace.id),
            format!("/workspaces/{}/graph?limit_edges=-1", workspace.id),
        ] {
            let response = r.request(Some(&r.admin), Method::GET, &path, None).await?;
            response.assert_status(StatusCode::BAD_REQUEST);
        }

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_graph_returns_bounded_visible_neighborhood(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("object graph").await?;
        let workspace = space.workspace;

        let upstream = r
            .create_object(
                workspace.id,
                "Upstream",
                &test_body("Upstream", "Upstream body."),
                object_metadata("upstream"),
            )
            .await?;

        let root = r
            .create_object(
                workspace.id,
                "Root",
                &test_body("Root", "Root body."),
                object_metadata("root"),
            )
            .await?;

        let downstream = r
            .create_object(
                workspace.id,
                "Downstream",
                &test_body("Downstream", "Downstream body."),
                object_metadata("downstream"),
            )
            .await?;

        r.create_edge(workspace.id, upstream.id, root.id).await?;
        r.create_edge(workspace.id, root.id, downstream.id).await?;
        r.update_object(workspace.id, root.id, Some("Root Renamed"), None, None).await?;

        let graph = r
            .object_graph_as(
                &r.admin,
                workspace.id,
                root.id,
                "depth=1&direction=both&max_nodes=10&max_edges=10",
            )
            .await?;

        assert_eq!(graph.workspace_id, workspace.id);
        assert_eq!(graph.root_object_id, root.id);
        assert_eq!(graph.depth, 1);
        assert!(!graph.truncated);

        assert_node(&graph.nodes, root.id, 0);
        assert_eq!(
            graph.nodes.iter().find(|node| node.id == root.id).expect("root node").title,
            "Root Renamed",
        );
        assert_node(&graph.nodes, upstream.id, 1);
        assert_node(&graph.nodes, downstream.id, 1);

        assert_edge(&graph.edges, upstream.id, root.id);
        assert_edge(&graph.edges, root.id, downstream.id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn graphs_project_current_wikilinks_and_merge_explicit_relationships(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("wikilink graph").await?;

        let target = r
            .create_object(
                workspace.id,
                "Graph Wiki Target",
                &test_body("Graph Wiki Target", "Target body."),
                object_metadata("graph-wiki-target"),
            )
            .await?;
        let source = r
            .create_object(
                workspace.id,
                "Graph Wiki Source",
                &test_body(
                    "Graph Wiki Source",
                    "See [[Graph Wiki Target]] and [[Graph Wiki Target|the target]] again.",
                ),
                object_metadata("graph-wiki-source"),
            )
            .await?;

        let object_graph = r
            .object_graph_as(
                &r.admin,
                workspace.id,
                source.id,
                "depth=1&direction=both&max_nodes=10&max_edges=10",
            )
            .await?;
        assert_node(&object_graph.nodes, target.id, 1);
        assert_edge_kind(
            &object_graph.edges,
            source.id,
            target.id,
            GraphEdgeKind::Wikilink,
        );
        assert_eq!(
            object_graph
                .edges
                .iter()
                .filter(|edge| {
                    edge.source_object_id == source.id && edge.target_object_id == target.id
                })
                .count(),
            1,
            "repeated wikilinks should project as one graph connection",
        );

        let workspace_graph =
            r.workspace_graph_as(&r.admin, workspace.id, "limit_nodes=50&limit_edges=50").await?;
        assert_workspace_edge_kind(
            &workspace_graph.edges,
            source.id,
            target.id,
            GraphEdgeKind::Wikilink,
        );
        assert_eq!(
            workspace_graph
                .nodes
                .iter()
                .find(|node| node.id == source.id)
                .expect("source node")
                .out_degree,
            1,
        );
        assert_eq!(
            workspace_graph
                .nodes
                .iter()
                .find(|node| node.id == target.id)
                .expect("target node")
                .in_degree,
            1,
        );

        r.create_edge(workspace.id, source.id, target.id).await?;
        let merged =
            r.workspace_graph_as(&r.admin, workspace.id, "limit_nodes=50&limit_edges=50").await?;
        assert_workspace_edge_kind(
            &merged.edges,
            source.id,
            target.id,
            GraphEdgeKind::RelationshipAndWikilink,
        );
        assert_eq!(
            merged
                .edges
                .iter()
                .filter(|edge| {
                    edge.source_object_id == source.id && edge.target_object_id == target.id
                })
                .count(),
            1,
            "relationship and wikilink should merge into one projected connection",
        );

        r.update_object(
            workspace.id,
            source.id,
            None,
            Some("The current version no longer links to the target."),
            None,
        )
        .await?;
        let relationship_only =
            r.workspace_graph_as(&r.admin, workspace.id, "limit_nodes=50&limit_edges=50").await?;
        assert_workspace_edge_kind(
            &relationship_only.edges,
            source.id,
            target.id,
            GraphEdgeKind::Relationship,
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_graph_hides_unreadable_neighbors_and_edges(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let workspace = r.create_workspace("object graph auth").await?;

        let visible_group = r.create_group("visible graph editors").await?;
        let hidden_group = r.create_group("hidden graph editors").await?;

        r.add_group_to_workspace(workspace.id, visible_group.id).await?;
        r.add_group_to_workspace(workspace.id, hidden_group.id).await?;

        let reader = r.create_user("object-graph-reader").await?;

        r.add_user_to_workspace(workspace.id, reader.id, MembershipRole::Member).await?;
        r.add_user_to_group(visible_group.id, reader.id, MembershipRole::Member).await?;

        let root = r
            .create_object(
                workspace.id,
                "Readable Root",
                &test_body("Readable Root", "Root body."),
                object_metadata("root"),
            )
            .await?;

        let visible_neighbor = r
            .create_object(
                workspace.id,
                "Readable Neighbor",
                &test_body("Readable Neighbor", "Visible neighbor body."),
                object_metadata("visible-neighbor"),
            )
            .await?;

        let hidden_neighbor = r
            .create_object(
                workspace.id,
                "Hidden Neighbor",
                &test_body("Hidden Neighbor", "Hidden neighbor body."),
                object_metadata("hidden-neighbor"),
            )
            .await?;

        r.update_object(
            workspace.id,
            root.id,
            None,
            Some("Root body links to [[Hidden Neighbor]]."),
            None,
        )
        .await?;

        r.create_object_grant(
            workspace.id,
            root.id,
            GrantPrincipal::Group(visible_group.id),
            ObjectRole::Viewer,
        )
        .await?;

        r.create_object_grant(
            workspace.id,
            visible_neighbor.id,
            GrantPrincipal::Group(visible_group.id),
            ObjectRole::Viewer,
        )
        .await?;

        r.create_edge(workspace.id, root.id, visible_neighbor.id).await?;
        r.create_edge(workspace.id, hidden_neighbor.id, root.id).await?;

        let admin_graph = r
            .object_graph_as(
                &r.admin,
                workspace.id,
                root.id,
                "depth=1&direction=both&max_nodes=10&max_edges=10",
            )
            .await?;

        assert!(
            has_node(&admin_graph.nodes, hidden_neighbor.id),
            "admin should see the hidden neighbor",
        );
        assert_edge(&admin_graph.edges, hidden_neighbor.id, root.id);
        assert_edge_kind(
            &admin_graph.edges,
            root.id,
            hidden_neighbor.id,
            GraphEdgeKind::Wikilink,
        );

        let reader_graph = r
            .object_graph_as(
                &reader,
                workspace.id,
                root.id,
                "depth=1&direction=both&max_nodes=10&max_edges=10",
            )
            .await?;

        assert_node(&reader_graph.nodes, root.id, 0);
        assert_node(&reader_graph.nodes, visible_neighbor.id, 1);

        assert!(
            !has_node(&reader_graph.nodes, hidden_neighbor.id),
            "reader should not see unreadable neighbor node",
        );

        assert_edge(&reader_graph.edges, root.id, visible_neighbor.id);

        assert!(
            !has_edge(&reader_graph.edges, hidden_neighbor.id, root.id),
            "reader should not see edge from unreadable neighbor",
        );

        assert!(
            !has_edge(&reader_graph.edges, root.id, hidden_neighbor.id),
            "reader should not see wikilink connection to unreadable neighbor",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_graph_hides_unreadable_nodes_and_edges(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let workspace = r.create_workspace("workspace graph auth").await?;

        let visible_group = r.create_group("visible workspace graph editors").await?;
        let hidden_group = r.create_group("hidden workspace graph editors").await?;

        r.add_group_to_workspace(workspace.id, visible_group.id).await?;
        r.add_group_to_workspace(workspace.id, hidden_group.id).await?;

        let reader = r.create_user("workspace-graph-reader").await?;

        r.add_user_to_workspace(workspace.id, reader.id, MembershipRole::Member).await?;
        r.add_user_to_group(visible_group.id, reader.id, MembershipRole::Member).await?;

        let visible_a = r
            .create_object(
                workspace.id,
                "Visible Graph A",
                &test_body("Visible Graph A", "Visible A body."),
                object_metadata("visible-a"),
            )
            .await?;

        let visible_b = r
            .create_object(
                workspace.id,
                "Visible Graph B",
                &test_body("Visible Graph B", "Visible B body."),
                object_metadata("visible-b"),
            )
            .await?;

        let visible_isolated = r
            .create_object(
                workspace.id,
                "Visible Isolated Node",
                &test_body("Visible Isolated Node", "Visible isolated body."),
                object_metadata("visible-isolated"),
            )
            .await?;

        let hidden = r
            .create_object(
                workspace.id,
                "Hidden Graph Node",
                &test_body("Hidden Graph Node", "Hidden body."),
                object_metadata("hidden"),
            )
            .await?;

        r.create_object_grant(
            workspace.id,
            visible_a.id,
            GrantPrincipal::Group(visible_group.id),
            ObjectRole::Viewer,
        )
        .await?;

        r.create_object_grant(
            workspace.id,
            visible_b.id,
            GrantPrincipal::Group(visible_group.id),
            ObjectRole::Viewer,
        )
        .await?;

        r.create_object_grant(
            workspace.id,
            visible_isolated.id,
            GrantPrincipal::Group(visible_group.id),
            ObjectRole::Viewer,
        )
        .await?;

        r.create_edge(workspace.id, visible_a.id, visible_b.id).await?;
        r.create_edge(workspace.id, hidden.id, visible_a.id).await?;

        let admin_graph =
            r.workspace_graph_as(&r.admin, workspace.id, "limit_nodes=50&limit_edges=50").await?;

        assert!(
            has_workspace_node(&admin_graph.nodes, hidden.id),
            "admin should see hidden node in workspace graph",
        );

        assert_workspace_edge(&admin_graph.edges, hidden.id, visible_a.id);

        let reader_graph =
            r.workspace_graph_as(&reader, workspace.id, "limit_nodes=50&limit_edges=50").await?;

        assert!(
            has_workspace_node(&reader_graph.nodes, visible_a.id),
            "reader should see visible graph node A",
        );
        assert!(
            has_workspace_node(&reader_graph.nodes, visible_b.id),
            "reader should see visible graph node B",
        );

        assert!(
            has_workspace_node(&reader_graph.nodes, visible_isolated.id),
            "workspace graph should include visible isolated nodes by default",
        );

        assert!(
            !has_workspace_node(&reader_graph.nodes, hidden.id),
            "reader should not see unreadable workspace graph node",
        );

        assert_workspace_edge(&reader_graph.edges, visible_a.id, visible_b.id);

        assert!(
            !has_workspace_edge(&reader_graph.edges, hidden.id, visible_a.id),
            "reader should not see workspace graph edge from unreadable node",
        );

        let reader_graph_without_isolated = r
            .workspace_graph_as(
                &reader,
                workspace.id,
                "limit_nodes=50&limit_edges=50&exclude_isolated=true",
            )
            .await?;

        assert!(
            !has_workspace_node(&reader_graph_without_isolated.nodes, visible_isolated.id),
            "exclude_isolated should omit nodes with no visible filtered relation",
        );

        Ok(())
    }

    fn assert_node(nodes: &[ObjectGraphNode], id: Uuid, distance: i32) {
        let node = nodes
            .iter()
            .find(|node| node.id == id)
            .expect("expected object graph node to be present");

        assert_eq!(node.distance, distance);
    }

    fn has_node(nodes: &[ObjectGraphNode], id: Uuid) -> bool {
        nodes.iter().any(|node| node.id == id)
    }

    fn assert_edge(edges: &[ObjectGraphEdge], source: Uuid, target: Uuid) {
        assert!(has_edge(edges, source, target), "expected object graph edge {source} -> {target}",);
    }

    fn has_edge(edges: &[ObjectGraphEdge], source: Uuid, target: Uuid) -> bool {
        edges.iter().any(|edge| edge.source_object_id == source && edge.target_object_id == target)
    }

    fn assert_edge_kind(
        edges: &[ObjectGraphEdge],
        source: Uuid,
        target: Uuid,
        kind: GraphEdgeKind,
    ) {
        let edge = edges
            .iter()
            .find(|edge| edge.source_object_id == source && edge.target_object_id == target)
            .expect("expected object graph edge");
        assert_eq!(edge.kind, kind);
    }

    fn has_workspace_node(nodes: &[WorkspaceGraphNode], id: Uuid) -> bool {
        nodes.iter().any(|node| node.id == id)
    }

    fn assert_workspace_edge(edges: &[WorkspaceGraphEdge], source: Uuid, target: Uuid) {
        assert!(
            has_workspace_edge(edges, source, target),
            "expected workspace graph edge {source} -> {target}",
        );
    }

    fn has_workspace_edge(edges: &[WorkspaceGraphEdge], source: Uuid, target: Uuid) -> bool {
        edges.iter().any(|edge| edge.source_object_id == source && edge.target_object_id == target)
    }

    fn assert_workspace_edge_kind(
        edges: &[WorkspaceGraphEdge],
        source: Uuid,
        target: Uuid,
        kind: GraphEdgeKind,
    ) {
        let edge = edges
            .iter()
            .find(|edge| edge.source_object_id == source && edge.target_object_id == target)
            .expect("expected workspace graph edge");
        assert_eq!(edge.kind, kind);
    }
}
