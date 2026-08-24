//! Pagination API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        Group, ListResponse, ObjectListItem, ObjectVersion, User, Workspace, WorkspaceGroup,
    };
    use kival_tests::{
        TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt, object_metadata, test_body,
    };

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_list_returns_next_cursor_when_more_than_limit(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object pagination cursor").await?;
        for index in 0..3 {
            r.create_object(
                space.workspace.id,
                &format!("Paginated Object {index}"),
                &test_body(&format!("Paginated Object {index}"), "Body."),
                object_metadata("object-pagination"),
            )
            .await?;
        }

        let page: ListResponse<ObjectListItem> = r
            .get_json_as(&r.admin, &format!("/workspaces/{}/objects?limit=1", space.workspace.id))
            .await?
            .into_success()?;

        assert_eq!(page.items.len(), 1);
        assert!(page.next_cursor.is_some());

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_list_accepts_valid_next_cursor(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object pagination second page").await?;
        for index in 0..3 {
            r.create_object(
                space.workspace.id,
                &format!("Second Page Object {index}"),
                &test_body(&format!("Second Page Object {index}"), "Body."),
                object_metadata("object-pagination-second-page"),
            )
            .await?;
        }

        let first: ListResponse<ObjectListItem> = r
            .get_json_as(&r.admin, &format!("/workspaces/{}/objects?limit=1", space.workspace.id))
            .await?
            .into_success()?;
        let cursor = first.next_cursor.expect("first page should include a cursor");

        let second: ListResponse<ObjectListItem> = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/objects?limit=1&cursor={cursor}", space.workspace.id),
            )
            .await?
            .into_success()?;

        assert_eq!(second.items.len(), 1);
        assert_ne!(second.items[0].id, first.items[0].id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_list_rejects_invalid_cursor(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object invalid cursor").await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!("/workspaces/{}/objects?cursor=not-a-cursor", space.workspace.id),
                None,
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_list_rejects_cursor_from_wrong_scope(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let first_space = r.object_space("object cursor first scope").await?;
        let second_space = r.object_space("object cursor second scope").await?;
        for index in 0..2 {
            r.create_object(
                first_space.workspace.id,
                &format!("First Scope Object {index}"),
                &test_body(&format!("First Scope Object {index}"), "Body."),
                object_metadata("object-cursor-first-scope"),
            )
            .await?;
        }

        let first: ListResponse<ObjectListItem> = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/objects?limit=1", first_space.workspace.id),
            )
            .await?
            .into_success()?;
        let cursor = first.next_cursor.expect("first scope should produce a cursor");

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!("/workspaces/{}/objects?cursor={cursor}", second_space.workspace.id),
                None,
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_version_list_returns_next_cursor(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("version pagination").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Version Pagination Object",
                &test_body("Version Pagination Object", "Version 1."),
                object_metadata("version-pagination"),
            )
            .await?;
        for index in 0..2 {
            let title = format!("Version Pagination Object {index}");
            let body = format!("Version body {index}.");
            r.update_object(
                space.workspace.id,
                object.id,
                Some(&title),
                Some(&body),
                Some(object_metadata("version-pagination")),
            )
            .await?;
        }

        let page: ListResponse<ObjectVersion> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/versions?limit=1",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;

        assert_eq!(page.items.len(), 1);
        assert!(page.next_cursor.is_some());

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_version_list_accepts_valid_next_cursor(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("version pagination second page").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Version Second Page Object",
                &test_body("Version Second Page Object", "Version 1."),
                object_metadata("version-pagination-second-page"),
            )
            .await?;
        for index in 0..2 {
            let title = format!("Version Second Page Object {index}");
            let body = format!("Version body {index}.");
            r.update_object(
                space.workspace.id,
                object.id,
                Some(&title),
                Some(&body),
                Some(object_metadata("version-pagination-second-page")),
            )
            .await?;
        }

        let first: ListResponse<ObjectVersion> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/versions?limit=1",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;
        let cursor = first.next_cursor.expect("first version page should include a cursor");

        let second: ListResponse<ObjectVersion> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/versions?limit=1&cursor={cursor}",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;

        assert_eq!(second.items.len(), 1);
        assert_ne!(second.items[0].id, first.items[0].id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_version_list_rejects_wrong_cursor_kind(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("version wrong cursor").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Version Wrong Cursor Object",
                &test_body("Version Wrong Cursor Object", "Version 1."),
                object_metadata("version-wrong-cursor"),
            )
            .await?;
        for index in 0..2 {
            r.create_object(
                space.workspace.id,
                &format!("Cursor Kind Object {index}"),
                &test_body(&format!("Cursor Kind Object {index}"), "Body."),
                object_metadata("version-wrong-cursor-object"),
            )
            .await?;
        }

        let object_page: ListResponse<ObjectListItem> = r
            .get_json_as(&r.admin, &format!("/workspaces/{}/objects?limit=1", space.workspace.id))
            .await?
            .into_success()?;
        let wrong_cursor = object_page.next_cursor.expect("object list should produce a cursor");

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!(
                    "/workspaces/{}/objects/{}/versions?cursor={wrong_cursor}",
                    space.workspace.id, object.id,
                ),
                None,
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn group_list_paginates_with_status_all(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let active = r.create_group("group pagination active").await?;
        let archived = r.create_group("group pagination archived").await?;
        r.archive_group(archived.id).await?;

        let first: ListResponse<Group> =
            r.get_json_as(&r.admin, "/groups?status=all&limit=1").await?.into_success()?;
        assert_eq!(first.items.len(), 1);

        let mut listed_ids = first.items.iter().map(|group| group.id).collect::<Vec<_>>();
        let mut cursor = first.next_cursor;

        while !(listed_ids.contains(&active.id) && listed_ids.contains(&archived.id)) {
            let Some(next_cursor) = cursor.take() else {
                break;
            };

            let page: ListResponse<Group> = r
                .get_json_as(&r.admin, &format!("/groups?status=all&limit=1&cursor={next_cursor}"))
                .await?
                .into_success()?;
            listed_ids.extend(page.items.iter().map(|group| group.id));
            cursor = page.next_cursor;
        }

        assert!(listed_ids.contains(&active.id));
        assert!(listed_ids.contains(&archived.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_list_paginates_with_status_archived(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object archived pagination").await?;
        let mut archived_ids = Vec::new();
        for index in 0..2 {
            let object = r
                .create_object(
                    space.workspace.id,
                    &format!("Archived Page Object {index}"),
                    &test_body(&format!("Archived Page Object {index}"), "Body."),
                    object_metadata("object-archived-pagination"),
                )
                .await?;
            r.archive_object(space.workspace.id, object.id).await?;
            archived_ids.push(object.id);
        }

        let first: ListResponse<ObjectListItem> = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/objects?status=archived&limit=1", space.workspace.id),
            )
            .await?
            .into_success()?;
        let cursor = first.next_cursor.expect("archived object list should produce a cursor");

        let second: ListResponse<ObjectListItem> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects?status=archived&limit=1&cursor={cursor}",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;

        let listed_ids = first
            .items
            .iter()
            .chain(second.items.iter())
            .map(|object| object.id)
            .collect::<Vec<_>>();
        assert!(archived_ids.iter().all(|id| listed_ids.contains(id)));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn group_list_paginates_with_status_archived(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let active = r.create_group("group archived pagination active").await?;
        let mut archived_ids = Vec::new();
        for index in 0..2 {
            let group = r.create_group(&format!("group archived pagination {index}")).await?;
            r.archive_group(group.id).await?;
            archived_ids.push(group.id);
        }

        let first: ListResponse<Group> =
            r.get_json_as(&r.admin, "/groups?status=archived&limit=1").await?.into_success()?;
        assert_eq!(first.items.len(), 1);

        let mut listed_ids = first.items.iter().map(|group| group.id).collect::<Vec<_>>();
        let mut cursor = first.next_cursor;

        while !archived_ids.iter().all(|id| listed_ids.contains(id)) {
            let Some(next_cursor) = cursor.take() else {
                break;
            };

            let page: ListResponse<Group> = r
                .get_json_as(
                    &r.admin,
                    &format!("/groups?status=archived&limit=1&cursor={next_cursor}"),
                )
                .await?
                .into_success()?;
            listed_ids.extend(page.items.iter().map(|group| group.id));
            cursor = page.next_cursor;
        }

        assert!(archived_ids.iter().all(|id| listed_ids.contains(id)));
        assert!(!listed_ids.contains(&active.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_list_paginates_with_status_all(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object all pagination").await?;
        let active = r
            .create_object(
                space.workspace.id,
                "All Page Active Object",
                &test_body("All Page Active Object", "Body."),
                object_metadata("object-all-pagination-active"),
            )
            .await?;
        let archived = r
            .create_object(
                space.workspace.id,
                "All Page Archived Object",
                &test_body("All Page Archived Object", "Body."),
                object_metadata("object-all-pagination-archived"),
            )
            .await?;
        r.archive_object(space.workspace.id, archived.id).await?;

        let first: ListResponse<ObjectListItem> = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/objects?status=all&limit=1", space.workspace.id),
            )
            .await?
            .into_success()?;
        let cursor = first.next_cursor.expect("all object list should produce a cursor");

        let second: ListResponse<ObjectListItem> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects?status=all&limit=1&cursor={cursor}",
                    space.workspace.id
                ),
            )
            .await?
            .into_success()?;

        let listed_ids = first
            .items
            .iter()
            .chain(second.items.iter())
            .map(|object| object.id)
            .collect::<Vec<_>>();
        assert!(listed_ids.contains(&active.id));
        assert!(listed_ids.contains(&archived.id));

        Ok(())
    }
    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_list_cursor_is_bound_to_status_and_order(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object cursor filters").await?;
        for index in 0..2 {
            r.create_object(
                space.workspace.id,
                &format!("Cursor Filter Object {index}"),
                &test_body(&format!("Cursor Filter Object {index}"), "Body."),
                object_metadata("cursor-filter"),
            )
            .await?;
        }

        let first: ListResponse<ObjectListItem> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects?status=active&order=created&limit=1",
                    space.workspace.id
                ),
            )
            .await?
            .into_success()?;
        let cursor = first.next_cursor.expect("filtered list should produce a cursor");

        for query in [
            format!(
                "/workspaces/{}/objects?status=archived&order=created&cursor={cursor}",
                space.workspace.id
            ),
            format!(
                "/workspaces/{}/objects?status=active&order=updated&cursor={cursor}",
                space.workspace.id
            ),
        ] {
            let response = r.request(Some(&r.admin), Method::GET, &query, None).await?;
            response.assert_status(StatusCode::BAD_REQUEST);
        }

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn filtered_collection_cursors_reject_changed_filters(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        r.create_user("cursor-user-one").await?;
        r.create_user("cursor-user-two").await?;
        let users: ListResponse<User> =
            r.get_json_as(&r.admin, "/users?status=active&limit=1").await?.into_success()?;
        let user_cursor = users.next_cursor.expect("user list should produce a cursor");
        r.request(
            Some(&r.admin),
            Method::GET,
            &format!("/users?status=disabled&cursor={user_cursor}"),
            None,
        )
        .await?
        .assert_status(StatusCode::BAD_REQUEST);

        let filtered_users: ListResponse<User> = r
            .get_json_as(&r.admin, "/users?status=active&q=cursor-user&limit=1")
            .await?
            .into_success()?;
        let filtered_user_cursor =
            filtered_users.next_cursor.expect("filtered user list should produce a cursor");
        r.request(
            Some(&r.admin),
            Method::GET,
            &format!("/users?status=active&q=different-query&cursor={filtered_user_cursor}"),
            None,
        )
        .await?
        .assert_status(StatusCode::BAD_REQUEST);

        r.create_workspace("cursor-workspace-one").await?;
        r.create_workspace("cursor-workspace-two").await?;
        let workspaces: ListResponse<Workspace> =
            r.get_json_as(&r.admin, "/workspaces?status=active&limit=1").await?.into_success()?;
        let workspace_cursor =
            workspaces.next_cursor.expect("workspace list should produce a cursor");
        r.request(
            Some(&r.admin),
            Method::GET,
            &format!("/workspaces?status=archived&cursor={workspace_cursor}"),
            None,
        )
        .await?
        .assert_status(StatusCode::BAD_REQUEST);

        r.create_group("cursor-group-one").await?;
        r.create_group("cursor-group-two").await?;
        let groups: ListResponse<Group> =
            r.get_json_as(&r.admin, "/groups?status=active&limit=1").await?.into_success()?;
        let group_cursor = groups.next_cursor.expect("group list should produce a cursor");
        r.request(
            Some(&r.admin),
            Method::GET,
            &format!("/groups?status=archived&cursor={group_cursor}"),
            None,
        )
        .await?
        .assert_status(StatusCode::BAD_REQUEST);

        let workspace = r.create_workspace("cursor-workspace-groups").await?;
        let first_group = r.create_group("cursor-linked-group-one").await?;
        let second_group = r.create_group("cursor-linked-group-two").await?;
        r.add_group_to_workspace(workspace.id, first_group.id).await?;
        r.add_group_to_workspace(workspace.id, second_group.id).await?;
        let links: ListResponse<WorkspaceGroup> = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/groups?status=active&limit=1", workspace.id),
            )
            .await?
            .into_success()?;
        let link_cursor = links.next_cursor.expect("workspace-group list should produce a cursor");
        r.request(
            Some(&r.admin),
            Method::GET,
            &format!("/workspaces/{}/groups?status=archived&cursor={link_cursor}", workspace.id),
            None,
        )
        .await?
        .assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_list_orders_and_paginates_by_updated_at(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("updated object ordering").await?;
        let first = r
            .create_object(
                space.workspace.id,
                "Updated Ordering First",
                &test_body("Updated Ordering First", "Initial."),
                object_metadata("updated-ordering-first"),
            )
            .await?;
        let second = r
            .create_object(
                space.workspace.id,
                "Updated Ordering Second",
                &test_body("Updated Ordering Second", "Initial."),
                object_metadata("updated-ordering-second"),
            )
            .await?;
        r.update_object(space.workspace.id, first.id, None, Some("Updated most recently."), None)
            .await?;

        let page: ListResponse<ObjectListItem> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects?status=active&order=updated&limit=1",
                    space.workspace.id
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(page.items[0].id, first.id);
        let cursor = page.next_cursor.expect("updated ordering should paginate");

        let next: ListResponse<ObjectListItem> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects?status=active&order=updated&limit=1&cursor={cursor}",
                    space.workspace.id
                ),
            )
            .await?
            .into_success()?;
        assert!(next.items.iter().any(|object| object.id == second.id));

        Ok(())
    }
}
