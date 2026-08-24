//! API key delegation and authorization scenario tests.

#[cfg(test)]
mod tests {
    use axum::http::{
        Method, StatusCode,
        header::{CACHE_CONTROL, SET_COOKIE},
    };
    use eyre::Result;
    use kival_sdk::{
        ApiKey, ApiKeyResponse, ApiKeyScope, CreateApiKeyRequest, CreateApiKeyResponse, Event,
        ListResponse, MembershipRole, UpdateApiKeyRequest, UserResponse, Workspace,
    };
    use kival_tests::{TestFixtureExt, TestKival, TestResponseExt};
    use serde_json::json;

    async fn mark_actor_fresh(r: &TestKival, actor: &kival_tests::TestActor) -> Result<()> {
        sqlx::query(
            "UPDATE kival.sessions SET fresh_authenticated_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(actor.id)
        .execute(&r.pool)
        .await?;
        Ok(())
    }

    async fn create_api_key(
        r: &TestKival,
        actor: &kival_tests::TestActor,
        scopes: Vec<ApiKeyScope>,
        workspace_ids: Vec<uuid::Uuid>,
    ) -> Result<CreateApiKeyResponse> {
        mark_actor_fresh(r, actor).await?;
        r.request_json_as(
            actor,
            Method::POST,
            "/auth/api-keys",
            &CreateApiKeyRequest {
                label: "test-agent".to_owned(),
                scopes,
                workspace_ids,
                expires_at: None,
            },
        )
        .await?
        .into_success()
    }

    fn assert_clears_auth_cookies(response: &axum::response::Response) {
        let cookies = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>();
        for cookie in &cookies {
            if cookie.starts_with("__Host-") {
                assert!(cookie.contains("; Path=/"));
                assert!(cookie.contains("; Secure"));
                assert!(!cookie.contains("Domain="));
            }
        }

        assert!(cookies.iter().any(|cookie| {
            cookie.starts_with("__Host-kival_session=;") && cookie.contains("Max-Age=0")
        }));
        assert!(cookies.iter().any(|cookie| {
            cookie.starts_with("__Host-kival_csrf=;") && cookie.contains("Max-Age=0")
        }));
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_creation_requires_fresh_authentication(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-fresh-auth").await?;
        let request = CreateApiKeyRequest {
            label: "fresh-auth-required".to_owned(),
            scopes: vec![ApiKeyScope::WorkspaceRead],
            workspace_ids: vec![workspace.id],
            expires_at: None,
        };

        let stale =
            r.request_json_raw_as(&r.admin, Method::POST, "/auth/api-keys", &request).await?;
        assert_eq!(stale.status(), StatusCode::FORBIDDEN);

        mark_actor_fresh(&r, &r.admin).await?;
        let fresh =
            r.request_json_raw_as(&r.admin, Method::POST, "/auth/api-keys", &request).await?;
        assert_eq!(fresh.status(), StatusCode::OK);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_creation_response_is_not_cacheable(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-no-store").await?;
        mark_actor_fresh(&r, &r.admin).await?;
        let response = r
            .request_json_raw_as(
                &r.admin,
                Method::POST,
                "/auth/api-keys",
                &CreateApiKeyRequest {
                    label: "no-store".to_owned(),
                    scopes: vec![ApiKeyScope::WorkspaceRead],
                    workspace_ids: vec![workspace.id],
                    expires_at: None,
                },
            )
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CACHE_CONTROL)
                .expect("API key creation should set cache-control")
                .to_str()?,
            "private, no-store"
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn newly_issued_api_keys_use_the_kival_prefix(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let created = create_api_key(&r, &r.admin, vec![ApiKeyScope::Admin], vec![]).await?;

        assert!(created.token.starts_with("kvl_"));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_can_resolve_own_identity_without_an_identity_scope(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-identity").await?;
        let actor = r
            .create_workspace_actor(workspace.id, "api-key-identity-user", MembershipRole::Member)
            .await?;
        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::ObjectRead], vec![workspace.id]).await?;

        let response: UserResponse = r
            .request_json_with_api_key(&created.token, Method::GET, "/auth/whoami", None)
            .await?
            .into_success()?;

        assert_eq!(response.user.id, actor.id);
        assert_eq!(response.user.username, actor.username);
        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_is_limited_to_explicit_workspaces(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let public = r.create_workspace("api-key-public").await?;
        let protected = r.create_workspace("api-key-protected").await?;
        let actor = r
            .create_workspace_actor(public.id, "api-key-workspace-scope", MembershipRole::Member)
            .await?;
        r.add_user_to_workspace(protected.id, actor.id, MembershipRole::Member).await?;

        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::WorkspaceRead], vec![public.id]).await?;

        assert_eq!(
            r.request_with_api_key(
                &created.token,
                Method::GET,
                &format!("/workspaces/{}", public.id),
                None,
            )
            .await?
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            r.request_with_api_key(
                &created.token,
                Method::GET,
                &format!("/workspaces/{}", protected.id),
                None,
            )
            .await?
            .status(),
            StatusCode::FORBIDDEN
        );

        let listed: ListResponse<Workspace> = r
            .request_json_with_api_key(&created.token, Method::GET, "/workspaces", None)
            .await?
            .into_success()?;
        assert!(listed.items.iter().any(|workspace| workspace.id == public.id));
        assert!(!listed.items.iter().any(|workspace| workspace.id == protected.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn owner_can_update_api_key_authorization_without_rotating_the_token(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let original = r.create_workspace("api-key-update-original").await?;
        let added = r.create_workspace("api-key-update-added").await?;
        let actor = r
            .create_workspace_actor(original.id, "key-update-user", MembershipRole::Member)
            .await?;
        r.add_user_to_workspace(added.id, actor.id, MembershipRole::Member).await?;
        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::WorkspaceRead], vec![original.id]).await?;

        mark_actor_fresh(&r, &actor).await?;
        let updated: ApiKeyResponse = r
            .request_json_as(
                &actor,
                Method::PATCH,
                &format!("/auth/api-keys/{}", created.api_key.id),
                &UpdateApiKeyRequest {
                    authorization_revision: created.api_key.authorization_revision,
                    scopes: vec![ApiKeyScope::WorkspaceRead, ApiKeyScope::ObjectRead],
                    workspace_ids: vec![added.id],
                },
            )
            .await?
            .into_success()?;

        assert_eq!(updated.api_key.id, created.api_key.id);
        assert_eq!(
            updated.api_key.authorization_revision,
            created.api_key.authorization_revision + 1
        );
        assert!(updated.api_key.scopes.contains(&ApiKeyScope::ObjectRead));
        assert_eq!(updated.api_key.workspace_ids, vec![added.id]);
        assert_eq!(
            r.request_with_api_key(
                &created.token,
                Method::GET,
                &format!("/workspaces/{}", original.id),
                None,
            )
            .await?
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            r.request_with_api_key(
                &created.token,
                Method::GET,
                &format!("/workspaces/{}", added.id),
                None,
            )
            .await?
            .status(),
            StatusCode::OK
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_authorization_update_rejects_stale_revision(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let first = r.create_workspace("key-revision-first").await?;
        let second = r.create_workspace("key-revision-second").await?;
        let created =
            create_api_key(&r, &r.admin, vec![ApiKeyScope::WorkspaceRead], vec![first.id]).await?;

        mark_actor_fresh(&r, &r.admin).await?;
        let first_update: ApiKeyResponse = r
            .request_json_as(
                &r.admin,
                Method::PATCH,
                &format!("/auth/api-keys/{}", created.api_key.id),
                &UpdateApiKeyRequest {
                    authorization_revision: created.api_key.authorization_revision,
                    scopes: vec![ApiKeyScope::WorkspaceRead, ApiKeyScope::ObjectRead],
                    workspace_ids: vec![second.id],
                },
            )
            .await?
            .into_success()?;

        let stale = r
            .request_json_raw_as(
                &r.admin,
                Method::PATCH,
                &format!("/auth/api-keys/{}", created.api_key.id),
                &UpdateApiKeyRequest {
                    authorization_revision: created.api_key.authorization_revision,
                    scopes: vec![ApiKeyScope::WorkspaceRead],
                    workspace_ids: vec![first.id],
                },
            )
            .await?;
        assert_eq!(stale.status(), StatusCode::CONFLICT);

        let listed: ListResponse<ApiKey> = r
            .request_json_as(&r.admin, Method::GET, "/auth/api-keys", &())
            .await?
            .into_success()?;
        let persisted = listed
            .items
            .iter()
            .find(|api_key| api_key.id == created.api_key.id)
            .expect("updated API key should remain listed");
        assert_eq!(persisted.authorization_revision, first_update.api_key.authorization_revision);
        assert!(persisted.scopes.contains(&ApiKeyScope::ObjectRead));
        assert_eq!(persisted.workspace_ids, vec![second.id]);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_authorization_update_requires_fresh_authentication(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-update-fresh").await?;
        let created =
            create_api_key(&r, &r.admin, vec![ApiKeyScope::WorkspaceRead], vec![workspace.id])
                .await?;
        sqlx::query(
            "UPDATE kival.sessions SET fresh_authenticated_at = NULL WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(r.admin.id)
        .execute(&r.pool)
        .await?;
        let request = UpdateApiKeyRequest {
            authorization_revision: created.api_key.authorization_revision,
            scopes: vec![ApiKeyScope::WorkspaceRead, ApiKeyScope::ObjectRead],
            workspace_ids: vec![workspace.id],
        };

        let stale = r
            .request_json_raw_as(
                &r.admin,
                Method::PATCH,
                &format!("/auth/api-keys/{}", created.api_key.id),
                &request,
            )
            .await?;
        assert_eq!(stale.status(), StatusCode::FORBIDDEN);

        mark_actor_fresh(&r, &r.admin).await?;
        let fresh = r
            .request_json_raw_as(
                &r.admin,
                Method::PATCH,
                &format!("/auth/api-keys/{}", created.api_key.id),
                &request,
            )
            .await?;
        assert_eq!(fresh.status(), StatusCode::OK);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn malformed_bearer_never_falls_back_to_session(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let response = r
            .request_with_authorization_as(
                &r.admin,
                "Bearer",
                Method::GET,
                &format!("/users/{}", r.admin.id),
                None,
            )
            .await?;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn malformed_workspace_id_returns_bad_request_for_api_keys(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-malformed-workspace-id").await?;
        let actor = r
            .create_workspace_actor(workspace.id, "key-bad-workspace-user", MembershipRole::Member)
            .await?;
        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::WorkspaceRead], vec![workspace.id])
                .await?;

        let response = r
            .request_with_api_key(&created.token, Method::GET, "/workspaces/not-a-uuid", None)
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_requires_the_route_scope(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-scope").await?;
        let actor = r
            .create_workspace_actor(workspace.id, "api-key-scope-user", MembershipRole::Member)
            .await?;
        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::WorkspaceRead], vec![workspace.id])
                .await?;

        let response = r
            .request_with_api_key(
                &created.token,
                Method::GET,
                &format!("/workspaces/{}/objects", workspace.id),
                None,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn realtime_route_requires_its_dedicated_scope(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-realtime-scope").await?;
        let actor = r
            .create_workspace_actor(
                workspace.id,
                "api-key-realtime-scope-user",
                MembershipRole::Member,
            )
            .await?;
        let without_realtime =
            create_api_key(&r, &actor, vec![ApiKeyScope::ObjectRead], vec![workspace.id]).await?;

        let denied =
            r.request_with_api_key(&without_realtime.token, Method::GET, "/realtime", None).await?;
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);

        let with_realtime = create_api_key(
            &r,
            &actor,
            vec![ApiKeyScope::ObjectRead, ApiKeyScope::RealtimeRead],
            vec![workspace.id],
        )
        .await?;
        let handshake_required =
            r.request_with_api_key(&with_realtime.token, Method::GET, "/realtime", None).await?;
        assert_eq!(handshake_required.status(), StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn write_scope_includes_the_corresponding_read_scope(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-write-implies-read").await?;
        let actor = r
            .create_workspace_actor(workspace.id, "key-write-read-user", MembershipRole::Member)
            .await?;
        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::ObjectWrite], vec![workspace.id]).await?;

        let object: serde_json::Value = r
            .request_json_with_api_key(
                &created.token,
                Method::POST,
                &format!("/workspaces/{}/objects", workspace.id),
                Some(json!({
                    "title": "write scope object",
                    "body": "readable through the implied read scope",
                    "metadata": {}
                })),
            )
            .await?
            .into_success()?;
        let object_id =
            object["object"]["id"].as_str().expect("created object should contain an ID");

        let response = r
            .request_with_api_key(
                &created.token,
                Method::GET,
                &format!("/workspaces/{}/objects/{object_id}", workspace.id),
                None,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn admin_scope_is_not_a_wildcard_for_other_scopes(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-admin-scope").await?;
        let created =
            create_api_key(&r, &r.admin, vec![ApiKeyScope::Admin], vec![workspace.id]).await?;

        assert_eq!(
            r.request_with_api_key(
                &created.token,
                Method::GET,
                &format!("/workspaces/{}", workspace.id),
                None,
            )
            .await?
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            r.request_with_api_key(&created.token, Method::GET, "/events?limit=1", None)
                .await?
                .status(),
            StatusCode::OK
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_scope_cannot_exceed_owning_user_authority(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-authority-ceiling").await?;
        let actor = r
            .create_workspace_actor(workspace.id, "api-key-member", MembershipRole::Member)
            .await?;
        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::WorkspaceWrite], vec![workspace.id])
                .await?;

        let response = r
            .request_with_api_key(
                &created.token,
                Method::PATCH,
                &format!("/workspaces/{}", workspace.id),
                Some(json!({ "name": "not allowed" })),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn removing_workspace_membership_immediately_revokes_api_key_access(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-membership-revocation").await?;
        let actor = r
            .create_workspace_actor(workspace.id, "key-membership-user", MembershipRole::Member)
            .await?;
        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::WorkspaceRead], vec![workspace.id])
                .await?;

        assert_eq!(
            r.request_with_api_key(
                &created.token,
                Method::GET,
                &format!("/workspaces/{}", workspace.id),
                None,
            )
            .await?
            .status(),
            StatusCode::OK
        );

        let membership_id = sqlx::query_scalar::<_, uuid::Uuid>(
            r#"
            SELECT id
            FROM kival.workspace_memberships
            WHERE workspace_id = $1
                AND user_id = $2
                AND revoked_at IS NULL
            "#,
        )
        .bind(workspace.id)
        .bind(actor.id)
        .fetch_one(&r.pool)
        .await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::POST,
                &format!("/workspaces/{}/memberships/{membership_id}/revoke", workspace.id),
                None,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);

        assert_eq!(
            r.request_with_api_key(
                &created.token,
                Method::GET,
                &format!("/workspaces/{}", workspace.id),
                None,
            )
            .await?
            .status(),
            StatusCode::FORBIDDEN
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn bearer_api_key_can_mutate_without_csrf_when_scoped_and_authorized(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-no-csrf").await?;
        let created =
            create_api_key(&r, &r.admin, vec![ApiKeyScope::WorkspaceWrite], vec![workspace.id])
                .await?;

        let response = r
            .request_with_api_key(
                &created.token,
                Method::PATCH,
                &format!("/workspaces/{}", workspace.id),
                Some(json!({ "name": "renamed by agent" })),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);

        let events: ListResponse<Event> = r
            .get_json_as(
                &r.admin,
                &format!("/workspaces/{}/events?event_kind=workspace.updated", workspace.id),
            )
            .await?
            .into_success()?;
        let event = events
            .items
            .iter()
            .find(|event| event.api_key_id == Some(created.api_key.id))
            .expect("API-key-authenticated mutation should be audited with key ID");
        assert_eq!(event.actor_user_id, Some(r.admin.id));
        assert_eq!(event.api_key_label.as_deref(), Some("test-agent"));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn workspace_creation_requires_an_interactive_session(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-workspace-creation").await?;
        let created =
            create_api_key(&r, &r.admin, vec![ApiKeyScope::WorkspaceWrite], vec![workspace.id])
                .await?;

        let response = r
            .request_with_api_key(
                &created.token,
                Method::POST,
                "/workspaces",
                Some(json!({ "name": "created by api key" })),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_keys_cannot_manage_sessions_or_api_keys(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-session-only").await?;
        let actor = r
            .create_workspace_actor(
                workspace.id,
                "api-key-session-only-user",
                MembershipRole::Member,
            )
            .await?;
        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::WorkspaceRead], vec![workspace.id])
                .await?;

        for path in ["/auth/sessions", "/auth/api-keys"] {
            assert_eq!(
                r.request_with_api_key(&created.token, Method::GET, path, None).await?.status(),
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(
            r.request_with_api_key(&created.token, Method::POST, "/auth/logout", None)
                .await?
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            r.request_with_api_key(
                &created.token,
                Method::PATCH,
                &format!("/auth/api-keys/{}", created.api_key.id),
                Some(json!({
                    "authorization_revision": created.api_key.authorization_revision,
                    "scopes": ["workspaces:read"],
                    "workspace_ids": [workspace.id],
                })),
            )
            .await?
            .status(),
            StatusCode::FORBIDDEN
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn logout_is_idempotent_for_missing_and_expired_sessions(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let response = r.request(None, Method::POST, "/auth/logout", None).await?;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_clears_auth_cookies(&response);

        sqlx::query(
            "UPDATE kival.sessions SET expires_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(r.admin.id)
        .execute(&r.pool)
        .await?;

        let response = r.request(Some(&r.admin), Method::POST, "/auth/logout", None).await?;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_clears_auth_cookies(&response);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn revoked_api_key_stops_working_immediately(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-revocation").await?;
        let actor = r
            .create_workspace_actor(workspace.id, "api-key-revocation-user", MembershipRole::Member)
            .await?;
        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::WorkspaceRead], vec![workspace.id])
                .await?;

        assert_eq!(
            r.request_with_api_key(&created.token, Method::GET, "/workspaces", None)
                .await?
                .status(),
            StatusCode::OK
        );

        let revoked: ApiKeyResponse = r
            .empty_json_as(
                &actor,
                Method::POST,
                &format!("/auth/api-keys/{}/revoke", created.api_key.id),
            )
            .await?
            .into_success()?;
        assert!(revoked.api_key.revoked_at.is_some());

        assert_eq!(
            r.request_with_api_key(&created.token, Method::GET, "/workspaces", None)
                .await?
                .status(),
            StatusCode::UNAUTHORIZED
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn expired_and_malformed_api_keys_are_rejected(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-expiry").await?;
        let actor = r
            .create_workspace_actor(workspace.id, "api-key-expiry-user", MembershipRole::Member)
            .await?;
        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::WorkspaceRead], vec![workspace.id])
                .await?;

        sqlx::query("UPDATE kival.api_keys SET expires_at = now() WHERE id = $1")
            .bind(created.api_key.id)
            .execute(&r.pool)
            .await?;

        assert_eq!(
            r.request_with_api_key(&created.token, Method::GET, "/workspaces", None)
                .await?
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            r.request_with_api_key("kvl_not-a-valid-key", Method::GET, "/workspaces", None)
                .await?
                .status(),
            StatusCode::UNAUTHORIZED
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn disabling_the_owner_blocks_api_keys_until_the_owner_is_enabled(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-disabled-owner").await?;
        let actor = r
            .create_workspace_actor(
                workspace.id,
                "api-key-disabled-owner-user",
                MembershipRole::Member,
            )
            .await?;
        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::WorkspaceRead], vec![workspace.id])
                .await?;

        let response = r
            .request(Some(&r.admin), Method::POST, &format!("/users/{}/disable", actor.id), None)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);

        assert_eq!(
            r.request_with_api_key(&created.token, Method::GET, "/workspaces", None)
                .await?
                .status(),
            StatusCode::UNAUTHORIZED
        );

        let response = r
            .request(Some(&r.admin), Method::POST, &format!("/users/{}/enable", actor.id), None)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);

        assert_eq!(
            r.request_with_api_key(&created.token, Method::GET, "/workspaces", None)
                .await?
                .status(),
            StatusCode::OK
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_creation_requires_label_and_rejects_legacy_name(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-required-label").await?;
        mark_actor_fresh(&r, &r.admin).await?;

        let response = r
            .request_json_raw_as(
                &r.admin,
                Method::POST,
                "/auth/api-keys",
                &json!({
                    "scopes": ["workspaces:read"],
                    "workspace_ids": [workspace.id],
                    "expires_at": null
                }),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = r
            .request_json_raw_as(
                &r.admin,
                Method::POST,
                "/auth/api-keys",
                &json!({
                    "name": "legacy name",
                    "label": "legacy-name",
                    "scopes": ["workspaces:read"],
                    "workspace_ids": [workspace.id],
                    "expires_at": null
                }),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = r
            .request_json_raw_as(
                &r.admin,
                Method::POST,
                "/auth/api-keys",
                &CreateApiKeyRequest {
                    label: "  ".to_owned(),
                    scopes: vec![ApiKeyScope::WorkspaceRead],
                    workspace_ids: vec![workspace.id],
                    expires_at: None,
                },
            )
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_label_round_trips_in_management_responses(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-label-round-trip").await?;
        mark_actor_fresh(&r, &r.admin).await?;
        let created: CreateApiKeyResponse = r
            .request_json_as(
                &r.admin,
                Method::POST,
                "/auth/api-keys",
                &CreateApiKeyRequest {
                    label: "nightly-ingest".to_owned(),
                    scopes: vec![ApiKeyScope::WorkspaceRead],
                    workspace_ids: vec![workspace.id],
                    expires_at: None,
                },
            )
            .await?
            .into_success()?;

        assert_eq!(created.api_key.label, "nightly-ingest");

        let listed: ListResponse<ApiKey> =
            r.get_json_as(&r.admin, "/auth/api-keys").await?.into_success()?;
        assert!(listed.items.iter().any(|api_key| {
            api_key.id == created.api_key.id && api_key.label == "nightly-ingest"
        }));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_creation_accepts_all_declared_scopes(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let created = create_api_key(&r, &r.admin, ApiKeyScope::ALL.to_vec(), vec![]).await?;

        let mut expected = ApiKeyScope::ALL.to_vec();
        expected.sort_unstable_by_key(|scope| scope.as_str());
        assert_eq!(created.api_key.scopes, expected);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn bearer_api_key_takes_precedence_over_session_cookie(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-precedence").await?;
        let actor = r
            .create_workspace_actor(workspace.id, "api-key-precedence-user", MembershipRole::Member)
            .await?;
        let created =
            create_api_key(&r, &actor, vec![ApiKeyScope::WorkspaceRead], vec![workspace.id])
                .await?;

        let response = r
            .request_with_authorization_as(
                &r.admin,
                &format!("Bearer {}", created.token),
                Method::GET,
                "/events?limit=1",
                None,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn malformed_bearer_variants_never_fall_back_to_session(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        for authorization in ["Bearer", "Bearer ", "Bearer\ttoken", "Bearer token with-space"] {
            let response = r
                .request_with_authorization_as(
                    &r.admin,
                    authorization,
                    Method::GET,
                    &format!("/users/{}", r.admin.id),
                    None,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{authorization}");
        }

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn admin_scope_still_requires_owner_global_admin_authority(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-admin-owner-ceiling").await?;
        let actor = r
            .create_workspace_actor(workspace.id, "key-admin-ceiling-user", MembershipRole::Member)
            .await?;
        let created = create_api_key(&r, &actor, vec![ApiKeyScope::Admin], vec![]).await?;

        let response =
            r.request_with_api_key(&created.token, Method::GET, "/events?limit=1", None).await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_audit_attribution_preserves_owner_identity_and_key_label(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("api-key-audit-attribution").await?;
        let actor = r
            .create_workspace_actor(
                workspace.id,
                "api-key-audit-attribution-user",
                MembershipRole::Admin,
            )
            .await?;
        mark_actor_fresh(&r, &actor).await?;
        let created: CreateApiKeyResponse = r
            .request_json_as(
                &actor,
                Method::POST,
                "/auth/api-keys",
                &CreateApiKeyRequest {
                    label: "build-agent-7".to_owned(),
                    scopes: vec![ApiKeyScope::ObjectWrite, ApiKeyScope::EventRead],
                    workspace_ids: vec![workspace.id],
                    expires_at: None,
                },
            )
            .await?
            .into_success()?;

        let object: serde_json::Value = r
            .request_json_with_api_key(
                &created.token,
                Method::POST,
                &format!("/workspaces/{}/objects", workspace.id),
                Some(json!({
                    "title": "audit attribution",
                    "body": "created through an API key",
                    "metadata": {}
                })),
            )
            .await?
            .into_success()?;
        let object_id =
            object["object"]["id"].as_str().expect("created object should contain an ID");

        let events: ListResponse<Event> = r
            .request_json_with_api_key(
                &created.token,
                Method::GET,
                &format!("/workspaces/{}/events?event_kind=object.created", workspace.id),
                None,
            )
            .await?
            .into_success()?;
        let event = events
            .items
            .iter()
            .find(|event| event.object_id.map(|id| id.to_string()) == Some(object_id.to_owned()))
            .expect("object creation should be audited");

        assert_eq!(event.actor_user_id, Some(actor.id));
        assert_eq!(event.api_key_id, Some(created.api_key.id));
        assert_eq!(event.api_key_label.as_deref(), Some("build-agent-7"));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn api_key_creation_rejects_undelegatable_workspaces(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let accessible = r.create_workspace("api-key-accessible").await?;
        let inaccessible = r.create_workspace("api-key-inaccessible").await?;
        let actor = r
            .create_workspace_actor(
                accessible.id,
                "api-key-delegation-user",
                MembershipRole::Member,
            )
            .await?;
        mark_actor_fresh(&r, &actor).await?;

        let response = r
            .request_json_raw_as(
                &actor,
                Method::POST,
                "/auth/api-keys",
                &CreateApiKeyRequest {
                    label: "invalid-delegation".to_owned(),
                    scopes: vec![ApiKeyScope::WorkspaceRead],
                    workspace_ids: vec![inaccessible.id],
                    expires_at: None,
                },
            )
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        Ok(())
    }
}
