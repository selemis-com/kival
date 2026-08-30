//! Passkey credential, ceremony, and enrollment state bindings.

use std::{fmt, str::FromStr};

use sqlx::{PgPool, Postgres, Transaction};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{KernelError, Result, parse_stored};

/// Purpose bound to an operator-issued passkey enrollment capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasskeyEnrollmentPurpose {
    /// Initial passkey enrollment for a user without an active credential.
    Enrollment,
    /// Passkey recovery after credentials have been lost or removed.
    PasskeyReset,
}

impl PasskeyEnrollmentPurpose {
    /// Returns the authoritative persisted representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enrollment => "enrollment",
            Self::PasskeyReset => "passkey_reset",
        }
    }
}

impl FromStr for PasskeyEnrollmentPurpose {
    type Err = ();

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "enrollment" => Ok(Self::Enrollment),
            "passkey_reset" => Ok(Self::PasskeyReset),
            _ => Err(()),
        }
    }
}

impl fmt::Display for PasskeyEnrollmentPurpose {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Stored passkey credential projection.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PasskeyRow {
    /// Row identifier.
    pub id: Uuid,
    /// User identifier.
    pub user_id: Uuid,
    /// Human-readable credential label.
    pub label: Option<String>,
    /// `WebAuthn` credential identifier bytes.
    pub credential_id: Vec<u8>,
    /// Stored credential public-key bytes.
    pub public_key: Vec<u8>,
    /// Last accepted authenticator signature counter.
    pub signature_count: i64,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Last successful credential-use timestamp.
    pub last_used_at: Option<DateTime<Utc>>,
    /// Revocation timestamp, when revoked.
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Enrollment-code identity locked while starting registration.
#[derive(Debug, Clone)]
pub struct EnrollmentIdentity {
    /// Enrollment-code identifier.
    pub code_id: Uuid,
    /// User identifier.
    pub user_id: Uuid,
    /// Enrollment or ceremony purpose.
    pub purpose: PasskeyEnrollmentPurpose,
    /// Username associated with the user.
    pub username: String,
    /// Display name associated with the user.
    pub display_name: String,
}

/// State bound to a fresh-authentication ceremony and its current session.
#[derive(Debug, Clone)]
pub struct FreshAuthenticationCeremony {
    /// Persisted `WebAuthn` challenge bytes.
    pub challenge: Vec<u8>,
    /// Expiration timestamp of the session being freshly authenticated.
    pub session_expires_at: DateTime<Utc>,
    /// Captured user-agent value, when available.
    pub user_agent: Option<String>,
    /// Captured peer IP address, when available.
    pub ip_address: Option<String>,
}

/// Locked state required to complete an enrollment-code ceremony.
#[derive(Debug, Clone)]
pub struct EnrollmentCompletion {
    /// Enrollment-code identifier.
    pub code_id: Uuid,
    /// Persisted `WebAuthn` challenge bytes.
    pub challenge: Vec<u8>,
    /// Enrollment or ceremony purpose.
    pub purpose: PasskeyEnrollmentPurpose,
}

/// Returns whether a user owns at least one active passkey.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn has_active_passkey(pool: &PgPool, user_id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM kival.passkey_credentials
            WHERE user_id = $1
                AND revoked_at IS NULL
        )
        "#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?)
}

/// Returns whether a user owns at least one active passkey inside a transaction.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn has_active_passkey_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM kival.passkey_credentials
            WHERE user_id = $1
                AND revoked_at IS NULL
        )
        "#,
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?)
}

/// Creates a username-first login ceremony with a caller-generated public identifier.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn create_authentication_ceremony(
    pool: &PgPool,
    ceremony_id: Uuid,
    user_id: Uuid,
    challenge: &[u8],
    ttl_interval: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO kival.webauthn_ceremonies (id, user_id, kind, challenge, expires_at)
        VALUES ($1, $2, 'authentication', $3, now() + $4::interval)
        "#,
    )
    .bind(ceremony_id)
    .bind(user_id)
    .bind(challenge)
    .bind(ttl_interval)
    .execute(pool)
    .await?;
    Ok(())
}

