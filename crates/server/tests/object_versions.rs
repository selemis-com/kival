//! Object version API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        Event, ListResponse, MembershipRole, ObjectResponse, ObjectRole, ObjectVersion,
        ObjectVersionResponse, ObjectVersionWikilinksResponse, UpdateObjectRequest,
    };
    use kival_tests::{
        TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt, object_metadata, test_body,
    };

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_version_wikilinks_return_resolved_and_unresolved_targets(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("version wikilinks").await?;
        let target = r
            .create_object(
                space.workspace.id,
                "Wikilink Target",
                &test_body("Wikilink Target", "Target body."),
                object_metadata("wikilink-target"),
            )
            .await?;
        let source = r
            .create_object(
                space.workspace.id,
                "Wikilink Source",
                "See [[Wikilink Target]], [[Wikilink Target|the target]], and [[Missing Target]].",
                object_metadata("wikilink-source"),
            )
            .await?;
        let source_response: ObjectResponse = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, source.id),
            )
            .await?
            .into_success()?;
        let version =
            source_response.current_version.expect("created object should have a current version");

        let by_id: ObjectVersionWikilinksResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/versions/{}/wikilinks",
                    space.workspace.id, source.id, version.id,
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(by_id.items.len(), 3);
        assert_eq!(by_id.items[0].raw_target, "Wikilink Target");
        assert_eq!(by_id.items[0].display_text, None);
        assert_eq!(by_id.items[0].target_object_id, Some(target.id));
        assert_eq!(by_id.items[1].raw_target, "Wikilink Target");
        assert_eq!(by_id.items[1].display_text.as_deref(), Some("the target"));
        assert_eq!(by_id.items[1].target_object_id, Some(target.id));
        assert_eq!(by_id.items[2].raw_target, "Missing Target");
        assert_eq!(by_id.items[2].display_text, None);
        assert_eq!(by_id.items[2].target_object_id, None);

        let by_number: ObjectVersionWikilinksResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/versions/1/wikilinks",
                    space.workspace.id, source.id,
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(by_number, by_id);

        let source_reader = r
            .create_object_actor(
                space.workspace.id,
                source.id,
                "wikilink-source-reader",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        let masked: ObjectVersionWikilinksResponse = r
            .get_json_as(
                &source_reader,
                &format!(
                    "/workspaces/{}/objects/{}/versions/{}/wikilinks",
                    space.workspace.id, source.id, version.id,
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(masked.items.len(), 3);
        assert!(masked.items.iter().all(|reference| reference.target_object_id.is_none()));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn archived_object_admin_can_list_and_get_versions(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("archived version reads").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Archived Version Object",
                &test_body("Archived Version Object", "Version one."),
                object_metadata("archived-version-object-v1"),
            )
            .await?;
        let second_body = test_body("Archived Version Object v2", "Version two.");
        let second = r
            .update_object(
                space.workspace.id,
                object.id,
                Some("Archived Version Object v2"),
                Some(&second_body),
                Some(object_metadata("archived-version-object-v2")),
            )
            .await?
            .current_version
            .expect("updated object should have a current version");
        let object_admin = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "archived-version-admin",
                MembershipRole::Member,
                ObjectRole::Admin,
            )
            .await?;
        r.archive_object(space.workspace.id, object.id).await?;

        let listed: ListResponse<ObjectVersion> = r
            .get_json_as(
                &object_admin,
                &format!(
                    "/workspaces/{}/objects/{}/versions?limit=10",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;
        assert!(listed.items.iter().any(|version| version.id == second.id));

        let fetched: ObjectVersionResponse = r
            .get_json_as(
                &object_admin,
                &format!(
                    "/workspaces/{}/objects/{}/versions/{}",
                    space.workspace.id, object.id, second.id,
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(fetched.version.id, second.id);
        assert_eq!(fetched.version.object_id, object.id);
        assert_eq!(fetched.version.title, "Archived Version Object v2");
        assert_eq!(fetched.version.created_by_username.as_deref(), Some(r.admin.username.as_str()));
        assert!(fetched.version.created_by_display_name.is_some());
        assert_eq!(fetched.version.created_by_workspace_role, Some(MembershipRole::Admin));
        assert_eq!(fetched.version.created_by_object_role, Some(ObjectRole::Admin));
        assert!(listed.items.iter().all(|version| version.created_by_username.is_some()));

        let fetched_by_number: ObjectVersionResponse = r
            .get_json_as(
                &object_admin,
                &format!("/workspaces/{}/objects/{}/versions/2", space.workspace.id, object.id,),
            )
            .await?
            .into_success()?;
        assert_eq!(fetched_by_number.version, fetched.version);

        let invalid_number = r
            .request(
                Some(&object_admin),
                Method::GET,
                &format!("/workspaces/{}/objects/{}/versions/0", space.workspace.id, object.id,),
                None,
            )
            .await?;
        invalid_number.assert_status(StatusCode::BAD_REQUEST);

        let invalid_identifier = r
            .request(
                Some(&object_admin),
                Method::GET,
                &format!(
                    "/workspaces/{}/objects/{}/versions/not-a-version",
                    space.workspace.id, object.id,
                ),
                None,
            )
            .await?;
        invalid_identifier.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn archived_object_editor_cannot_list_or_get_versions(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("archived version editor denial").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Archived Version Editor Object",
                &test_body("Archived Version Editor Object", "Body."),
                object_metadata("archived-version-editor-object"),
            )
            .await?;
        let object_editor = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "archived-version-editor",
                MembershipRole::Member,
                ObjectRole::Editor,
            )
            .await?;
        r.archive_object(space.workspace.id, object.id).await?;

        let list_response = r
            .request(
                Some(&object_editor),
                Method::GET,
                &format!("/workspaces/{}/objects/{}/versions", space.workspace.id, object.id),
                None,
            )
            .await?;
        list_response.assert_status(StatusCode::FORBIDDEN);

        let version_id = object.current_version_id.expect("object should have initial version");
        let get_response = r
            .request(
                Some(&object_editor),
                Method::GET,
                &format!(
                    "/workspaces/{}/objects/{}/versions/{}",
                    space.workspace.id, object.id, version_id,
                ),
                None,
            )
            .await?;
        get_response.assert_status(StatusCode::FORBIDDEN);

        let get_by_number_response = r
            .request(
                Some(&object_editor),
                Method::GET,
                &format!("/workspaces/{}/objects/{}/versions/1", space.workspace.id, object.id,),
                None,
            )
            .await?;
        get_by_number_response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_versions_endpoint_is_read_only(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("version mutation endpoint removed").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Read-only Version History",
                &test_body("Read-only Version History", "Body."),
                object_metadata("read-only-version-history"),
            )
            .await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::POST,
                &format!("/workspaces/{}/objects/{}/versions", space.workspace.id, object.id),
                None,
            )
            .await?;
        response.assert_status(StatusCode::METHOD_NOT_ALLOWED);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_update_rejects_stale_expected_current_version(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("stale object update").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Stale Update Object",
                &test_body("Stale Update Object", "Version one."),
                object_metadata("stale-update-v1"),
            )
            .await?;

        let initial: ObjectResponse = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
            )
            .await?
            .into_success()?;
        let initial_version =
            initial.current_version.expect("created object should have a current version");

        let newer: ObjectResponse = r
            .request_json_as(
                &r.admin,
                Method::PATCH,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
                &UpdateObjectRequest {
                    expected_current_version_id: initial_version.id,
                    title: Some("Stale Update Object v2".to_owned()),
                    body: None,
                    metadata: None,
                },
            )
            .await?
            .into_success()?;
        let newer_version =
            newer.current_version.expect("matching expected version should update the object");

        let response = r
            .request_json_raw_as(
                &r.admin,
                Method::PATCH,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
                &UpdateObjectRequest {
                    expected_current_version_id: initial_version.id,
                    title: None,
                    body: Some("stale replacement".to_owned()),
                    metadata: None,
                },
            )
            .await?;
        response.assert_status(StatusCode::CONFLICT);

        let current: ObjectResponse = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
            )
            .await?
            .into_success()?;
        let current_version =
            current.current_version.expect("object should retain a current version");
        assert_eq!(current_version.id, newer_version.id);
        assert_ne!(current_version.body, "stale replacement");

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_update_noop_keeps_version_timestamp_and_events(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("no-op object update").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "No-op Object",
                &test_body("No-op Object", "Unchanged body."),
                object_metadata("no-op-object"),
            )
            .await?;

        let before: ObjectResponse = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
            )
            .await?
            .into_success()?;
        let before_version =
            before.current_version.as_ref().expect("created object should have a current version");

        let before_updated_events: ListResponse<Event> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/events?event_kind=object.updated&limit=20",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;
        let before_version_events: ListResponse<Event> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/events?event_kind=object.version_appended&limit=20",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;

        let unchanged: ObjectResponse = r
            .request_json_as(
                &r.admin,
                Method::PATCH,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
                &UpdateObjectRequest {
                    expected_current_version_id: before_version.id,
                    title: Some(before_version.title.clone()),
                    body: None,
                    metadata: None,
                },
            )
            .await?
            .into_success()?;

        assert_eq!(unchanged.object.current_version_id, Some(before_version.id));
        assert_eq!(unchanged.object.updated_at, before.object.updated_at);
        assert_eq!(
            unchanged.current_version.as_ref().map(|version| version.id),
            Some(before_version.id),
        );

        let versions: ListResponse<ObjectVersion> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/versions?limit=20",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(versions.items.len(), 1);

        let after_updated_events: ListResponse<Event> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/events?event_kind=object.updated&limit=20",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;
        let after_version_events: ListResponse<Event> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/events?event_kind=object.version_appended&limit=20",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;

        assert_eq!(after_updated_events.items.len(), before_updated_events.items.len());
        assert_eq!(after_version_events.items.len(), before_version_events.items.len());

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_update_creates_new_current_version_and_inherits_omitted_fields(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("versioned object update").await?;
        let original_body = test_body("Versioned Update Object", "Original body.");
        let original_metadata = object_metadata("versioned-update-original");
        let object = r
            .create_object(
                space.workspace.id,
                "Versioned Update Object",
                &original_body,
                original_metadata.clone(),
            )
            .await?;

        let initial: ObjectResponse = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
            )
            .await?
            .into_success()?;
        let initial_version =
            initial.current_version.expect("created object should have a current version");

        let renamed = r
            .update_object(
                space.workspace.id,
                object.id,
                Some("Versioned Update Object Renamed"),
                None,
                None,
            )
            .await?;
        let renamed_version =
            renamed.current_version.expect("updated object should have a current version");

        assert_ne!(renamed_version.id, initial_version.id);
        assert_eq!(renamed_version.version_number, initial_version.version_number + 1);
        assert_eq!(renamed.object.current_version_id, Some(renamed_version.id));
        assert_eq!(renamed.object.title, "Versioned Update Object Renamed");
        assert_eq!(renamed_version.title, "Versioned Update Object Renamed");
        assert_eq!(renamed_version.body, original_body);
        assert_eq!(renamed_version.metadata, original_metadata);

        let replacement_body = test_body("Versioned Update Object Renamed", "Replacement body.");
        let body_updated = r
            .update_object(space.workspace.id, object.id, None, Some(&replacement_body), None)
            .await?;
        let body_updated_version =
            body_updated.current_version.expect("updated object should have a current version");

        assert_ne!(body_updated_version.id, renamed_version.id);
        assert_eq!(body_updated_version.version_number, renamed_version.version_number + 1);
        assert_eq!(body_updated.object.title, "Versioned Update Object Renamed");
        assert_eq!(body_updated_version.title, "Versioned Update Object Renamed");
        assert_eq!(body_updated_version.body, replacement_body);
        assert_eq!(body_updated_version.metadata, original_metadata);

        let replacement_metadata = object_metadata("versioned-update-metadata-only");
        let metadata_updated = r
            .update_object(
                space.workspace.id,
                object.id,
                None,
                None,
                Some(replacement_metadata.clone()),
            )
            .await?;
        let metadata_updated_version =
            metadata_updated.current_version.expect("updated object should have a current version");

        assert_ne!(metadata_updated_version.id, body_updated_version.id);
        assert_eq!(
            metadata_updated_version.version_number,
            body_updated_version.version_number + 1
        );
        assert_eq!(metadata_updated.object.title, "Versioned Update Object Renamed");
        assert_eq!(metadata_updated_version.title, "Versioned Update Object Renamed");
        assert_eq!(metadata_updated_version.body, replacement_body);
        assert_eq!(metadata_updated_version.metadata, replacement_metadata);

        let body_cleared =
            r.update_object(space.workspace.id, object.id, None, Some(""), None).await?;
        let body_cleared_version =
            body_cleared.current_version.expect("updated object should have a current version");

        assert_ne!(body_cleared_version.id, metadata_updated_version.id);
        assert_eq!(
            body_cleared_version.version_number,
            metadata_updated_version.version_number + 1
        );
        assert_eq!(body_cleared.object.title, "Versioned Update Object Renamed");
        assert_eq!(body_cleared_version.title, "Versioned Update Object Renamed");
        assert_eq!(body_cleared_version.body, "");
        assert_eq!(body_cleared_version.metadata, replacement_metadata);

        Ok(())
    }
}
