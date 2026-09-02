//! Workspace-group link API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        ArchiveStatus, CreateWorkspaceGroupRequest, ListResponse, MembershipRole, WorkspaceGroup,
        WorkspaceGroupResponse,
    };
    use kival_tests::{TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt};

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_admin_can_link_list_archive_and_unarchive_group(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("workspace group lifecycle").await?;
        let group_name = "workspace linked group";
        let group = r.create_group(group_name).await?;
        let workspace_admin = r
            .create_workspace_actor(workspace.id, "workspace-group-admin", MembershipRole::Admin)
            .await?;

        let linked: WorkspaceGroupResponse = r
            .request_json_as(
                &workspace_admin,
                Method::POST,
                &format!("/workspaces/{}/groups", workspace.id),
                &CreateWorkspaceGroupRequest { group_id: group.id },
            )
            .await?
            .into_success()?;
        assert_eq!(linked.workspace_group.workspace_id, workspace.id);
        assert_eq!(linked.workspace_group.group_id, group.id);
        assert!(linked.workspace_group.group_name.starts_with(group_name));
        assert_eq!(
            linked.workspace_group.group_description.as_deref(),
            Some("Test group for workspace linked group")
        );
        let created_group_name = linked.workspace_group.group_name.clone();
        assert_eq!(linked.workspace_group.status, ArchiveStatus::Active);

        let listed: ListResponse<WorkspaceGroup> = r
            .get_json_as(&workspace_admin, &format!("/workspaces/{}/groups", workspace.id))
            .await?
            .into_success()?;
        let listed_group = listed
            .items
            .iter()
            .find(|item| item.id == linked.workspace_group.id)
            .expect("linked group should be listed");
        assert_eq!(listed_group.group_name, created_group_name);

        let archived: WorkspaceGroupResponse = r
            .empty_json_as(
                &workspace_admin,
                Method::POST,
                &format!("/workspaces/{}/groups/{}/archive", workspace.id, group.id),
            )
            .await?
            .into_success()?;
        assert_eq!(archived.workspace_group.status, ArchiveStatus::Archived);
        assert_eq!(archived.workspace_group.group_name, created_group_name);
        assert_eq!(archived.workspace_group.archived_by, Some(workspace_admin.id));
        assert!(archived.workspace_group.archived_at.is_some());

        let listed: ListResponse<WorkspaceGroup> = r
            .get_json_as(&workspace_admin, &format!("/workspaces/{}/groups", workspace.id))
            .await?
            .into_success()?;
        assert!(!listed.items.iter().any(|item| item.id == linked.workspace_group.id));

        let archived_list: ListResponse<WorkspaceGroup> = r
            .get_json_as(
                &workspace_admin,
                &format!("/workspaces/{}/groups?status=archived", workspace.id),
            )
            .await?
            .into_success()?;
        assert!(
            archived_list.items.iter().any(|item| item.id == linked.workspace_group.id),
            "archived links must remain discoverable so administrators can restore them"
        );

        let unarchived: WorkspaceGroupResponse = r
            .empty_json_as(
                &workspace_admin,
                Method::POST,
                &format!("/workspaces/{}/groups/{}/unarchive", workspace.id, group.id),
            )
            .await?
            .into_success()?;
        assert_eq!(unarchived.workspace_group.status, ArchiveStatus::Active);
        assert_eq!(unarchived.workspace_group.group_name, created_group_name);
        assert_eq!(unarchived.workspace_group.archived_by, None);
        assert_eq!(unarchived.workspace_group.archived_at, None);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_group_link_maps_archived_group_to_bad_request(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("workspace archived group link").await?;
        let group = r.create_group("archived group link target").await?;
        let workspace_admin = r
            .create_workspace_actor(
                workspace.id,
                "archived-group-link-admin",
                MembershipRole::Admin,
            )
            .await?;

        r.archive_group(group.id).await?;

        let response = r
            .request_json_raw_as(
                &workspace_admin,
                Method::POST,
                &format!("/workspaces/{}/groups", workspace.id),
                &CreateWorkspaceGroupRequest { group_id: group.id },
            )
            .await?;
        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_group_unarchive_maps_archived_group_to_not_found(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("workspace group restore active group").await?;
        let group = r.create_group("workspace group restore target").await?;
        let workspace_admin = r
            .create_workspace_actor(
                workspace.id,
                "workspace-group-restore-admin",
                MembershipRole::Admin,
            )
            .await?;
        r.add_group_to_workspace(workspace.id, group.id).await?;

        let _: WorkspaceGroupResponse = r
            .empty_json_as(
                &workspace_admin,
                Method::POST,
                &format!("/workspaces/{}/groups/{}/archive", workspace.id, group.id),
            )
            .await?
            .into_success()?;
        r.archive_group(group.id).await?;

        let response = r
            .request(
                Some(&workspace_admin),
                Method::POST,
                &format!("/workspaces/{}/groups/{}/unarchive", workspace.id, group.id),
                None,
            )
            .await?;
        response.assert_status(StatusCode::NOT_FOUND);

        Ok(())
    }
}
