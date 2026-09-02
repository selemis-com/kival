//! Group API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        ArchiveStatus, CreateGroupMembershipRequest, Group, GroupResponse, ListResponse,
        MembershipRole, PatchField, UpdateGroupRequest,
    };
    use kival_tests::{TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt};
    use serde_json::json;

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn group_admin_can_read_active_group_but_unrelated_user_cannot(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("group readable").await?;
        let group_admin = r.create_user("group-reader-admin").await?;
        let unrelated = r.create_user("group-reader-unrelated").await?;
        r.add_user_to_group(group.id, group_admin.id, MembershipRole::Admin).await?;

        let fetched: GroupResponse =
            r.get_json_as(&group_admin, &format!("/groups/{}", group.id)).await?.into_success()?;
        assert_eq!(fetched.group.id, group.id);
        assert_eq!(fetched.group.status, ArchiveStatus::Active);

        let response = r
            .request(Some(&unrelated), Method::GET, &format!("/groups/{}", group.id), None)
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn group_admin_can_read_archived_group_but_cannot_manage_memberships(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("archived group readable").await?;
        let group_admin = r.create_user("archived-group-admin").await?;
        let target = r.create_user("archived-group-target").await?;
        r.add_user_to_group(group.id, group_admin.id, MembershipRole::Admin).await?;

        let archived: GroupResponse = r
            .empty_json_as(&r.admin, Method::POST, &format!("/groups/{}/archive", group.id))
            .await?
            .into_success()?;
        assert_eq!(archived.group.status, ArchiveStatus::Archived);

        let fetched: GroupResponse =
            r.get_json_as(&group_admin, &format!("/groups/{}", group.id)).await?.into_success()?;
        assert_eq!(fetched.group.status, ArchiveStatus::Archived);

        let response = r
            .request(
                Some(&group_admin),
                Method::POST,
                &format!("/groups/{}/memberships", group.id),
                Some(json!(CreateGroupMembershipRequest {
                    user_id: Some(target.id),
                    username: None,
                    group_role: MembershipRole::Member,
                })),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn global_group_list_filters_by_archive_status(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let active_group = r.create_group("active group list").await?;
        let archived_group = r.create_group("archived group list").await?;

        let _: GroupResponse = r
            .empty_json_as(
                &r.admin,
                Method::POST,
                &format!("/groups/{}/archive", archived_group.id),
            )
            .await?
            .into_success()?;

        let active: ListResponse<Group> =
            r.get_json_as(&r.admin, "/groups").await?.into_success()?;
        assert!(active.items.iter().any(|group| group.id == active_group.id));
        assert!(!active.items.iter().any(|group| group.id == archived_group.id));

        let archived: ListResponse<Group> =
            r.get_json_as(&r.admin, "/groups?status=archived").await?.into_success()?;
        assert!(!archived.items.iter().any(|group| group.id == active_group.id));
        assert!(archived.items.iter().any(|group| group.id == archived_group.id));

        let all: ListResponse<Group> =
            r.get_json_as(&r.admin, "/groups?status=all").await?.into_success()?;
        assert!(all.items.iter().any(|group| group.id == active_group.id));
        assert!(all.items.iter().any(|group| group.id == archived_group.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn global_group_list_searches_names_case_insensitively(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let matching = r.create_group("Searchable Atlas Team").await?;
        let unrelated = r.create_group("Unrelated Group").await?;

        let groups: ListResponse<Group> =
            r.get_json_as(&r.admin, "/groups?q=ATLAS").await?.into_success()?;

        assert!(groups.items.iter().any(|group| group.id == matching.id));
        assert!(!groups.items.iter().any(|group| group.id == unrelated.id));
        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn group_admin_lists_only_administered_groups(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let administered = r.create_group("group list administered").await?;
        let unrelated = r.create_group("group list unrelated").await?;
        let group_admin = r.create_user("group-list-admin").await?;
        r.add_user_to_group(administered.id, group_admin.id, MembershipRole::Admin).await?;

        let groups: ListResponse<Group> =
            r.get_json_as(&group_admin, "/groups").await?.into_success()?;

        assert!(groups.items.iter().any(|group| group.id == administered.id));
        assert!(!groups.items.iter().any(|group| group.id == unrelated.id));
        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn groups_list_rejects_invalid_status_filter(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let response =
            r.request(Some(&r.admin), Method::GET, "/groups?status=deleted", None).await?;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn global_admin_can_get_and_unarchive_group_but_group_admin_cannot_unarchive(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("group lifecycle control").await?;
        let group_admin = r.create_user("group-lifecycle-admin").await?;
        r.add_user_to_group(group.id, group_admin.id, MembershipRole::Admin).await?;

        let archived: GroupResponse = r
            .empty_json_as(&r.admin, Method::POST, &format!("/groups/{}/archive", group.id))
            .await?
            .into_success()?;
        assert_eq!(archived.group.status, ArchiveStatus::Archived);
        assert_eq!(archived.group.archived_by, Some(r.admin.id));
        assert!(archived.group.archived_at.is_some());

        let fetched: GroupResponse =
            r.get_json_as(&r.admin, &format!("/groups/{}", group.id)).await?.into_success()?;
        assert_eq!(fetched.group.status, ArchiveStatus::Archived);

        let response = r
            .request(
                Some(&group_admin),
                Method::POST,
                &format!("/groups/{}/unarchive", group.id),
                None,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let unarchived: GroupResponse = r
            .empty_json_as(&r.admin, Method::POST, &format!("/groups/{}/unarchive", group.id))
            .await?
            .into_success()?;
        assert_eq!(unarchived.group.status, ArchiveStatus::Active);
        assert_eq!(unarchived.group.archived_by, None);
        assert_eq!(unarchived.group.archived_at, None);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn group_admin_role_does_not_grant_workspace_access(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("group without workspace access").await?;
        let group_admin = r.create_user("isolated-group-admin").await?;
        let workspace = r.create_workspace("unrelated workspace").await?;
        r.add_user_to_group(group.id, group_admin.id, MembershipRole::Admin).await?;

        let response = r
            .request(
                Some(&group_admin),
                Method::GET,
                &format!("/workspaces/{}", workspace.id),
                None,
            )
            .await?;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn archived_group_membership_endpoints_return_not_found_for_global_admin(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("archived membership immutable").await?;
        let existing = r.create_user("archived-existing-member").await?;
        let target = r.create_user("archived-new-member").await?;
        r.add_user_to_group(group.id, existing.id, MembershipRole::Member).await?;

        let membership_id = r.active_group_membership_id(group.id, existing.id).await?;

        r.archive_group(group.id).await?;

        let create = r
            .request_json_raw_as(
                &r.admin,
                Method::POST,
                &format!("/groups/{}/memberships", group.id),
                &CreateGroupMembershipRequest {
                    user_id: Some(target.id),
                    username: None,
                    group_role: MembershipRole::Member,
                },
            )
            .await?;
        create.assert_status(StatusCode::NOT_FOUND);

        let revoke = r
            .request(
                Some(&r.admin),
                Method::POST,
                &format!("/groups/{}/memberships/{membership_id}/revoke", group.id),
                None,
            )
            .await?;
        revoke.assert_status(StatusCode::NOT_FOUND);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn global_admin_can_update_group_name_and_description(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("group update").await?;

        let updated: GroupResponse = r
            .request_json_as(
                &r.admin,
                Method::PATCH,
                &format!("/groups/{}", group.id),
                &UpdateGroupRequest {
                    name: Some("  Updated Group  ".to_owned()),
                    description: PatchField::Value("  Updated description  ".to_owned()),
                },
            )
            .await?
            .into_success()?;

        assert_eq!(updated.group.name, "Updated Group");
        assert_eq!(updated.group.description.as_deref(), Some("Updated description"));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn update_group_rejects_empty_patch(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("group empty update").await?;

        let response = r
            .request_json_raw_as(
                &r.admin,
                Method::PATCH,
                &format!("/groups/{}", group.id),
                &UpdateGroupRequest { name: None, description: PatchField::Missing },
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn update_group_rejects_archived_group(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("group archived update").await?;
        r.archive_group(group.id).await?;

        let response = r
            .request_json_raw_as(
                &r.admin,
                Method::PATCH,
                &format!("/groups/{}", group.id),
                &UpdateGroupRequest {
                    name: Some("Archived Update".to_owned()),
                    description: PatchField::Missing,
                },
            )
            .await?;

        response.assert_status(StatusCode::NOT_FOUND);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn group_admin_cannot_update_group_metadata(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("group admin metadata denied").await?;
        let group_admin = r.create_user("group-admin-metadata-denied").await?;
        r.add_user_to_group(group.id, group_admin.id, MembershipRole::Admin).await?;

        let response = r
            .request_json_raw_as(
                &group_admin,
                Method::PATCH,
                &format!("/groups/{}", group.id),
                &UpdateGroupRequest {
                    name: Some("Forbidden Group Update".to_owned()),
                    description: PatchField::Missing,
                },
            )
            .await?;

        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }
}
