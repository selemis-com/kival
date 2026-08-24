//! Object commentary API scenario tests.

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        CommentListResponse, CommentMentionCandidateListResponse, CommentResponse, CommentStatus,
        CommentThreadListResponse, CommentThreadResponse, CreateCommentRequest, ListResponse,
        MembershipRole, ObjectRole, UpdateCommentRequest,
    };
    use kival_tests::{
        TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt, object_metadata, test_body,
    };

    fn commentary_path(workspace_id: uuid::Uuid, object_id: uuid::Uuid) -> String {
        format!("/workspaces/{workspace_id}/objects/{object_id}/commentary")
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn commentary_inherits_object_access_without_creating_versions(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("commentary access").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Commentary Object",
                &test_body("Commentary Object", "Durable body."),
                object_metadata("commentary-object"),
            )
            .await?;
        let editor = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "commentary-editor",
                MembershipRole::Member,
                ObjectRole::Editor,
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "commentary-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        let unrelated = r
            .create_workspace_actor(
                space.workspace.id,
                "commentary-unrelated",
                MembershipRole::Member,
            )
            .await?;

        let version_count_before = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM kival.object_versions WHERE object_id = $1",
        )
        .bind(object.id)
        .fetch_one(&r.pool)
        .await?;

        let created: CommentThreadResponse = r
            .request_json_as(
                &viewer,
                Method::POST,
                &commentary_path(space.workspace.id, object.id),
                &CreateCommentRequest {
                    body: format!("Please review this, @{}.", editor.username),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?
            .into_success()?;

        assert_eq!(created.thread.comments.len(), 1);
        assert_eq!(created.thread.comments[0].mentions[0].user_id, editor.id);

        let listed: CommentThreadListResponse = r
            .get_json_as(&viewer, &commentary_path(space.workspace.id, object.id))
            .await?
            .into_success()?;
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].id, created.thread.id);

        let unrelated_list = r
            .request(
                Some(&unrelated),
                Method::GET,
                &commentary_path(space.workspace.id, object.id),
                None,
            )
            .await?;
        unrelated_list.assert_status(StatusCode::FORBIDDEN);

        let version_count_after = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM kival.object_versions WHERE object_id = $1",
        )
        .bind(object.id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(version_count_after, version_count_before);

        let mention_events = r
            .object_events_as(
                &editor,
                space.workspace.id,
                object.id,
                "event_kind=comment.mentioned",
            )
            .await?;
        assert_eq!(mention_events.items.len(), 1);
        assert_eq!(mention_events.items[0].target_user_id, Some(editor.id));
        assert_eq!(mention_events.items[0].comment_thread_id, Some(created.thread.id));
        assert_eq!(mention_events.items[0].comment_id, Some(created.thread.comments[0].id),);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn commentary_authors_control_text_and_thread_owners_control_resolution(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("commentary ownership").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Comment Ownership",
                &test_body("Comment Ownership", "Body."),
                object_metadata("comment-ownership"),
            )
            .await?;
        let first_viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "comment-owner",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        let second_viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "comment-other-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;

        let created: CommentThreadResponse = r
            .request_json_as(
                &first_viewer,
                Method::POST,
                &commentary_path(space.workspace.id, object.id),
                &CreateCommentRequest {
                    body: "Initial comment".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?
            .into_success()?;
        let root = &created.thread.comments[0];

        let foreign_edit = r
            .request_json_raw_as(
                &second_viewer,
                Method::PATCH,
                &format!(
                    "{}/comments/{}",
                    commentary_path(space.workspace.id, object.id),
                    root.id,
                ),
                &UpdateCommentRequest {
                    body: "Rewritten by someone else".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?;
        foreign_edit.assert_status(StatusCode::FORBIDDEN);

        let foreign_delete = r
            .request(
                Some(&second_viewer),
                Method::DELETE,
                &format!(
                    "{}/comments/{}",
                    commentary_path(space.workspace.id, object.id),
                    root.id,
                ),
                None,
            )
            .await?;
        foreign_delete.assert_status(StatusCode::FORBIDDEN);

        let reply: CommentResponse = r
            .request_json_as(
                &second_viewer,
                Method::POST,
                &format!(
                    "{}/{}/replies",
                    commentary_path(space.workspace.id, object.id),
                    created.thread.id,
                ),
                &CreateCommentRequest {
                    body: "A reply".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?
            .into_success()?;
        assert_eq!(reply.comment.parent_comment_id, Some(root.id));

        let reply_events = r
            .object_events_as(
                &first_viewer,
                space.workspace.id,
                object.id,
                "event_kind=comment.replied",
            )
            .await?;
        assert_eq!(reply_events.items.len(), 1);
        assert_eq!(reply_events.items[0].target_user_id, Some(first_viewer.id));
        assert_eq!(reply_events.items[0].comment_thread_id, Some(created.thread.id));
        assert_eq!(reply_events.items[0].comment_id, Some(reply.comment.id));

        let foreign_resolve = r
            .request(
                Some(&second_viewer),
                Method::POST,
                &format!(
                    "{}/{}/resolve",
                    commentary_path(space.workspace.id, object.id),
                    created.thread.id,
                ),
                None,
            )
            .await?;
        foreign_resolve.assert_status(StatusCode::FORBIDDEN);

        let resolved: CommentThreadResponse = r
            .empty_json_as(
                &first_viewer,
                Method::POST,
                &format!(
                    "{}/{}/resolve",
                    commentary_path(space.workspace.id, object.id),
                    created.thread.id,
                ),
            )
            .await?
            .into_success()?;
        assert!(resolved.thread.resolved_at.is_some());
        assert_eq!(resolved.thread.comments.len(), 2);

        let reply_to_resolved = r
            .request_json_raw_as(
                &first_viewer,
                Method::POST,
                &format!(
                    "{}/{}/replies",
                    commentary_path(space.workspace.id, object.id),
                    created.thread.id,
                ),
                &CreateCommentRequest {
                    body: "Should require reopening".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?;
        reply_to_resolved.assert_status(StatusCode::CONFLICT);

        let edit_resolved = r
            .request_json_raw_as(
                &first_viewer,
                Method::PATCH,
                &format!(
                    "{}/comments/{}",
                    commentary_path(space.workspace.id, object.id),
                    root.id,
                ),
                &UpdateCommentRequest {
                    body: "Resolved discussion should stay frozen".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?;
        edit_resolved.assert_status(StatusCode::CONFLICT);

        let deleted_while_resolved: CommentResponse = r
            .empty_json_as(
                &r.admin,
                Method::DELETE,
                &format!(
                    "{}/comments/{}",
                    commentary_path(space.workspace.id, object.id),
                    reply.comment.id,
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(deleted_while_resolved.comment.status, CommentStatus::Deleted);

        let reopened: CommentThreadResponse = r
            .empty_json_as(
                &r.admin,
                Method::POST,
                &format!(
                    "{}/{}/reopen",
                    commentary_path(space.workspace.id, object.id),
                    created.thread.id,
                ),
            )
            .await?
            .into_success()?;
        assert!(reopened.thread.resolved_at.is_none());

        assert!(deleted_while_resolved.comment.body.is_none());

        let author_deleted: CommentResponse = r
            .empty_json_as(
                &first_viewer,
                Method::DELETE,
                &format!(
                    "{}/comments/{}",
                    commentary_path(space.workspace.id, object.id),
                    root.id,
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(author_deleted.comment.status, CommentStatus::Deleted);
        assert!(author_deleted.comment.body.is_none());

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn commentary_pages_threads_by_latest_activity(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("commentary activity order").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Commentary Activity",
                &test_body("Commentary Activity", "Body."),
                object_metadata("commentary-activity"),
            )
            .await?;
        let path = commentary_path(space.workspace.id, object.id);

        let older: CommentThreadResponse = r
            .request_json_as(
                &r.admin,
                Method::POST,
                &path,
                &CreateCommentRequest {
                    body: "Older thread".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?
            .into_success()?;
        let newer: CommentThreadResponse = r
            .request_json_as(
                &r.admin,
                Method::POST,
                &path,
                &CreateCommentRequest {
                    body: "Newer thread".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?
            .into_success()?;

        tokio::time::sleep(Duration::from_millis(2)).await;

        let _: CommentResponse = r
            .request_json_as(
                &r.admin,
                Method::POST,
                &format!("{}/{}/replies", path, older.thread.id),
                &CreateCommentRequest {
                    body: "Latest activity".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?
            .into_success()?;

        let first_page: CommentThreadListResponse =
            r.get_json_as(&r.admin, &format!("{path}?limit=1")).await?.into_success()?;
        assert_eq!(first_page.items[0].id, older.thread.id);
        let cursor = first_page.next_cursor.expect("second page cursor");

        let second_page: CommentThreadListResponse = r
            .get_json_as(&r.admin, &format!("{path}?limit=1&cursor={cursor}"))
            .await?
            .into_success()?;
        assert_eq!(second_page.items[0].id, newer.thread.id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn inaccessible_mentions_are_rejected_without_creating_commentary_or_events(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("commentary mention denial").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Mention Boundary",
                &test_body("Mention Boundary", "Body."),
                object_metadata("mention-boundary"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "mentioning-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        let inaccessible = r
            .create_workspace_actor(
                space.workspace.id,
                "inaccessible-mentioned-user",
                MembershipRole::Member,
            )
            .await?;

        let response = r
            .request_json_raw_as(
                &viewer,
                Method::POST,
                &commentary_path(space.workspace.id, object.id),
                &CreateCommentRequest {
                    body: format!("@{} should not see this", inaccessible.username),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?;
        response.assert_status(StatusCode::BAD_REQUEST);

        let thread_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM kival.comment_threads WHERE object_id = $1",
        )
        .bind(object.id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(thread_count, 0);

        let mention_event_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM kival.events WHERE object_id = $1 AND event_kind = 'comment.mentioned'",
        )
        .bind(object.id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(mention_event_count, 0);

        let inaccessible_events = r
            .request(
                Some(&inaccessible),
                Method::GET,
                &format!(
                    "/workspaces/{}/objects/{}/events?event_kind=comment.mentioned",
                    space.workspace.id, object.id,
                ),
                None,
            )
            .await?;
        inaccessible_events.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn mention_candidates_are_limited_to_object_viewers(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("commentary mention candidates").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Mention Candidates",
                &test_body("Mention Candidates", "Body."),
                object_metadata("mention-candidates"),
            )
            .await?;
        let editor = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "mention-candidate-editor",
                MembershipRole::Member,
                ObjectRole::Editor,
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "mention-candidate-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        let inaccessible = r
            .create_workspace_actor(
                space.workspace.id,
                "mention-candidate-hidden",
                MembershipRole::Member,
            )
            .await?;
        let path =
            format!("{}/mention-candidates", commentary_path(space.workspace.id, object.id),);

        let viewer_path = format!("{path}?q={}", viewer.username);
        let viewer_candidates: CommentMentionCandidateListResponse =
            r.get_json_as(&viewer, &viewer_path).await?.into_success()?;
        assert_eq!(viewer_candidates.items.len(), 1);
        assert_eq!(viewer_candidates.items[0].user_id, viewer.id);

        let editor_path = format!("{path}?q={}", editor.username);
        let editor_candidates: CommentMentionCandidateListResponse =
            r.get_json_as(&viewer, &editor_path).await?.into_success()?;
        assert_eq!(editor_candidates.items.len(), 1);
        assert_eq!(editor_candidates.items[0].user_id, editor.id);

        let inaccessible_path = format!("{path}?q={}", inaccessible.username);
        let inaccessible_candidates: CommentMentionCandidateListResponse =
            r.get_json_as(&viewer, &inaccessible_path).await?.into_success()?;
        assert!(inaccessible_candidates.items.is_empty());

        let viewer_lookup: CommentMentionCandidateListResponse =
            r.get_json_as(&viewer, &path).await?.into_success()?;
        assert!(viewer_lookup.items.iter().any(|candidate| candidate.user_id == viewer.id));
        assert!(viewer_lookup.items.iter().any(|candidate| candidate.user_id == editor.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn editing_replaces_mentions_and_only_new_mentions_emit_events(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("commentary mention edits").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Mention Edits",
                &test_body("Mention Edits", "Body."),
                object_metadata("mention-edits"),
            )
            .await?;
        let viewer_author = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "mention-edit-author",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        let first = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "mention-edit-first",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        let second = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "mention-edit-second",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;

        let created: CommentThreadResponse = r
            .request_json_as(
                &viewer_author,
                Method::POST,
                &commentary_path(space.workspace.id, object.id),
                &CreateCommentRequest {
                    body: format!("Initial @{}", first.username),
                    mentioned_user_ids: vec![first.id],
                },
            )
            .await?
            .into_success()?;
        let comment_id = created.thread.comments[0].id;

        let edited: CommentResponse = r
            .request_json_as(
                &viewer_author,
                Method::PATCH,
                &format!(
                    "{}/comments/{comment_id}",
                    commentary_path(space.workspace.id, object.id),
                ),
                &UpdateCommentRequest {
                    body: format!("Now @{}", second.username),
                    mentioned_user_ids: vec![second.id],
                },
            )
            .await?
            .into_success()?;
        assert_eq!(edited.comment.mentions.len(), 1);
        assert_eq!(edited.comment.mentions[0].user_id, second.id);
        assert!(edited.comment.edited_at.is_some());

        let _: CommentResponse = r
            .request_json_as(
                &viewer_author,
                Method::PATCH,
                &format!(
                    "{}/comments/{comment_id}",
                    commentary_path(space.workspace.id, object.id),
                ),
                &UpdateCommentRequest {
                    body: format!("Still @{}", second.username),
                    mentioned_user_ids: vec![second.id],
                },
            )
            .await?
            .into_success()?;

        let first_events = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
            FROM kival.events
            WHERE comment_id = $1
                AND event_kind = 'comment.mentioned'
                AND target_user_id = $2
            "#,
        )
        .bind(comment_id)
        .bind(first.id)
        .fetch_one(&r.pool)
        .await?;
        let second_events = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
            FROM kival.events
            WHERE comment_id = $1
                AND event_kind = 'comment.mentioned'
                AND target_user_id = $2
            "#,
        )
        .bind(comment_id)
        .bind(second.id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(first_events, 1);
        assert_eq!(second_events, 1);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn self_mentions_are_recorded_without_emitting_mention_events(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("commentary self mention").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Self Mention",
                &test_body("Self Mention", "Body."),
                object_metadata("self-mention"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "self-mention-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;

        let created: CommentThreadResponse = r
            .request_json_as(
                &viewer,
                Method::POST,
                &commentary_path(space.workspace.id, object.id),
                &CreateCommentRequest {
                    body: format!("Note to @{}", viewer.username),
                    mentioned_user_ids: vec![viewer.id],
                },
            )
            .await?
            .into_success()?;
        assert_eq!(created.thread.comments[0].mentions[0].user_id, viewer.id);

        let mention_events = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM kival.events WHERE comment_id = $1 AND event_kind = 'comment.mentioned'",
        )
        .bind(created.thread.comments[0].id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(mention_events, 0);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn hot_threads_page_comments_independently_from_threads(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("commentary reply pagination").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Reply Pagination",
                &test_body("Reply Pagination", "Body."),
                object_metadata("reply-pagination"),
            )
            .await?;
        let path = commentary_path(space.workspace.id, object.id);
        let created: CommentThreadResponse = r
            .request_json_as(
                &r.admin,
                Method::POST,
                &path,
                &CreateCommentRequest { body: "Root".to_owned(), mentioned_user_ids: Vec::new() },
            )
            .await?
            .into_success()?;

        for index in 0..21 {
            let _: CommentResponse = r
                .request_json_as(
                    &r.admin,
                    Method::POST,
                    &format!("{}/{}/replies", path, created.thread.id),
                    &CreateCommentRequest {
                        body: format!("Reply {index}"),
                        mentioned_user_ids: Vec::new(),
                    },
                )
                .await?
                .into_success()?;
        }

        let commentary: CommentThreadListResponse =
            r.get_json_as(&r.admin, &path).await?.into_success()?;
        let thread = &commentary.items[0];
        assert_eq!(thread.comments.len(), 20);
        let cursor = thread.comments_next_cursor.as_ref().expect("remaining reply cursor");

        let remaining: CommentListResponse = r
            .get_json_as(&r.admin, &format!("{}/{}/comments?cursor={cursor}", path, thread.id))
            .await?
            .into_success()?;
        assert_eq!(remaining.items.len(), 2);
        assert!(remaining.next_cursor.is_none());
        assert!(remaining.items.iter().all(|comment| comment.parent_comment_id.is_some()));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn archived_objects_keep_commentary_read_only(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("archived commentary").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Archived Commentary",
                &test_body("Archived Commentary", "Body."),
                object_metadata("archived-commentary"),
            )
            .await?;
        let path = commentary_path(space.workspace.id, object.id);
        let _: CommentThreadResponse = r
            .request_json_as(
                &r.admin,
                Method::POST,
                &path,
                &CreateCommentRequest {
                    body: "Before archive".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?
            .into_success()?;

        r.archive_object(space.workspace.id, object.id).await?;

        let listed: CommentThreadListResponse =
            r.get_json_as(&r.admin, &path).await?.into_success()?;
        assert_eq!(listed.items.len(), 1);

        let create_after_archive = r
            .request_json_raw_as(
                &r.admin,
                Method::POST,
                &path,
                &CreateCommentRequest {
                    body: "No longer writable".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?;
        create_after_archive.assert_status(StatusCode::NOT_FOUND);

        let archived_comment_id = listed.items[0].comments[0].id;
        let edit_after_archive = r
            .request_json_raw_as(
                &r.admin,
                Method::PATCH,
                &format!("{path}/comments/{archived_comment_id}"),
                &UpdateCommentRequest {
                    body: "Still not writable".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?;
        edit_after_archive.assert_status(StatusCode::NOT_FOUND);

        let candidates_after_archive = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!("{path}/mention-candidates?q=admin"),
                None,
            )
            .await?;
        candidates_after_archive.assert_status(StatusCode::NOT_FOUND);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn retention_tombstones_comments_and_can_purge_threads(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("commentary retention").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Retention Object",
                &test_body("Retention Object", "Body."),
                object_metadata("retention-object"),
            )
            .await?;

        let created: CommentThreadResponse = r
            .request_json_as(
                &r.admin,
                Method::POST,
                &commentary_path(space.workspace.id, object.id),
                &CreateCommentRequest {
                    body: "Ephemeral working context".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?
            .into_success()?;
        let comment_id = created.thread.comments[0].id;

        sqlx::query("UPDATE kival.comments SET retention_expires_at = now() WHERE id = $1")
            .bind(comment_id)
            .execute(&r.pool)
            .await?;

        let due_before_worker: CommentThreadListResponse = r
            .get_json_as(&r.admin, &commentary_path(space.workspace.id, object.id))
            .await?
            .into_success()?;
        assert_eq!(due_before_worker.items[0].comments[0].status, CommentStatus::Expired);
        assert!(due_before_worker.items[0].comments[0].body.is_none());
        assert!(due_before_worker.items[0].comments[0].mentions.is_empty());

        let edit_due = r
            .request(
                Some(&r.admin),
                Method::PATCH,
                &format!(
                    "{}/comments/{comment_id}",
                    commentary_path(space.workspace.id, object.id),
                ),
                Some(serde_json::to_value(UpdateCommentRequest {
                    body: "Too late".to_owned(),
                    mentioned_user_ids: Vec::new(),
                })?),
            )
            .await?;
        edit_due.assert_status(StatusCode::CONFLICT);

        let applied = sqlx::query_as::<_, (i32, i32)>(
            "SELECT expired_comments, purged_threads FROM kival.apply_commentary_retention(10)",
        )
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(applied, (1, 0));

        let listed: CommentThreadListResponse = r
            .get_json_as(&r.admin, &commentary_path(space.workspace.id, object.id))
            .await?
            .into_success()?;
        assert_eq!(listed.items[0].comments[0].status, CommentStatus::Expired);
        assert!(listed.items[0].comments[0].body.is_none());

        sqlx::query("UPDATE kival.comment_threads SET retention_expires_at = now() WHERE id = $1")
            .bind(created.thread.id)
            .execute(&r.pool)
            .await?;
        let applied = sqlx::query_as::<_, (i32, i32)>(
            "SELECT expired_comments, purged_threads FROM kival.apply_commentary_retention(10)",
        )
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(applied, (0, 1));

        let event_ids: ListResponse<kival_sdk::Event> = r
            .get_json_as(
                &r.admin,
                &format!(
                    "/workspaces/{}/objects/{}/events?event_kind=comment.created",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;
        assert_eq!(event_ids.items[0].comment_thread_id, Some(created.thread.id));
        assert_eq!(event_ids.items[0].comment_id, Some(comment_id));

        let thread_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM kival.comment_threads WHERE id = $1)",
        )
        .bind(created.thread.id)
        .fetch_one(&r.pool)
        .await?;
        assert!(!thread_exists);

        Ok(())
    }
}
