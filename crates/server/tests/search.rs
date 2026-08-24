//! Search API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        GrantPrincipal, MAX_LIMIT, MembershipRole, ObjectRole, SearchMatchKind, SearchResponse,
    };
    use kival_tests::{
        TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt, object_metadata, test_body,
    };
    use kival_types::{ArchiveStatus, SearchCategory};

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_rejects_users_without_workspace_access(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("search workspace access").await?;
        let outsider = r.create_user("search-workspace-outsider").await?;

        let response = r
            .request(
                Some(&outsider),
                Method::GET,
                &format!(
                    "/workspaces/{}/search?q=missing&categories=title&mode=exact",
                    workspace.id
                ),
                None,
            )
            .await?;

        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_hides_objects_without_object_access(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let workspace = r.create_workspace("search auth").await?;

        let visible_group = r.create_group("visible search editors").await?;
        let hidden_group = r.create_group("hidden search editors").await?;

        r.add_group_to_workspace(workspace.id, visible_group.id).await?;
        r.add_group_to_workspace(workspace.id, hidden_group.id).await?;

        let reader = r.create_user("search-reader").await?;

        r.add_user_to_workspace(workspace.id, reader.id, MembershipRole::Member).await?;
        r.add_user_to_group(visible_group.id, reader.id, MembershipRole::Member).await?;

        let needle = "kivalsearchneedlehidden";

        let visible_object = r
            .create_object(
                workspace.id,
                "Visible Search Object",
                &test_body("Visible Search Object", &format!("Visible body containing {needle}.")),
                object_metadata("visible-object"),
            )
            .await?;

        let hidden_object = r
            .create_object(
                workspace.id,
                "Hidden Search Object",
                &test_body("Hidden Search Object", &format!("Hidden body containing {needle}.")),
                object_metadata("hidden-object"),
            )
            .await?;

        r.create_object_grant(
            workspace.id,
            visible_object.id,
            GrantPrincipal::Group(visible_group.id),
            ObjectRole::Viewer,
        )
        .await?;

        let admin_results = r.search_as(&r.admin, workspace.id, needle).await?;

        assert!(
            admin_results.items.iter().any(|hit| hit.object_id == visible_object.id),
            "admin should see the visible object search hit",
        );

        assert!(
            admin_results.items.iter().any(|hit| hit.object_id == hidden_object.id),
            "admin should see the hidden object search hit",
        );

        let reader_results = r.search_as(&reader, workspace.id, needle).await?;

        assert!(
            reader_results.items.iter().any(|hit| hit.object_id == visible_object.id),
            "reader should see search hits for granted objects",
        );

        assert!(
            reader_results.items.iter().all(|hit| hit.object_id != hidden_object.id),
            "reader should not see search hits for unreadable objects",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_returns_objects_with_direct_viewer_grant(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("search direct grant").await?;
        let workspace = space.workspace;

        let reader = r.create_user("search-direct-reader").await?;

        r.add_user_to_workspace(workspace.id, reader.id, MembershipRole::Member).await?;

        let needle = "kivalsearchneedledirect";

        let object = r
            .create_object(
                workspace.id,
                "Direct Grant Search Object",
                &test_body(
                    "Direct Grant Search Object",
                    &format!("Searchable body containing {needle}."),
                ),
                object_metadata("direct-grant-object"),
            )
            .await?;

        r.create_object_grant(
            workspace.id,
            object.id,
            GrantPrincipal::User(reader.id),
            ObjectRole::Viewer,
        )
        .await?;

        let results = r.search_as(&reader, workspace.id, needle).await?;

        assert!(
            results.items.iter().any(|hit| hit.object_id == object.id),
            "reader should see object search hit through direct Viewer grant",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_hides_archived_objects_from_explicit_object_admin_by_default(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("search archived object admin").await?;
        let workspace = space.workspace;

        let needle = "kivalsearchneedlearchivedadmin";

        let object = r
            .create_object(
                workspace.id,
                "Archived Admin Search Object",
                &test_body(
                    "Archived Admin Search Object",
                    &format!("Archived admin body containing {needle}."),
                ),
                object_metadata("archived-admin-object"),
            )
            .await?;
        let object_admin = r
            .create_object_actor(
                workspace.id,
                object.id,
                "search-archived-object-admin",
                MembershipRole::Member,
                ObjectRole::Admin,
            )
            .await?;

        let before_archive = r.search_as(&object_admin, workspace.id, needle).await?;
        assert!(
            before_archive.items.iter().any(|hit| hit.object_id == object.id),
            "object admin should see the search hit before archive",
        );

        r.archive_object(workspace.id, object.id).await?;

        let after_archive = r.search_as(&object_admin, workspace.id, needle).await?;
        assert!(
            after_archive.items.iter().all(|hit| hit.object_id != object.id),
            "archived object should not appear in default search, even for explicit object admins",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_hides_archived_objects(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("search archived").await?;
        let workspace = space.workspace;

        let needle = "kivalsearchneedlearchived";

        let object = r
            .create_object(
                workspace.id,
                "Archived Search Object",
                &test_body(
                    "Archived Search Object",
                    &format!("Archived body containing {needle}."),
                ),
                object_metadata("archived-object"),
            )
            .await?;

        let before_archive = r.search_as(&r.admin, workspace.id, needle).await?;

        assert!(
            before_archive.items.iter().any(|hit| hit.object_id == object.id),
            "object should appear in search before archive",
        );

        r.archive_object(workspace.id, object.id).await?;

        let after_archive = r.search_as(&r.admin, workspace.id, needle).await?;

        assert!(
            after_archive.items.iter().all(|hit| hit.object_id != object.id),
            "archived object should not appear in default search",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_admin_can_search_archived_objects_with_status_archived(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search archived status admin").await?;
        let needle = "kivalsearchneedlearchivedstatusadmin";
        let object = r
            .create_object(
                space.workspace.id,
                "Archived Status Search Object",
                &test_body(
                    "Archived Status Search Object",
                    &format!("Archived status body containing {needle}."),
                ),
                object_metadata("archived-status-search-object"),
            )
            .await?;

        r.archive_object(space.workspace.id, object.id).await?;

        let results: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/search?q={needle}&status=archived", space.workspace.id,),
            )
            .await?
            .into_success()?;

        assert!(results.items.iter().any(|hit| hit.object_id == object.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn explicit_object_admin_can_search_archived_objects_with_status_archived(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search archived explicit object admin").await?;
        let needle = "kivalsearchneedlearchivedexplicitadmin";
        let object = r
            .create_object(
                space.workspace.id,
                "Explicit Archived Search Object",
                &test_body(
                    "Explicit Archived Search Object",
                    &format!("Explicit admin archived body containing {needle}."),
                ),
                object_metadata("explicit-archived-search-object"),
            )
            .await?;
        let object_admin = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "search-status-archived-object-admin",
                MembershipRole::Member,
                ObjectRole::Admin,
            )
            .await?;

        r.archive_object(space.workspace.id, object.id).await?;

        let results: SearchResponse = r
            .get_json_as(
                &object_admin,
                &format!("/workspaces/{}/search?q={needle}&status=archived", space.workspace.id,),
            )
            .await?
            .into_success()?;

        assert!(results.items.iter().any(|hit| hit.object_id == object.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_editor_cannot_search_archived_objects_with_status_archived(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search archived explicit object editor").await?;
        let needle = "kivalsearchneedlearchivededitor";
        let object = r
            .create_object(
                space.workspace.id,
                "Editor Archived Search Object",
                &test_body(
                    "Editor Archived Search Object",
                    &format!("Editor archived body containing {needle}."),
                ),
                object_metadata("editor-archived-search-object"),
            )
            .await?;
        let object_editor = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "search-status-archived-object-editor",
                MembershipRole::Member,
                ObjectRole::Editor,
            )
            .await?;

        r.archive_object(space.workspace.id, object.id).await?;

        let results: SearchResponse = r
            .get_json_as(
                &object_editor,
                &format!("/workspaces/{}/search?q={needle}&status=archived", space.workspace.id,),
            )
            .await?
            .into_success()?;

        assert!(results.items.iter().all(|hit| hit.object_id != object.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_status_all_includes_active_and_archived_for_admin(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search all status admin").await?;
        let needle = "kivalsearchneedlestatusall";
        let active = r
            .create_object(
                space.workspace.id,
                "Active All Search Object",
                &test_body("Active All Search Object", &format!("Active body {needle}.")),
                object_metadata("active-all-search-object"),
            )
            .await?;
        let archived = r
            .create_object(
                space.workspace.id,
                "Archived All Search Object",
                &test_body("Archived All Search Object", &format!("Archived body {needle}.")),
                object_metadata("archived-all-search-object"),
            )
            .await?;
        r.archive_object(space.workspace.id, archived.id).await?;

        let results: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/search?q={needle}&status=all", space.workspace.id),
            )
            .await?
            .into_success()?;

        assert!(results.items.iter().any(|hit| hit.object_id == active.id));
        assert!(results.items.iter().any(|hit| hit.object_id == archived.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_history_recalls_previous_immutable_versions(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search history").await?;
        let old_title = "kivalsearchhistoricaltitle";
        let old_body_needle = "kivalsearchhistoricalbody";

        let object = r
            .create_object(
                space.workspace.id,
                old_title,
                &test_body(old_title, old_body_needle),
                object_metadata("search-history-old"),
            )
            .await?;
        let old_version_id = object.current_version_id.expect("created object version");

        let updated = r
            .update_object(
                space.workspace.id,
                object.id,
                Some("kivalsearchcurrenttitle"),
                Some(&test_body("kivalsearchcurrenttitle", "current body only")),
                Some(object_metadata("search-history-current")),
            )
            .await?;
        let current_version_id = updated.object.current_version_id.expect("updated object version");
        assert_ne!(old_version_id, current_version_id);

        let current_results: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q={old_title}&categories=title&mode=exact",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;
        assert!(
            current_results.items.iter().all(|hit| hit.object_id != object.id),
            "normal search must not return previous versions",
        );

        let historical_title_results: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q={old_title}&categories=title&mode=exact&include_history=true",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;
        assert!(historical_title_results.items.iter().any(|hit| {
            hit.object_id == object.id
                && hit.version_id == old_version_id
                && hit.version_number == 1
                && hit.title == old_title
                && hit.matched_category == SearchCategory::Title
                && hit.match_kind == SearchMatchKind::Exact
        }));

        let historical_body_results: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q={old_body_needle}&categories=body&mode=literal&include_history=true",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;
        assert!(historical_body_results.items.iter().any(|hit| {
            hit.object_id == object.id
                && hit.version_id == old_version_id
                && hit.title == old_title
                && hit.matched_category == SearchCategory::Body
                && hit.match_kind == SearchMatchKind::Literal
        }));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_rejects_empty_query(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("search empty query").await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!("/workspaces/{}/search?q=%20%20", workspace.id),
                None,
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_rejects_zero_limit(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("search zero limit").await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!("/workspaces/{}/search?q=test&limit=0", workspace.id),
                None,
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_rejects_removed_version_title_category(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("search removed version title category").await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!("/workspaces/{}/search?q=test&categories=version_title", workspace.id),
                None,
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_rejects_empty_category(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("search empty category").await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!("/workspaces/{}/search?q=test&categories=title,,body", workspace.id),
                None,
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_deduplicates_repeated_categories(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search repeated categories").await?;
        let title = "Repeated Category Unique Title";
        let object = r
            .create_object(
                space.workspace.id,
                title,
                &test_body(title, "Body."),
                object_metadata("search-repeated-categories"),
            )
            .await?;

        let results: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q=Repeated%20Category%20Unique%20Title&categories=title,title&mode=exact",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;

        let title_hits = results
            .items
            .iter()
            .filter(|hit| {
                hit.object_id == object.id && hit.matched_category == SearchCategory::Title
            })
            .count();
        assert_eq!(title_hits, 1, "duplicate category filters should not duplicate hits");

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_returns_one_hit_per_matching_version_before_limit(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search deduplicated hits").await?;
        let query = "Shared Search Phrase";

        let first = r
            .create_object(
                space.workspace.id,
                &format!("{query} Alpha"),
                &test_body(&format!("{query} Alpha"), "Body."),
                object_metadata("search-deduplicated-first"),
            )
            .await?;
        let second = r
            .create_object(
                space.workspace.id,
                &format!("{query} Beta"),
                &test_body(&format!("{query} Beta"), "Body."),
                object_metadata("search-deduplicated-second"),
            )
            .await?;

        let results: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q=Shared%20Search%20Phrase&limit=2",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;

        assert_eq!(results.items.len(), 2);
        assert!(results.items.iter().any(|hit| hit.object_id == first.id));
        assert!(results.items.iter().any(|hit| hit.object_id == second.id));
        assert!(results.items.iter().all(|hit| hit.matched_category == SearchCategory::Title));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_exact_mode_returns_exact_match_kind(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search exact mode").await?;
        let title = "Exact Mode Unique Title";
        let object = r
            .create_object(
                space.workspace.id,
                title,
                &test_body(title, "Body."),
                object_metadata("search-exact-mode"),
            )
            .await?;

        let results: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q=Exact%20Mode%20Unique%20Title&categories=title&mode=exact",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;

        assert!(results.items.iter().any(|hit| {
            hit.object_id == object.id
                && hit.matched_category == SearchCategory::Title
                && hit.match_kind == SearchMatchKind::Exact
        }));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_auto_mode_returns_exact_match_kind(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search auto exact mode").await?;
        let title = "Auto Mode Exact Unique Title";
        let object = r
            .create_object(
                space.workspace.id,
                title,
                &test_body(title, "Body."),
                object_metadata("search-auto-exact-mode"),
            )
            .await?;

        let results: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q=Auto%20Mode%20Exact%20Unique%20Title&categories=title",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;

        assert!(results.items.iter().any(|hit| {
            hit.object_id == object.id
                && hit.matched_category == SearchCategory::Title
                && hit.match_kind == SearchMatchKind::Exact
        }));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_text_mode_returns_text_match_kind(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search text mode").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Text Mode Object",
                &test_body("Text Mode Object", "The quick brown fox jumps over the lazy dog."),
                object_metadata("search-text-mode"),
            )
            .await?;

        let results: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q=quick%20fox&categories=body&mode=text",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;

        assert!(results.items.iter().any(|hit| {
            hit.object_id == object.id
                && hit.matched_category == SearchCategory::Body
                && hit.match_kind == SearchMatchKind::Text
        }));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_hits_include_object_status_and_version_metadata(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search hit context").await?;
        let title = "Search Context Unique Title";
        let object = r
            .create_object(
                space.workspace.id,
                title,
                &test_body(title, "Body."),
                serde_json::json!({
                    "kind": "runbook",
                    "owner": "Platform",
                    "sensitivity": "internal",
                }),
            )
            .await?;

        let results: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q=Search%20Context%20Unique%20Title&categories=title",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;

        let hit = results
            .items
            .iter()
            .find(|hit| hit.object_id == object.id)
            .expect("created object should be searchable");
        assert_eq!(hit.status, ArchiveStatus::Active);
        assert_eq!(hit.metadata["kind"], "runbook");
        assert_eq!(hit.metadata["owner"], "Platform");
        assert_eq!(hit.metadata["sensitivity"], "internal");

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_paginates_ranked_results_without_overlap(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search pagination").await?;
        let query = "Pagination Needle";
        let mut expected_ids = Vec::new();

        for suffix in ["Alpha", "Beta", "Gamma"] {
            let object = r
                .create_object(
                    space.workspace.id,
                    &format!("{query} {suffix}"),
                    &test_body(&format!("{query} {suffix}"), "Body."),
                    object_metadata(&format!("search-pagination-{suffix}")),
                )
                .await?;
            expected_ids.push(object.id);
        }

        let first: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q=Pagination%20Needle&limit=2",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(first.items.len(), 2);
        let cursor = first.next_cursor.expect("first page should have a continuation cursor");

        let second: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q=Pagination%20Needle&limit=2&cursor={cursor}",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(second.items.len(), 1);
        assert!(second.next_cursor.is_none());

        let mut actual_ids = first.items.iter().map(|hit| hit.object_id).collect::<Vec<_>>();
        actual_ids.extend(second.items.iter().map(|hit| hit.object_id));
        actual_ids.sort_unstable();
        expected_ids.sort_unstable();
        assert_eq!(actual_ids, expected_ids);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_rejects_cursor_reuse_with_different_query(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search cursor query binding").await?;

        for suffix in ["Alpha", "Beta"] {
            r.create_object(
                space.workspace.id,
                &format!("Cursor Binding {suffix}"),
                &test_body(&format!("Cursor Binding {suffix}"), "Body."),
                object_metadata(&format!("search-cursor-binding-{suffix}")),
            )
            .await?;
        }

        let first: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q=Cursor%20Binding&limit=1",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;
        let cursor = first.next_cursor.expect("first page should have a continuation cursor");

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!(
                    "/workspaces/{}/search?q=Different%20Query&limit=1&cursor={cursor}",
                    space.workspace.id,
                ),
                None,
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn search_clamps_large_limit(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("search large limit").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Large Limit Object",
                &test_body("Large Limit Object", "Body."),
                object_metadata("search-large-limit"),
            )
            .await?;

        let results: SearchResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/search?q=Large%20Limit%20Object&limit=100000",
                    space.workspace.id,
                ),
            )
            .await?
            .into_success()?;

        assert!(results.items.len() <= MAX_LIMIT as usize);
        assert!(results.items.iter().any(|hit| hit.object_id == object.id));

        Ok(())
    }
}
