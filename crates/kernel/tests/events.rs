//! Integration tests for kernel event queries.

#[cfg(test)]
mod tests {
    use kival_kernel::{
        CreateInitialObject, EventOrder, KernelError, ListEvents, Result, create_api_key,
        create_initial_object, create_user, create_workspace, list_events, list_workspace_events,
    };
    use serde_json::json;
    use sqlx::PgPool;
    use uuid::Uuid;

    const fn empty_query() -> ListEvents<'static> {
        ListEvents {
            after_sequence: None,
            before_sequence: None,
            event_kind: None,
            actor_user_id: None,
            target_user_id: None,
            object_id: None,
            group_id: None,
            order: EventOrder::Asc,
            limit: 10,
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn protected_event_reads_authorize_even_when_empty(pool: PgPool) -> Result<()> {
        let mut tx = pool.begin().await?;
        let admin = create_user(&mut tx, "admin", "Admin").await?;
        let outsider = create_user(&mut tx, "outsider", "Outsider").await?;
        sqlx::query(
            r#"
            INSERT INTO kival.global_admins (user_id, created_by)
            VALUES ($1, $1)
            "#,
        )
        .bind(admin.id)
        .execute(&mut *tx)
        .await?;
        let workspace = create_workspace(&mut tx, "workspace", None, admin.id).await?;
        tx.commit().await?;

        let error = list_events(&pool, outsider.id, empty_query())
            .await
            .expect_err("non-admin must not list global events");
        assert!(matches!(error, KernelError::CapabilityRequired));
        assert!(list_events(&pool, admin.id, empty_query()).await?.is_empty());

        let error = list_workspace_events(&pool, workspace.id, outsider.id, empty_query())
            .await
            .expect_err("non-admin must not list workspace events");
        assert!(matches!(error, KernelError::CapabilityRequired));
        assert!(
            list_workspace_events(&pool, workspace.id, admin.id, empty_query()).await?.is_empty()
        );

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn event_storage_enforces_complete_and_consistent_api_key_attribution(
        pool: PgPool,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;
        let actor = create_user(&mut tx, "event-key-owner", "Event Key Owner").await?;
        let other = create_user(&mut tx, "event-other", "Event Other").await?;
        let key = create_api_key(&mut tx, actor.id, "event-key", &[1_u8; 32], None)
            .await?
            .expect("non-expiring API key must be created");
        tx.commit().await?;

        let missing_label = sqlx::query(
            r#"
            INSERT INTO kival.events (actor_user_id, api_key_id, event_kind)
            VALUES ($1, $2, 'test.api_key_attribution')
            "#,
        )
        .bind(actor.id)
        .bind(key.id)
        .execute(&pool)
        .await
        .expect_err("API key attribution must include a label");
        assert_eq!(
            missing_label.as_database_error().and_then(|error| error.constraint()),
            Some("events_api_key_attribution_complete")
        );

        let wrong_label = sqlx::query(
            r#"
            INSERT INTO kival.events (actor_user_id, api_key_id, api_key_label, event_kind)
            VALUES ($1, $2, $3, 'test.api_key_attribution')
            "#,
        )
        .bind(actor.id)
        .bind(key.id)
        .bind("wrong-label")
        .execute(&pool)
        .await
        .expect_err("API key attribution must use the key's stable label");
        assert_eq!(
            wrong_label.as_database_error().and_then(|error| error.constraint()),
            Some("events_api_key_attribution_matches_key")
        );

        let wrong_actor = sqlx::query(
            r#"
            INSERT INTO kival.events (actor_user_id, api_key_id, api_key_label, event_kind)
            VALUES ($1, $2, $3, 'test.api_key_attribution')
            "#,
        )
        .bind(other.id)
        .bind(key.id)
        .bind(&key.label)
        .execute(&pool)
        .await
        .expect_err("API key attribution must retain the key owner as actor");
        assert_eq!(
            wrong_actor.as_database_error().and_then(|error| error.constraint()),
            Some("events_api_key_attribution_matches_key")
        );

        sqlx::query(
            r#"
            INSERT INTO kival.events (actor_user_id, api_key_id, api_key_label, event_kind)
            VALUES ($1, $2, $3, 'test.api_key_attribution')
            "#,
        )
        .bind(actor.id)
        .bind(key.id)
        .bind(&key.label)
        .execute(&pool)
        .await?;

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn commentary_event_subjects_require_matching_thread_and_comment(
        pool: PgPool,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;
        let actor = create_user(&mut tx, "event-comment", "Event Comment Actor").await?;
        let workspace =
            create_workspace(&mut tx, "comment event workspace", None, actor.id).await?;
        let object = create_initial_object(
            &mut tx,
            CreateInitialObject {
                workspace_id: workspace.id,
                title: "Comment Event Object".to_owned(),
                body: "Body.".to_owned(),
                metadata: json!({}),
                created_by: actor.id,
            },
        )
        .await?;
        tx.commit().await?;

        let first_thread = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.comment_threads (workspace_id, object_id, created_by)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
        )
        .bind(workspace.id)
        .bind(object.object_id)
        .bind(actor.id)
        .fetch_one(&pool)
        .await?;
        let first_comment = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.comments (workspace_id, object_id, thread_id, author_user_id, body)
            VALUES ($1, $2, $3, $4, 'First')
            RETURNING id
            "#,
        )
        .bind(workspace.id)
        .bind(object.object_id)
        .bind(first_thread)
        .bind(actor.id)
        .fetch_one(&pool)
        .await?;
        let second_thread = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.comment_threads (workspace_id, object_id, created_by)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
        )
        .bind(workspace.id)
        .bind(object.object_id)
        .bind(actor.id)
        .fetch_one(&pool)
        .await?;

        let mismatch = sqlx::query(
            r#"
            INSERT INTO kival.events (
                workspace_id, actor_user_id, event_kind, object_id, comment_thread_id, comment_id
            )
            VALUES ($1, $2, 'test.comment_subject_mismatch', $3, $4, $5)
            "#,
        )
        .bind(workspace.id)
        .bind(actor.id)
        .bind(object.object_id)
        .bind(second_thread)
        .bind(first_comment)
        .execute(&pool)
        .await
        .expect_err("comment event must identify its actual thread");
        assert!(mismatch.to_string().contains("event comment does not belong to comment_thread"));

        let missing_thread = sqlx::query(
            r#"
            INSERT INTO kival.events (
                workspace_id, actor_user_id, event_kind, object_id, comment_id
            )
            VALUES ($1, $2, 'test.comment_subject_missing_thread', $3, $4)
            "#,
        )
        .bind(workspace.id)
        .bind(actor.id)
        .bind(object.object_id)
        .bind(first_comment)
        .execute(&pool)
        .await
        .expect_err("comment event must also identify its thread");
        assert!(
            missing_thread.to_string().contains("comment_thread_id is required for comment event")
        );

        Ok(())
    }
}
