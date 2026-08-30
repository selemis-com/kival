//! Browser-session state bindings.

use sqlx::{Acquire, PgPool, Postgres, Transaction};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::Result;

/// Persistent browser-session projection.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionRow {
    /// Row identifier.
    pub id: Uuid,
    /// User identifier.
    pub user_id: Uuid,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Expiration timestamp.
    pub expires_at: DateTime<Utc>,
    /// Revocation timestamp, when revoked.
    pub revoked_at: Option<DateTime<Utc>>,
    /// User that revoked the row, when retained.
    pub revoked_by: Option<Uuid>,
    /// Recorded reason for session revocation.
    pub revocation_reason: Option<String>,
    /// Last activity timestamp recorded for the session.
    pub last_seen_at: Option<DateTime<Utc>>,
    /// Captured user-agent value, when available.
    pub user_agent: Option<String>,
    /// Captured peer IP address, when available.
    pub ip_address: Option<String>,
}

/// Session identity resolved during authentication.
#[derive(Debug, Clone, Copy)]
pub struct AuthenticatedSession {
    /// Session identifier.
    pub session_id: Uuid,
    /// User identifier.
    pub user_id: Uuid,
}

/// Parameters for rotating a session after successful fresh authentication.
#[derive(Debug, Clone, Copy)]
pub struct FreshAuthenticationSessionRotation<'a> {
    /// User whose session is being rotated.
    pub user_id: Uuid,
    /// Active session replaced by the fresh session.
    pub previous_session_id: Uuid,
    /// Hash of the replacement session token.
    pub session_token_hash: &'a [u8],
    /// Hash of the replacement CSRF token.
    pub csrf_token_hash: &'a [u8],
    /// Expiration timestamp preserved from the previous session.
    pub expires_at: DateTime<Utc>,
    /// Captured user-agent value, when available.
    pub user_agent: Option<&'a str>,
    /// Captured peer IP address, when available.
    pub ip_address: Option<&'a str>,
}

/// Creates a replacement session preserving the previous session's expiration, then revokes it.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn rotate_session_after_fresh_authentication(
    tx: &mut Transaction<'_, Postgres>,
    rotation: FreshAuthenticationSessionRotation<'_>,
) -> Result<Option<Uuid>> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result =
        rotate_session_after_fresh_authentication_in_savepoint(&mut savepoint, rotation).await;

    match result {
        Ok(replacement) => {
            savepoint.commit().await?;
            Ok(replacement)
        }
        Err(error) => {
            savepoint.rollback().await?;
            Err(error)
        }
    }
}

/// Applies fresh-authentication session rotation inside a cancellation-safe savepoint.
async fn rotate_session_after_fresh_authentication_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    rotation: FreshAuthenticationSessionRotation<'_>,
) -> Result<Option<Uuid>> {
    let replacement = sqlx::query_scalar(
        r#"
        INSERT INTO kival.sessions (
            user_id, session_token_hash, csrf_token_hash, expires_at,
            fresh_authenticated_at, last_seen_at, user_agent, ip_address
        )
        SELECT $1, $2, $3, $4, now(), now(), $5, $6::inet
        WHERE $4 > clock_timestamp()
        RETURNING id
        "#,
    )
    .bind(rotation.user_id)
    .bind(rotation.session_token_hash)
    .bind(rotation.csrf_token_hash)
    .bind(rotation.expires_at)
    .bind(rotation.user_agent)
    .bind(rotation.ip_address)
    .fetch_optional(&mut **tx)
    .await?;

    if replacement.is_some() {
        sqlx::query(
            r#"
            UPDATE kival.sessions
            SET revoked_at = now(), revoked_by = user_id,
                revocation_reason = 'fresh_authentication_rotated'
            WHERE id = $1
                AND revoked_at IS NULL
            "#,
        )
        .bind(rotation.previous_session_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(replacement)
}

/// Resolves an active session and active user by token hash.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn authenticate_session(
    pool: &PgPool,
    session_token_hash: &[u8],
) -> Result<Option<AuthenticatedSession>> {
    let row = sqlx::query_as::<_, (Uuid, Uuid)>(
        r#"
        SELECT s.id, u.id
        FROM kival.sessions s
        JOIN kival.users u
            ON u.id = s.user_id
        WHERE s.session_token_hash = $1
            AND s.revoked_at IS NULL
            AND s.expires_at > now()
            AND u.disabled_at IS NULL
        "#,
    )
    .bind(session_token_hash)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(session_id, user_id)| AuthenticatedSession { session_id, user_id }))
}

/// Opportunistically updates an active session's last-seen timestamp.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn touch_session_last_seen(pool: &PgPool, session_token_hash: &[u8]) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE kival.sessions
        SET last_seen_at = now()
        WHERE session_token_hash = $1
            AND revoked_at IS NULL
            AND (last_seen_at IS NULL OR last_seen_at < now() - interval '5 minutes')
        "#,
    )
    .bind(session_token_hash)
    .execute(pool)
    .await?;
    Ok(())
}

