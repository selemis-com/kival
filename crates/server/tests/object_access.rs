//! Object access API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        ArchiveStatus, CreateObjectRequest, GrantPrincipal, MembershipRole, ObjectResponse,
        ObjectRole, UpdateObjectRequest,
    };
    use kival_tests::{
        TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt, object_metadata, test_body,
    };
    use serde_json::json;

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn viewer_can_get_object_but_cannot_update(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("object access viewer").await?;
        let workspace = space.workspace;
        let group = space.group;

        let viewer = r.create_user("object-viewer").await?;

        r.add_user_to_workspace(workspace.id, viewer.id, MembershipRole::Member).await?;
        r.add_user_to_group(group.id, viewer.id, MembershipRole::Member).await?;

        let object = r
            .create_object(
                workspace.id,
                "Viewer Object",
                &test_body("Viewer Object", "Viewer-readable body."),
                object_metadata("viewer-object"),
            )
            .await?;

        r.create_object_grant(
            workspace.id,
            object.id,
            GrantPrincipal::Group(group.id),
            ObjectRole::Viewer,
        )
        .await?;

        let fetched = r.get_object_as(&viewer, workspace.id, object.id).await?;
        assert_eq!(fetched.id, object.id);

        let body = UpdateObjectRequest {
            expected_current_version_id: object
                .current_version_id
                .expect("created object has current version"),
            title: Some("Viewer Edited Title".to_owned()),
            body: None,
            metadata: None,
        };

        let response = r
            .request(
                Some(&viewer),
                Method::PATCH,
                &format!("/workspaces/{}/objects/{}", workspace.id, object.id),
                Some(json!(body)),
            )
            .await?;

        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn editor_can_update_object(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("object access editor").await?;
        let workspace = space.workspace;
        let group = space.group;

        let editor = r.create_user("object-editor").await?;

        r.add_user_to_workspace(workspace.id, editor.id, MembershipRole::Member).await?;
        r.add_user_to_group(group.id, editor.id, MembershipRole::Member).await?;

        let object = r
            .create_object(
                workspace.id,
                "Editor Object",
                &test_body("Editor Object", "Original body."),
                object_metadata("editor-object-v1"),
            )
            .await?;

        r.create_object_grant(
            workspace.id,
            object.id,
            GrantPrincipal::Group(group.id),
            ObjectRole::Editor,
        )
        .await?;

        let updated_body = test_body("Editor Object v2", "Updated by editor.");
        let updated = r
            .update_object_as(
                &editor,
                workspace.id,
                object.id,
                Some("Editor Object v2"),
                Some(&updated_body),
                Some(object_metadata("editor-object-v2")),
            )
            .await?;
        let version =
            updated.current_version.expect("updated object should have a current version");

        assert_eq!(version.object_id, object.id);
        assert_eq!(version.title, "Editor Object v2");

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_member_without_object_grant_cannot_get_object(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let workspace = r.create_workspace("object access no grant").await?;
        let unrelated_group = r.create_group("unrelated group").await?;

        r.add_group_to_workspace(workspace.id, unrelated_group.id).await?;

        let member = r.create_user("object-no-grant-member").await?;

        r.add_user_to_workspace(workspace.id, member.id, MembershipRole::Member).await?;
        r.add_user_to_group(unrelated_group.id, member.id, MembershipRole::Member).await?;

        let object = r
            .create_object(
                workspace.id,
                "Private Object",
                &test_body("Private Object", "Private body."),
                object_metadata("private-object"),
            )
            .await?;

        let response = r
            .request(
                Some(&member),
                Method::GET,
                &format!("/workspaces/{}/objects/{}", workspace.id, object.id),
                None,
            )
            .await?;

        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_admin_can_get_object_without_explicit_object_grant(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("object access workspace admin").await?;
        let workspace = space.workspace;

        let workspace_admin = r.create_user("workspace-admin").await?;

        r.add_user_to_workspace(workspace.id, workspace_admin.id, MembershipRole::Admin).await?;

        let object = r
            .create_object(
                workspace.id,
                "Workspace Admin Object",
                &test_body("Workspace Admin Object", "Body."),
                object_metadata("workspace-admin-object"),
            )
            .await?;

        let fetched = r.get_object_as(&workspace_admin, workspace.id, object.id).await?;

        assert_eq!(fetched.id, object.id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_get_returns_archived_object_to_admin(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("object access archived").await?;
        let workspace = space.workspace;

        let object = r
            .create_object(
                workspace.id,
                "Archived Object",
                &test_body("Archived Object", "Body."),
                object_metadata("archived-object"),
            )
            .await?;

        let before_archive = r.get_object_as(&r.admin, workspace.id, object.id).await?;
        assert_eq!(before_archive.id, object.id);
        assert_eq!(before_archive.status, ArchiveStatus::Active);

        r.archive_object(workspace.id, object.id).await?;

        let archived = r.get_object_as(&r.admin, workspace.id, object.id).await?;

        assert_eq!(archived.id, object.id);
        assert_eq!(archived.status, ArchiveStatus::Archived);
        assert!(archived.archived_at.is_some());

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn explicit_object_admin_can_get_archived_object_by_id(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("archived explicit object admin").await?;
        let object_admin = r.create_user("archived-object-admin").await?;
        r.add_user_to_workspace(space.workspace.id, object_admin.id, MembershipRole::Member)
            .await?;
        r.add_user_to_group(space.group.id, object_admin.id, MembershipRole::Member).await?;

        let object = r
            .create_object(
                space.workspace.id,
                "Archived Admin Object",
                &test_body("Archived Admin Object", "Body."),
                object_metadata("archived-admin-object"),
            )
            .await?;
        r.create_object_grant(
            space.workspace.id,
            object.id,
            GrantPrincipal::Group(space.group.id),
            ObjectRole::Admin,
        )
        .await?;
        r.archive_object(space.workspace.id, object.id).await?;

        let archived = r.get_object_as(&object_admin, space.workspace.id, object.id).await?;
        assert_eq!(archived.id, object.id);
        assert_eq!(archived.status, ArchiveStatus::Archived);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn explicit_object_editor_cannot_get_archived_object_by_id(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("archived explicit object editor").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Archived Editor Object",
                &test_body("Archived Editor Object", "Body."),
                object_metadata("archived-editor-object"),
            )
            .await?;
        let object_editor = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "archived-object-editor",
                MembershipRole::Member,
                ObjectRole::Editor,
            )
            .await?;
        r.archive_object(space.workspace.id, object.id).await?;

        let response = r
            .request(
                Some(&object_editor),
                Method::GET,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
                None,
            )
            .await?;
        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_editor_can_update_object_title(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object update editor").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Original Object Title",
                &test_body("Original Object Title", "Body."),
                object_metadata("object-update-editor"),
            )
            .await?;
        let object_editor = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "object-update-editor",
                MembershipRole::Member,
                ObjectRole::Editor,
            )
            .await?;

        let updated = r
            .update_object_title_as(
                &object_editor,
                space.workspace.id,
                object.id,
                "Updated Object Title",
            )
            .await?;

        assert_eq!(updated.title, "Updated Object Title");

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_create_rejects_nested_metadata(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("object create nested metadata").await?;

        let response = r
            .request_json_raw_as(
                &r.admin,
                Method::POST,
                &format!("/workspaces/{}/objects", workspace.id),
                &CreateObjectRequest {
                    title: "Nested Metadata Object".to_owned(),
                    body: "Body".to_owned(),
                    metadata: json!({ "config": { "enabled": true } }),
                },
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_update_rejects_nested_metadata(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object update nested metadata").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Flat Metadata Object",
                &test_body("Flat Metadata Object", "Body."),
                object_metadata("flat-metadata-object"),
            )
            .await?;

        let response = r
            .request_json_raw_as(
                &r.admin,
                Method::PATCH,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
                &UpdateObjectRequest {
                    expected_current_version_id: object
                        .current_version_id
                        .expect("created object has current version"),
                    title: None,
                    body: None,
                    metadata: Some(json!({ "matrix": [[1, 2], [3, 4]] })),
                },
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_update_requires_expected_current_version(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object update expected version required").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Expected Version Object",
                &test_body("Expected Version Object", "Body."),
                object_metadata("expected-version-object"),
            )
            .await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::PATCH,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
                Some(json!({ "title": "Missing expected version" })),
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_update_rejects_empty_patch(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object update empty").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Empty Patch Object",
                &test_body("Empty Patch Object", "Body."),
                object_metadata("object-update-empty"),
            )
            .await?;

        let response = r
            .request_json_raw_as(
                &r.admin,
                Method::PATCH,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
                &UpdateObjectRequest {
                    expected_current_version_id: object
                        .current_version_id
                        .expect("created object has current version"),
                    title: None,
                    body: None,
                    metadata: None,
                },
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_update_rejects_explicit_null_fields(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object update explicit null").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Explicit Null Object",
                &test_body("Explicit Null Object", "Body."),
                object_metadata("object-update-explicit-null"),
            )
            .await?;

        let expected_current_version_id =
            object.current_version_id.expect("created object has current version");
        for request in [
            json!({
                "expected_current_version_id": expected_current_version_id,
                "title": null,
            }),
            json!({
                "expected_current_version_id": expected_current_version_id,
                "body": null,
            }),
            json!({
                "expected_current_version_id": expected_current_version_id,
                "metadata": null,
            }),
        ] {
            let response = r
                .request_json_raw_as(
                    &r.admin,
                    Method::PATCH,
                    &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
                    &request,
                )
                .await?;

            response.assert_status(StatusCode::BAD_REQUEST);
        }

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_update_rejects_archived_object(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object update archived").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Archived Update Object",
                &test_body("Archived Update Object", "Body."),
                object_metadata("object-update-archived"),
            )
            .await?;
        r.archive_object(space.workspace.id, object.id).await?;

        let response = r
            .request_json_raw_as(
                &r.admin,
                Method::PATCH,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
                &UpdateObjectRequest {
                    expected_current_version_id: object
                        .current_version_id
                        .expect("created object has current version"),
                    title: Some("Forbidden Archived Update".to_owned()),
                    body: None,
                    metadata: None,
                },
            )
            .await?;

        response.assert_status(StatusCode::NOT_FOUND);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_admin_can_unarchive_object(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object unarchive workspace admin").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Unarchive Object",
                &test_body("Unarchive Object", "Body."),
                object_metadata("object-unarchive"),
            )
            .await?;
        let workspace_admin = r
            .create_workspace_actor(
                space.workspace.id,
                "object-unarchive-admin",
                MembershipRole::Admin,
            )
            .await?;
        r.archive_object(space.workspace.id, object.id).await?;

        let response: ObjectResponse = r
            .empty_json_as(
                &workspace_admin,
                Method::POST,
                &format!("/workspaces/{}/objects/{}/unarchive", space.workspace.id, object.id),
            )
            .await?
            .into_success()?;

        assert_eq!(response.object.status, ArchiveStatus::Active);
        assert!(response.object.archived_at.is_none());

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_admin_can_unarchive_object(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object unarchive object admin").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Object Admin Unarchive",
                &test_body("Object Admin Unarchive", "Body."),
                object_metadata("object-unarchive-admin"),
            )
            .await?;
        let object_admin = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "object-unarchive-object-admin",
                MembershipRole::Member,
                ObjectRole::Admin,
            )
            .await?;
        r.archive_object(space.workspace.id, object.id).await?;

        let response: ObjectResponse = r
            .empty_json_as(
                &object_admin,
                Method::POST,
                &format!("/workspaces/{}/objects/{}/unarchive", space.workspace.id, object.id),
            )
            .await?
            .into_success()?;

        assert_eq!(response.object.status, ArchiveStatus::Active);
        assert!(response.object.archived_at.is_none());

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn creator_can_archive_and_unarchive_own_object(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("creator object lifecycle").await?;
        let creator = r
            .create_workspace_actor(
                space.workspace.id,
                "object-lifecycle-creator",
                MembershipRole::Member,
            )
            .await?;

        let created: ObjectResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                &format!("/workspaces/{}/objects", space.workspace.id),
                &CreateObjectRequest {
                    title: "Creator Lifecycle Object".to_owned(),
                    body: test_body("Creator Lifecycle Object", "Body."),
                    metadata: object_metadata("creator-object-lifecycle"),
                },
            )
            .await?
            .into_success()?;

        let archived: ObjectResponse = r
            .empty_json_as(
                &creator,
                Method::POST,
                &format!(
                    "/workspaces/{}/objects/{}/archive",
                    space.workspace.id, created.object.id
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(archived.object.status, ArchiveStatus::Archived);
        assert!(archived.object.archived_at.is_some());

        let unarchived: ObjectResponse = r
            .empty_json_as(
                &creator,
                Method::POST,
                &format!(
                    "/workspaces/{}/objects/{}/unarchive",
                    space.workspace.id, created.object.id
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(unarchived.object.status, ArchiveStatus::Active);
        assert!(unarchived.object.archived_at.is_none());

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn unarchive_object_re_resolves_backlinks(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object unarchive backlinks").await?;
        let target = r
            .create_object(
                space.workspace.id,
                "Recovered Target",
                &test_body("Recovered Target", "Body."),
                object_metadata("object-unarchive-target"),
            )
            .await?;
        r.archive_object(space.workspace.id, target.id).await?;

        let source = r
            .create_object(
                space.workspace.id,
                "Recovered Source",
                &test_body("Recovered Source", "This references [[Recovered Target]]."),
                object_metadata("object-unarchive-source"),
            )
            .await?;

        let before = r.backlinks_as(&r.admin, space.workspace.id, target.id).await?;
        assert!(
            before
                .incoming_references
                .iter()
                .all(|reference| reference.source_object.id != source.id),
            "archived target should not receive newly resolved wikilinks",
        );

        r.unarchive_object(space.workspace.id, target.id).await?;

        let after = r.backlinks_as(&r.admin, space.workspace.id, target.id).await?;
        assert!(
            after
                .incoming_references
                .iter()
                .any(|reference| reference.source_object.id == source.id),
            "unarchiving should re-resolve current wikilinks pointing at the target title",
        );

        Ok(())
    }
}
