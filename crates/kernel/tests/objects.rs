//! Integration tests for kernel object lifecycle transitions.

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use kival_kernel::{
        ArchiveStatus, CreateInitialObject, KernelError, ObjectRole, Result, archive_object,
        create_initial_object, fetch_object, unarchive_object,
    };
    use serde_json::json;
    use sqlx::PgPool;
    use uuid::Uuid;

    /// Builds a unique database name for this test process.
    fn unique_name(prefix: &str) -> String {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        format!("{prefix}-{suffix}")
    }

    /// Inserts a user for state-transition attribution.
    async fn insert_user(pool: &PgPool) -> Result<Uuid> {
        sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.users (username, display_name)
            VALUES ($1, $2)
            RETURNING id
            "#,
        )
        .bind(unique_name("kuser"))
        .bind("Kernel User")
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    /// Inserts an active workspace and creator administrator membership.
    async fn insert_workspace(pool: &PgPool, user_id: Uuid) -> Result<Uuid> {
        let workspace_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.workspaces (name, created_by)
            VALUES ($1, $2)
            RETURNING id
            "#,
        )
        .bind(unique_name("kernel-workspace"))
        .bind(user_id)
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
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(workspace_id)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn failed_initial_object_transition_can_be_caught_and_outer_tx_committed(
        pool: PgPool,
    ) -> Result<()> {
        let user_id = insert_user(&pool).await?;
        let workspace_id = insert_workspace(&pool, user_id).await?;

        let mut tx = pool.begin().await?;
        let error = create_initial_object(
            &mut tx,
            CreateInitialObject {
                workspace_id,
                title: "Invalid object".to_owned(),
                body: "Body".to_owned(),
                metadata: json!({"nested": {"value": true}}),
                created_by: user_id,
            },
        )
        .await
        .expect_err("nested metadata must fail after object bootstrap starts");
        assert!(matches!(error, KernelError::Database(sqlx::Error::Database(_))));

        tx.commit().await?;

        let object_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM kival.objects WHERE workspace_id = $1 AND created_by = $2",
        )
        .bind(workspace_id)
        .bind(user_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(object_count, 0);

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn protected_object_read_rechecks_current_access(pool: PgPool) -> Result<()> {
        let user_id = insert_user(&pool).await?;
        let workspace_id = insert_workspace(&pool, user_id).await?;

        let mut tx = pool.begin().await?;
        let created = create_initial_object(
            &mut tx,
            CreateInitialObject {
                workspace_id,
                title: "Protected object".to_owned(),
                body: "Body".to_owned(),
                metadata: json!({}),
                created_by: user_id,
            },
        )
        .await?;
        tx.commit().await?;

        fetch_object(&pool, user_id, workspace_id, created.object_id).await?;

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
        .bind(user_id)
        .execute(&pool)
        .await?;

        let error = fetch_object(&pool, user_id, workspace_id, created.object_id)
            .await
            .expect_err("revoked readers must not receive object data");
        assert!(matches!(error, KernelError::CapabilityRequired));

        Ok(())
    }

    /// Verifies a concurrent workspace archive cannot pass a held child-resource lock.
    async fn assert_workspace_archive_blocked(
        pool: &PgPool,
        workspace_id: Uuid,
        actor_id: Uuid,
    ) -> Result<()> {
        let mut competing = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *competing).await?;
        let error = sqlx::query(
            r#"
            UPDATE kival.workspaces
            SET status = 'archived',
                archived_at = now(),
                archived_by = $2
            WHERE id = $1
                AND archived_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(actor_id)
        .execute(&mut *competing)
        .await
        .expect_err("workspace archival must wait for the child-resource lock");
        let sqlx::Error::Database(database) = error else {
            panic!("expected PostgreSQL lock timeout");
        };
        assert_eq!(database.code().as_deref(), Some("55P03"));
        competing.rollback().await?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn object_mutations_pin_active_workspace_lifecycle(pool: PgPool) -> Result<()> {
        let user_id = insert_user(&pool).await?;
        let workspace_id = insert_workspace(&pool, user_id).await?;

        let mut create_tx = pool.begin().await?;
        let created = create_initial_object(
            &mut create_tx,
            CreateInitialObject {
                workspace_id,
                title: "Pinned workspace object".to_owned(),
                body: "Body".to_owned(),
                metadata: json!({}),
                created_by: user_id,
            },
        )
        .await?;
        create_tx.commit().await?;

        let mut archive_tx = pool.begin().await?;
        archive_object(&mut archive_tx, workspace_id, created.object_id, user_id).await?;
        assert_workspace_archive_blocked(&pool, workspace_id, user_id).await?;
        archive_tx.commit().await?;

        let mut restore_tx = pool.begin().await?;
        unarchive_object(&mut restore_tx, workspace_id, created.object_id).await?;
        assert_workspace_archive_blocked(&pool, workspace_id, user_id).await?;
        restore_tx.rollback().await?;

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn object_bindings_follow_postgres_lifecycle(pool: PgPool) -> Result<()> {
        let user_id = insert_user(&pool).await?;
        let workspace_id = insert_workspace(&pool, user_id).await?;

        let mut tx = pool.begin().await?;
        let created = create_initial_object(
            &mut tx,
            CreateInitialObject {
                workspace_id,
                title: "Kernel object".to_owned(),
                body: "Body".to_owned(),
                metadata: json!({"kind": "test"}),
                created_by: user_id,
            },
        )
        .await?;
        let object_id = created.object_id;
        let creator_grant_id = created.creator_grant_id;
        let version = created.version;
        tx.commit().await?;

        let readable = fetch_object(&pool, user_id, workspace_id, object_id).await?;
        assert_eq!(readable.effective_role, ObjectRole::Admin);
        assert_eq!(readable.object.current_version_id, Some(version.id));
        assert_eq!(readable.object.status, ArchiveStatus::Active);

        let role = sqlx::query_scalar::<_, String>(
            r#"
            SELECT object_role::text
            FROM kival.object_grants
            WHERE id = $1
            "#,
        )
        .bind(creator_grant_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(role, "admin");

        let mut tx = pool.begin().await?;
        archive_object(&mut tx, workspace_id, object_id, user_id).await?;
        tx.commit().await?;

        let archived = fetch_object(&pool, user_id, workspace_id, object_id).await?;
        assert_eq!(archived.effective_role, ObjectRole::Admin);
        assert_eq!(archived.object.status, ArchiveStatus::Archived);
        assert_eq!(archived.object.archived_by, Some(user_id));
        assert!(archived.object.archived_at.is_some());

        let mut tx = pool.begin().await?;
        unarchive_object(&mut tx, workspace_id, object_id).await?;
        tx.commit().await?;

        let active = fetch_object(&pool, user_id, workspace_id, object_id).await?;
        assert_eq!(active.effective_role, ObjectRole::Admin);
        assert_eq!(active.object.status, ArchiveStatus::Active);
        assert!(active.object.archived_by.is_none());
        assert!(active.object.archived_at.is_none());

        Ok(())
    }
}