/// Resolves the user binding of a currently usable username-first login ceremony.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn login_ceremony_user_id(
    tx: &mut Transaction<'_, Postgres>,
    ceremony_id: Uuid,
) -> Result<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT user_id
        FROM kival.webauthn_ceremonies
        WHERE id = $1
            AND kind = 'authentication'
            AND session_id IS NULL
            AND consumed_at IS NULL
            AND expires_at > now()
        "#,
    )
    .bind(ceremony_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Locks and revalidates a username-first login ceremony.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_login_ceremony(
    tx: &mut Transaction<'_, Postgres>,
    ceremony_id: Uuid,
    user_id: Uuid,
) -> Result<Option<Vec<u8>>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT challenge
        FROM kival.webauthn_ceremonies
        WHERE id = $1
            AND user_id = $2
            AND kind = 'authentication'
            AND session_id IS NULL
            AND consumed_at IS NULL
            AND expires_at > now()
        FOR UPDATE
        "#,
    )
    .bind(ceremony_id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Creates a fresh-authentication ceremony bound to an active session.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn create_fresh_authentication_ceremony(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    challenge: &[u8],
    ttl_interval: &str,
) -> Result<Uuid> {
    Ok(sqlx::query_scalar(
        r#"
        INSERT INTO kival.webauthn_ceremonies (user_id, session_id, kind, challenge, expires_at)
        VALUES ($1, $2, 'fresh_authentication', $3, now() + $4::interval)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(session_id)
    .bind(challenge)
    .bind(ttl_interval)
    .fetch_one(pool)
    .await?)
}

/// Locks a usable fresh-authentication ceremony together with its active session.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_fresh_authentication_ceremony(
    tx: &mut Transaction<'_, Postgres>,
    ceremony_id: Uuid,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<Option<FreshAuthenticationCeremony>> {
    let row = sqlx::query_as::<_, (Vec<u8>, DateTime<Utc>, Option<String>, Option<String>)>(
        r#"
        SELECT c.challenge, s.expires_at, s.user_agent, s.ip_address::text
        FROM kival.webauthn_ceremonies c
        JOIN kival.sessions s
            ON s.id = c.session_id
            AND s.user_id = c.user_id
        WHERE c.id = $1
            AND c.user_id = $2
            AND c.session_id = $3
            AND c.kind = 'fresh_authentication'
            AND c.consumed_at IS NULL
            AND c.expires_at > now()
            AND s.revoked_at IS NULL
            AND s.expires_at > now()
        FOR UPDATE OF c, s
        "#,
    )
    .bind(ceremony_id)
    .bind(user_id)
    .bind(session_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|(challenge, session_expires_at, user_agent, ip_address)| {
        FreshAuthenticationCeremony { challenge, session_expires_at, user_agent, ip_address }
    }))
}

/// Marks a passkey as successfully used and advances its signature counter.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn record_passkey_use(
    tx: &mut Transaction<'_, Postgres>,
    passkey_id: Uuid,
    signature_count: i64,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE kival.passkey_credentials
        SET signature_count = $2, last_used_at = now()
        WHERE id = $1
            AND revoked_at IS NULL
        "#,
    )
    .bind(passkey_id)
    .bind(signature_count)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Consumes one still-unconsumed `WebAuthn` ceremony.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn consume_ceremony(tx: &mut Transaction<'_, Postgres>, ceremony_id: Uuid) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE kival.webauthn_ceremonies
        SET consumed_at = now()
        WHERE id = $1
            AND consumed_at IS NULL
        "#,
    )
    .bind(ceremony_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Loads active registration identity and credential IDs for a signed-in user.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn registration_identity(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<(String, String, Vec<Vec<u8>>)>> {
    let identity = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT username, display_name
        FROM kival.users
        WHERE id = $1
            AND disabled_at IS NULL
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let Some((username, display_name)) = identity else {
        return Ok(None);
    };
    let excluded = sqlx::query_scalar(
        r#"
        SELECT credential_id
        FROM kival.passkey_credentials
        WHERE user_id = $1
            AND revoked_at IS NULL ORDER BY created_at, id
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(Some((username, display_name, excluded)))
}

/// Creates a signed-in passkey registration ceremony.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn create_registration_ceremony(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    challenge: &[u8],
    ttl_interval: &str,
) -> Result<Uuid> {
    Ok(sqlx::query_scalar(
        r#"
        INSERT INTO kival.webauthn_ceremonies (user_id, session_id, kind, challenge, expires_at)
        VALUES ($1, $2, 'registration', $3, now() + $4::interval)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(session_id)
    .bind(challenge)
    .bind(ttl_interval)
    .fetch_one(pool)
    .await?)
}

/// Locks a signed-in registration ceremony and its active browser session.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_registration_ceremony(
    tx: &mut Transaction<'_, Postgres>,
    ceremony_id: Uuid,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<Option<Vec<u8>>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT c.challenge
        FROM kival.webauthn_ceremonies c
        JOIN kival.sessions s
            ON s.id = c.session_id
            AND s.user_id = c.user_id
        WHERE c.id = $1
            AND c.user_id = $2
            AND c.session_id = $3
            AND c.kind = 'registration'
            AND c.consumed_at IS NULL
            AND c.expires_at > now()
            AND s.revoked_at IS NULL
            AND s.expires_at > now()
        FOR UPDATE OF c, s
        "#,
    )
    .bind(ceremony_id)
    .bind(user_id)
    .bind(session_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Lists active passkeys for a user.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn list_passkeys(pool: &PgPool, user_id: Uuid) -> Result<Vec<PasskeyRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT id, user_id, label, credential_id, public_key, signature_count,
               created_at, updated_at, last_used_at, revoked_at
        FROM kival.passkey_credentials
        WHERE user_id = $1
            AND revoked_at IS NULL ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

/// Locks all active passkey IDs for a user.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_active_passkey_ids(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT id
        FROM kival.passkey_credentials
        WHERE user_id = $1
            AND revoked_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?)
}

/// Revokes one active passkey owned by a user.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn revoke_passkey(
    tx: &mut Transaction<'_, Postgres>,
    passkey_id: Uuid,
    user_id: Uuid,
) -> Result<PasskeyRow> {
    Ok(sqlx::query_as(
        r#"
        UPDATE kival.passkey_credentials
        SET revoked_at = now(), revoked_by = $2, revocation_reason = 'user_revoked'
        WHERE id = $1
            AND user_id = $2
            AND revoked_at IS NULL
        RETURNING id, user_id, label, credential_id, public_key, signature_count,
                  created_at, updated_at, last_used_at, revoked_at
        "#,
    )
    .bind(passkey_id)
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?)
}

