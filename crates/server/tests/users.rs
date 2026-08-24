//! User API scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use eyre::Result;
    use kival_sdk::{ListResponse, UpdateUserRequest, User, UserResponse, UserStatus};
    use kival_tests::{TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt};
    use serde_json::Value;
    use uuid::Uuid;

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn user_json_exposes_username_without_identity_implementation_details(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("user-json-contract").await?;

        let response: Value =
            r.get_json_as(&user, &format!("/users/{}", user.id)).await?.into_success()?;
        let resource = &response["user"];

        assert_eq!(resource["username"], user.username);
        assert!(resource.get("email").is_none());
        assert!(resource.get("username_normalized").is_none());
        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn global_admin_lists_users_by_status(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let active = r.create_user("user-list-active").await?;
        let disabled = r.create_user("user-list-disabled").await?;

        let _: UserResponse = r
            .empty_json_as(&r.admin, Method::POST, &format!("/users/{}/disable", disabled.id))
            .await?
            .into_success()?;

        let active_users: ListResponse<User> = r
            .get_json_as(&r.admin, &format!("/users?q={}", active.username))
            .await?
            .into_success()?;
        assert!(active_users.items.iter().any(|user| user.id == active.id));
        assert!(active_users.items.iter().all(|user| user.id != disabled.id));
        assert!(active_users.items.iter().all(|user| user.status == UserStatus::Active));

        let disabled_users: ListResponse<User> = r
            .get_json_as(&r.admin, &format!("/users?status=disabled&q={}", disabled.username))
            .await?
            .into_success()?;
        assert!(disabled_users.items.iter().any(|user| user.id == disabled.id));
        assert!(disabled_users.items.iter().all(|user| user.status == UserStatus::Disabled));

        let all_users: ListResponse<User> =
            r.get_json_as(&r.admin, "/users?status=all&q=user-list-").await?.into_success()?;
        assert!(all_users.items.iter().any(|user| user.id == active.id));
        assert!(all_users.items.iter().any(|user| user.id == disabled.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn global_admin_searches_users_by_username_or_display_name(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let username_match = r.create_user("searchable-username").await?;
        let display_name_match = r.create_user("different-username").await?;
        let unrelated = r.create_user("unrelated-user").await?;

        let _: UserResponse = r
            .request_json_as(
                &r.admin,
                Method::PATCH,
                &format!("/users/{}", display_name_match.id),
                &UpdateUserRequest { display_name: Some("Atlas Person".to_owned()) },
            )
            .await?
            .into_success()?;

        let by_username: ListResponse<User> =
            r.get_json_as(&r.admin, "/users?q=SEARCHABLE").await?.into_success()?;
        assert!(by_username.items.iter().any(|user| user.id == username_match.id));
        assert!(!by_username.items.iter().any(|user| user.id == unrelated.id));

        let by_display_name: ListResponse<User> =
            r.get_json_as(&r.admin, "/users?q=ATLAS").await?.into_success()?;
        assert!(by_display_name.items.iter().any(|user| user.id == display_name_match.id));
        assert!(!by_display_name.items.iter().any(|user| user.id == unrelated.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn non_global_admin_cannot_list_users(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("user-list-denied").await?;

        let response = r.request(Some(&user), Method::GET, "/users", None).await?;

        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn user_can_get_self_and_global_admin_can_get_other_user(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("user-get-self").await?;

        let self_response: UserResponse =
            r.get_json_as(&user, &format!("/users/{}", user.id)).await?.into_success()?;
        assert_eq!(self_response.user.id, user.id);

        let admin_response: UserResponse =
            r.get_json_as(&r.admin, &format!("/users/{}", user.id)).await?.into_success()?;
        assert_eq!(admin_response.user.id, user.id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn user_cannot_get_other_user(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("user-get-denied").await?;
        let other = r.create_user("user-get-other").await?;

        let response =
            r.request(Some(&user), Method::GET, &format!("/users/{}", other.id), None).await?;

        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn non_global_admin_cannot_update_other_user(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("user-update-other-denied").await?;
        let other = r.create_user("user-update-other-target").await?;

        let response = r
            .request_json_raw_as(
                &user,
                Method::PATCH,
                &format!("/users/{}", other.id),
                &UpdateUserRequest { display_name: Some("Denied Update".to_owned()) },
            )
            .await?;

        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn user_can_update_self_display_name(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("user-update-self").await?;

        let updated: UserResponse = r
            .request_json_as(
                &user,
                Method::PATCH,
                &format!("/users/{}", user.id),
                &UpdateUserRequest { display_name: Some("  Updated Self  ".to_owned()) },
            )
            .await?
            .into_success()?;

        assert_eq!(updated.user.display_name, "Updated Self");

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn global_admin_can_update_other_user_display_name(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("user-update-admin").await?;

        let updated: UserResponse = r
            .request_json_as(
                &r.admin,
                Method::PATCH,
                &format!("/users/{}", user.id),
                &UpdateUserRequest { display_name: Some("  Updated By Admin  ".to_owned()) },
            )
            .await?
            .into_success()?;

        assert_eq!(updated.user.display_name, "Updated By Admin");

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn update_user_rejects_empty_patch(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("user-update-empty").await?;

        let response = r
            .request_json_raw_as(
                &user,
                Method::PATCH,
                &format!("/users/{}", user.id),
                &UpdateUserRequest { display_name: None },
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn global_admin_cannot_disable_self(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let response = r
            .request(Some(&r.admin), Method::POST, &format!("/users/{}/disable", r.admin.id), None)
            .await?;
        response.assert_status(StatusCode::CONFLICT);

        let stored: UserResponse =
            r.get_json_as(&r.admin, &format!("/users/{}", r.admin.id)).await?.into_success()?;
        assert_eq!(stored.user.status, UserStatus::Active);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn global_admin_can_enable_user_without_replacing_session_or_passkey(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("user-enable").await?;
        let credential_id = Uuid::now_v7().as_bytes().to_vec();
        let mut public_key = vec![0_u8; 65];
        public_key[0] = 4;

        sqlx::query(
            r#"
            INSERT INTO kival.passkey_credentials (user_id, credential_id, public_key, label)
            VALUES ($1, $2, $3, 'Existing passkey')
            "#,
        )
        .bind(user.id)
        .bind(&credential_id)
        .bind(public_key)
        .execute(&r.pool)
        .await?;

        let disabled: UserResponse = r
            .empty_json_as(&r.admin, Method::POST, &format!("/users/{}/disable", user.id))
            .await?
            .into_success()?;
        assert_eq!(disabled.user.status, UserStatus::Disabled);
        assert_eq!(
            r.request(Some(&user), Method::GET, "/auth/whoami", None).await?.status(),
            StatusCode::UNAUTHORIZED
        );

        let enabled: UserResponse = r
            .empty_json_as(&r.admin, Method::POST, &format!("/users/{}/enable", user.id))
            .await?
            .into_success()?;
        assert_eq!(enabled.user.status, UserStatus::Active);
        assert!(enabled.user.disabled_at.is_none());
        assert!(enabled.user.disabled_by.is_none());

        let restored: UserResponse = r.get_json_as(&user, "/auth/whoami").await?.into_success()?;
        assert_eq!(restored.user.id, user.id);

        let active_passkey_count: i64 = sqlx::query_scalar(
            r#"
            SELECT count(*)
            FROM kival.passkey_credentials
            WHERE user_id = $1
                AND credential_id = $2
                AND revoked_at IS NULL
            "#,
        )
        .bind(user.id)
        .bind(&credential_id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(active_passkey_count, 1);

        let enabled_event_count: i64 = sqlx::query_scalar(
            r#"
            SELECT count(*)
            FROM kival.events
            WHERE event_kind = 'user.enabled'
                AND actor_user_id = $1
                AND target_user_id = $2
            "#,
        )
        .bind(r.admin.id)
        .bind(user.id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(enabled_event_count, 1);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn non_global_admin_cannot_enable_user(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("user-enable-denied").await?;
        let target = r.create_user("user-enable-target").await?;

        let _: UserResponse = r
            .empty_json_as(&r.admin, Method::POST, &format!("/users/{}/disable", target.id))
            .await?
            .into_success()?;

        let response = r
            .request(Some(&user), Method::POST, &format!("/users/{}/enable", target.id), None)
            .await?;
        response.assert_status(StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn disabled_user_cannot_update_self(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let user = r.create_user("user-update-disabled").await?;

        let _: UserResponse = r
            .empty_json_as(&r.admin, Method::POST, &format!("/users/{}/disable", user.id))
            .await?
            .into_success()?;

        let response = r
            .request_json_raw_as(
                &user,
                Method::PATCH,
                &format!("/users/{}", user.id),
                &UpdateUserRequest { display_name: Some("Should Not Update".to_owned()) },
            )
            .await?;

        response.assert_status(StatusCode::UNAUTHORIZED);

        Ok(())
    }
}
