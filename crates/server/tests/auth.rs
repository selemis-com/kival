//! Authentication and session API scenario tests.

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use axum::{
        Router,
        body::Body,
        extract::ConnectInfo,
        http::{HeaderValue, Method, Request, StatusCode, header::COOKIE},
    };
    use eyre::Result;
    use kival_common::security;
    use kival_sdk::{
        API_PREFIX, MembershipRole, SessionListResponse, SessionOnlyResponse, WhoamiResponse,
    };
    use kival_tests::{TestFixtureExt, TestKival, TestResponseExt};
    use tower::ServiceExt;

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn protected_routes_require_authentication(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        for path in ["/auth/whoami", "/auth/sessions"] {
            let response = r.request(None, Method::GET, path, None).await?;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn session_can_resolve_own_identity(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let actor = r.create_user("session-identity").await?;

        let response: WhoamiResponse =
            r.get_json_as(&actor, "/auth/whoami").await?.into_success()?;

        assert_eq!(response.user.id, actor.id);
        assert!(!response.is_global_admin);
        assert!(!response.can_manage_groups);
        assert_eq!(response.scopes, None);
        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn whoami_reports_group_management_capability(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let group = r.create_group("whoami managed group").await?;
        let group_admin = r.create_user("whoami-group-admin").await?;
        r.add_user_to_group(group.id, group_admin.id, MembershipRole::Admin).await?;

        let group_identity: WhoamiResponse =
            r.get_json_as(&group_admin, "/auth/whoami").await?.into_success()?;
        assert!(!group_identity.is_global_admin);
        assert!(group_identity.can_manage_groups);

        let global_identity: WhoamiResponse =
            r.get_json_as(&r.admin, "/auth/whoami").await?.into_success()?;
        assert!(global_identity.is_global_admin);
        assert!(global_identity.can_manage_groups);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn unsafe_routes_reject_invalid_csrf_token(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let mut invalid_csrf_actor = r.admin.clone();
        invalid_csrf_actor.csrf_token = HeaderValue::from_static("invalid-csrf-token");

        let response =
            r.request(Some(&invalid_csrf_actor), Method::POST, "/auth/logout", None).await?;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let identity: WhoamiResponse =
            r.get_json_as(&r.admin, "/auth/whoami").await?.into_success()?;
        assert_eq!(identity.user.id, r.admin.id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn unsafe_routes_reject_missing_csrf_token(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let response = raw_authenticated_request_without_csrf(
            r.app.clone(),
            &r.admin,
            Method::POST,
            "/auth/logout",
        )
        .await?;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let identity: WhoamiResponse =
            r.get_json_as(&r.admin, "/auth/whoami").await?.into_success()?;
        assert_eq!(identity.user.id, r.admin.id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn logout_revokes_current_session(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let actor = r.create_user("logout").await?;

        let response = r.request(Some(&actor), Method::POST, "/auth/logout", None).await?;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = r.request(Some(&actor), Method::GET, "/auth/whoami", None).await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn user_can_list_and_revoke_another_own_session(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let first = r.create_user("sessions").await?;
        let second = r.additional_session(&first).await?;

        let second_session_id = sqlx::query_scalar::<_, uuid::Uuid>(
            r#"
            SELECT id
            FROM kival.sessions
            WHERE user_id = $1
                AND revoked_at IS NULL
            ORDER BY created_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(first.id)
        .fetch_one(&r.pool)
        .await?;
        let sessions: SessionListResponse =
            r.get_json_as(&first, "/auth/sessions").await?.into_success()?;
        assert!(sessions.items.len() >= 2);
        assert!(sessions.items.iter().all(|session| session.user_id == first.id));
        assert!(sessions.items.iter().any(|session| session.id == second_session_id));
        assert_eq!(sessions.items.iter().filter(|session| session.is_current).count(), 1);

        let revoked: SessionOnlyResponse = r
            .empty_json_as(
                &first,
                Method::POST,
                &format!("/auth/sessions/{second_session_id}/revoke"),
            )
            .await?
            .into_success()?;
        assert_eq!(revoked.session.id, second_session_id);
        assert!(revoked.session.revoked_at.is_some());
        assert_eq!(revoked.session.revoked_by, Some(first.id));

        let response = r.request(Some(&second), Method::GET, "/auth/whoami", None).await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = r.request(Some(&first), Method::GET, "/auth/whoami", None).await?;
        assert_eq!(response.status(), StatusCode::OK);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn global_admin_cannot_revoke_another_users_session_directly(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let actor = r.create_user("session-owner").await?;
        let sessions: SessionListResponse =
            r.get_json_as(&actor, "/auth/sessions").await?.into_success()?;
        let session_id = sessions.items.first().expect("session must be listed").id;

        let response = r
            .request(
                Some(&r.admin),
                Method::POST,
                &format!("/auth/sessions/{session_id}/revoke"),
                None,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = r.request(Some(&actor), Method::GET, "/auth/whoami", None).await?;
        assert_eq!(response.status(), StatusCode::OK);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn session_last_seen_updates_are_throttled(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let actor = r.create_user("session-last-seen").await?;
        let session_token = security::generate_secret_token()?;
        let csrf_token = security::generate_secret_token()?;

        let session_id = sqlx::query_scalar::<_, uuid::Uuid>(
            r#"
            INSERT INTO kival.sessions (
                user_id,
                session_token_hash,
                csrf_token_hash,
                created_at,
                expires_at,
                last_seen_at
            )
            VALUES (
                $1,
                $2,
                $3,
                now() - interval '20 minutes',
                now() + interval '30 days',
                now() - interval '10 minutes'
            )
            RETURNING id
            "#,
        )
        .bind(actor.id)
        .bind(security::hash_token(&session_token))
        .bind(security::hash_token(&csrf_token))
        .fetch_one(&r.pool)
        .await?;

        let mut stale_actor = actor.clone();
        let cookie =
            format!("__Host-kival_session={session_token}; __Host-kival_csrf={csrf_token}");
        stale_actor.cookie_header = HeaderValue::from_bytes(cookie.as_bytes())?;
        stale_actor.csrf_token = HeaderValue::from_bytes(csrf_token.as_bytes())?;

        let old = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
            "SELECT last_seen_at FROM kival.sessions WHERE id = $1",
        )
        .bind(session_id)
        .fetch_one(&r.pool)
        .await?;

        r.get_json_as::<WhoamiResponse>(&stale_actor, "/auth/whoami").await?.into_success()?;
        let first = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
            "SELECT last_seen_at FROM kival.sessions WHERE id = $1",
        )
        .bind(session_id)
        .fetch_one(&r.pool)
        .await?;
        assert!(first > old);

        r.get_json_as::<WhoamiResponse>(&stale_actor, "/auth/whoami").await?.into_success()?;
        let second = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
            "SELECT last_seen_at FROM kival.sessions WHERE id = $1",
        )
        .bind(session_id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(second, first);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn listing_sessions_prunes_old_terminal_rows(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let actor = r.create_user("session-cleanup").await?;
        let old_token = security::generate_secret_token()?;
        let old_csrf = security::generate_secret_token()?;
        let recent_token = security::generate_secret_token()?;
        let recent_csrf = security::generate_secret_token()?;

        let old_id = sqlx::query_scalar::<_, uuid::Uuid>(
            r#"
            INSERT INTO kival.sessions (
                user_id, session_token_hash, csrf_token_hash, created_at, expires_at,
                revoked_at, revoked_by, revocation_reason
            )
            VALUES (
                $1, $2, $3, now() - interval '60 days', now() - interval '40 days',
                now() - interval '31 days', $1, 'test_cleanup'
            )
            RETURNING id
            "#,
        )
        .bind(actor.id)
        .bind(security::hash_token(&old_token))
        .bind(security::hash_token(&old_csrf))
        .fetch_one(&r.pool)
        .await?;
        let recent_id = sqlx::query_scalar::<_, uuid::Uuid>(
            r#"
            INSERT INTO kival.sessions (
                user_id, session_token_hash, csrf_token_hash, created_at, expires_at,
                revoked_at, revoked_by, revocation_reason
            )
            VALUES (
                $1, $2, $3, now() - interval '10 days', now() + interval '20 days',
                now() - interval '1 day', $1, 'test_cleanup'
            )
            RETURNING id
            "#,
        )
        .bind(actor.id)
        .bind(security::hash_token(&recent_token))
        .bind(security::hash_token(&recent_csrf))
        .fetch_one(&r.pool)
        .await?;

        r.get_json_as::<SessionListResponse>(&actor, "/auth/sessions").await?.into_success()?;

        let old_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM kival.sessions WHERE id = $1)",
        )
        .bind(old_id)
        .fetch_one(&r.pool)
        .await?;
        let recent_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM kival.sessions WHERE id = $1)",
        )
        .bind(recent_id)
        .fetch_one(&r.pool)
        .await?;
        assert!(!old_exists);
        assert!(recent_exists);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn disabling_user_blocks_browser_sessions(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let actor = r.create_user("disabled-user").await?;

        let response = r
            .request(Some(&r.admin), Method::POST, &format!("/users/{}/disable", actor.id), None)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);

        let response = r.request(Some(&actor), Method::GET, "/auth/whoami", None).await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        Ok(())
    }

    async fn raw_authenticated_request_without_csrf(
        app: Router,
        actor: &kival_tests::TestActor,
        method: Method,
        path: &str,
    ) -> Result<axum::response::Response> {
        let uri = format!("{API_PREFIX}{path}");
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(COOKIE, actor.cookie_header.clone())
            .body(Body::empty())?;
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((Ipv4Addr::LOCALHOST, 31000))));

        app.oneshot(request).await.map_err(|error| eyre::eyre!("request failed: {error}"))
    }
}