/// Locks one active passkey by owner and credential identifier.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_passkey(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    credential_id: &[u8],
) -> Result<Option<PasskeyRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT id, user_id, label, credential_id, public_key, signature_count,
               created_at, updated_at, last_used_at, revoked_at
        FROM kival.passkey_credentials
        WHERE user_id = $1
            AND credential_id = $2
            AND revoked_at IS NULL FOR UPDATE
        "#,
    )
    .bind(user_id)
    .bind(credential_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Stores one verified public-key credential.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn create_passkey(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    label: Option<&str>,
    credential_id: &[u8],
    public_key: &[u8],
    signature_count: i64,
) -> Result<PasskeyRow> {
    Ok(sqlx::query_as(
        r#"
        INSERT INTO kival.passkey_credentials (
            user_id, credential_id, public_key, label, signature_count
        )
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, user_id, label, credential_id, public_key, signature_count,
                  created_at, updated_at, last_used_at, revoked_at
        "#,
    )
    .bind(user_id)
    .bind(credential_id)
    .bind(public_key)
    .bind(label)
    .bind(signature_count)
    .fetch_one(&mut **tx)
    .await?)
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredEnrollmentIdentity {
    /// Stored enrollment-code identifier.
    code_id: Uuid,
    /// Stored user identifier.
    user_id: Uuid,
    /// Stored enrollment purpose before typed parsing.
    purpose: String,
    /// Stored username.
    username: String,
    /// Stored display name.
    display_name: String,
}

impl TryFrom<StoredEnrollmentIdentity> for EnrollmentIdentity {
    type Error = KernelError;

    fn try_from(row: StoredEnrollmentIdentity) -> Result<Self> {
        Ok(Self {
            code_id: row.code_id,
            user_id: row.user_id,
            purpose: parse_stored("passkey enrollment purpose", row.purpose)?,
            username: row.username,
            display_name: row.display_name,
        })
    }
}

/// Locks a valid enrollment code and its active user by code hash and username.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_enrollment_identity(
    tx: &mut Transaction<'_, Postgres>,
    code_hash: &[u8],
    username: &str,
) -> Result<Option<EnrollmentIdentity>> {
    sqlx::query_as::<_, StoredEnrollmentIdentity>(
        r#"
        WITH locked_user AS MATERIALIZED (
            SELECT id, username, display_name
            FROM kival.users
            WHERE username_normalized = lower($2)
                AND disabled_at IS NULL FOR UPDATE
        )
        SELECT c.id AS code_id, c.user_id, c.purpose, u.username, u.display_name
        FROM locked_user u
        JOIN kival.passkey_enrollment_codes c
            ON c.user_id = u.id
        WHERE c.code_hash = $1
            AND c.consumed_at IS NULL
            AND c.revoked_at IS NULL
            AND c.expires_at > now() FOR UPDATE OF c
        "#,
    )
    .bind(code_hash)
    .bind(username)
    .fetch_optional(&mut **tx)
    .await?
    .map(TryInto::try_into)
    .transpose()
}

/// Consumes expired ceremonies attached to an enrollment code.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn consume_expired_enrollment_ceremonies(
    tx: &mut Transaction<'_, Postgres>,
    code_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE kival.webauthn_ceremonies
        SET consumed_at = now()
        WHERE enrollment_code_id = $1
            AND consumed_at IS NULL
            AND expires_at <= now()
        "#,
    )
    .bind(code_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Returns an existing live enrollment ceremony, if present.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn active_enrollment_ceremony(
    tx: &mut Transaction<'_, Postgres>,
    code_id: Uuid,
) -> Result<Option<(Uuid, Vec<u8>)>> {
    Ok(sqlx::query_as(
        r#"
        SELECT id, challenge
        FROM kival.webauthn_ceremonies
        WHERE enrollment_code_id = $1
            AND kind = 'enrollment_registration'
            AND consumed_at IS NULL
            AND expires_at > now()
        "#,
    )
    .bind(code_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Creates a registration ceremony for an enrollment code.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn create_enrollment_ceremony(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    code_id: Uuid,
    challenge: &[u8],
    ttl_interval: &str,
) -> Result<Uuid> {
    Ok(sqlx::query_scalar(
        r#"
        INSERT INTO kival.webauthn_ceremonies
            (user_id, enrollment_code_id, kind, challenge, expires_at)
        VALUES ($1, $2, 'enrollment_registration', $3, now() + $4::interval)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(code_id)
    .bind(challenge)
    .bind(ttl_interval)
    .fetch_one(&mut **tx)
    .await?)
}

/// Lists active credential IDs inside a transaction.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn active_credential_ids_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<Vec<Vec<u8>>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT credential_id
        FROM kival.passkey_credentials
        WHERE user_id = $1
            AND revoked_at IS NULL ORDER BY created_at, id
        "#,
    )
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?)
}

/// Resolves the user of a still-valid enrollment completion tuple.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn enrollment_completion_user_id(
    tx: &mut Transaction<'_, Postgres>,
    code_hash: &[u8],
    username: &str,
    ceremony_id: Uuid,
) -> Result<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT c.user_id
        FROM kival.passkey_enrollment_codes c
        JOIN kival.users u
            ON u.id = c.user_id
        JOIN kival.webauthn_ceremonies wc
            ON wc.enrollment_code_id = c.id
        WHERE c.code_hash = $1
            AND u.username_normalized = lower($2)
            AND c.consumed_at IS NULL
            AND c.revoked_at IS NULL
            AND c.expires_at > now()
            AND u.disabled_at IS NULL
            AND wc.id = $3
            AND wc.kind = 'enrollment_registration'
            AND wc.consumed_at IS NULL
            AND wc.expires_at > now()
        "#,
    )
    .bind(code_hash)
    .bind(username)
    .bind(ceremony_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Locks an enrollment code and ceremony for final registration verification.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_enrollment_completion(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    code_hash: &[u8],
    username: &str,
    ceremony_id: Uuid,
) -> Result<Option<EnrollmentCompletion>> {
    let row = sqlx::query_as::<_, (Uuid, Vec<u8>, String)>(
        r#"
        SELECT c.id, wc.challenge, c.purpose
        FROM kival.passkey_enrollment_codes c
        JOIN kival.users u
            ON u.id = c.user_id
        JOIN kival.webauthn_ceremonies wc
            ON wc.enrollment_code_id = c.id
        WHERE c.user_id = $1
            AND c.code_hash = $2
            AND u.username_normalized = lower($3)
            AND c.consumed_at IS NULL
            AND c.revoked_at IS NULL
            AND c.expires_at > now()
            AND u.disabled_at IS NULL
            AND wc.id = $4
            AND wc.kind = 'enrollment_registration'
            AND wc.consumed_at IS NULL
            AND wc.expires_at > now()
        FOR UPDATE OF c, wc
        "#,
    )
    .bind(user_id)
    .bind(code_hash)
    .bind(username)
    .bind(ceremony_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.map(|(code_id, challenge, purpose)| {
        Ok(EnrollmentCompletion {
            code_id,
            challenge,
            purpose: parse_stored("passkey enrollment purpose", purpose)?,
        })
    })
    .transpose()
}

/// Consumes a still-active passkey enrollment code.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn consume_enrollment_code(
    tx: &mut Transaction<'_, Postgres>,
    code_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE kival.passkey_enrollment_codes
        SET consumed_at = now()
        WHERE id = $1
            AND consumed_at IS NULL
            AND revoked_at IS NULL
        "#,
    )
    .bind(code_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Deletes a bounded batch of terminal `WebAuthn` ceremonies for a user.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn prune_terminal_ceremonies(
    pool: &PgPool,
    user_id: Uuid,
    batch_size: i64,
) -> Result<u64> {
    let result = sqlx::query(
        r#"
        WITH terminal AS (
            SELECT id
            FROM kival.webauthn_ceremonies
            WHERE user_id = $1
                AND (consumed_at IS NOT NULL OR expires_at <= now())
            ORDER BY expires_at, id
            LIMIT $2
            FOR UPDATE SKIP LOCKED
        )
        DELETE FROM kival.webauthn_ceremonies AS ceremony
        USING terminal
        WHERE ceremony.id = terminal.id
        "#,
    )
    .bind(user_id)
    .bind(batch_size)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
