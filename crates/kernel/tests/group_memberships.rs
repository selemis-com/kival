//! Integration tests for kernel group membership transitions.

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use kival_kernel::{
        KernelError, MembershipRole, Result, create_group_membership, replace_group_membership,
        revoke_group_membership,
    };
    use sqlx::PgPool;
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    fn unique_name(prefix: &str) -> String {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        format!("{prefix}-{suffix}")
    }

    struct TestUser {
        id: Uuid,
    }

    async fn insert_user(pool: &PgPool) -> Result<TestUser> {
        let username = unique_name("member");
        let display_name = "Group Member".to_owned();
        let id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.users (username, display_name)
            VALUES ($1, $2)
            RETURNING id
            "#,
        )
        .bind(&username)
        .bind(&display_name)
        .fetch_one(pool)
        .await?;
        Ok(TestUser { id })
    }

    async fn insert_group(pool: &PgPool, actor_id: Uuid) -> Result<Uuid> {
        Ok(sqlx::query_scalar(
            r#"
            INSERT INTO kival.groups (name, created_by)
            VALUES ($1, $2)
            RETURNING id
            "#,
        )
        .bind(unique_name("group"))
        .bind(actor_id)
        .fetch_one(pool)
        .await?)
    }

    async fn assert_group_archive_blocked(
        pool: &PgPool,
        group_id: Uuid,
        actor_id: Uuid,
    ) -> Result<()> {
        let mut archive_tx = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *archive_tx).await?;
        let error = sqlx::query(
            r#"
            UPDATE kival.groups
            SET status = 'archived',
                archived_at = now(),
                archived_by = $2
            WHERE id = $1
                AND archived_at IS NULL
            "#,
        )
        .bind(group_id)
        .bind(actor_id)
        .execute(&mut *archive_tx)
        .await
        .expect_err("group archival must wait for membership write");
        let sqlx::Error::Database(database) = error else {
            panic!("expected PostgreSQL lock timeout");
        };
        assert_eq!(database.code().as_deref(), Some("55P03"));
        archive_tx.rollback().await?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn group_membership_writes_require_active_group(pool: PgPool) -> Result<()> {
        let actor = insert_user(&pool).await?;
        let member = insert_user(&pool).await?;
        let group_id = insert_group(&pool, actor.id).await?;

        let mut tx = pool.begin().await?;
        let membership = create_group_membership(
            &mut tx,
            group_id,
            Some(member.id),
            None,
            MembershipRole::Member,
            actor.id,
        )
        .await?;
        tx.commit().await?;

        sqlx::query(
            r#"
            UPDATE kival.groups
            SET status = 'archived',
                archived_at = now(),
                archived_by = $2
            WHERE id = $1
            "#,
        )
        .bind(group_id)
        .bind(actor.id)
        .execute(&pool)
        .await?;

        let mut create_tx = pool.begin().await?;
        let error = create_group_membership(
            &mut create_tx,
            group_id,
            Some(actor.id),
            None,
            MembershipRole::Member,
            actor.id,
        )
        .await
        .expect_err("archived group must reject membership creation");
        assert!(matches!(error, KernelError::ResourceNotFound));
        create_tx.commit().await?;

        let mut revoke_tx = pool.begin().await?;
        let error = revoke_group_membership(&mut revoke_tx, group_id, membership.id, actor.id)
            .await
            .expect_err("archived group must reject membership revocation");
        assert!(matches!(error, KernelError::ResourceNotFound));
        revoke_tx.commit().await?;

        let mut replace_tx = pool.begin().await?;
        let error = replace_group_membership(
            &mut replace_tx,
            group_id,
            membership.id,
            MembershipRole::Admin,
            actor.id,
        )
        .await
        .expect_err("archived group must reject membership replacement");
        assert!(matches!(error, KernelError::ResourceNotFound));
        replace_tx.commit().await?;

        let revoked_at = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
            "SELECT revoked_at FROM kival.group_memberships WHERE id = $1",
        )
        .bind(membership.id)
        .fetch_one(&pool)
        .await?;
        assert!(revoked_at.is_none());

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn group_membership_write_pins_group_lifecycle(pool: PgPool) -> Result<()> {
        let actor = insert_user(&pool).await?;
        let member = insert_user(&pool).await?;
        let group_id = insert_group(&pool, actor.id).await?;

        let mut membership_tx = pool.begin().await?;
        create_group_membership(
            &mut membership_tx,
            group_id,
            Some(member.id),
            None,
            MembershipRole::Member,
            actor.id,
        )
        .await?;

        assert_group_archive_blocked(&pool, group_id, actor.id).await?;
        membership_tx.rollback().await?;

        let mut seed_tx = pool.begin().await?;
        let membership = create_group_membership(
            &mut seed_tx,
            group_id,
            Some(member.id),
            None,
            MembershipRole::Member,
            actor.id,
        )
        .await?;
        seed_tx.commit().await?;

        let mut revoke_tx = pool.begin().await?;
        revoke_group_membership(&mut revoke_tx, group_id, membership.id, actor.id).await?;
        assert_group_archive_blocked(&pool, group_id, actor.id).await?;
        revoke_tx.rollback().await?;

        Ok(())
    }
}