/// Inserts a fresh browser session and returns its expiration time.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn create_session(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    session_token_hash: &[u8],
    csrf_token_hash: &[u8],
    ttl_interval: &str,
    user_agent: Option<&str>,
    ip_address: &str,
) -> Result<DateTime<Utc>> {
    Ok(sqlx::query_scalar(
        r#"
        INSERT INTO kival.sessions (
            user_id, session_token_hash, csrf_token_hash, expires_at,
            fresh_authenticated_at, user_agent, ip_address
        )
        VALUES ($1, $2, $3, now() + $4::interval, now(), $5, $6::inet)
        RETURNING expires_at
        "#,
    )
    .bind(user_id)
    .bind(session_token_hash)
    .bind(csrf_token_hash)
    .bind(ttl_interval)
    .bind(user_agent)
    .bind(ip_address)
    .fetch_one(&mut **tx)
    .await?)
}

/// Resolves the current active session ID for a user and token hash.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn current_session_id(
    pool: &PgPool,
    user_id: Uuid,
    session_token_hash: &[u8],
) -> Result<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT id
        FROM kival.sessions
        WHERE user_id = $1
            AND session_token_hash = $2
            AND revoked_at IS NULL
            AND expires_at > now()
        "#,
    )
    .bind(user_id)
    .bind(session_token_hash)
    .fetch_optional(pool)
    .await?)
}

/// Locks and checks that an active session was freshly authenticated.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_fresh_session(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    session_token_hash: &[u8],
    fresh_interval: &str,
) -> Result<Option<(Uuid, bool)>> {
    Ok(sqlx::query_as(
        r#"
        SELECT id,
               COALESCE(fresh_authenticated_at > now() - $3::interval, false) AS allowed
        FROM kival.sessions
        WHERE user_id = $1
            AND session_token_hash = $2
            AND revoked_at IS NULL
            AND expires_at > now()
        FOR UPDATE
        "#,
    )
    .bind(user_id)
    .bind(session_token_hash)
    .bind(fresh_interval)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Revokes an active session selected by token hash as an explicit logout.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn revoke_session_for_logout(
    tx: &mut Transaction<'_, Postgres>,
    session_token_hash: &[u8],
) -> Result<Option<SessionRow>> {
    Ok(sqlx::query_as(
        r#"
        UPDATE kival.sessions
        SET revoked_at = now(), revoked_by = user_id, revocation_reason = 'logout'
        WHERE session_token_hash = $1
            AND revoked_at IS NULL
        RETURNING
            id, user_id, created_at, updated_at, expires_at, revoked_at, revoked_by,
            revocation_reason, last_seen_at, user_agent, ip_address::text AS ip_address
        "#,
    )
    .bind(session_token_hash)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Revokes one active browser session owned by the given user.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn revoke_session(
    tx: &mut Transaction<'_, Postgres>,
    session_id: Uuid,
    user_id: Uuid,
) -> Result<SessionRow> {
    Ok(sqlx::query_as(
        r#"
        UPDATE kival.sessions
        SET revoked_at = now(), revoked_by = $2, revocation_reason = 'user_revoked'
        WHERE id = $1
            AND user_id = $2
            AND revoked_at IS NULL
            AND expires_at > now()
        RETURNING
            id, user_id, created_at, updated_at, expires_at, revoked_at, revoked_by,
            revocation_reason, last_seen_at, user_agent, ip_address::text AS ip_address
        "#,
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?)
}

/// Lists active browser sessions for a user.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn list_active_sessions(pool: &PgPool, user_id: Uuid) -> Result<Vec<SessionRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT
            id, user_id, created_at, updated_at, expires_at, revoked_at, revoked_by,
            revocation_reason, last_seen_at, user_agent, ip_address::text AS ip_address
        FROM kival.sessions
        WHERE user_id = $1
            AND revoked_at IS NULL
            AND expires_at > now()
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

/// Deletes a bounded batch of old terminal sessions and returns the affected row count.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn prune_terminal_sessions(pool: &PgPool, batch_size: i64) -> Result<u64> {
    let result = sqlx::query(
        r#"
        WITH terminal AS (
            SELECT id
            FROM kival.sessions
            WHERE COALESCE(revoked_at, expires_at) <= now() - interval '30 days'
            ORDER BY COALESCE(revoked_at, expires_at), id
            LIMIT $1
            FOR UPDATE SKIP LOCKED
        )
        DELETE FROM kival.sessions AS session
        USING terminal
        WHERE session.id = terminal.id
        "#,
    )
    .bind(batch_size)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Returns the stored CSRF hash for an active session, if any.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn active_session_csrf_hash(
    pool: &PgPool,
    session_token_hash: &[u8],
) -> Result<Option<Vec<u8>>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT csrf_token_hash
        FROM kival.sessions
        WHERE session_token_hash = $1
            AND revoked_at IS NULL
            AND expires_at > now()
        "#,
    )
    .bind(session_token_hash)
    .fetch_optional(pool)
    .await?)
}
