//! Object edge API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        CreateObjectEdgeRequest, GrantPrincipal, ListResponse, MembershipRole, ObjectEdge,
        ObjectEdgeResponse, ObjectRole,
    };
    use kival_tests::{
        TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt, object_metadata, test_body,
    };

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn edge_create_requires_source_edit_and_target_inspect(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("edge create permissions").await?;
        let actor = r
            .create_workspace_actor(space.workspace.id, "edge-create-actor", MembershipRole::Member)
            .await?;

        let source = r
            .create_object(
                space.workspace.id,
                "Edge Source",
                &test_body("Edge Source", "Source body."),
                object_metadata("edge-source"),
            )
            .await?;
        let target = r
            .create_object(
                space.workspace.id,
                "Edge Target",
                &test_body("Edge Target", "Target body."),
                object_metadata("edge-target"),
            )
            .await?;

        r.create_object_grant(
            space.workspace.id,
            target.id,
            GrantPrincipal::User(actor.id),
            ObjectRole::Viewer,
        )
        .await?;

        let missing_source_edit = r
            .request_json_raw_as(
                &actor,
                Method::POST,
                &format!("/workspaces/{}/edges", space.workspace.id),
                &CreateObjectEdgeRequest {
                    source_object_id: source.id,
                    target_object_id: target.id,
                },
            )
            .await?;
        missing_source_edit.assert_status(StatusCode::FORBIDDEN);

        r.create_object_grant(
            space.workspace.id,
            source.id,
            GrantPrincipal::User(actor.id),
            ObjectRole::Editor,
        )
        .await?;

        let ungranted_target = r
            .create_object(
                space.workspace.id,
                "Ungranted Edge Target",
                &test_body("Ungranted Edge Target", "Target body."),
                object_metadata("ungranted-edge-target"),
            )
            .await?;

        let missing_target_inspect = r
            .request_json_raw_as(
                &actor,
                Method::POST,
                &format!("/workspaces/{}/edges", space.workspace.id),
                &CreateObjectEdgeRequest {
                    source_object_id: source.id,
                    target_object_id: ungranted_target.id,
                },
            )
            .await?;
        missing_target_inspect.assert_status(StatusCode::FORBIDDEN);

        let created: ObjectEdgeResponse = r
            .request_json_as(
                &actor,
                Method::POST,
                &format!("/workspaces/{}/edges", space.workspace.id),
                &CreateObjectEdgeRequest {
                    source_object_id: source.id,
                    target_object_id: target.id,
                },
            )
            .await?
            .into_success()?;

        assert_eq!(created.edge.source_object_id, source.id);
        assert_eq!(created.edge.target_object_id, target.id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn edge_get_and_list_require_both_object_visibility(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("edge read permissions").await?;
        let reader = r
            .create_workspace_actor(space.workspace.id, "edge-read-actor", MembershipRole::Member)
            .await?;

        let source = r
            .create_object(
                space.workspace.id,
                "Readable Edge Source",
                &test_body("Readable Edge Source", "Source body."),
                object_metadata("readable-edge-source"),
            )
            .await?;
        let target = r
            .create_object(
                space.workspace.id,
                "Hidden Edge Target",
                &test_body("Hidden Edge Target", "Target body."),
                object_metadata("hidden-edge-target"),
            )
            .await?;
        let edge = r.create_edge(space.workspace.id, source.id, target.id).await?;

        r.create_object_grant(
            space.workspace.id,
            source.id,
            GrantPrincipal::User(reader.id),
            ObjectRole::Viewer,
        )
        .await?;

        let listed: ListResponse<ObjectEdge> = r
            .get_json_as(
                &reader,
                &format!("/workspaces/{}/objects/{}/edges", space.workspace.id, source.id),
            )
            .await?
            .into_success()?;
        assert!(!listed.items.iter().any(|item| item.id == edge.id));

        let hidden_get = r
            .request(
                Some(&reader),
                Method::GET,
                &format!("/workspaces/{}/edges/{}", space.workspace.id, edge.id),
                None,
            )
            .await?;
        hidden_get.assert_status(StatusCode::FORBIDDEN);

        r.create_object_grant(
            space.workspace.id,
            target.id,
            GrantPrincipal::User(reader.id),
            ObjectRole::Viewer,
        )
        .await?;

        let visible: ObjectEdgeResponse = r
            .get_json_as(&reader, &format!("/workspaces/{}/edges/{}", space.workspace.id, edge.id))
            .await?
            .into_success()?;
        assert_eq!(visible.edge.id, edge.id);

        let listed: ListResponse<ObjectEdge> = r
            .get_json_as(
                &reader,
                &format!("/workspaces/{}/objects/{}/edges", space.workspace.id, source.id),
            )
            .await?
            .into_success()?;
        assert!(listed.items.iter().any(|item| item.id == edge.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn edge_get_preserves_source_then_target_authorization_order(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("edge read authorization order").await?;
        let reader = r
            .create_workspace_actor(
                space.workspace.id,
                "edge-read-order-reader",
                MembershipRole::Member,
            )
            .await?;

        let source = r
            .create_object(
                space.workspace.id,
                "Authorization Order Source",
                &test_body("Authorization Order Source", "Source body."),
                object_metadata("authorization-order-source"),
            )
            .await?;
        let target = r
            .create_object(
                space.workspace.id,
                "Authorization Order Target",
                &test_body("Authorization Order Target", "Target body."),
                object_metadata("authorization-order-target"),
            )
            .await?;
        let edge = r.create_edge(space.workspace.id, source.id, target.id).await?;
        r.archive_object(space.workspace.id, target.id).await?;

        let hidden_source = r
            .request(
                Some(&reader),
                Method::GET,
                &format!("/workspaces/{}/edges/{}", space.workspace.id, edge.id),
                None,
            )
            .await?;
        hidden_source.assert_status(StatusCode::FORBIDDEN);

        r.create_object_grant(
            space.workspace.id,
            source.id,
            GrantPrincipal::User(reader.id),
            ObjectRole::Viewer,
        )
        .await?;

        let archived_target = r
            .request(
                Some(&reader),
                Method::GET,
                &format!("/workspaces/{}/edges/{}", space.workspace.id, edge.id),
                None,
            )
            .await?;
        archived_target.assert_status(StatusCode::NOT_FOUND);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn source_editor_can_revoke_edge_but_viewer_cannot(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("edge revoke permissions").await?;
        let source_editor = r
            .create_workspace_actor(
                space.workspace.id,
                "edge-source-editor",
                MembershipRole::Member,
            )
            .await?;
        let source_viewer = r
            .create_workspace_actor(
                space.workspace.id,
                "edge-source-viewer",
                MembershipRole::Member,
            )
            .await?;
        let source_editor_without_target = r
            .create_workspace_actor(
                space.workspace.id,
                "edge-source-editor-without-target-view",
                MembershipRole::Member,
            )
            .await?;

        let source = r
            .create_object(
                space.workspace.id,
                "Revokable Edge Source",
                &test_body("Revokable Edge Source", "Source body."),
                object_metadata("revokable-edge-source"),
            )
            .await?;
        let target = r
            .create_object(
                space.workspace.id,
                "Revokable Edge Target",
                &test_body("Revokable Edge Target", "Target body."),
                object_metadata("revokable-edge-target"),
            )
            .await?;
        let edge = r.create_edge(space.workspace.id, source.id, target.id).await?;

        r.create_object_grant(
            space.workspace.id,
            source.id,
            GrantPrincipal::User(source_editor.id),
            ObjectRole::Editor,
        )
        .await?;

        r.create_object_grant(
            space.workspace.id,
            source.id,
            GrantPrincipal::User(source_viewer.id),
            ObjectRole::Viewer,
        )
        .await?;
        r.create_object_grant(
            space.workspace.id,
            source.id,
            GrantPrincipal::User(source_editor_without_target.id),
            ObjectRole::Editor,
        )
        .await?;
        r.create_object_grant(
            space.workspace.id,
            target.id,
            GrantPrincipal::User(source_editor.id),
            ObjectRole::Viewer,
        )
        .await?;
        r.create_object_grant(
            space.workspace.id,
            target.id,
            GrantPrincipal::User(source_viewer.id),
            ObjectRole::Viewer,
        )
        .await?;

        let denied = r
            .request(
                Some(&source_viewer),
                Method::POST,
                &format!("/workspaces/{}/edges/{}/revoke", space.workspace.id, edge.id),
                None,
            )
            .await?;
        denied.assert_status(StatusCode::FORBIDDEN);

        let hidden_target = r
            .request(
                Some(&source_editor_without_target),
                Method::POST,
                &format!("/workspaces/{}/edges/{}/revoke", space.workspace.id, edge.id),
                None,
            )
            .await?;
        hidden_target.assert_status(StatusCode::FORBIDDEN);

        let revoked: ObjectEdgeResponse = r
            .empty_json_as(
                &source_editor,
                Method::POST,
                &format!("/workspaces/{}/edges/{}/revoke", space.workspace.id, edge.id),
            )
            .await?
            .into_success()?;
        assert_eq!(revoked.edge.id, edge.id);
        assert!(revoked.edge.revoked_at.is_some());

        let get_revoked = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!("/workspaces/{}/edges/{}", space.workspace.id, edge.id),
                None,
            )
            .await?;
        get_revoked.assert_status(StatusCode::NOT_FOUND);

        Ok(())
    }
}
