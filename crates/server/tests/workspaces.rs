//! Workspace API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        ArchiveStatus, CreateObjectRequest, CreateWorkspaceRequest, ListResponse, MembershipRole,
        PatchField, PinState, UpdateWorkspaceRequest, Workspace, WorkspaceListItem,
        WorkspaceMembership, WorkspaceMembershipResponse, WorkspaceResponse,
    };
    use kival_tests::{
        TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt, unique_name,
    };

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn creator_is_workspace_admin_and_can_manage_workspace(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let creator = r.create_user("workspace-creator").await?;
        let name = unique_name("creator workspace");

        let created: WorkspaceResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                "/workspaces",
                &CreateWorkspaceRequest {
                    name: name.clone(),
                    description: Some("  initial description  ".to_owned()),
                },
            )
            .await?
            .into_success()?;
        assert_eq!(created.workspace.name, name);
        assert_eq!(created.workspace.description.as_deref(), Some("initial description"));
        assert_eq!(created.workspace.created_by, Some(creator.id));
        assert_eq!(created.workspace.effective_role, MembershipRole::Admin);

        let fetched: WorkspaceResponse = r
            .get_json_as(&creator, &format!("/workspaces/{}", created.workspace.id))
            .await?
            .into_success()?;
        assert_eq!(fetched.workspace, created.workspace);

        let updated: WorkspaceResponse = r
            .request_json_as(
                &creator,
                Method::PATCH,
                &format!("/workspaces/{}", created.workspace.id),
                &UpdateWorkspaceRequest {
                    name: Some("  Updated Workspace  ".to_owned()),
                    description: PatchField::Null,
                },
            )
            .await?
            .into_success()?;
        assert_eq!(updated.workspace.name, "Updated Workspace");
        assert_eq!(updated.workspace.description, None);

        let memberships: ListResponse<WorkspaceMembership> = r
            .get_json_as(&creator, &format!("/workspaces/{}/memberships", created.workspace.id))
            .await?
            .into_success()?;
        assert!(memberships.items.iter().any(|membership| {
            membership.user_id == creator.id && membership.workspace_role == MembershipRole::Admin
        }));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_effective_role_reflects_current_authority_not_creator_provenance(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let creator = r.create_user("ws-role-creator").await?;
        let created: WorkspaceResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                "/workspaces",
                &CreateWorkspaceRequest { name: unique_name("role workspace"), description: None },
            )
            .await?
            .into_success()?;

        let membership_id =
            r.active_workspace_membership_id(created.workspace.id, creator.id).await?;
        let _: WorkspaceMembershipResponse = r
            .empty_json_as(
                &r.admin,
                Method::POST,
                &format!("/workspaces/{}/memberships/{membership_id}/revoke", created.workspace.id),
            )
            .await?
            .into_success()?;
        r.add_user_to_workspace(created.workspace.id, creator.id, MembershipRole::Member).await?;

        let fetched: WorkspaceResponse = r
            .get_json_as(&creator, &format!("/workspaces/{}", created.workspace.id))
            .await?
            .into_success()?;
        assert_eq!(fetched.workspace.created_by, Some(creator.id));
        assert_eq!(fetched.workspace.effective_role, MembershipRole::Member);

        let global_admin: WorkspaceResponse = r
            .get_json_as(&r.admin, &format!("/workspaces/{}", created.workspace.id))
            .await?
            .into_success()?;
        assert_eq!(global_admin.workspace.effective_role, MembershipRole::Admin);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_pagination_remains_complete_when_an_unseen_workspace_is_updated(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let creator = r.create_user("workspace-stable-page").await?;

        let older: WorkspaceResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                "/workspaces",
                &CreateWorkspaceRequest { name: unique_name("older workspace"), description: None },
            )
            .await?
            .into_success()?;
        let newer: WorkspaceResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                "/workspaces",
                &CreateWorkspaceRequest { name: unique_name("newer workspace"), description: None },
            )
            .await?
            .into_success()?;

        let first_page: ListResponse<Workspace> =
            r.get_json_as(&creator, "/workspaces?limit=1").await?.into_success()?;
        assert_eq!(first_page.items.len(), 1);
        assert_eq!(first_page.items[0].id, newer.workspace.id);
        let cursor = first_page.next_cursor.expect("a second workspace should remain");

        let updated: WorkspaceResponse = r
            .request_json_as(
                &creator,
                Method::PATCH,
                &format!("/workspaces/{}", older.workspace.id),
                &UpdateWorkspaceRequest {
                    name: Some(unique_name("updated unseen workspace")),
                    description: PatchField::Missing,
                },
            )
            .await?
            .into_success()?;
        assert!(updated.workspace.updated_at >= older.workspace.updated_at);

        let second_page: ListResponse<Workspace> = r
            .get_json_as(&creator, &format!("/workspaces?limit=1&cursor={cursor}"))
            .await?
            .into_success()?;
        assert_eq!(second_page.items.len(), 1);
        assert_eq!(second_page.items[0].id, older.workspace.id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn pinned_workspace_filter_is_independent_of_normal_pagination(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let creator = r.create_user("workspace-pin-pagination").await?;

        let older: WorkspaceResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                "/workspaces",
                &CreateWorkspaceRequest {
                    name: unique_name("older pinned workspace"),
                    description: None,
                },
            )
            .await?
            .into_success()?;
        let newer: WorkspaceResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                "/workspaces",
                &CreateWorkspaceRequest {
                    name: unique_name("newer unpinned workspace"),
                    description: None,
                },
            )
            .await?
            .into_success()?;

        let normal_before_pin: ListResponse<WorkspaceListItem> =
            r.get_json_as(&creator, "/workspaces?limit=1").await?.into_success()?;
        let hidden_id = if normal_before_pin.items[0].id == older.workspace.id {
            newer.workspace.id
        } else {
            older.workspace.id
        };

        let pin: PinState = r
            .empty_json_as(&creator, Method::POST, &format!("/workspaces/{hidden_id}/pin"))
            .await?
            .into_success()?;
        assert!(pin.pinned);

        let normal_after_pin: ListResponse<WorkspaceListItem> =
            r.get_json_as(&creator, "/workspaces?limit=1").await?.into_success()?;
        assert_ne!(normal_after_pin.items[0].id, hidden_id);

        let pinned: ListResponse<WorkspaceListItem> =
            r.get_json_as(&creator, "/workspaces?pinned=true&limit=1").await?.into_success()?;
        assert_eq!(pinned.items.len(), 1);
        assert_eq!(pinned.items[0].id, hidden_id);
        assert!(pinned.items[0].pinned);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn lists_workspaces_matching_name_case_insensitively(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let creator = r.create_user("workspace-name-search").await?;

        let matching: WorkspaceResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                "/workspaces",
                &CreateWorkspaceRequest {
                    name: unique_name("FindableWorkspaceNeedle"),
                    description: None,
                },
            )
            .await?
            .into_success()?;
        let unrelated: WorkspaceResponse = r
            .request_json_as(
                &creator,
                Method::POST,
                "/workspaces",
                &CreateWorkspaceRequest {
                    name: unique_name("UnrelatedWorkspace"),
                    description: None,
                },
            )
            .await?
            .into_success()?;

        let listed: ListResponse<Workspace> = r
            .get_json_as(&creator, "/workspaces?q=findableworkspaceneedle")
            .await?
            .into_success()?;

        assert!(listed.items.iter().any(|workspace| workspace.id == matching.workspace.id));
        assert!(!listed.items.iter().any(|workspace| workspace.id == unrelated.workspace.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn member_can_inspect_but_cannot_manage_workspace(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("member permissions").await?;
        let member = r
            .create_workspace_actor(workspace.id, "workspace-member", MembershipRole::Member)
            .await?;

        let fetched: WorkspaceResponse = r
            .get_json_as(&member, &format!("/workspaces/{}", workspace.id))
            .await?
            .into_success()?;
        assert_eq!(fetched.workspace.id, workspace.id);
        assert_eq!(fetched.workspace.effective_role, MembershipRole::Member);

        let listed: ListResponse<Workspace> =
            r.get_json_as(&member, "/workspaces").await?.into_success()?;
        assert!(listed.items.iter().any(|item| {
            item.id == workspace.id && item.effective_role == MembershipRole::Member
        }));

        let update = r
            .request_json_raw_as(
                &member,
                Method::PATCH,
                &format!("/workspaces/{}", workspace.id),
                &UpdateWorkspaceRequest {
                    name: Some("Forbidden Update".to_owned()),
                    description: PatchField::Missing,
                },
            )
            .await?;
        update.assert_status(StatusCode::FORBIDDEN);

        let archive = r
            .request(
                Some(&member),
                Method::POST,
                &format!("/workspaces/{}/archive", workspace.id),
                None,
            )
            .await?;
        archive.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_is_hidden_from_unrelated_user(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("hidden workspace").await?;
        let unrelated = r.create_user("workspace-unrelated").await?;

        let response = r
            .request(Some(&unrelated), Method::GET, &format!("/workspaces/{}", workspace.id), None)
            .await?;
        response.assert_status(StatusCode::FORBIDDEN);

        let listed: ListResponse<Workspace> =
            r.get_json_as(&unrelated, "/workspaces").await?.into_success()?;
        assert!(!listed.items.iter().any(|item| item.id == workspace.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn archived_workspace_rejects_object_creation(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("archived object creation").await?;
        let admin = r
            .create_workspace_actor(
                workspace.id,
                "archived-object-create-admin",
                MembershipRole::Admin,
            )
            .await?;

        let archived: WorkspaceResponse = r
            .empty_json_as(&admin, Method::POST, &format!("/workspaces/{}/archive", workspace.id))
            .await?
            .into_success()?;
        assert_eq!(archived.workspace.status, ArchiveStatus::Archived);

        let response = r
            .request_json_raw_as(
                &admin,
                Method::POST,
                &format!("/workspaces/{}/objects", workspace.id),
                &CreateObjectRequest {
                    title: "Archived Workspace Object".to_owned(),
                    body: "Body.".to_owned(),
                    metadata: kival_tests::object_metadata("archived-workspace-object"),
                },
            )
            .await?;
        response.assert_status(StatusCode::NOT_FOUND);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn archive_filters_and_unarchive_preserve_admin_access(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("archive lifecycle").await?;
        let admin = r
            .create_workspace_actor(
                workspace.id,
                "workspace-lifecycle-admin",
                MembershipRole::Admin,
            )
            .await?;

        let archived: WorkspaceResponse = r
            .empty_json_as(&admin, Method::POST, &format!("/workspaces/{}/archive", workspace.id))
            .await?
            .into_success()?;
        assert_eq!(archived.workspace.status, ArchiveStatus::Archived);
        assert_eq!(archived.workspace.archived_by, Some(admin.id));
        assert!(archived.workspace.archived_at.is_some());

        let active: ListResponse<Workspace> =
            r.get_json_as(&admin, "/workspaces").await?.into_success()?;
        assert!(!active.items.iter().any(|item| item.id == workspace.id));

        let archived_list: ListResponse<Workspace> =
            r.get_json_as(&admin, "/workspaces?status=archived").await?.into_success()?;
        assert!(archived_list.items.iter().any(|item| item.id == workspace.id));

        let fetched: WorkspaceResponse = r
            .get_json_as(&admin, &format!("/workspaces/{}", workspace.id))
            .await?
            .into_success()?;
        assert_eq!(fetched.workspace.status, ArchiveStatus::Archived);
        assert_eq!(fetched.workspace.archived_by, Some(admin.id));

        let update = r
            .request_json_raw_as(
                &admin,
                Method::PATCH,
                &format!("/workspaces/{}", workspace.id),
                &UpdateWorkspaceRequest {
                    name: Some("Archived Update".to_owned()),
                    description: PatchField::Missing,
                },
            )
            .await?;
        update.assert_status(StatusCode::NOT_FOUND);

        let unarchived: WorkspaceResponse = r
            .empty_json_as(&admin, Method::POST, &format!("/workspaces/{}/unarchive", workspace.id))
            .await?
            .into_success()?;
        assert_eq!(unarchived.workspace.status, ArchiveStatus::Active);
        assert_eq!(unarchived.workspace.archived_by, None);
        assert_eq!(unarchived.workspace.archived_at, None);

        Ok(())
    }
}
