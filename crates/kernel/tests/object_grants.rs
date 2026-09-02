//! Integration tests for kernel object grant transitions.

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{DateTime, Utc};
    use kival_kernel::{
        CreateInitialObject, GrantPrincipal, KernelError, MembershipRole, ObjectRole, Result,
        create_initial_object, create_object_grant, replace_object_grant, revoke_object_grant,
    };
    use serde_json::json;
    use sqlx::PgPool;
    use uuid::Uuid;

    /// Builds a unique name for rows created by these tests.
    fn unique_name(prefix: &str) -> String {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        format!("{prefix}-{suffix}")
    }

    /// Builds a unique username that stays within the 30-character database limit.
    fn unique_username() -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch");
        let seconds = now.as_secs();
        let nanoseconds = now.subsec_nanos();
        format!("grant-{seconds}-{nanoseconds:09}")
    }

    /// Inserts an active user for grant-transition tests.
    async fn insert_user(pool: &PgPool) -> Result<Uuid> {
        sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.users (username, display_name)
            VALUES ($1, $2)
            RETURNING id
            "#,
        )
        .bind(unique_username())
        .bind("Grant Invariant User")
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    /// Inserts an active workspace and creator administrator membership for grant-transition tests.
    async fn insert_workspace(pool: &PgPool, user_id: Uuid) -> Result<Uuid> {
        let workspace_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.workspaces (name, created_by)
            VALUES ($1, $2)
            RETURNING id
            "#,
        )
        .bind(unique_name("grant-invariant-workspace"))
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        insert_workspace_member(pool, workspace_id, user_id, MembershipRole::Admin).await?;
        Ok(workspace_id)
    }

    /// Inserts an active workspace membership for grant-transition tests.
    async fn insert_workspace_member(
        pool: &PgPool,
        workspace_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO kival.workspace_memberships (
                workspace_id, user_id, workspace_role, created_by
            )
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(workspace_id)
        .bind(user_id)
        .bind(role.as_str())
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn final_admin_grant_is_preserved_before_mutation(pool: PgPool) -> Result<()> {
        let user_id = insert_user(&pool).await?;
        let workspace_id = insert_workspace(&pool, user_id).await?;

        let mut tx = pool.begin().await?;
        let created = create_initial_object(
            &mut tx,
            CreateInitialObject {
                workspace_id,
                title: "Grant invariant object".to_owned(),
                body: "Body".to_owned(),
                metadata: json!({}),
                created_by: user_id,
            },
        )
        .await?;
        tx.commit().await?;

        let mut tx = pool.begin().await?;
        let error = replace_object_grant(
            &mut tx,
            workspace_id,
            created.object_id,
            created.creator_grant_id,
            ObjectRole::Editor,
            user_id,
        )
        .await
        .expect_err("final admin grant must not be demoted");
        assert!(matches!(error, KernelError::ObjectMustRetainAdminGrant));
        tx.commit().await?;

        let mut tx = pool.begin().await?;
        let error = revoke_object_grant(
            &mut tx,
            workspace_id,
            created.object_id,
            created.creator_grant_id,
            user_id,
        )
        .await
        .expect_err("final admin grant must not be revoked");
        assert!(matches!(error, KernelError::ObjectMustRetainAdminGrant));
        tx.commit().await?;

        let (role, revoked_at) = sqlx::query_as::<_, (String, Option<DateTime<Utc>>)>(
            r#"
            SELECT object_role::text, revoked_at
            FROM kival.object_grants
            WHERE id = $1
            "#,
        )
        .bind(created.creator_grant_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(role, "admin");
        assert!(revoked_at.is_none());

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn admin_grant_can_be_demoted_when_another_admin_remains(pool: PgPool) -> Result<()> {
        let first_user_id = insert_user(&pool).await?;
        let second_user_id = insert_user(&pool).await?;
        let workspace_id = insert_workspace(&pool, first_user_id).await?;
        insert_workspace_member(&pool, workspace_id, second_user_id, MembershipRole::Member)
            .await?;

        let mut tx = pool.begin().await?;
        let created = create_initial_object(
            &mut tx,
            CreateInitialObject {
                workspace_id,
                title: "Multiple admin object".to_owned(),
                body: "Body".to_owned(),
                metadata: json!({}),
                created_by: first_user_id,
            },
        )
        .await?;
        let second_admin = create_object_grant(
            &mut tx,
            workspace_id,
            created.object_id,
            GrantPrincipal::User(second_user_id),
            ObjectRole::Admin,
            first_user_id,
        )
        .await?;
        tx.commit().await?;

        let mut tx = pool.begin().await?;
        let (_, replacement) = replace_object_grant(
            &mut tx,
            workspace_id,
            created.object_id,
            created.creator_grant_id,
            ObjectRole::Editor,
            first_user_id,
        )
        .await?;
        tx.commit().await?;

        assert_eq!(replacement.principal, GrantPrincipal::User(first_user_id));
        assert_eq!(replacement.object_role, ObjectRole::Editor);

        let active_admin_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM kival.object_grants
            WHERE workspace_id = $1
                AND object_id = $2
                AND object_role = 'admin'
                AND revoked_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(created.object_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(active_admin_count, 1);

        let second_admin_revoked_at = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
            r#"
            SELECT revoked_at
            FROM kival.object_grants
            WHERE id = $1
            "#,
        )
        .bind(second_admin.id)
        .fetch_one(&pool)
        .await?;
        assert!(second_admin_revoked_at.is_none());

        Ok(())
    }
}
