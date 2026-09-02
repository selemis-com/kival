//! Integration tests for kernel user queries.

#[cfg(test)]
mod tests {
    use kival_kernel::{KernelError, Result, UserListStatus, create_user, fetch_user, list_users};
    use uuid::Uuid;

    fn unique_username(prefix: &str) -> String {
        format!("{prefix}-{}", short_suffix())
    }

    fn short_suffix() -> String {
        let compact = Uuid::now_v7().simple().to_string();
        compact[compact.len() - 12..].to_owned()
    }

    async fn insert_user(pool: &sqlx::PgPool, username: &str) -> Result<Uuid> {
        Ok(sqlx::query_scalar(
            r#"
            INSERT INTO kival.users (username, display_name)
            VALUES ($1, 'Username Invariant Test')
            RETURNING id
            "#,
        )
        .bind(username)
        .fetch_one(pool)
        .await?)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn protected_user_reads_authorize_in_kernel(pool: sqlx::PgPool) -> Result<()> {
        let mut tx = pool.begin().await?;
        let admin = create_user(&mut tx, "admin", "Admin").await?;
        let member = create_user(&mut tx, "member", "Member").await?;
        sqlx::query(
            r#"
            INSERT INTO kival.global_admins (user_id, created_by)
            VALUES ($1, $1)
            "#,
        )
        .bind(admin.id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let error = list_users(&pool, member.id, None, None, 10, UserListStatus::All, None)
            .await
            .expect_err("non-admin must not list users");
        assert!(matches!(error, KernelError::CapabilityRequired));
        assert_eq!(
            list_users(&pool, admin.id, None, None, 10, UserListStatus::All, None).await?.len(),
            2
        );

        let error = fetch_user(&pool, member.id, admin.id)
            .await
            .expect_err("non-admin must not fetch another user");
        assert!(matches!(error, KernelError::CapabilityRequired));
        assert_eq!(fetch_user(&pool, member.id, member.id).await?.id, member.id);
        assert_eq!(fetch_user(&pool, admin.id, member.id).await?.id, member.id);

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn usernames_are_unique(pool: sqlx::PgPool) -> Result<()> {
        let username = unique_username("kival-user");
        insert_user(&pool, &username).await?;

        let error = insert_user(&pool, &username).await.expect_err("username must be unique");
        assert!(error.to_string().contains("users_username_normalized_unique"));

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn usernames_accept_lowercase_handle_grammar(pool: sqlx::PgPool) -> Result<()> {
        for username in ["alice", "alice.smith", "team-7"] {
            insert_user(&pool, username).await?;
        }

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn usernames_reject_surrounding_whitespace(pool: sqlx::PgPool) -> Result<()> {
        let suffix = short_suffix();

        for username in [format!(" leading-{suffix}"), format!("trailing-{suffix} ")] {
            assert!(insert_user(&pool, &username).await.is_err(), "{username:?} must be rejected");
        }

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn usernames_are_limited_to_30_characters(pool: sqlx::PgPool) -> Result<()> {
        let compact_uuid = Uuid::now_v7().simple().to_string();
        assert_eq!(compact_uuid.chars().count(), 32);
        assert!(Uuid::parse_str(&compact_uuid).is_ok());

        let username = compact_uuid[..30].to_owned();
        assert_eq!(username.chars().count(), 30);

        insert_user(&pool, &username).await?;
        assert!(insert_user(&pool, &format!("{username}a")).await.is_err());
        assert!(insert_user(&pool, &compact_uuid).await.is_err());

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn usernames_reject_characters_outside_handle_grammar(pool: sqlx::PgPool) -> Result<()> {
        let suffix = short_suffix();

        for username in [
            "Alice".to_owned(),
            "ALICE".to_owned(),
            "aLice".to_owned(),
            "Alice.Smith".to_owned(),
            format!("has space-{suffix}"),
            format!("line\nbreak-{suffix}"),
            format!("kival@user-{suffix}"),
            format!("@leading-{suffix}"),
            format!("réalm-{suffix}"),
            format!(".leading-{suffix}"),
            format!("_leading-{suffix}"),
            format!("-leading-{suffix}"),
            format!("trailing-{suffix}."),
            format!("trailing-{suffix}_"),
            format!("trailing-{suffix}-"),
        ] {
            assert!(insert_user(&pool, &username).await.is_err(), "{username:?} must be rejected");
        }

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn usernames_are_immutable(pool: sqlx::PgPool) -> Result<()> {
        let mut tx = pool.begin().await?;
        let user = create_user(&mut tx, "immutable-user", "Immutable User").await?;
        tx.commit().await?;

        let error = sqlx::query("UPDATE kival.users SET username = $2 WHERE id = $1")
            .bind(user.id)
            .bind(unique_username("renamed"))
            .execute(&pool)
            .await
            .expect_err("direct username mutation must fail");
        assert!(error.to_string().contains("username is immutable"));

        let stored =
            sqlx::query_scalar::<_, String>("SELECT username FROM kival.users WHERE id = $1")
                .bind(user.id)
                .fetch_one(&pool)
                .await?;
        assert_eq!(stored, user.username);

        Ok(())
    }
}
