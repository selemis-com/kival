//! Event API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        CreateObjectRequest, Event, GrantPrincipal, ListResponse, MembershipRole, ObjectResponse,
        ObjectRole,
    };
    use kival_tests::{
        TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt, object_metadata, test_body,
    };

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn events_show_object_events_to_object_admin(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("events visible").await?;
        let workspace = space.workspace;
        let group = space.group;

        let reader = r.create_user("events-reader").await?;

        r.add_user_to_workspace(workspace.id, reader.id, MembershipRole::Member).await?;
        r.add_user_to_group(group.id, reader.id, MembershipRole::Member).await?;

        let object = r
            .create_object(
                workspace.id,
                "Visible Event Object",
                &test_body("Visible Event Object", "Body."),
                object_metadata("event-object"),
            )
            .await?;

        r.create_object_grant(
            workspace.id,
            object.id,
            GrantPrincipal::Group(group.id),
            ObjectRole::Admin,
        )
        .await?;

        let events = r.object_events_as(&reader, workspace.id, object.id, "limit=20").await?;

        assert!(
            events.items.iter().any(|event| event.event_kind == "object.created"),
            "object admin should see object.created for the object",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_viewer_cannot_inspect_object_grant_events(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("object grant event visibility").await?;
        let object = r
            .create_object(
                workspace.id,
                "Grant Event Visibility Object",
                &test_body("Grant Event Visibility Object", "Body."),
                object_metadata("grant-event-visibility-object"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                workspace.id,
                object.id,
                "grant-event-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;

        let events = r.object_events_as(&viewer, workspace.id, object.id, "limit=50").await?;
        assert!(
            events.items.iter().any(|event| event.event_kind == "object.created"),
            "viewer should retain access to ordinary object activity",
        );
        assert!(
            events.items.iter().all(|event| event.object_grant_id.is_none()),
            "viewer must not receive object-grant event metadata",
        );

        let grant_events = r
            .object_events_as(
                &viewer,
                workspace.id,
                object.id,
                "event_kind=object_grant.created&limit=50",
            )
            .await?;
        assert!(
            grant_events.items.is_empty(),
            "grant-event filters must not bypass object access-event visibility",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_creation_emits_creator_grant_event(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("creator grant event").await?;
        let creator = r
            .create_workspace_actor(
                workspace.id,
                "creator-grant-event-user",
                MembershipRole::Member,
            )
            .await?;

        let created: ObjectResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                &format!("/workspaces/{}/objects", workspace.id),
                &CreateObjectRequest {
                    title: "Creator Grant Event Object".to_owned(),
                    body: test_body("Creator Grant Event Object", "Body."),
                    metadata: object_metadata("creator-grant-event-object"),
                },
            )
            .await?
            .into_success()?;

        let events =
            r.object_events_as(&creator, workspace.id, created.object.id, "limit=20").await?;
        let grant_event = events
            .items
            .iter()
            .find(|event| event.event_kind == "object_grant.created")
            .expect("object creation should emit object_grant.created for the creator grant");

        assert_eq!(grant_event.actor_user_id, Some(creator.id));
        assert_eq!(grant_event.object_id, Some(created.object.id));
        assert_eq!(grant_event.target_user_id, Some(creator.id));
        let grant_id = grant_event
            .object_grant_id
            .expect("creator grant event should reference the created grant");
        assert_eq!(
            grant_event.payload,
            serde_json::json!({
                "object_grant_id": grant_id,
                "object_role": "admin",
            }),
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_update_events_reference_the_same_new_version(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("object update version events").await?;
        let object = r
            .create_object(
                workspace.id,
                "Object Update Version Event",
                &test_body("Object Update Version Event", "Original body."),
                object_metadata("object-update-version-event"),
            )
            .await?;

        let updated_body = test_body("Object Update Version Event", "Updated body.");
        let updated =
            r.update_object(workspace.id, object.id, None, Some(&updated_body), None).await?;
        let version_id = updated
            .object
            .current_version_id
            .expect("updated object should reference its new current version");

        let events = r.object_events_as(&r.admin, workspace.id, object.id, "limit=20").await?;
        let version_event = events
            .items
            .iter()
            .find(|event| {
                event.event_kind == "object.version_appended"
                    && event.object_version_id == Some(version_id)
            })
            .expect("object update should emit object.version_appended for the new version");
        let update_event = events
            .items
            .iter()
            .find(|event| {
                event.event_kind == "object.updated" && event.object_version_id == Some(version_id)
            })
            .expect("object update should emit object.updated for the new version");

        assert_eq!(version_event.object_id, Some(object.id));
        assert_eq!(update_event.object_id, Some(object.id));
        assert_eq!(version_event.object_version_id, update_event.object_version_id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_grant_events_include_granted_role(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("object grant role events").await?;
        let object = r
            .create_object(
                workspace.id,
                "Object Grant Role Event",
                &test_body("Object Grant Role Event", "Body."),
                object_metadata("object-grant-role-event"),
            )
            .await?;
        let grantee = r
            .create_workspace_actor(
                workspace.id,
                "object-grant-event-grantee",
                MembershipRole::Member,
            )
            .await?;

        let grant = r
            .create_object_grant(
                workspace.id,
                object.id,
                GrantPrincipal::User(grantee.id),
                ObjectRole::Editor,
            )
            .await?;

        let events = r.object_events_as(&r.admin, workspace.id, object.id, "limit=50").await?;
        let created = events
            .items
            .iter()
            .find(|event| {
                event.event_kind == "object_grant.created"
                    && event.object_grant_id == Some(grant.id)
            })
            .expect("explicit grant creation should emit object_grant.created");
        assert_eq!(created.payload["object_role"], serde_json::json!("editor"));

        let revoked = r
            .request(
                Some(&r.admin),
                Method::POST,
                &format!(
                    "/workspaces/{}/objects/{}/grants/{}/revoke",
                    workspace.id, object.id, grant.id
                ),
                None,
            )
            .await?;
        revoked.assert_status(StatusCode::OK);

        let events = r.object_events_as(&r.admin, workspace.id, object.id, "limit=50").await?;
        let revoked = events
            .items
            .iter()
            .find(|event| {
                event.event_kind == "object_grant.revoked"
                    && event.object_grant_id == Some(grant.id)
            })
            .expect("grant revocation should emit object_grant.revoked");
        assert_eq!(revoked.payload["object_role"], serde_json::json!("editor"));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn membership_events_include_assigned_role(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("membership role events").await?;
        let group = r.create_group("membership role event group").await?;
        let user = r.create_user("membership-role-event-user").await?;

        r.add_user_to_workspace(workspace.id, user.id, MembershipRole::Member).await?;
        r.add_user_to_group(group.id, user.id, MembershipRole::Admin).await?;

        let workspace_events: ListResponse<Event> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/events?event_kind=workspace.membership_created&target_user_id={}&limit=50",
                    user.id,
                ),
            )
            .await?
            .into_success()?;
        let workspace_membership = workspace_events
            .items
            .iter()
            .find(|event| {
                event.workspace_id == Some(workspace.id) && event.target_user_id == Some(user.id)
            })
            .expect("workspace membership creation event should be present");
        assert_eq!(workspace_membership.payload["workspace_role"], serde_json::json!("member"));

        let group_events: ListResponse<Event> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/events?event_kind=group.membership_created&target_user_id={}&group_id={}&limit=50",
                    user.id, group.id,
                ),
            )
            .await?
            .into_success()?;
        let group_membership = group_events
            .items
            .iter()
            .find(|event| event.group_id == Some(group.id) && event.target_user_id == Some(user.id))
            .expect("group membership creation event should be present");
        assert_eq!(group_membership.payload["group_role"], serde_json::json!("admin"));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn events_reject_non_admin_object_event_access(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let workspace = r.create_workspace("events auth").await?;

        let visible_group = r.create_group("visible events editors").await?;
        let hidden_group = r.create_group("hidden events editors").await?;

        r.add_group_to_workspace(workspace.id, visible_group.id).await?;
        r.add_group_to_workspace(workspace.id, hidden_group.id).await?;

        let reader = r.create_user("events-hidden-reader").await?;

        r.add_user_to_workspace(workspace.id, reader.id, MembershipRole::Member).await?;
        r.add_user_to_group(visible_group.id, reader.id, MembershipRole::Member).await?;

        let hidden_object = r
            .create_object(
                workspace.id,
                "Hidden Event Object",
                &test_body("Hidden Event Object", "Hidden body."),
                object_metadata("hidden-event-object"),
            )
            .await?;

        let response = r
            .request(
                Some(&reader),
                Method::GET,
                &format!(
                    "/workspaces/{}/objects/{}/events?limit=20",
                    workspace.id, hidden_object.id,
                ),
                None,
            )
            .await?;

        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn archived_object_admin_can_list_object_events(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("archived object events").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Archived Event Object",
                &test_body("Archived Event Object", "Body."),
                object_metadata("archived-event-object"),
            )
            .await?;
        let object_admin = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "archived-object-events-admin",
                MembershipRole::Member,
                ObjectRole::Admin,
            )
            .await?;
        r.archive_object(space.workspace.id, object.id).await?;

        let events =
            r.object_events_as(&object_admin, space.workspace.id, object.id, "limit=20").await?;

        assert!(
            events.items.iter().any(|event| event.event_kind == "object.created"),
            "archived object admin should be able to inspect object audit events",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn archived_object_editor_cannot_list_object_events(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("archived object events editor denial").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Archived Event Editor Object",
                &test_body("Archived Event Editor Object", "Body."),
                object_metadata("archived-event-editor-object"),
            )
            .await?;
        let object_editor = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "archived-object-events-editor",
                MembershipRole::Member,
                ObjectRole::Editor,
            )
            .await?;
        r.archive_object(space.workspace.id, object.id).await?;

        let response = r
            .request(
                Some(&object_editor),
                Method::GET,
                &format!(
                    "/workspaces/{}/objects/{}/events?limit=20",
                    space.workspace.id, object.id,
                ),
                None,
            )
            .await?;

        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn events_do_not_emit_zero_count_reference_update(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("events reference noise").await?;
        let workspace = space.workspace;

        let object = r
            .create_object(
                workspace.id,
                "No Reference Object",
                &test_body("No Reference Object", "This body contains no links."),
                object_metadata("no-reference-object"),
            )
            .await?;

        let events = r.object_events_as(&r.admin, workspace.id, object.id, "limit=20").await?;

        assert!(
            events.items.iter().all(|event| event.event_kind != "object.references_updated"),
            "object.references_updated should not be emitted when no references changed",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn global_admin_can_list_global_events_with_bounded_filter(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("global-event-target").await?;

        let events: ListResponse<Event> = r
            .get_json_as(&r.admin, &format!("/events?target_user_id={}&limit=20", user.id))
            .await?
            .into_success()?;

        assert!(
            events.items.iter().any(|event| {
                event.event_kind == "user.created" && event.target_user_id == Some(user.id)
            }),
            "bounded global event query should include the target user's creation event",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn global_event_list_supports_a_bounded_newest_first_page_without_filters(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        r.create_user("global-event-page").await?;

        let events: ListResponse<Event> =
            r.get_json_as(&r.admin, "/events?limit=20&order=desc").await?.into_success()?;

        assert!(!events.items.is_empty());
        assert!(
            events.items.windows(2).all(|pair| pair[0].sequence_number > pair[1].sequence_number)
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn non_global_admin_cannot_list_global_events(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("global-event-non-admin").await?;

        let response = r
            .request(
                Some(&user),
                Method::GET,
                &format!("/events?target_user_id={}&limit=20", user.id),
                None,
            )
            .await?;

        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_admin_can_list_workspace_events(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("workspace events").await?;
        let admin = r
            .create_workspace_actor(workspace.id, "workspace-event-admin", MembershipRole::Admin)
            .await?;

        let events: ListResponse<Event> = r
            .get_json_as(&admin, &format!("/workspaces/{}/events?limit=20", workspace.id))
            .await?
            .into_success()?;

        assert!(
            events.items.iter().any(|event| {
                event.event_kind == "workspace.created" && event.workspace_id == Some(workspace.id)
            }),
            "workspace admin should see workspace-scoped audit events",
        );
        assert!(
            events
                .items
                .iter()
                .filter(|event| event.actor_user_id.is_some())
                .all(|event| event.actor_username.is_some()),
            "user-authored events should include the actor username",
        );

        let newest: ListResponse<Event> = r
            .get_json_as(&admin, &format!("/workspaces/{}/events?limit=1&order=desc", workspace.id))
            .await?
            .into_success()?;
        let newest_event = newest.items.first().expect("workspace should have events");

        let older: ListResponse<Event> = r
            .get_json_as(
                &admin,
                &format!(
                    "/workspaces/{}/events?limit=20&order=desc&before_sequence={}",
                    workspace.id, newest_event.sequence_number
                ),
            )
            .await?
            .into_success()?;
        assert!(
            older.items.iter().all(|event| event.sequence_number < newest_event.sequence_number),
            "before_sequence must page toward older events",
        );
        assert!(
            older
                .items
                .windows(2)
                .all(|events| events[0].sequence_number > events[1].sequence_number),
            "descending event pages must remain newest first",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_member_cannot_list_workspace_events(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("workspace events denied").await?;
        let member = r
            .create_workspace_actor(workspace.id, "workspace-event-member", MembershipRole::Member)
            .await?;

        let response = r
            .request(
                Some(&member),
                Method::GET,
                &format!("/workspaces/{}/events?limit=20", workspace.id),
                None,
            )
            .await?;

        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_events_respect_event_kind_filter(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("workspace events filter").await?;
        let admin = r
            .create_workspace_actor(
                workspace.id,
                "workspace-event-filter-admin",
                MembershipRole::Admin,
            )
            .await?;
        let member = r.create_user("workspace-event-filter-member").await?;
        r.add_user_to_workspace(workspace.id, member.id, MembershipRole::Member).await?;

        let events: ListResponse<Event> = r
            .get_json_as(
                &admin,
                &format!(
                    "/workspaces/{}/events?event_kind=workspace.membership_created&limit=20",
                    workspace.id,
                ),
            )
            .await?
            .into_success()?;

        assert!(!events.items.is_empty(), "filter should return matching membership events");
        assert!(
            events.items.iter().all(|event| event.event_kind == "workspace.membership_created"),
            "event_kind filter should not return other workspace event kinds",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn event_lists_reject_negative_sequence_boundaries(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("event negative sequence").await?;
        let object = r
            .create_object(
                workspace.id,
                "Event Negative Sequence Object",
                &test_body("Event Negative Sequence Object", "Body."),
                object_metadata("event-negative-sequence-object"),
            )
            .await?;

        for path in [
            "/events?after_sequence=-1&limit=20".to_owned(),
            "/events?before_sequence=-1&limit=20".to_owned(),
            format!("/workspaces/{}/events?after_sequence=-1&limit=20", workspace.id),
            format!("/workspaces/{}/events?before_sequence=-1&limit=20", workspace.id),
            format!(
                "/workspaces/{}/objects/{}/events?after_sequence=-1&limit=20",
                workspace.id, object.id,
            ),
            format!(
                "/workspaces/{}/objects/{}/events?before_sequence=-1&limit=20",
                workspace.id, object.id,
            ),
        ] {
            let response = r.request(Some(&r.admin), Method::GET, &path, None).await?;
            response.assert_status(StatusCode::BAD_REQUEST);
        }

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn event_list_rejects_invalid_limit(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("event invalid limit").await?;

        let workspace_response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!("/workspaces/{}/events?limit=0", workspace.id),
                None,
            )
            .await?;
        workspace_response.assert_status(StatusCode::BAD_REQUEST);

        let global_response = r
            .request(
                Some(&r.admin),
                Method::GET,
                "/events?event_kind=workspace.created&limit=0",
                None,
            )
            .await?;
        global_response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }
}
