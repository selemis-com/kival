//! Database bootstrap helpers for Kival tests.

use kival_common::security;
use sqlx::PgPool;

use crate::TestResult;

/// Raw session material used to construct an authenticated test actor.
pub(crate) struct SessionFixture {
    /// Authenticated user ID.
    pub(crate) user_id: uuid::Uuid,
    /// Authenticated user username.
    pub(crate) username: String,
    /// Raw session token used only by the in-process test client.
    pub(crate) session_token: String,
    /// Raw CSRF token used only by the in-process test client.
    pub(crate) csrf_token: String,
}

/// Inserts a fresh global admin and an authenticated browser session.
pub(crate) async fn insert_global_admin(pool: &PgPool) -> TestResult<SessionFixture> {
    let suffix = uuid::Uuid::now_v7();
    let username = test_username("kival-test-admin", suffix);
    let mut tx = pool.begin().await?;

    let user_id = sqlx::query_scalar::<_, uuid::Uuid>(
        r#"
        INSERT INTO kival.users (username, display_name)
        VALUES ($1, $2)
        RETURNING id
        "#,
    )
    .bind(&username)
    .bind(format!("Kival Test Admin {suffix}"))
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO kival.global_admins (user_id, created_by)
        VALUES ($1, $1)
        "#,
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    insert_session(pool, user_id, username).await
}

/// Inserts a fresh user, its creation event, and an authenticated browser session.
pub(crate) async fn insert_user_session(
    pool: &PgPool,
    prefix: &str,
    created_by: uuid::Uuid,
) -> TestResult<SessionFixture> {
    let suffix = uuid::Uuid::now_v7();
    let username = test_username(prefix, suffix);
    let mut tx = pool.begin().await?;
    let user_id = sqlx::query_scalar::<_, uuid::Uuid>(
        r#"
        INSERT INTO kival.users (username, display_name)
        VALUES ($1, $2)
        RETURNING id
        "#,
    )
    .bind(&username)
    .bind(format!("Kival Test User {suffix}"))
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO kival.events (actor_user_id, event_kind, target_user_id, payload)
        VALUES ($1, 'user.created', $2, jsonb_build_object('user_id', $2))
        "#,
    )
    .bind(created_by)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    insert_session(pool, user_id, username).await
}

/// Builds a unique test username within the production 30-character limit.
fn test_username(prefix: &str, suffix: uuid::Uuid) -> String {
    const SUFFIX_LENGTH: usize = 16;
    const PREFIX_LENGTH: usize = 30 - 1 - SUFFIX_LENGTH;

    let compact_suffix = suffix.simple().to_string();
    let compact_suffix = &compact_suffix[compact_suffix.len() - SUFFIX_LENGTH..];
    let prefix = prefix.chars().take(PREFIX_LENGTH).collect::<String>();

    format!("{prefix}-{compact_suffix}")
}

/// Inserts another authenticated browser session for an existing user.
pub(crate) async fn insert_session(
    pool: &PgPool,
    user_id: uuid::Uuid,
    username: String,
) -> TestResult<SessionFixture> {
    let session_token = security::generate_secret_token()?;
    let csrf_token = security::generate_secret_token()?;

    sqlx::query(
        r#"
        INSERT INTO kival.sessions (
            user_id,
            session_token_hash,
            csrf_token_hash,
            expires_at,
            last_seen_at
        )
        VALUES ($1, $2, $3, now() + interval '30 days', now())
        "#,
    )
    .bind(user_id)
    .bind(security::hash_token(&session_token))
    .bind(security::hash_token(&csrf_token))
    .execute(pool)
    .await?;

    Ok(SessionFixture { user_id, username, session_token, csrf_token })
}
