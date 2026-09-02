//! Object list lifecycle and visibility scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        GrantPrincipal, ListResponse, MembershipRole, ObjectListItem, ObjectRole, PinState,
    };
    use kival_tests::{
        TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt, object_metadata, test_body,
    };

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_admin_lists_objects_by_archive_status(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object status list").await?;
        let active = r
            .create_object(
                space.workspace.id,
                "Active Listed Object",
                &test_body("Active Listed Object", "Body."),
                object_metadata("active-listed-object"),
            )
            .await?;
        let archived = r
            .create_object(
                space.workspace.id,
                "Archived Listed Object",
                &test_body("Archived Listed Object", "Body."),
                object_metadata("archived-listed-object"),
            )
            .await?;
        r.archive_object(space.workspace.id, archived.id).await?;

        let active_list: ListResponse<ObjectListItem> = r
            .get_json_as(&r.admin, &format!("/workspaces/{}/objects", space.workspace.id))
            .await?
            .into_success()?;
        assert!(active_list.items.iter().any(|object| object.id == active.id));
        assert!(!active_list.items.iter().any(|object| object.id == archived.id));

        let archived_list: ListResponse<ObjectListItem> = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/objects?status=archived", space.workspace.id),
            )
            .await?
            .into_success()?;
        assert!(!archived_list.items.iter().any(|object| object.id == active.id));
        assert!(archived_list.items.iter().any(|object| object.id == archived.id));

        let all: ListResponse<ObjectListItem> = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/objects?status=all", space.workspace.id),
            )
            .await?
            .into_success()?;
        assert!(all.items.iter().any(|object| object.id == active.id));
        assert!(all.items.iter().any(|object| object.id == archived.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn pinned_object_filter_is_independent_of_normal_pagination(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object pin pagination").await?;
        let older = r
            .create_object(
                space.workspace.id,
                "Older Pinned Object",
                &test_body("Older Pinned Object", "Body."),
                object_metadata("older-pinned-object"),
            )
            .await?;
        let newer = r
            .create_object(
                space.workspace.id,
                "Newer Unpinned Object",
                &test_body("Newer Unpinned Object", "Body."),
                object_metadata("newer-unpinned-object"),
            )
            .await?;

        let normal_before_pin: ListResponse<ObjectListItem> = r
            .get_json_as(&r.admin, &format!("/workspaces/{}/objects?limit=1", space.workspace.id))
            .await?
            .into_success()?;
        let hidden_id = if normal_before_pin.items[0].id == older.id { newer.id } else { older.id };

        let pin: PinState = r
            .empty_json_as(
                &r.admin,
                Method::POST,
                &format!("/workspaces/{}/objects/{hidden_id}/pin", space.workspace.id),
            )
            .await?
            .into_success()?;
        assert!(pin.pinned);

        let normal_after_pin: ListResponse<ObjectListItem> = r
            .get_json_as(&r.admin, &format!("/workspaces/{}/objects?limit=1", space.workspace.id))
            .await?
            .into_success()?;
        assert_ne!(normal_after_pin.items[0].id, hidden_id);

        let pinned: ListResponse<ObjectListItem> = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/objects?pinned=true&limit=1", space.workspace.id),
            )
            .await?
            .into_success()?;
        assert_eq!(pinned.items.len(), 1);
        assert_eq!(pinned.items[0].id, hidden_id);
        assert!(pinned.items[0].pinned);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn explicit_object_admin_lists_archived_objects_but_viewer_does_not(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object archived visibility").await?;
        let actor = r.create_user("archived-object-list-actor").await?;
        r.add_user_to_workspace(space.workspace.id, actor.id, MembershipRole::Member).await?;

        let administered = r
            .create_object(
                space.workspace.id,
                "Administered Archived Object",
                &test_body("Administered Archived Object", "Body."),
                object_metadata("administered-archived-object"),
            )
            .await?;
        let viewed = r
            .create_object(
                space.workspace.id,
                "Viewed Archived Object",
                &test_body("Viewed Archived Object", "Body."),
                object_metadata("viewed-archived-object"),
            )
            .await?;
        r.create_object_grant(
            space.workspace.id,
            administered.id,
            GrantPrincipal::User(actor.id),
            ObjectRole::Admin,
        )
        .await?;
        r.create_object_grant(
            space.workspace.id,
            viewed.id,
            GrantPrincipal::User(actor.id),
            ObjectRole::Viewer,
        )
        .await?;
        r.archive_object(space.workspace.id, administered.id).await?;
        r.archive_object(space.workspace.id, viewed.id).await?;

        let archived: ListResponse<ObjectListItem> = r
            .get_json_as(
                &actor,
                &format!("/workspaces/{}/objects?status=archived", space.workspace.id),
            )
            .await?
            .into_success()?;
        assert!(archived.items.iter().any(|object| object.id == administered.id));
        assert!(!archived.items.iter().any(|object| object.id == viewed.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn explicit_object_editor_does_not_list_archived_objects(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("archived object editor list").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Edited Archived Object",
                &test_body("Edited Archived Object", "Body."),
                object_metadata("edited-archived-object"),
            )
            .await?;
        let editor = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "archived-object-list-editor",
                MembershipRole::Member,
                ObjectRole::Editor,
            )
            .await?;
        r.archive_object(space.workspace.id, object.id).await?;

        let archived: ListResponse<ObjectListItem> = r
            .get_json_as(
                &editor,
                &format!("/workspaces/{}/objects?status=archived", space.workspace.id),
            )
            .await?
            .into_success()?;

        assert!(!archived.items.iter().any(|item| item.id == object.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_list_rejects_invalid_status_filter(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("invalid object status").await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!("/workspaces/{}/objects?status=deleted", workspace.id),
                None,
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);
        Ok(())
    }
}
