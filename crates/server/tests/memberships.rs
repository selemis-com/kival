//! Workspace and group membership API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        CreateGroupMembershipRequest, CreateWorkspaceMembershipRequest, GroupMembership,
        GroupMembershipResponse, ListResponse, MembershipRole, UpdateGroupMembershipRequest,
        UpdateWorkspaceMembershipRequest, WorkspaceMembership, WorkspaceMembershipResponse,
        WorkspaceResponse,
    };
    use kival_tests::{TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt};
    use serde_json::json;

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_admin_can_create_update_list_and_revoke_membership(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("workspace membership admin").await?;
        let workspace_admin = r.create_user("workspace-membership-admin").await?;
        let member = r.create_user("workspace-membership-member").await?;

        r.add_user_to_workspace(workspace.id, workspace_admin.id, MembershipRole::Admin).await?;

        let created: WorkspaceMembershipResponse = r
            .request_json_as(
                &workspace_admin,
                Method::POST,
                &format!("/workspaces/{}/memberships", workspace.id),
                &CreateWorkspaceMembershipRequest {
                    user_id: Some(member.id),
                    username: None,
                    workspace_role: MembershipRole::Member,
                },
            )
            .await?
            .into_success()?;

        assert_eq!(created.membership.workspace_id, workspace.id);
        assert_eq!(created.membership.user_id, member.id);
        assert_eq!(created.membership.workspace_role, MembershipRole::Member);

        let updated: WorkspaceMembershipResponse = r
            .request_json_as(
                &workspace_admin,
                Method::PATCH,
                &format!("/workspaces/{}/memberships/{}", workspace.id, created.membership.id),
                &UpdateWorkspaceMembershipRequest { workspace_role: MembershipRole::Admin },
            )
            .await?
            .into_success()?;

        assert_ne!(updated.membership.id, created.membership.id);
        assert_eq!(updated.membership.user_id, member.id);
        assert_eq!(updated.membership.workspace_role, MembershipRole::Admin);

        let previous_revoked_by: Option<uuid::Uuid> =
            sqlx::query_scalar("SELECT revoked_by FROM kival.workspace_memberships WHERE id = $1")
                .bind(created.membership.id)
                .fetch_one(&r.pool)
                .await?;
        assert_eq!(previous_revoked_by, Some(workspace_admin.id));

        let listed: ListResponse<WorkspaceMembership> = r
            .get_json_as(&workspace_admin, &format!("/workspaces/{}/memberships", workspace.id))
            .await?
            .into_success()?;
        assert!(listed.items.iter().any(|membership| membership.id == updated.membership.id));
        assert!(!listed.items.iter().any(|membership| membership.id == created.membership.id));

        let revoked: WorkspaceMembershipResponse = r
            .empty_json_as(
                &workspace_admin,
                Method::POST,
                &format!(
                    "/workspaces/{}/memberships/{}/revoke",
                    workspace.id, updated.membership.id
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(revoked.membership.revoked_by, Some(workspace_admin.id));
        assert!(revoked.membership.revoked_at.is_some());

        let listed: ListResponse<WorkspaceMembership> = r
            .get_json_as(&workspace_admin, &format!("/workspaces/{}/memberships", workspace.id))
            .await?
            .into_success()?;
        assert!(!listed.items.iter().any(|membership| membership.id == updated.membership.id));
        assert!(!listed.items.iter().any(|membership| membership.id == created.membership.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_admin_can_add_existing_user_by_username(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("workspace username add").await?;
        let workspace_admin = r.create_user("ws-username-admin").await?;
        let target = r.create_user("ws-username-target").await?;

        r.add_user_to_workspace(workspace.id, workspace_admin.id, MembershipRole::Admin).await?;

        let created: WorkspaceMembershipResponse = r
            .request_json_as(
                &workspace_admin,
                Method::POST,
                &format!("/workspaces/{}/memberships", workspace.id),
                &CreateWorkspaceMembershipRequest {
                    user_id: None,
                    username: Some(format!("  {}  ", target.username.to_uppercase())),
                    workspace_role: MembershipRole::Member,
                },
            )
            .await?
            .into_success()?;

        assert_eq!(created.membership.user_id, target.id);
        assert_eq!(created.membership.user_username, target.username);
        assert!(!created.membership.user_display_name.is_empty());

        let missing = r
            .request_json_raw_as(
                &workspace_admin,
                Method::POST,
                &format!("/workspaces/{}/memberships", workspace.id),
                &CreateWorkspaceMembershipRequest {
                    user_id: None,
                    username: Some("missing-user".to_owned()),
                    workspace_role: MembershipRole::Member,
                },
            )
            .await?;
        missing.assert_status(StatusCode::NOT_FOUND);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_membership_create_requires_exactly_one_user_identifier(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("workspace member identifier").await?;
        let workspace_admin = r.create_user("ws-identifier-admin").await?;
        let target = r.create_user("ws-identifier-target").await?;

        r.add_user_to_workspace(workspace.id, workspace_admin.id, MembershipRole::Admin).await?;

        for request in [
            CreateWorkspaceMembershipRequest {
                user_id: None,
                username: None,
                workspace_role: MembershipRole::Member,
            },
            CreateWorkspaceMembershipRequest {
                user_id: Some(target.id),
                username: Some(target.username.clone()),
                workspace_role: MembershipRole::Member,
            },
        ] {
            let response = r
                .request_json_raw_as(
                    &workspace_admin,
                    Method::POST,
                    &format!("/workspaces/{}/memberships", workspace.id),
                    &request,
                )
                .await?;

            response.assert_status(StatusCode::BAD_REQUEST);
        }

        let membership_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM kival.workspace_memberships WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(workspace.id)
        .bind(target.id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(membership_count, 0);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_member_cannot_manage_memberships(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("workspace membership member").await?;
        let member = r.create_user("workspace-regular-member").await?;
        let target = r.create_user("workspace-membership-target").await?;

        r.add_user_to_workspace(workspace.id, member.id, MembershipRole::Member).await?;

        let response = r
            .request(
                Some(&member),
                Method::POST,
                &format!("/workspaces/{}/memberships", workspace.id),
                Some(json!(CreateWorkspaceMembershipRequest {
                    user_id: Some(target.id),
                    username: None,
                    workspace_role: MembershipRole::Member,
                })),
            )
            .await?;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let membership_id = r.active_workspace_membership_id(workspace.id, member.id).await?;
        let update_response = r
            .request(
                Some(&member),
                Method::PATCH,
                &format!("/workspaces/{}/memberships/{membership_id}", workspace.id),
                Some(json!(UpdateWorkspaceMembershipRequest {
                    workspace_role: MembershipRole::Admin,
                })),
            )
            .await?;

        assert_eq!(update_response.status(), StatusCode::FORBIDDEN);

        let username_response = r
            .request(
                Some(&member),
                Method::POST,
                &format!("/workspaces/{}/memberships", workspace.id),
                Some(json!(CreateWorkspaceMembershipRequest {
                    user_id: None,
                    username: Some("missing-user".to_owned()),
                    workspace_role: MembershipRole::Member,
                })),
            )
            .await?;

        assert_eq!(username_response.status(), StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn archived_workspace_membership_endpoints_return_not_found(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("archived workspace membership immutable").await?;
        let workspace_admin = r.create_user("archived-ws-admin").await?;
        let existing = r.create_user("archived-ws-existing").await?;
        let target = r.create_user("archived-ws-new").await?;

        r.add_user_to_workspace(workspace.id, workspace_admin.id, MembershipRole::Admin).await?;
        r.add_user_to_workspace(workspace.id, existing.id, MembershipRole::Member).await?;

        let membership_id = r.active_workspace_membership_id(workspace.id, existing.id).await?;

        let _: WorkspaceResponse = r
            .empty_json_as(
                &workspace_admin,
                Method::POST,
                &format!("/workspaces/{}/archive", workspace.id),
            )
            .await?
            .into_success()?;

        let list = r
            .request(
                Some(&workspace_admin),
                Method::GET,
                &format!("/workspaces/{}/memberships", workspace.id),
                None,
            )
            .await?;
        assert_eq!(list.status(), StatusCode::NOT_FOUND);

        let create = r
            .request_json_raw_as(
                &workspace_admin,
                Method::POST,
                &format!("/workspaces/{}/memberships", workspace.id),
                &CreateWorkspaceMembershipRequest {
                    user_id: Some(target.id),
                    username: None,
                    workspace_role: MembershipRole::Member,
                },
            )
            .await?;
        create.assert_status(StatusCode::NOT_FOUND);

        let update = r
            .request_json_raw_as(
                &workspace_admin,
                Method::PATCH,
                &format!("/workspaces/{}/memberships/{membership_id}", workspace.id),
                &UpdateWorkspaceMembershipRequest { workspace_role: MembershipRole::Admin },
            )
            .await?;
        update.assert_status(StatusCode::NOT_FOUND);

        let revoke = r
            .request(
                Some(&workspace_admin),
                Method::POST,
                &format!("/workspaces/{}/memberships/{membership_id}/revoke", workspace.id),
                None,
            )
            .await?;
        revoke.assert_status(StatusCode::NOT_FOUND);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn group_membership_create_requires_exactly_one_user_identifier(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("group member identifier").await?;
        let group_admin = r.create_user("group-identifier-admin").await?;
        let target = r.create_user("group-identifier-target").await?;

        r.add_user_to_group(group.id, group_admin.id, MembershipRole::Admin).await?;

        for request in [
            CreateGroupMembershipRequest {
                user_id: None,
                username: None,
                group_role: MembershipRole::Member,
            },
            CreateGroupMembershipRequest {
                user_id: Some(target.id),
                username: Some(target.username.clone()),
                group_role: MembershipRole::Member,
            },
        ] {
            let response = r
                .request_json_raw_as(
                    &group_admin,
                    Method::POST,
                    &format!("/groups/{}/memberships", group.id),
                    &request,
                )
                .await?;

            response.assert_status(StatusCode::BAD_REQUEST);
        }

        let membership_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM kival.group_memberships WHERE group_id = $1 AND user_id = $2",
        )
        .bind(group.id)
        .bind(target.id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(membership_count, 0);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn group_admin_can_create_list_and_revoke_membership(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("group membership admin").await?;
        let group_admin = r.create_user("group-membership-admin").await?;
        let member = r.create_user("group-membership-member").await?;

        r.add_user_to_group(group.id, group_admin.id, MembershipRole::Admin).await?;

        let created: GroupMembershipResponse = r
            .request_json_as(
                &group_admin,
                Method::POST,
                &format!("/groups/{}/memberships", group.id),
                &CreateGroupMembershipRequest {
                    user_id: None,
                    username: Some(member.username.clone()),
                    group_role: MembershipRole::Member,
                },
            )
            .await?
            .into_success()?;

        assert_eq!(created.membership.group_id, group.id);
        assert_eq!(created.membership.user_id, member.id);
        assert_eq!(created.membership.user_username, member.username);
        assert!(!created.membership.user_display_name.is_empty());
        assert_eq!(created.membership.group_role, MembershipRole::Member);

        let updated: GroupMembershipResponse = r
            .request_json_as(
                &group_admin,
                Method::PATCH,
                &format!("/groups/{}/memberships/{}", group.id, created.membership.id),
                &UpdateGroupMembershipRequest { group_role: MembershipRole::Admin },
            )
            .await?
            .into_success()?;
        assert_eq!(updated.membership.group_role, MembershipRole::Admin);
        assert_ne!(updated.membership.id, created.membership.id);

        let listed: ListResponse<GroupMembership> = r
            .get_json_as(&group_admin, &format!("/groups/{}/memberships", group.id))
            .await?
            .into_success()?;
        let listed_membership = listed
            .items
            .iter()
            .find(|membership| membership.id == updated.membership.id)
            .expect("updated group membership should be listed");
        assert_eq!(listed_membership.user_username, member.username);
        assert_eq!(listed_membership.group_role, MembershipRole::Admin);

        let revoked: GroupMembershipResponse = r
            .empty_json_as(
                &group_admin,
                Method::POST,
                &format!("/groups/{}/memberships/{}/revoke", group.id, updated.membership.id),
            )
            .await?
            .into_success()?;
        assert_eq!(revoked.membership.user_username, member.username);
        assert_eq!(revoked.membership.revoked_by, Some(group_admin.id));
        assert!(revoked.membership.revoked_at.is_some());

        let listed: ListResponse<GroupMembership> = r
            .get_json_as(&group_admin, &format!("/groups/{}/memberships", group.id))
            .await?
            .into_success()?;
        assert!(!listed.items.iter().any(|membership| membership.id == updated.membership.id));

        let group_admin_membership_id =
            r.active_group_membership_id(group.id, group_admin.id).await?;
        let demoted: GroupMembershipResponse = r
            .request_json_as(
                &group_admin,
                Method::PATCH,
                &format!("/groups/{}/memberships/{group_admin_membership_id}", group.id),
                &UpdateGroupMembershipRequest { group_role: MembershipRole::Member },
            )
            .await?
            .into_success()?;
        assert_eq!(demoted.membership.group_role, MembershipRole::Member);

        let response = r
            .request(
                Some(&group_admin),
                Method::GET,
                &format!("/groups/{}/memberships", group.id),
                None,
            )
            .await?;
        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn group_member_cannot_manage_memberships(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("group membership member").await?;
        let member = r.create_user("group-regular-member").await?;
        let target = r.create_user("group-membership-target").await?;

        r.add_user_to_group(group.id, member.id, MembershipRole::Member).await?;

        let response = r
            .request(
                Some(&member),
                Method::POST,
                &format!("/groups/{}/memberships", group.id),
                Some(json!(CreateGroupMembershipRequest {
                    user_id: Some(target.id),
                    username: None,
                    group_role: MembershipRole::Member,
                })),
            )
            .await?;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        Ok(())
    }
}
