//! Notification preference, inbox projection, and authorization scenarios.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{
        CommentResponse, CommentThreadResponse, CreateCommentRequest, GrantPrincipal, InboxEntry,
        InboxUnreadCountResponse, ListResponse, MembershipRole, ObjectGrantResponse,
        ObjectNotificationPreference, ObjectRole, UpdateObjectNotificationPreferenceRequest,
        UserResponse,
    };
    use kival_tests::{
        TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt, object_metadata, test_body,
    };

    /// Drains the bounded notification projection until it catches up.
    async fn project_notifications(r: &TestKival) -> Result<()> {
        loop {
            let (processed, _, _): (i32, i32, i64) =
                sqlx::query_as("SELECT * FROM kival.process_notification_candidate_batch(100)")
                    .fetch_one(&r.pool)
                    .await?;
            if processed < 100 {
                return Ok(());
            }
        }
    }

    /// Projects fixture-setup events and clears their inbox rows for one user.
    async fn clear_setup_notifications(r: &TestKival, user_id: uuid::Uuid) -> Result<()> {
        project_notifications(r).await?;
        sqlx::query("DELETE FROM kival.inbox_notifications WHERE recipient_user_id = $1")
            .bind(user_id)
            .execute(&r.pool)
            .await?;
        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn ordinary_activity_is_enabled_by_default_and_can_be_suppressed(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("notification preference").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Notification Preference",
                &test_body("Notification Preference", "Version one."),
                object_metadata("notification-preference-v1"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "notification-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        clear_setup_notifications(&r, viewer.id).await?;

        let preference: ObjectNotificationPreference = r
            .get_json_as(
                &viewer,
                &format!(
                    "/workspaces/{}/objects/{}/notification-preference",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;
        assert!(preference.ordinary_notifications_enabled);
        assert!(!preference.explicit);

        r.update_object(
            space.workspace.id,
            object.id,
            Some("Notification Preference v2"),
            None,
            None,
        )
        .await?;
        project_notifications(&r).await?;

        let first_page: ListResponse<InboxEntry> =
            r.get_json_as(&viewer, "/inbox").await?.into_success()?;
        assert_eq!(first_page.items.len(), 1);
        assert_eq!(first_page.items[0].reason, "object_activity");

        let preference: ObjectNotificationPreference = r
            .request_json_as(
                &viewer,
                Method::PATCH,
                &format!(
                    "/workspaces/{}/objects/{}/notification-preference",
                    space.workspace.id, object.id,
                ),
                &UpdateObjectNotificationPreferenceRequest {
                    ordinary_notifications_enabled: false,
                },
            )
            .await?
            .into_success()?;
        assert!(!preference.ordinary_notifications_enabled);
        assert!(preference.explicit);

        let restored: ObjectNotificationPreference = r
            .request_json_as(
                &viewer,
                Method::PATCH,
                &format!(
                    "/workspaces/{}/objects/{}/notification-preference",
                    space.workspace.id, object.id,
                ),
                &UpdateObjectNotificationPreferenceRequest { ordinary_notifications_enabled: true },
            )
            .await?
            .into_success()?;
        assert!(restored.ordinary_notifications_enabled);
        assert!(restored.explicit);

        let preference: ObjectNotificationPreference = r
            .request_json_as(
                &viewer,
                Method::PATCH,
                &format!(
                    "/workspaces/{}/objects/{}/notification-preference",
                    space.workspace.id, object.id,
                ),
                &UpdateObjectNotificationPreferenceRequest {
                    ordinary_notifications_enabled: false,
                },
            )
            .await?
            .into_success()?;
        assert!(!preference.ordinary_notifications_enabled);
        assert!(preference.explicit);

        r.update_object(
            space.workspace.id,
            object.id,
            Some("Notification Preference v3"),
            None,
            None,
        )
        .await?;
        project_notifications(&r).await?;

        let second_page: ListResponse<InboxEntry> =
            r.get_json_as(&viewer, "/inbox").await?.into_success()?;
        assert_eq!(second_page.items.len(), 1);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn inbox_preserves_attribution_after_actor_is_disabled(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("disabled notification actor").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Disabled Notification Actor",
                &test_body("Disabled Notification Actor", "Version one."),
                object_metadata("disabled-notification-actor-v1"),
            )
            .await?;
        let editor = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "disabled-notification-editor",
                MembershipRole::Member,
                ObjectRole::Editor,
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "disabled-notification-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        clear_setup_notifications(&r, viewer.id).await?;

        r.update_object_as(
            &editor,
            space.workspace.id,
            object.id,
            Some("Disabled Notification Actor v2"),
            None,
            None,
        )
        .await?;
        let _: UserResponse = r
            .empty_json_as(
                &r.admin,
                Method::POST,
                &format!("/users/{}/disable", editor.id),
            )
            .await?
            .into_success()?;

        project_notifications(&r).await?;

        let inbox: ListResponse<InboxEntry> =
            r.get_json_as(&viewer, "/inbox").await?.into_success()?;
        let activity = inbox
            .items
            .iter()
            .find(|entry| entry.reason == "object_activity")
            .expect("object activity notification");
        assert_eq!(activity.actor_user_id, Some(editor.id));
        assert_eq!(activity.actor_username.as_deref(), Some(editor.username.as_str()));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn directed_commentary_overrides_opt_out_but_current_access_still_applies(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("directed notification").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Directed Notification",
                &test_body("Directed Notification", "Body."),
                object_metadata("directed-notification"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "directed-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        clear_setup_notifications(&r, viewer.id).await?;

        let _: ObjectNotificationPreference = r
            .request_json_as(
                &viewer,
                Method::PATCH,
                &format!(
                    "/workspaces/{}/objects/{}/notification-preference",
                    space.workspace.id, object.id,
                ),
                &UpdateObjectNotificationPreferenceRequest {
                    ordinary_notifications_enabled: false,
                },
            )
            .await?
            .into_success()?;

        let created: CommentThreadResponse = r
            .request_json_as(
                &r.admin,
                Method::POST,
                &format!("/workspaces/{}/objects/{}/commentary", space.workspace.id, object.id,),
                &CreateCommentRequest {
                    body: format!("Please review this, @{}.", viewer.username),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?
            .into_success()?;
        let root_comment = &created.thread.comments[0];

        let durable_projection_tasks: i64 = sqlx::query_scalar(
            r#"
            SELECT count(*)
            FROM steda.tasks_kival task
            JOIN kival.notification_candidates candidate
                ON task.idempotency_key = 'notification-event:' || candidate.event_id::text
            WHERE task.name = 'project-notifications'
                AND task.state = 'pending'
                AND candidate.recipient_user_id = $1
                AND candidate.reason = 'mention'
                AND candidate.projected_at IS NULL
            "#,
        )
        .bind(viewer.id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(durable_projection_tasks, 1);

        project_notifications(&r).await?;

        let inbox: ListResponse<InboxEntry> =
            r.get_json_as(&viewer, "/inbox").await?.into_success()?;
        assert_eq!(inbox.items.len(), 1);
        assert_eq!(inbox.items[0].reason, "mention");
        assert_eq!(inbox.items[0].thread_id, Some(created.thread.id));
        assert_eq!(inbox.items[0].comment_id, Some(root_comment.id));
        assert_eq!(
            inbox.items[0].comment_excerpt.as_deref(),
            Some(format!("Please review this, @{}.", viewer.username).as_str())
        );

        let grant_id: uuid::Uuid = sqlx::query_scalar(
            r#"
            SELECT id
            FROM kival.object_grants
            WHERE workspace_id = $1
                AND object_id = $2
                AND principal_user_id = $3
                AND revoked_at IS NULL
            "#,
        )
        .bind(space.workspace.id)
        .bind(object.id)
        .bind(viewer.id)
        .fetch_one(&r.pool)
        .await?;
        let _revoked: ObjectGrantResponse = r
            .empty_json_as(
                &r.admin,
                Method::POST,
                &format!(
                    "/workspaces/{}/objects/{}/grants/{grant_id}/revoke",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;

        let hidden: ListResponse<InboxEntry> =
            r.get_json_as(&viewer, "/inbox").await?.into_success()?;
        assert!(hidden.items.is_empty());
        let count: InboxUnreadCountResponse =
            r.get_json_as(&viewer, "/inbox/unread-count").await?.into_success()?;
        assert_eq!(count.unread_count, 0);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn reply_to_thread_author_overrides_object_opt_out(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("directed reply notification").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Directed Reply",
                &test_body("Directed Reply", "Body."),
                object_metadata("directed-reply"),
            )
            .await?;
        let author = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "reply-author",
                MembershipRole::Member,
                ObjectRole::Editor,
            )
            .await?;
        clear_setup_notifications(&r, author.id).await?;

        let _: ObjectNotificationPreference = r
            .request_json_as(
                &author,
                Method::PATCH,
                &format!(
                    "/workspaces/{}/objects/{}/notification-preference",
                    space.workspace.id, object.id,
                ),
                &UpdateObjectNotificationPreferenceRequest {
                    ordinary_notifications_enabled: false,
                },
            )
            .await?
            .into_success()?;

        let created: CommentThreadResponse = r
            .request_json_as(
                &author,
                Method::POST,
                &format!("/workspaces/{}/objects/{}/commentary", space.workspace.id, object.id,),
                &CreateCommentRequest {
                    body: "Initial thread".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?
            .into_success()?;

        let _reply: CommentResponse = r
            .request_json_as(
                &r.admin,
                Method::POST,
                &format!(
                    "/workspaces/{}/objects/{}/commentary/{}/replies",
                    space.workspace.id, object.id, created.thread.id,
                ),
                &CreateCommentRequest {
                    body: "Reply to the thread owner".to_owned(),
                    mentioned_user_ids: Vec::new(),
                },
            )
            .await?
            .into_success()?;
        project_notifications(&r).await?;

        let inbox: ListResponse<InboxEntry> =
            r.get_json_as(&author, "/inbox").await?.into_success()?;
        assert_eq!(inbox.items.len(), 1);
        assert_eq!(inbox.items[0].reason, "reply");
        assert_eq!(inbox.items[0].thread_id, Some(created.thread.id));
        assert_eq!(inbox.items[0].comment_excerpt.as_deref(), Some("Reply to the thread owner"));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn realtime_invalidations_include_the_actor_without_self_inbox_noise(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("realtime actor invalidation").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Realtime Actor Invalidation",
                &test_body("Realtime Actor Invalidation", "Version one."),
                object_metadata("realtime-actor-v1"),
            )
            .await?;

        r.update_object(
            space.workspace.id,
            object.id,
            Some("Realtime Actor Invalidation v2"),
            None,
            None,
        )
        .await?;

        let (inbox_candidates, realtime_candidates): (i64, i64) = sqlx::query_as(
            r#"
            SELECT
                count(*) FILTER (WHERE candidate.delivery_kind = 'inbox'),
                count(*) FILTER (WHERE candidate.delivery_kind = 'realtime')
            FROM kival.notification_candidates candidate
            JOIN kival.events event
                ON event.id = candidate.event_id
            WHERE event.event_kind = 'object.updated'
                AND event.workspace_id = $1
                AND event.object_id = $2
                AND event.actor_user_id = $3
                AND candidate.recipient_user_id = $3
            "#,
        )
        .bind(space.workspace.id)
        .bind(object.id)
        .bind(r.admin.id)
        .fetch_one(&r.pool)
        .await?;

        assert_eq!(inbox_candidates, 0);
        assert_eq!(realtime_candidates, 1);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_access_grant_does_not_replay_earlier_object_history(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("notification history").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Notification History",
                &test_body("Notification History", "Version one."),
                object_metadata("notification-history-v1"),
            )
            .await?;
        let future_viewer = r
            .create_workspace_actor(
                space.workspace.id,
                "future-notification-viewer",
                MembershipRole::Member,
            )
            .await?;
        clear_setup_notifications(&r, future_viewer.id).await?;

        r.update_object(space.workspace.id, object.id, Some("Notification History v2"), None, None)
            .await?;
        r.create_object_grant(
            space.workspace.id,
            object.id,
            GrantPrincipal::User(future_viewer.id),
            ObjectRole::Viewer,
        )
        .await?;
        project_notifications(&r).await?;

        let inbox: ListResponse<InboxEntry> =
            r.get_json_as(&future_viewer, "/inbox").await?.into_success()?;
        assert_eq!(inbox.items.len(), 1);
        assert_eq!(inbox.items[0].reason, "object_access_granted");
        assert!(inbox.items.iter().all(|entry| entry.reason != "object_activity"));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn direct_workspace_and_object_access_grants_create_inbox_notifications(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("access notification").await?;
        let target = r.create_user("access-notification-target").await?;
        let workspace_name: String =
            sqlx::query_scalar("SELECT name FROM kival.workspaces WHERE id = $1")
                .bind(workspace.id)
                .fetch_one(&r.pool)
                .await?;

        r.add_user_to_workspace(workspace.id, target.id, MembershipRole::Member).await?;
        project_notifications(&r).await?;

        let workspace_inbox: ListResponse<InboxEntry> =
            r.get_json_as(&target, "/inbox").await?.into_success()?;
        assert_eq!(workspace_inbox.items.len(), 1);
        assert_eq!(workspace_inbox.items[0].reason, "workspace_access_granted");
        assert_eq!(workspace_inbox.items[0].workspace_id, workspace.id);
        assert_eq!(workspace_inbox.items[0].workspace_name, workspace_name);
        assert_eq!(workspace_inbox.items[0].object_id, None);
        assert_eq!(workspace_inbox.items[0].object_title, None);
        assert_eq!(workspace_inbox.items[0].actor_user_id, Some(r.admin.id));

        let object = r
            .create_object(
                workspace.id,
                "Granted Object",
                &test_body("Granted Object", "Body."),
                object_metadata("granted-object"),
            )
            .await?;
        r.create_object_grant(
            workspace.id,
            object.id,
            GrantPrincipal::User(target.id),
            ObjectRole::Viewer,
        )
        .await?;
        project_notifications(&r).await?;

        let inbox: ListResponse<InboxEntry> =
            r.get_json_as(&target, "/inbox").await?.into_success()?;
        let object_notice = inbox
            .items
            .iter()
            .find(|entry| entry.reason == "object_access_granted")
            .expect("object access notification");
        assert_eq!(object_notice.workspace_name, workspace_name);
        assert_eq!(object_notice.object_id, Some(object.id));
        assert_eq!(object_notice.object_title.as_deref(), Some("Granted Object"));
        assert_eq!(object_notice.actor_user_id, Some(r.admin.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn projection_does_not_skip_events_that_commit_out_of_sequence_order(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("out of order notification projection").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Out of Order Projection",
                &test_body("Out of Order Projection", "Body."),
                object_metadata("out-of-order-projection"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "out-of-order-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        clear_setup_notifications(&r, viewer.id).await?;

        let mut older_tx = r.pool.begin().await?;
        let (older_event_id, older_sequence): (uuid::Uuid, i64) = sqlx::query_as(
            r#"
            INSERT INTO kival.events (
                workspace_id,
                actor_user_id,
                event_kind,
                object_id,
                payload
            )
            VALUES ($1, $2, 'object.updated', $3, '{}'::jsonb)
            RETURNING id, sequence_number
            "#,
        )
        .bind(space.workspace.id)
        .bind(r.admin.id)
        .bind(object.id)
        .fetch_one(&mut *older_tx)
        .await?;

        let mut newer_tx = r.pool.begin().await?;
        let (newer_event_id, newer_sequence): (uuid::Uuid, i64) = sqlx::query_as(
            r#"
            INSERT INTO kival.events (
                workspace_id,
                actor_user_id,
                event_kind,
                object_id,
                payload
            )
            VALUES ($1, $2, 'object.updated', $3, '{}'::jsonb)
            RETURNING id, sequence_number
            "#,
        )
        .bind(space.workspace.id)
        .bind(r.admin.id)
        .bind(object.id)
        .fetch_one(&mut *newer_tx)
        .await?;
        assert!(older_sequence < newer_sequence);
        newer_tx.commit().await?;

        project_notifications(&r).await?;
        let after_newer: ListResponse<InboxEntry> =
            r.get_json_as(&viewer, "/inbox").await?.into_success()?;
        assert_eq!(after_newer.items.len(), 1);
        assert_eq!(after_newer.items[0].latest_event_id, newer_event_id);

        older_tx.commit().await?;
        project_notifications(&r).await?;

        let after_older: ListResponse<InboxEntry> =
            r.get_json_as(&viewer, "/inbox").await?.into_success()?;
        assert_eq!(after_older.items.len(), 2);
        assert!(after_older.items.iter().any(|entry| entry.latest_event_id == older_event_id));
        assert!(after_older.items.iter().any(|entry| entry.latest_event_id == newer_event_id));

        let pending: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM kival.notification_candidates WHERE projected_at IS NULL",
        )
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(pending, 0);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn grouped_inbox_projection_is_source_ordered_across_workers(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("grouped notification projection order").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Grouped Notification Projection",
                &test_body("Grouped Notification Projection", "Body."),
                object_metadata("grouped-notification-projection"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "grouped-projection-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        clear_setup_notifications(&r, viewer.id).await?;

        let older_event_id: uuid::Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO kival.events (
                workspace_id,
                actor_user_id,
                event_kind,
                object_id,
                payload
            )
            VALUES ($1, $2, 'test.grouped.reply', $3, '{}'::jsonb)
            RETURNING id
            "#,
        )
        .bind(space.workspace.id)
        .bind(r.admin.id)
        .bind(object.id)
        .fetch_one(&r.pool)
        .await?;
        let newer_event_id: uuid::Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO kival.events (
                workspace_id,
                actor_user_id,
                event_kind,
                object_id,
                payload
            )
            VALUES ($1, $2, 'test.grouped.mention', $3, '{}'::jsonb)
            RETURNING id
            "#,
        )
        .bind(space.workspace.id)
        .bind(r.admin.id)
        .bind(object.id)
        .fetch_one(&r.pool)
        .await?;

        let deduplication_key = format!("test-group:{}", object.id);
        let (older_candidate_id, older_sequence): (uuid::Uuid, i64) = sqlx::query_as(
            r#"
            INSERT INTO kival.notification_candidates (
                event_id,
                recipient_user_id,
                workspace_id,
                object_id,
                actor_user_id,
                delivery_kind,
                notification_type,
                reason,
                deduplication_key
            )
            VALUES ($1, $2, $3, $4, $5, 'inbox', 'reply', 'reply', $6)
            RETURNING id, sequence_number
            "#,
        )
        .bind(older_event_id)
        .bind(viewer.id)
        .bind(space.workspace.id)
        .bind(object.id)
        .bind(r.admin.id)
        .bind(&deduplication_key)
        .fetch_one(&r.pool)
        .await?;
        let (_newer_candidate_id, newer_sequence): (uuid::Uuid, i64) = sqlx::query_as(
            r#"
            INSERT INTO kival.notification_candidates (
                event_id,
                recipient_user_id,
                workspace_id,
                object_id,
                actor_user_id,
                delivery_kind,
                notification_type,
                reason,
                deduplication_key
            )
            VALUES ($1, $2, $3, $4, $5, 'inbox', 'mention', 'mention', $6)
            RETURNING id, sequence_number
            "#,
        )
        .bind(newer_event_id)
        .bind(viewer.id)
        .bind(space.workspace.id)
        .bind(object.id)
        .bind(r.admin.id)
        .bind(&deduplication_key)
        .fetch_one(&r.pool)
        .await?;
        assert!(older_sequence < newer_sequence);

        let mut blocked_older = r.pool.begin().await?;
        let _: uuid::Uuid = sqlx::query_scalar(
            "SELECT id FROM kival.notification_candidates WHERE id = $1 FOR UPDATE",
        )
        .bind(older_candidate_id)
        .fetch_one(&mut *blocked_older)
        .await?;

        let (processed, changed, _): (i32, i32, i64) =
            sqlx::query_as("SELECT * FROM kival.process_notification_candidate_batch(1)")
                .fetch_one(&r.pool)
                .await?;
        assert_eq!(processed, 1);
        assert_eq!(changed, 1);

        let (
            inbox_id,
            first_event_count,
            first_reason,
            first_source_event_id,
            first_latest_event_id,
            first_sequence,
        ): (uuid::Uuid, i32, String, uuid::Uuid, uuid::Uuid, i64) = sqlx::query_as(
            r#"
            SELECT
                id,
                event_count,
                reason,
                source_event_id,
                latest_event_id,
                sequence_number
            FROM kival.inbox_notifications
            WHERE recipient_user_id = $1
                AND deduplication_key = $2
            "#,
        )
        .bind(viewer.id)
        .bind(&deduplication_key)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(first_event_count, 1);
        assert_eq!(first_reason, "mention");
        assert_eq!(first_source_event_id, newer_event_id);
        assert_eq!(first_latest_event_id, newer_event_id);
        assert_eq!(first_sequence, newer_sequence);

        sqlx::query("UPDATE kival.inbox_notifications SET read_at = now() WHERE id = $1")
            .bind(inbox_id)
            .execute(&r.pool)
            .await?;

        blocked_older.commit().await?;
        project_notifications(&r).await?;

        let (
            event_count,
            notification_type,
            reason,
            source_event_id,
            latest_event_id,
            is_read,
            sequence_number,
            source_candidate_sequence_number,
            directed_candidate_sequence_number,
        ): (i32, String, String, uuid::Uuid, uuid::Uuid, bool, i64, i64, Option<i64>) =
            sqlx::query_as(
                r#"
                SELECT
                    event_count,
                    notification_type,
                    reason,
                    source_event_id,
                    latest_event_id,
                    read_at IS NOT NULL,
                    sequence_number,
                    source_candidate_sequence_number,
                    directed_candidate_sequence_number
                FROM kival.inbox_notifications
                WHERE id = $1
                "#,
            )
            .bind(inbox_id)
            .fetch_one(&r.pool)
            .await?;

        assert_eq!(event_count, 2);
        assert_eq!(notification_type, "mention");
        assert_eq!(reason, "mention");
        assert_eq!(source_event_id, older_event_id);
        assert_eq!(latest_event_id, newer_event_id);
        assert!(is_read);
        assert_eq!(sequence_number, newer_sequence);
        assert_eq!(source_candidate_sequence_number, older_sequence);
        assert_eq!(directed_candidate_sequence_number, Some(newer_sequence));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn access_revocation_emits_identifier_free_realtime_resync(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("access revocation realtime resync").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Access Revocation Resync",
                &test_body("Access Revocation Resync", "Body."),
                object_metadata("access-revocation-resync"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "access-revocation-resync-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        clear_setup_notifications(&r, viewer.id).await?;

        let grant_id: uuid::Uuid = sqlx::query_scalar(
            r#"
            SELECT id
            FROM kival.object_grants
            WHERE workspace_id = $1
                AND object_id = $2
                AND principal_user_id = $3
                AND revoked_at IS NULL
            "#,
        )
        .bind(space.workspace.id)
        .bind(object.id)
        .bind(viewer.id)
        .fetch_one(&r.pool)
        .await?;

        let mut listener = sqlx::postgres::PgListener::connect_with(&r.pool).await?;
        listener.listen("kival_realtime").await?;

        let _: ObjectGrantResponse = r
            .empty_json_as(
                &r.admin,
                Method::POST,
                &format!(
                    "/workspaces/{}/objects/{}/grants/{grant_id}/revoke",
                    space.workspace.id, object.id,
                ),
            )
            .await?
            .into_success()?;

        let notification =
            tokio::time::timeout(std::time::Duration::from_secs(2), listener.recv()).await??;
        let payload: serde_json::Value = serde_json::from_str(notification.payload())?;
        let viewer_id = viewer.id.to_string();
        assert_eq!(payload["recipient_user_id"].as_str(), Some(viewer_id.as_str()));
        assert_eq!(payload["type"], "realtime.resync_required");
        assert!(payload["workspace_id"].is_null());
        assert!(payload["object_id"].is_null());
        assert!(payload["event_id"].is_null());
        assert!(payload["inbox_entry_id"].is_null());

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_archive_resync_reaches_viewer_without_resource_identifiers(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("object archive realtime resync").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Object Archive Resync",
                &test_body("Object Archive Resync", "Body."),
                object_metadata("object-archive-resync"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "object-archive-resync-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        clear_setup_notifications(&r, viewer.id).await?;

        let mut listener = sqlx::postgres::PgListener::connect_with(&r.pool).await?;
        listener.listen("kival_realtime").await?;

        r.archive_object(space.workspace.id, object.id).await?;
        let viewer_id = viewer.id.to_string();

        let payload = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let notification = listener.recv().await?;
                let payload: serde_json::Value = serde_json::from_str(notification.payload())?;
                if payload["recipient_user_id"].as_str() == Some(viewer_id.as_str()) {
                    return Ok::<_, eyre::Report>(payload);
                }
            }
        })
        .await??;

        assert_eq!(payload["type"], "realtime.resync_required");
        assert!(payload["workspace_id"].is_null());
        assert!(payload["object_id"].is_null());
        assert!(payload["event_id"].is_null());
        assert!(payload["inbox_entry_id"].is_null());

        let durable_archive_invalidations: i64 = sqlx::query_scalar(
            r#"
            SELECT count(*)
            FROM kival.notification_candidates candidate
            JOIN kival.events event
                ON event.id = candidate.event_id
            WHERE event.event_kind = 'object.archived'
                AND event.workspace_id = $1
                AND event.object_id = $2
                AND candidate.delivery_kind = 'realtime'
            "#,
        )
        .bind(space.workspace.id)
        .bind(object.id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(durable_archive_invalidations, 0);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn inbox_cursors_are_bound_to_filters_and_recipient(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("inbox cursor binding").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Inbox Cursor Binding",
                &test_body("Inbox Cursor Binding", "Body."),
                object_metadata("inbox-cursor-binding"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "inbox-cursor-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        clear_setup_notifications(&r, viewer.id).await?;

        r.update_object(space.workspace.id, object.id, Some("Inbox Cursor v2"), None, None).await?;
        r.update_object(space.workspace.id, object.id, Some("Inbox Cursor v3"), None, None).await?;
        project_notifications(&r).await?;

        let unread: ListResponse<InboxEntry> =
            r.get_json_as(&viewer, "/inbox?unread_only=true&limit=1").await?.into_success()?;
        let cursor = unread.next_cursor.expect("unread inbox should produce a cursor");

        r.request(Some(&viewer), Method::GET, &format!("/inbox?limit=1&cursor={cursor}"), None)
            .await?
            .assert_status(StatusCode::BAD_REQUEST);

        let other = r.create_user("inbox-cursor-other-user").await?;
        r.request(
            Some(&other),
            Method::GET,
            &format!("/inbox?unread_only=true&limit=1&cursor={cursor}"),
            None,
        )
        .await?
        .assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn expired_candidates_are_terminal_without_late_delivery(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("expired notification candidate").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Expired Notification Candidate",
                &test_body("Expired Notification Candidate", "Version one."),
                object_metadata("expired-notification-candidate-v1"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "expired-notification-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        clear_setup_notifications(&r, viewer.id).await?;

        r.update_object(
            space.workspace.id,
            object.id,
            Some("Expired Notification Candidate v2"),
            None,
            None,
        )
        .await?;

        let expired = sqlx::query(
            r#"
            UPDATE kival.notification_candidates
            SET expires_at = now()
            WHERE recipient_user_id = $1
                AND projected_at IS NULL
            "#,
        )
        .bind(viewer.id)
        .execute(&r.pool)
        .await?
        .rows_affected();
        assert!(expired > 0);

        project_notifications(&r).await?;

        let inbox: ListResponse<InboxEntry> =
            r.get_json_as(&viewer, "/inbox").await?.into_success()?;
        assert!(inbox.items.is_empty());

        let terminal: i64 = sqlx::query_scalar(
            r#"
            SELECT count(*)
            FROM kival.notification_candidates
            WHERE recipient_user_id = $1
                AND expires_at <= now()
                AND projected_at IS NOT NULL
            "#,
        )
        .bind(viewer.id)
        .fetch_one(&r.pool)
        .await?;
        assert!(terminal > 0);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn notification_retention_deletes_expired_state(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("notification retention").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Notification Retention",
                &test_body("Notification Retention", "Version one."),
                object_metadata("notification-retention-v1"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "notification-retention-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;
        clear_setup_notifications(&r, viewer.id).await?;

        r.update_object(
            space.workspace.id,
            object.id,
            Some("Notification Retention v2"),
            None,
            None,
        )
        .await?;
        project_notifications(&r).await?;

        let live_inbox: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM kival.inbox_notifications WHERE recipient_user_id = $1",
        )
        .bind(viewer.id)
        .fetch_one(&r.pool)
        .await?;
        assert!(live_inbox > 0);

        sqlx::query(
            r#"
            UPDATE kival.notification_candidates
            SET expires_at = now()
            WHERE recipient_user_id = $1
            "#,
        )
        .bind(viewer.id)
        .execute(&r.pool)
        .await?;
        sqlx::query(
            r#"
            UPDATE kival.inbox_notifications
            SET expires_at = now()
            WHERE recipient_user_id = $1
            "#,
        )
        .bind(viewer.id)
        .execute(&r.pool)
        .await?;

        let (candidates_deleted, inbox_deleted): (i32, i32) =
            sqlx::query_as("SELECT * FROM kival.apply_notification_retention(100)")
                .fetch_one(&r.pool)
                .await?;
        assert!(candidates_deleted > 0);
        assert_eq!(inbox_deleted, i32::try_from(live_inbox)?);

        let expired_candidates: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM kival.notification_candidates WHERE expires_at <= now()",
        )
        .fetch_one(&r.pool)
        .await?;
        let expired_inbox: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM kival.inbox_notifications WHERE expires_at <= now()",
        )
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(expired_candidates, 0);
        assert_eq!(expired_inbox, 0);

        Ok(())
    }
}
