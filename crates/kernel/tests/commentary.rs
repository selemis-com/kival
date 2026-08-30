//! Integration tests for kernel commentary behavior.

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use kival_kernel::{
        KernelError, Result, delete_comment, fetch_comment_mentions, replace_comment_mentions,
    };
    use sqlx::PgPool;
    use uuid::Uuid;

    async fn insert_user(pool: &PgPool, prefix: &str) -> Result<Uuid> {
        let suffix = Uuid::now_v7().simple().to_string();
        let username = format!("{prefix}-{}", &suffix[..12]);
        Ok(sqlx::query_scalar(
            r#"
            INSERT INTO kival.users (username, display_name)
            VALUES ($1, $2)
            RETURNING id
            "#,
        )
        .bind(username)
        .bind(prefix)
        .fetch_one(pool)
        .await?)
    }

    async fn insert_workspace_object(pool: &PgPool, actor_id: Uuid) -> Result<(Uuid, Uuid)> {
        let workspace_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.workspaces (name, created_by)
            VALUES ($1, $2)
            RETURNING id
            "#,
        )
        .bind(format!("commentary-{}", Uuid::now_v7().simple()))
        .bind(actor_id)
        .fetch_one(pool)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO kival.workspace_memberships (
                workspace_id, user_id, workspace_role, created_by
            )
            VALUES ($1, $2, 'admin', $2)
            "#,
        )
        .bind(workspace_id)
        .bind(actor_id)
        .execute(pool)
        .await?;
        let object_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO kival.objects (workspace_id, created_by) VALUES ($1, $2) RETURNING id",
        )
        .bind(workspace_id)
        .bind(actor_id)
        .fetch_one(pool)
        .await?;
        Ok((workspace_id, object_id))
    }

    async fn insert_comment_with_mention(
        pool: &PgPool,
        workspace_id: Uuid,
        object_id: Uuid,
        author_id: Uuid,
        mentioned_user_id: Uuid,
    ) -> Result<Uuid> {
        let thread_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.comment_threads (workspace_id, object_id, created_by)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
        )
        .bind(workspace_id)
        .bind(object_id)
        .bind(author_id)
        .fetch_one(pool)
        .await?;
        let comment_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.comments (
                workspace_id, object_id, thread_id, author_user_id, body
            )
            VALUES ($1, $2, $3, $4, 'mention')
            RETURNING id
            "#,
        )
        .bind(workspace_id)
        .bind(object_id)
        .bind(thread_id)
        .bind(author_id)
        .fetch_one(pool)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO kival.comment_mentions (
                workspace_id, object_id, comment_id, mentioned_user_id
            )
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(workspace_id)
        .bind(object_id)
        .bind(comment_id)
        .bind(mentioned_user_id)
        .execute(pool)
        .await?;
        Ok(comment_id)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mention_hydration_rechecks_access_and_object_scope(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "comment-reader").await?;
        let mentioned_user_id = insert_user(&pool, "mentioned-user").await?;
        let (workspace_id, object_id) = insert_workspace_object(&pool, actor_id).await?;
        let comment_id = insert_comment_with_mention(
            &pool,
            workspace_id,
            object_id,
            actor_id,
            mentioned_user_id,
        )
        .await?;
        let other_object_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO kival.objects (workspace_id, created_by) VALUES ($1, $2) RETURNING id",
        )
        .bind(workspace_id)
        .bind(actor_id)
        .fetch_one(&pool)
        .await?;
        let other_comment_id = insert_comment_with_mention(
            &pool,
            workspace_id,
            other_object_id,
            actor_id,
            mentioned_user_id,
        )
        .await?;

        let mentions = fetch_comment_mentions(
            &pool,
            actor_id,
            workspace_id,
            object_id,
            &[comment_id, other_comment_id],
        )
        .await?;
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].comment_id, comment_id);
        assert_eq!(mentions[0].user_id, mentioned_user_id);

        sqlx::query(
            r#"
            UPDATE kival.workspace_memberships
            SET revoked_at = now(),
                revoked_by = $2
            WHERE workspace_id = $1
                AND user_id = $2
                AND revoked_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(actor_id)
        .execute(&pool)
        .await?;

        let error = fetch_comment_mentions(&pool, actor_id, workspace_id, object_id, &[comment_id])
            .await
            .expect_err("revoked actor must not hydrate comment mentions");
        assert!(matches!(error, KernelError::CapabilityRequired));

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn comment_mention_mutations_preserve_resource_scope(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "scope-author").await?;
        let mentioned_user_id = insert_user(&pool, "scope-mention").await?;
        let (workspace_id, object_id) = insert_workspace_object(&pool, actor_id).await?;
        let other_object_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO kival.objects (workspace_id, created_by) VALUES ($1, $2) RETURNING id",
        )
        .bind(workspace_id)
        .bind(actor_id)
        .fetch_one(&pool)
        .await?;
        let comment_id = insert_comment_with_mention(
            &pool,
            workspace_id,
            other_object_id,
            actor_id,
            mentioned_user_id,
        )
        .await?;
        let (foreign_workspace_id, _) = insert_workspace_object(&pool, actor_id).await?;

        let mut tx = pool.begin().await?;
        let error = replace_comment_mentions(&mut tx, workspace_id, object_id, comment_id, &[])
            .await
            .expect_err("wrong object scope must reject mention replacement");
        assert!(matches!(error, KernelError::Database(sqlx::Error::RowNotFound)));

        let error = replace_comment_mentions(
            &mut tx,
            foreign_workspace_id,
            other_object_id,
            comment_id,
            &[],
        )
        .await
        .expect_err("wrong workspace scope must reject mention replacement");
        assert!(matches!(error, KernelError::Database(sqlx::Error::RowNotFound)));

        delete_comment(&mut tx, workspace_id, object_id, comment_id, actor_id).await?;
        delete_comment(&mut tx, foreign_workspace_id, other_object_id, comment_id, actor_id)
            .await?;
        tx.commit().await?;

        let (body, deleted_at) = sqlx::query_as::<_, (Option<String>, Option<DateTime<Utc>>)>(
            "SELECT body, deleted_at FROM kival.comments WHERE id = $1",
        )
        .bind(comment_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(body.as_deref(), Some("mention"));
        assert!(deleted_at.is_none());

        let mentions = sqlx::query_scalar::<_, Uuid>(
            "SELECT mentioned_user_id FROM kival.comment_mentions WHERE comment_id = $1",
        )
        .bind(comment_id)
        .fetch_all(&pool)
        .await?;
        assert_eq!(mentions, vec![mentioned_user_id]);

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mention_replacement_rolls_back_to_savepoint_on_insert_failure(
        pool: PgPool,
    ) -> Result<()> {
        let actor_id = insert_user(&pool, "comment-author").await?;
        let original_mention_id = insert_user(&pool, "original-mention").await?;
        let (workspace_id, object_id) = insert_workspace_object(&pool, actor_id).await?;
        let comment_id = insert_comment_with_mention(
            &pool,
            workspace_id,
            object_id,
            actor_id,
            original_mention_id,
        )
        .await?;
        let missing_user_id = Uuid::now_v7();

        let mut tx = pool.begin().await?;
        let error = replace_comment_mentions(
            &mut tx,
            workspace_id,
            object_id,
            comment_id,
            &[missing_user_id],
        )
        .await
        .expect_err("invalid replacement mention must fail");
        assert!(matches!(error, KernelError::Database(_)));

        // The inner savepoint must recover the transaction so callers can choose to commit it.
        tx.commit().await?;

        let mentions = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT mentioned_user_id
            FROM kival.comment_mentions
            WHERE comment_id = $1
            ORDER BY mentioned_user_id
            "#,
        )
        .bind(comment_id)
        .fetch_all(&pool)
        .await?;
        assert_eq!(mentions, vec![original_mention_id]);

        Ok(())
    }
}
