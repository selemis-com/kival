//! Object grant API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        CreateObjectGrantRequest, CreateObjectRequest, GrantPrincipal, ListResponse,
        MembershipRole, ObjectGrant, ObjectGrantResponse, ObjectResponse, ObjectRole,
        UpdateObjectGrantRequest,
    };
    use kival_tests::{
        TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt, object_metadata, test_body,
    };

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn non_workspace_member_cannot_create_object(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("non-member object create").await?;
        let non_member = r.create_user("non-member-object-creator").await?;

        let response = r
            .request_json_raw_as(
                &non_member,
                Method::POST,
                &format!("/workspaces/{}/objects", workspace.id),
                &CreateObjectRequest {
                    title: "Forbidden Object".to_owned(),
                    body: test_body("Forbidden Object", "Body."),
                    metadata: object_metadata("forbidden-object"),
                },
            )
            .await?;

        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_member_creates_object_with_creator_admin_grant(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("member-created object").await?;
        let creator = r
            .create_workspace_actor(
                space.workspace.id,
                "member-object-creator",
                MembershipRole::Member,
            )
            .await?;
        let grantee = r
            .create_workspace_actor(
                space.workspace.id,
                "member-object-grantee",
                MembershipRole::Member,
            )
            .await?;

        let created: ObjectResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                &format!("/workspaces/{}/objects", space.workspace.id),
                &CreateObjectRequest {
                    title: "Member Created Object".to_owned(),
                    body: test_body("Member Created Object", "Body."),
                    metadata: object_metadata("member-created-object"),
                },
            )
            .await?
            .into_success()?;
        assert_eq!(created.effective_role, ObjectRole::Admin);

        let grants: ListResponse<ObjectGrant> = r
            .get_json_as(
                &creator,
                &format!("/workspaces/{}/objects/{}/grants", space.workspace.id, created.object.id),
            )
            .await?
            .into_success()?;
        assert_eq!(grants.items.len(), 1, "new objects should have exactly one active grant");
        let creator_grant = &grants.items[0];
        assert_eq!(creator_grant.principal_user_id, Some(creator.id));
        assert!(creator_grant.principal_group_id.is_none());
        assert_eq!(creator_grant.object_role, ObjectRole::Admin);
        assert!(creator_grant.revoked_at.is_none());

        let denied = r
            .request(
                Some(&grantee),
                Method::GET,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, created.object.id),
                None,
            )
            .await?;
        denied.assert_status(StatusCode::FORBIDDEN);

        let grant: ObjectGrantResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                &format!("/workspaces/{}/objects/{}/grants", space.workspace.id, created.object.id),
                &CreateObjectGrantRequest {
                    principal: GrantPrincipal::User(grantee.id),
                    object_role: ObjectRole::Editor,
                },
            )
            .await?
            .into_success()?;
        assert_eq!(grant.grant.created_by, Some(creator.id));
        assert_eq!(grant.grant.object_role, ObjectRole::Editor);

        let fetched = r.get_object_as(&grantee, space.workspace.id, created.object.id).await?;
        assert_eq!(fetched.id, created.object.id);

        let response: ObjectResponse = r
            .get_json_as(
                &grantee,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, created.object.id),
            )
            .await?
            .into_success()?;
        assert_eq!(response.effective_role, ObjectRole::Editor);

        let updated: ObjectGrantResponse = r
            .request_json_as(
                &creator,
                Method::PATCH,
                &format!(
                    "/workspaces/{}/objects/{}/grants/{}",
                    space.workspace.id, created.object.id, grant.grant.id
                ),
                &UpdateObjectGrantRequest { object_role: ObjectRole::Admin },
            )
            .await?
            .into_success()?;
        assert_eq!(updated.grant.object_role, ObjectRole::Admin);
        assert_ne!(
            updated.grant.id, grant.grant.id,
            "a role change should replace the immutable grant row"
        );

        let response: ObjectResponse = r
            .get_json_as(
                &grantee,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, created.object.id),
            )
            .await?
            .into_success()?;
        assert_eq!(response.effective_role, ObjectRole::Admin);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn last_admin_grant_cannot_be_revoked_but_admin_can_remove_self_after_handoff(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object grant admin handoff").await?;
        let creator = r
            .create_workspace_actor(
                space.workspace.id,
                "admin-handoff-creator",
                MembershipRole::Member,
            )
            .await?;
        let successor = r
            .create_workspace_actor(
                space.workspace.id,
                "admin-handoff-successor",
                MembershipRole::Member,
            )
            .await?;
        let created: ObjectResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                &format!("/workspaces/{}/objects", space.workspace.id),
                &CreateObjectRequest {
                    title: "Admin Handoff Object".to_owned(),
                    body: test_body("Admin Handoff Object", "Body."),
                    metadata: object_metadata("admin-handoff-object"),
                },
            )
            .await?
            .into_success()?;
        let grants: ListResponse<ObjectGrant> = r
            .get_json_as(
                &creator,
                &format!("/workspaces/{}/objects/{}/grants", space.workspace.id, created.object.id),
            )
            .await?
            .into_success()?;
        let creator_grant_id = grants
            .items
            .iter()
            .find(|grant| grant.principal_user_id == Some(creator.id))
            .expect("creator admin grant")
            .id;

        let response = r
            .request_json_raw_as(
                &creator,
                Method::PATCH,
                &format!(
                    "/workspaces/{}/objects/{}/grants/{}",
                    space.workspace.id, created.object.id, creator_grant_id
                ),
                &UpdateObjectGrantRequest { object_role: ObjectRole::Editor },
            )
            .await?;
        response.assert_status(StatusCode::CONFLICT);

        let response = r
            .request(
                Some(&creator),
                Method::POST,
                &format!(
                    "/workspaces/{}/objects/{}/grants/{}/revoke",
                    space.workspace.id, created.object.id, creator_grant_id
                ),
                None,
            )
            .await?;
        response.assert_status(StatusCode::CONFLICT);

        let successor_grant: ObjectGrantResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                &format!("/workspaces/{}/objects/{}/grants", space.workspace.id, created.object.id),
                &CreateObjectGrantRequest {
                    principal: GrantPrincipal::User(successor.id),
                    object_role: ObjectRole::Admin,
                },
            )
            .await?
            .into_success()?;
        assert_eq!(successor_grant.grant.object_role, ObjectRole::Admin);

        let response = r
            .request(
                Some(&creator),
                Method::POST,
                &format!(
                    "/workspaces/{}/objects/{}/grants/{}/revoke",
                    space.workspace.id, created.object.id, creator_grant_id
                ),
                None,
            )
            .await?;
        response.assert_status(StatusCode::OK);

        let denied = r
            .request(
                Some(&creator),
                Method::GET,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, created.object.id),
                None,
            )
            .await?;
        denied.assert_status(StatusCode::FORBIDDEN);

        let successor_response: ObjectResponse = r
            .get_json_as(
                &successor,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, created.object.id),
            )
            .await?
            .into_success()?;
        assert_eq!(successor_response.effective_role, ObjectRole::Admin);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn revoked_workspace_member_cannot_use_direct_object_grant(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("revoked direct object grant").await?;
        let creator = r
            .create_workspace_actor(
                workspace.id,
                "revoked-direct-grant-user",
                MembershipRole::Member,
            )
            .await?;

        let created: ObjectResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                &format!("/workspaces/{}/objects", workspace.id),
                &CreateObjectRequest {
                    title: "Revoked Direct Grant Object".to_owned(),
                    body: test_body("Revoked Direct Grant Object", "Body."),
                    metadata: object_metadata("revoked-direct-grant-object"),
                },
            )
            .await?
            .into_success()?;

        let membership_id = sqlx::query_scalar::<_, uuid::Uuid>(
            r#"
            SELECT id
            FROM kival.workspace_memberships
            WHERE workspace_id = $1
                AND user_id = $2
                AND revoked_at IS NULL
            "#,
        )
        .bind(workspace.id)
        .bind(creator.id)
        .fetch_one(&r.pool)
        .await?;

        let revoked = r
            .request(
                Some(&r.admin),
                Method::POST,
                &format!("/workspaces/{}/memberships/{membership_id}/revoke", workspace.id),
                None,
            )
            .await?;
        revoked.assert_status(StatusCode::OK);

        let denied = r
            .request(
                Some(&creator),
                Method::GET,
                &format!("/workspaces/{}/objects/{}", workspace.id, created.object.id),
                None,
            )
            .await?;
        denied.assert_status(StatusCode::FORBIDDEN);

        let fetched = r.get_object_as(&r.admin, workspace.id, created.object.id).await?;
        assert_eq!(fetched.id, created.object.id);

        r.archive_object(workspace.id, created.object.id).await?;

        let denied_restore = r
            .request(
                Some(&creator),
                Method::POST,
                &format!("/workspaces/{}/objects/{}/unarchive", workspace.id, created.object.id),
                None,
            )
            .await?;
        denied_restore.assert_status(StatusCode::FORBIDDEN);

        let restored = r.unarchive_object(workspace.id, created.object.id).await?;
        assert_eq!(restored.id, created.object.id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn revoked_workspace_member_cannot_use_group_object_grant(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("revoked group object grant").await?;
        let group = r.create_group("revoked group object grant group").await?;
        r.add_group_to_workspace(workspace.id, group.id).await?;

        let member = r
            .create_workspace_actor(
                workspace.id,
                "revoked-group-grant-user",
                MembershipRole::Member,
            )
            .await?;
        r.add_user_to_group(group.id, member.id, MembershipRole::Member).await?;

        let object = r
            .create_object(
                workspace.id,
                "Revoked Group Grant Object",
                &test_body("Revoked Group Grant Object", "Body."),
                object_metadata("revoked-group-grant-object"),
            )
            .await?;
        r.create_object_grant(
            workspace.id,
            object.id,
            GrantPrincipal::Group(group.id),
            ObjectRole::Viewer,
        )
        .await?;

        let fetched = r.get_object_as(&member, workspace.id, object.id).await?;
        assert_eq!(fetched.id, object.id);

        let membership_id = sqlx::query_scalar::<_, uuid::Uuid>(
            r#"
            SELECT id
            FROM kival.workspace_memberships
            WHERE workspace_id = $1
                AND user_id = $2
                AND revoked_at IS NULL
            "#,
        )
        .bind(workspace.id)
        .bind(member.id)
        .fetch_one(&r.pool)
        .await?;

        let revoked = r
            .request(
                Some(&r.admin),
                Method::POST,
                &format!("/workspaces/{}/memberships/{membership_id}/revoke", workspace.id),
                None,
            )
            .await?;
        revoked.assert_status(StatusCode::OK);

        let denied = r
            .request(
                Some(&member),
                Method::GET,
                &format!("/workspaces/{}/objects/{}", workspace.id, object.id),
                None,
            )
            .await?;
        denied.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_admin_can_create_list_and_revoke_user_grant(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object grant lifecycle").await?;
        let grantee = r
            .create_workspace_actor(
                space.workspace.id,
                "object-grant-viewer",
                MembershipRole::Member,
            )
            .await?;

        let object = r
            .create_object(
                space.workspace.id,
                "Grant Lifecycle Object",
                &test_body("Grant Lifecycle Object", "Body."),
                object_metadata("grant-lifecycle-object"),
            )
            .await?;
        let object_admin = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "object-grant-admin",
                MembershipRole::Member,
                ObjectRole::Admin,
            )
            .await?;

        let created: ObjectGrantResponse = r
            .request_json_as(
                &object_admin,
                Method::POST,
                &format!("/workspaces/{}/objects/{}/grants", space.workspace.id, object.id),
                &CreateObjectGrantRequest {
                    principal: GrantPrincipal::User(grantee.id),
                    object_role: ObjectRole::Viewer,
                },
            )
            .await?
            .into_success()?;
        assert_eq!(created.grant.object_id, object.id);
        assert_eq!(created.grant.principal_user_id, Some(grantee.id));
        assert_eq!(created.grant.object_role, ObjectRole::Viewer);
        assert_eq!(created.grant.created_by, Some(object_admin.id));

        let listed: ListResponse<ObjectGrant> = r
            .get_json_as(
                &object_admin,
                &format!("/workspaces/{}/objects/{}/grants", space.workspace.id, object.id),
            )
            .await?
            .into_success()?;
        assert!(listed.items.iter().any(|grant| grant.id == created.grant.id));

        let fetched = r.get_object_as(&grantee, space.workspace.id, object.id).await?;
        assert_eq!(fetched.id, object.id);

        let revoked: ObjectGrantResponse = r
            .empty_json_as(
                &object_admin,
                Method::POST,
                &format!(
                    "/workspaces/{}/objects/{}/grants/{}/revoke",
                    space.workspace.id, object.id, created.grant.id
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(revoked.grant.revoked_by, Some(object_admin.id));
        assert!(revoked.grant.revoked_at.is_some());

        let response = r
            .request(
                Some(&grantee),
                Method::GET,
                &format!("/workspaces/{}/objects/{}", space.workspace.id, object.id),
                None,
            )
            .await?;
        response.assert_status(StatusCode::FORBIDDEN);

        let listed: ListResponse<ObjectGrant> = r
            .get_json_as(
                &object_admin,
                &format!("/workspaces/{}/objects/{}/grants", space.workspace.id, object.id),
            )
            .await?
            .into_success()?;
        assert!(!listed.items.iter().any(|grant| grant.id == created.grant.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_grant_user_principal_requires_active_workspace_membership(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object grant user principal").await?;
        let outsider = r.create_user("object-grant-outside-workspace").await?;

        let object = r
            .create_object(
                space.workspace.id,
                "User Principal Object",
                &test_body("User Principal Object", "Body."),
                object_metadata("user-principal-object"),
            )
            .await?;

        let outside_workspace = r
            .request_json_raw_as(
                &r.admin,
                Method::POST,
                &format!("/workspaces/{}/objects/{}/grants", space.workspace.id, object.id),
                &CreateObjectGrantRequest {
                    principal: GrantPrincipal::User(outsider.id),
                    object_role: ObjectRole::Viewer,
                },
            )
            .await?;
        outside_workspace.assert_status(StatusCode::BAD_REQUEST);

        r.add_user_to_workspace(space.workspace.id, outsider.id, MembershipRole::Member).await?;

        let granted: ObjectGrantResponse = r
            .request_json_as(
                &r.admin,
                Method::POST,
                &format!("/workspaces/{}/objects/{}/grants", space.workspace.id, object.id),
                &CreateObjectGrantRequest {
                    principal: GrantPrincipal::User(outsider.id),
                    object_role: ObjectRole::Viewer,
                },
            )
            .await?
            .into_success()?;
        assert_eq!(granted.grant.principal_user_id, Some(outsider.id));

        let revoked_member = r
            .create_workspace_actor(
                space.workspace.id,
                "revoked-grant-ws-member",
                MembershipRole::Member,
            )
            .await?;
        let membership_id =
            r.active_workspace_membership_id(space.workspace.id, revoked_member.id).await?;
        let revoked = r
            .request(
                Some(&r.admin),
                Method::POST,
                &format!("/workspaces/{}/memberships/{membership_id}/revoke", space.workspace.id),
                None,
            )
            .await?;
        revoked.assert_status(StatusCode::OK);

        let revoked_workspace_member = r
            .request_json_raw_as(
                &r.admin,
                Method::POST,
                &format!("/workspaces/{}/objects/{}/grants", space.workspace.id, object.id),
                &CreateObjectGrantRequest {
                    principal: GrantPrincipal::User(revoked_member.id),
                    object_role: ObjectRole::Viewer,
                },
            )
            .await?;
        revoked_workspace_member.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_grant_group_principal_requires_active_workspace_group(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object grant group principal").await?;
        let unlinked_group = r.create_group("object grant unlinked group").await?;
        let archived_link_group = r.create_group("object grant archived link group").await?;

        let object = r
            .create_object(
                space.workspace.id,
                "Group Principal Object",
                &test_body("Group Principal Object", "Body."),
                object_metadata("group-principal-object"),
            )
            .await?;

        let unlinked = r
            .request_json_raw_as(
                &r.admin,
                Method::POST,
                &format!("/workspaces/{}/objects/{}/grants", space.workspace.id, object.id),
                &CreateObjectGrantRequest {
                    principal: GrantPrincipal::Group(unlinked_group.id),
                    object_role: ObjectRole::Viewer,
                },
            )
            .await?;
        unlinked.assert_status(StatusCode::BAD_REQUEST);

        r.add_group_to_workspace(space.workspace.id, archived_link_group.id).await?;
        r.archive_workspace_group(space.workspace.id, archived_link_group.id).await?;

        let archived_link = r
            .request_json_raw_as(
                &r.admin,
                Method::POST,
                &format!("/workspaces/{}/objects/{}/grants", space.workspace.id, object.id),
                &CreateObjectGrantRequest {
                    principal: GrantPrincipal::Group(archived_link_group.id),
                    object_role: ObjectRole::Viewer,
                },
            )
            .await?;
        archived_link.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }
}
