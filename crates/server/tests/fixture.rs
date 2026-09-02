//! End-to-end tests for the shared multi-actor fixture.

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use axum::http::StatusCode;
    use eyre::Result;
    use kival_sdk::WhoamiResponse;
    use kival_tests::{Actor, Fixture, TEST_RP_ID, TestKival};
    use tokio::{net::TcpListener, sync::oneshot};

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn multi_actor_fixture_authenticates_every_actor(pool: sqlx::PgPool) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let fixture_users = kival.provision_fixture_users().await?;

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let base_url = format!("http://{address}");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let app = kival.app.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
        });

        let result = async {
            let origin = format!("http://{TEST_RP_ID}:5173");
            let fixture =
                Fixture::install_for_users(&kival.pool, &base_url, &origin, &fixture_users).await?;

            assert_eq!(fixture.base_url, base_url);
            assert_eq!(fixture.actors.iter().count(), Actor::ALL.len());
            assert_eq!(fixture.identities.iter().count(), Actor::ALL.len());

            for actor in Actor::ALL {
                let client = fixture.actors.get(actor);
                let identity = fixture.identities.get(actor);

                assert_eq!(client.actor, actor);
                assert_eq!(client.user_id, identity.user_id);
                assert_eq!(identity.actor, actor);
                assert!(!identity.credential_id.is_empty());

                let response =
                    client.browser.get(format!("{base_url}/api/v1/auth/whoami")).send().await?;
                assert_eq!(response.status(), StatusCode::OK);
                let whoami = response.json::<WhoamiResponse>().await?;
                assert_eq!(whoami.user.id, identity.user_id);
                assert_eq!(whoami.user.username, identity.username);

                let active_sessions = sqlx::query_scalar::<_, i64>(
                    r#"
                    SELECT count(*)
                    FROM kival.sessions
                    WHERE user_id = $1
                        AND revoked_at IS NULL
                        AND expires_at > now()
                    "#,
                )
                .bind(identity.user_id)
                .fetch_one(&kival.pool)
                .await?;
                assert!(active_sessions > 0, "{actor:?} should have an active session");
            }

            Ok::<_, eyre::Report>(())
        }
        .await;

        let _ = shutdown_tx.send(());
        server.await??;
        result
    }
}
