//! Backlink API scenario tests.

#[cfg(test)]
mod tests {
    use eyre::Result;
    use kival_sdk::{GrantPrincipal, MembershipRole, ObjectBacklinksResponse, ObjectRole};
    use kival_tests::{TestFixtureExt, TestKival, TestResponseExt, object_metadata, test_body};

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn backlinks_returns_resolved_wikilink_reference(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("backlinks").await?;
        let workspace = space.workspace;

        let target = r
            .create_object(
                workspace.id,
                "Harness Target",
                &test_body("Harness Target", "Target body."),
                object_metadata("target"),
            )
            .await?;

        let source = r
            .create_object(
                workspace.id,
                "Harness Source",
                &test_body("Harness Source", "[[Harness Target|display text]]"),
                object_metadata("source"),
            )
            .await?;

        let backlinks = r.backlinks_as(&r.admin, workspace.id, target.id).await?;

        assert_eq!(backlinks.object_id, target.id);
        assert!(backlinks.incoming_edges.is_empty());

        let reference = backlinks
            .incoming_references
            .iter()
            .find(|reference| reference.source_object.id == source.id)
            .expect("source wikilink should appear as a textual backlink");

        assert_eq!(reference.reference_kind, "wikilink");
        assert_eq!(reference.raw_target, "Harness Target");
        assert_eq!(reference.display_text.as_deref(), Some("display text"));
        assert_eq!(reference.target_object_id, target.id);
        assert_eq!(reference.source_object.title, "Harness Source");

        let renamed_body = test_body("Harness Source Renamed", "[[Harness Target|display text]]");
        r.update_object(
            workspace.id,
            source.id,
            Some("Harness Source Renamed"),
            Some(&renamed_body),
            None,
        )
        .await?;

        let backlinks = r.backlinks_as(&r.admin, workspace.id, target.id).await?;
        let reference = backlinks
            .incoming_references
            .iter()
            .find(|reference| reference.source_object.id == source.id)
            .expect("renamed source wikilink should remain a textual backlink");
        assert_eq!(reference.source_object.title, "Harness Source Renamed");

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn wikilink_becomes_ambiguous_when_duplicate_title_is_created(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("backlinks").await?;
        let workspace = space.workspace;

        let first_target = r
            .create_object(
                workspace.id,
                "Duplicate Target",
                &test_body("Duplicate Target", "First target."),
                object_metadata("first-target"),
            )
            .await?;

        let source = r
            .create_object(
                workspace.id,
                "Source",
                &test_body("Source", "[[Duplicate Target]]"),
                object_metadata("source"),
            )
            .await?;

        let backlinks = r.backlinks_as(&r.admin, workspace.id, first_target.id).await?;

        assert!(
            backlinks
                .incoming_references
                .iter()
                .any(|reference| reference.source_object.id == source.id),
            "source wikilink should resolve while the target title is unique",
        );

        let second_target = r
            .create_object(
                workspace.id,
                "Duplicate Target",
                &test_body("Duplicate Target", "Second target."),
                object_metadata("second-target"),
            )
            .await?;

        let first_backlinks = r.backlinks_as(&r.admin, workspace.id, first_target.id).await?;

        let second_backlinks = r.backlinks_as(&r.admin, workspace.id, second_target.id).await?;

        assert!(
            first_backlinks
                .incoming_references
                .iter()
                .all(|reference| reference.source_object.id != source.id),
            "ambiguous wikilink should no longer point at the first target",
        );

        assert!(
            second_backlinks
                .incoming_references
                .iter()
                .all(|reference| reference.source_object.id != source.id),
            "ambiguous wikilink should not point at the duplicate target either",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn update_object_stales_old_wikilink_reference(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("backlinks").await?;
        let workspace = space.workspace;

        let target = r
            .create_object(
                workspace.id,
                "Stale Target",
                &test_body("Stale Target", "Target body."),
                object_metadata("target"),
            )
            .await?;

        let source = r
            .create_object(
                workspace.id,
                "Source v1",
                &test_body("Source v1", "[[Stale Target]]"),
                object_metadata("source-v1"),
            )
            .await?;

        let backlinks = r.backlinks_as(&r.admin, workspace.id, target.id).await?;

        assert!(
            backlinks
                .incoming_references
                .iter()
                .any(|reference| reference.source_object.id == source.id),
            "v1 wikilink should appear before updating the source object",
        );

        let updated_body = test_body("Source v2", "This version no longer links to the target.");
        r.update_object(
            workspace.id,
            source.id,
            Some("Source v2"),
            Some(&updated_body),
            Some(object_metadata("source-v2")),
        )
        .await?;

        let backlinks = r.backlinks_as(&r.admin, workspace.id, target.id).await?;

        assert!(
            backlinks
                .incoming_references
                .iter()
                .all(|reference| reference.source_object.id != source.id),
            "old wikilink should disappear after updating the current version without the link",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn backlinks_hide_textual_references_from_unreadable_sources(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let workspace = r.create_workspace("backlinks auth").await?;

        let visible_group = r.create_group("visible editors").await?;
        let hidden_group = r.create_group("hidden editors").await?;

        r.add_group_to_workspace(workspace.id, visible_group.id).await?;
        r.add_group_to_workspace(workspace.id, hidden_group.id).await?;

        let reader = r.create_user("backlinks-reader").await?;

        r.add_user_to_workspace(workspace.id, reader.id, MembershipRole::Member).await?;
        r.add_user_to_group(visible_group.id, reader.id, MembershipRole::Member).await?;

        let target = r
            .create_object(
                workspace.id,
                "Visible Target",
                &test_body("Visible Target", "Target body."),
                object_metadata("target"),
            )
            .await?;

        r.create_object_grant(
            workspace.id,
            target.id,
            GrantPrincipal::Group(visible_group.id),
            ObjectRole::Viewer,
        )
        .await?;

        let hidden_source = r
            .create_object(
                workspace.id,
                "Hidden Source",
                &test_body("Hidden Source", "[[Visible Target]]"),
                object_metadata("hidden-source"),
            )
            .await?;

        let admin_backlinks = r.backlinks_as(&r.admin, workspace.id, target.id).await?;

        assert!(
            admin_backlinks
                .incoming_references
                .iter()
                .any(|reference| reference.source_object.id == hidden_source.id),
            "admin should see the hidden source backlink",
        );

        let reader_backlinks = r.backlinks_as(&reader, workspace.id, target.id).await?;

        assert!(
            reader_backlinks
                .incoming_references
                .iter()
                .all(|reference| reference.source_object.id != hidden_source.id),
            "reader should not see backlinks from unreadable source objects",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn backlinks_hide_explicit_edges_from_unreadable_sources(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let workspace = r.create_workspace("backlinks edge auth").await?;

        let visible_group = r.create_group("visible editors").await?;
        let hidden_group = r.create_group("hidden editors").await?;

        r.add_group_to_workspace(workspace.id, visible_group.id).await?;
        r.add_group_to_workspace(workspace.id, hidden_group.id).await?;

        let reader = r.create_user("backlinks-edge-reader").await?;

        r.add_user_to_workspace(workspace.id, reader.id, MembershipRole::Member).await?;
        r.add_user_to_group(visible_group.id, reader.id, MembershipRole::Member).await?;

        let target = r
            .create_object(
                workspace.id,
                "Visible Edge Target",
                &test_body("Visible Edge Target", "Target body."),
                object_metadata("target"),
            )
            .await?;

        r.create_object_grant(
            workspace.id,
            target.id,
            GrantPrincipal::Group(visible_group.id),
            ObjectRole::Viewer,
        )
        .await?;

        let hidden_source = r
            .create_object(
                workspace.id,
                "Hidden Edge Source",
                &test_body("Hidden Edge Source", "Hidden source body."),
                object_metadata("hidden-source"),
            )
            .await?;

        r.create_edge(workspace.id, hidden_source.id, target.id).await?;

        let admin_backlinks = r.backlinks_as(&r.admin, workspace.id, target.id).await?;

        assert!(
            admin_backlinks
                .incoming_edges
                .iter()
                .any(|edge| edge.source_object.id == hidden_source.id),
            "admin should see the hidden explicit source edge",
        );

        let reader_backlinks = r.backlinks_as(&reader, workspace.id, target.id).await?;

        assert!(
            reader_backlinks
                .incoming_edges
                .iter()
                .all(|edge| edge.source_object.id != hidden_source.id),
            "reader should not see explicit backlinks from unreadable source objects",
        );

        Ok(())
    }
    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn backlinks_paginate_explicit_edges_without_truncation(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("backlink edge pagination").await?;
        let target = r
            .create_object(
                space.workspace.id,
                "Backlink Edge Pagination Target",
                &test_body("Backlink Edge Pagination Target", "Target."),
                object_metadata("target"),
            )
            .await?;

        let mut source_ids = Vec::new();
        for index in 0..2 {
            let source = r
                .create_object(
                    space.workspace.id,
                    &format!("Backlink Edge Source {index}"),
                    &test_body(&format!("Backlink Edge Source {index}"), "Source."),
                    object_metadata("source"),
                )
                .await?;
            r.create_edge(space.workspace.id, source.id, target.id).await?;
            source_ids.push(source.id);
        }

        let first: ObjectBacklinksResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/backlinks?limit=1",
                    space.workspace.id, target.id
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(first.incoming_edges.len(), 1);
        let cursor = first.next_edge_cursor.expect("first edge page should include a cursor");

        let second: ObjectBacklinksResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/backlinks?limit=1&edge_cursor={cursor}",
                    space.workspace.id, target.id
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(second.incoming_edges.len(), 1);

        let listed = first
            .incoming_edges
            .iter()
            .chain(second.incoming_edges.iter())
            .map(|edge| edge.source_object.id)
            .collect::<Vec<_>>();
        assert!(source_ids.iter().all(|source_id| listed.contains(source_id)));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn backlinks_paginate_textual_references_without_truncation(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("backlink reference pagination").await?;
        let target = r
            .create_object(
                space.workspace.id,
                "Backlink Reference Pagination Target",
                &test_body("Backlink Reference Pagination Target", "Target."),
                object_metadata("target"),
            )
            .await?;

        let mut source_ids = Vec::new();
        for index in 0..2 {
            let source = r
                .create_object(
                    space.workspace.id,
                    &format!("Backlink Reference Source {index}"),
                    &test_body(
                        &format!("Backlink Reference Source {index}"),
                        "[[Backlink Reference Pagination Target]]",
                    ),
                    object_metadata("source"),
                )
                .await?;
            source_ids.push(source.id);
        }

        let first: ObjectBacklinksResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/backlinks?limit=1",
                    space.workspace.id, target.id
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(first.incoming_references.len(), 1);
        let cursor =
            first.next_reference_cursor.expect("first reference page should include a cursor");

        let second: ObjectBacklinksResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/backlinks?limit=1&reference_cursor={cursor}",
                    space.workspace.id, target.id
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(second.incoming_references.len(), 1);

        let listed = first
            .incoming_references
            .iter()
            .chain(second.incoming_references.iter())
            .map(|reference| reference.source_object.id)
            .collect::<Vec<_>>();
        assert!(source_ids.iter().all(|source_id| listed.contains(source_id)));

        Ok(())
    }

    /// Verifies that exhausting one backlink stream does not restart it while the other continues.
    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn backlinks_dual_cursor_pagination_does_not_restart_exhausted_streams(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("backlink dual cursor pagination").await?;
        let target = r
            .create_object(
                space.workspace.id,
                "Backlink Dual Cursor Target",
                &test_body("Backlink Dual Cursor Target", "Target."),
                object_metadata("target"),
            )
            .await?;

        let mut edge_source_ids = Vec::new();
        for index in 0..2 {
            let source = r
                .create_object(
                    space.workspace.id,
                    &format!("Dual Cursor Edge Source {index}"),
                    &test_body(&format!("Dual Cursor Edge Source {index}"), "Source."),
                    object_metadata("edge-source"),
                )
                .await?;
            r.create_edge(space.workspace.id, source.id, target.id).await?;
            edge_source_ids.push(source.id);
        }

        let mut reference_source_ids = Vec::new();
        for index in 0..3 {
            let source = r
                .create_object(
                    space.workspace.id,
                    &format!("Dual Cursor Reference Source {index}"),
                    &test_body(
                        &format!("Dual Cursor Reference Source {index}"),
                        "[[Backlink Dual Cursor Target]]",
                    ),
                    object_metadata("reference-source"),
                )
                .await?;
            reference_source_ids.push(source.id);
        }

        let first: ObjectBacklinksResponse = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/backlinks?limit=1",
                    space.workspace.id, target.id
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(first.incoming_edges.len(), 1);
        assert_eq!(first.incoming_references.len(), 1);
        let first_edge_cursor =
            first.next_edge_cursor.as_deref().expect("first page should continue explicit edges");
        let first_reference_cursor = first
            .next_reference_cursor
            .as_deref()
            .expect("first page should continue textual references");

        let second: ObjectBacklinksResponse = r
          .get_json_as(
              &r.admin,
              &format!(
                  "/workspaces/{}/objects/{}/backlinks?limit=1&edge_cursor={first_edge_cursor}&reference_cursor={first_reference_cursor}",
                  space.workspace.id, target.id
              ),
          )
          .await?
          .into_success()?;
        assert_eq!(second.incoming_edges.len(), 1);
        assert_eq!(second.incoming_references.len(), 1);
        assert!(second.next_edge_cursor.is_none(), "explicit edges should now be exhausted");
        let second_reference_cursor = second
            .next_reference_cursor
            .as_deref()
            .expect("textual references should still have one page remaining");

        let third: ObjectBacklinksResponse = r
          .get_json_as(
              &r.admin,
              &format!(
                  "/workspaces/{}/objects/{}/backlinks?limit=1&reference_cursor={second_reference_cursor}",
                  space.workspace.id, target.id
              ),
          )
          .await?
          .into_success()?;
        assert!(
            third.incoming_edges.is_empty(),
            "an exhausted explicit-edge stream must not restart when only references continue",
        );
        assert!(third.next_edge_cursor.is_none());
        assert_eq!(third.incoming_references.len(), 1);
        assert!(third.next_reference_cursor.is_none());

        let listed_edge_ids = first
            .incoming_edges
            .iter()
            .chain(second.incoming_edges.iter())
            .chain(third.incoming_edges.iter())
            .map(|edge| edge.source_object.id)
            .collect::<Vec<_>>();
        assert_eq!(listed_edge_ids.len(), edge_source_ids.len());
        assert!(edge_source_ids.iter().all(|source_id| listed_edge_ids.contains(source_id)));

        let listed_reference_ids = first
            .incoming_references
            .iter()
            .chain(second.incoming_references.iter())
            .chain(third.incoming_references.iter())
            .map(|reference| reference.source_object.id)
            .collect::<Vec<_>>();
        assert_eq!(listed_reference_ids.len(), reference_source_ids.len());
        assert!(
            reference_source_ids.iter().all(|source_id| listed_reference_ids.contains(source_id)),
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_admin_can_read_backlinks_for_archived_target(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("archived backlink target").await?;
        let reader = r
            .create_workspace_actor(
                workspace.id,
                "archived-backlink-reader",
                MembershipRole::Member,
            )
            .await?;
        let target = r
            .create_object(
                workspace.id,
                "Archived Backlink Target",
                &test_body("Archived Backlink Target", "Target."),
                object_metadata("target"),
            )
            .await?;
        let source = r
            .create_object(
                workspace.id,
                "Archived Backlink Source",
                &test_body("Archived Backlink Source", "Source."),
                object_metadata("source"),
            )
            .await?;
        r.create_edge(workspace.id, source.id, target.id).await?;
        r.create_object_grant(
            workspace.id,
            target.id,
            GrantPrincipal::User(reader.id),
            ObjectRole::Admin,
        )
        .await?;
        r.create_object_grant(
            workspace.id,
            source.id,
            GrantPrincipal::User(reader.id),
            ObjectRole::Viewer,
        )
        .await?;
        r.archive_object(workspace.id, target.id).await?;

        let backlinks = r.backlinks_as(&reader, workspace.id, target.id).await?;
        assert!(backlinks.incoming_edges.iter().any(|edge| edge.source_object.id == source.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn include_archived_backlinks_honors_object_admin_access_on_source(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("archived backlink source").await?;
        let reader = r
            .create_workspace_actor(workspace.id, "archived-source-reader", MembershipRole::Member)
            .await?;
        let target = r
            .create_object(
                workspace.id,
                "Archived Source Target",
                &test_body("Archived Source Target", "Target."),
                object_metadata("target"),
            )
            .await?;
        let source = r
            .create_object(
                workspace.id,
                "Archived Source",
                &test_body("Archived Source", "Source."),
                object_metadata("source"),
            )
            .await?;
        r.create_edge(workspace.id, source.id, target.id).await?;
        r.create_object_grant(
            workspace.id,
            target.id,
            GrantPrincipal::User(reader.id),
            ObjectRole::Viewer,
        )
        .await?;
        r.create_object_grant(
            workspace.id,
            source.id,
            GrantPrincipal::User(reader.id),
            ObjectRole::Admin,
        )
        .await?;
        r.archive_object(workspace.id, source.id).await?;

        let hidden = r.backlinks_as(&reader, workspace.id, target.id).await?;
        assert!(hidden.incoming_edges.iter().all(|edge| edge.source_object.id != source.id));

        let visible: ObjectBacklinksResponse = r
            .get_json_as(
                &reader,
                &format!(
                    "/workspaces/{}/objects/{}/backlinks?include_archived=true",
                    workspace.id, target.id
                ),
            )
            .await?
            .into_success()?;
        assert!(visible.incoming_edges.iter().any(|edge| edge.source_object.id == source.id));

        Ok(())
    }
}
