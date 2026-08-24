//! Deployment-operator state transitions.
//!
//! These bindings cover transitions initiated outside the authenticated HTTP API,
//! such as initial bootstrap, account lifecycle operations, and passkey recovery.

use sqlx::{Acquire, Postgres, Transaction};
use uuid::Uuid;

use crate::{EventKind, PasskeyEnrollmentPurpose, Result};

/// Acquires the transaction-scoped lock used for administrator provisioning.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_admin_provisioning(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query(
        r#"
        SELECT pg_advisory_xact_lock(hashtext('kival.admin.bootstrap'))
        "#,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Counts all users while running an operator transaction.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn user_count(tx: &mut Transaction<'_, Postgres>) -> Result<i64> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM kival.users
        "#,
    )
    .fetch_one(&mut **tx)
    .await?)
}

/// Counts enabled global administrators.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn enabled_global_admin_count(tx: &mut Transaction<'_, Postgres>) -> Result<i64> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM kival.global_admins ga
        JOIN kival.users u
            ON u.id = ga.user_id
        WHERE ga.revoked_at IS NULL
            AND u.status = 'active'
            AND u.disabled_at IS NULL
        "#,
    )
    .fetch_one(&mut **tx)
    .await?)
}

/// Returns whether the instance bootstrap has completed.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn is_bootstrapped(tx: &mut Transaction<'_, Postgres>) -> Result<bool> {
    Ok(enabled_global_admin_count(tx).await? > 0)
}

/// Grants global administrator status through an operator action.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn grant_global_admin_as_operator(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO kival.global_admins (user_id, created_by)
        VALUES ($1, NULL)
        "#,
    )
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Records completion of the initial bootstrap operation.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn record_bootstrap_completed(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    username: &str,
    enrollment_code_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO kival.events (actor_user_id, event_kind, target_user_id, payload)
        VALUES (
            NULL,
            $1,
            $2,
            jsonb_build_object(
                'user_id', $2::text,
                'username', $3,
                'enrollment_code_id', $4::text,
                'operator_bootstrap', true
            )
        )
        "#,
    )
    .bind(EventKind::AdminBootstrapCompleted.as_str())
    .bind(user_id)
    .bind(username)
    .bind(enrollment_code_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Records creation of a user by an operator command.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn record_operator_user_created(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    username: &str,
    enrollment_code_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO kival.events (event_kind, target_user_id, payload)
        VALUES (
            $1,
            $2,
            jsonb_build_object(
                'user_id', $2::text,
                'username', $3,
                'enrollment_code_id', $4::text,
                'operator_creation', true
            )
        )
        "#,
    )
    .bind(EventKind::UserCreated.as_str())
    .bind(user_id)
    .bind(username)
    .bind(enrollment_code_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Locks a user selected by ID or username for an operator action.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_user_for_operator(
    tx: &mut Transaction<'_, Postgres>,
    user: &str,
) -> Result<Option<(Uuid, String, bool)>> {
    let row = if let Ok(user_id) = Uuid::parse_str(user) {
        sqlx::query_as::<_, (Uuid, String, bool)>(
            r#"
            SELECT id, username, disabled_at IS NOT NULL
            FROM kival.users
            WHERE id = $1
            FOR UPDATE
            "#,
        )
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await?
    } else {
        sqlx::query_as::<_, (Uuid, String, bool)>(
            r#"
            SELECT id, username, disabled_at IS NOT NULL
            FROM kival.users
            WHERE username_normalized = lower($1)
            FOR UPDATE
            "#,
        )
        .bind(user)
        .fetch_optional(&mut **tx)
        .await?
    };
    Ok(row)
}

/// Enables or disables a user through an operator action.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn set_user_disabled_as_operator(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    disabled: bool,
) -> Result<()> {
    if disabled {
        sqlx::query(
            r#"
            UPDATE kival.users
            SET status = 'disabled', disabled_at = now(), disabled_by = NULL,
                disabled_by_operator = true
            WHERE id = $1
                AND disabled_at IS NULL
            "#,
        )
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query(
            r#"
            UPDATE kival.users
            SET status = 'active', disabled_at = NULL, disabled_by = NULL,
                disabled_by_operator = false
            WHERE id = $1
                AND disabled_at IS NOT NULL
            "#,
        )
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Records an operator-driven user lifecycle event.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn record_operator_user_lifecycle(
    tx: &mut Transaction<'_, Postgres>,
    event_kind: EventKind,
    user_id: Uuid,
    username: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO kival.events (event_kind, target_user_id, payload)
        VALUES (
            $1,
            $2,
            jsonb_build_object(
                'user_id', $2::text,
                'username', $3,
                'operator_lifecycle', true
            )
        )
        "#,
    )
    .bind(event_kind.as_str())
    .bind(user_id)
    .bind(username)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Locks an active user selected by ID or username.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_active_user_for_operator(
    tx: &mut Transaction<'_, Postgres>,
    user: &str,
) -> Result<Option<(Uuid, String)>> {
    let row = if let Ok(user_id) = Uuid::parse_str(user) {
        sqlx::query_as::<_, (Uuid, String)>(
            r#"
            SELECT id, username
            FROM kival.users
            WHERE id = $1
                AND disabled_at IS NULL
            FOR UPDATE
            "#,
        )
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await?
    } else {
        sqlx::query_as::<_, (Uuid, String)>(
            r#"
            SELECT id, username
            FROM kival.users
            WHERE username_normalized = lower($1)
                AND disabled_at IS NULL
            FOR UPDATE
            "#,
        )
        .bind(user)
        .fetch_optional(&mut **tx)
        .await?
    };
    Ok(row)
}

/// Credential revocation counts produced by operator recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorRecoveryRevocations {
    /// Number of passkeys revoked.
    pub passkeys: u64,
    /// Number of sessions revoked.
    pub sessions: u64,
    /// Number of API keys revoked.
    pub api_keys: u64,
}

/// Revokes credentials as part of operator passkey recovery.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn revoke_credentials_for_operator_recovery(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    revoke_api_keys: bool,
) -> Result<OperatorRecoveryRevocations> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result = revoke_credentials_for_operator_recovery_in_savepoint(
        &mut savepoint,
        user_id,
        revoke_api_keys,
    )
    .await;

    match result {
        Ok(revocations) => {
            savepoint.commit().await?;
            Ok(revocations)
        }
        Err(error) => {
            savepoint.rollback().await?;
            Err(error)
        }
    }
}

/// Applies operator credential revocation inside a cancellation-safe savepoint.
async fn revoke_credentials_for_operator_recovery_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    revoke_api_keys: bool,
) -> Result<OperatorRecoveryRevocations> {
    let passkeys = sqlx::query(
        r#"
        UPDATE kival.passkey_credentials
        SET revoked_at = now(), revoked_by = NULL, revoked_by_operator = true,
            revocation_reason = 'deployment_operator_recovery'
        WHERE user_id = $1
            AND revoked_at IS NULL
        "#,
    )
    .bind(user_id)
    .execute(&mut **tx)
    .await?
    .rows_affected();

    let sessions = sqlx::query(
        r#"
        UPDATE kival.sessions
        SET revoked_at = now(), revoked_by = NULL, revoked_by_operator = true,
            revocation_reason = 'deployment_operator_recovery'
        WHERE user_id = $1
            AND revoked_at IS NULL
        "#,
    )
    .bind(user_id)
    .execute(&mut **tx)
    .await?
    .rows_affected();

    let api_keys = if revoke_api_keys {
        sqlx::query(
            r#"
            UPDATE kival.api_keys
            SET revoked_at = now(), revoked_by = NULL, revoked_by_operator = true
            WHERE user_id = $1
                AND revoked_at IS NULL
            "#,
        )
        .bind(user_id)
        .execute(&mut **tx)
        .await?
        .rows_affected()
    } else {
        0
    };

    Ok(OperatorRecoveryRevocations { passkeys, sessions, api_keys })
}

/// Revokes outstanding enrollment codes for a user.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn revoke_outstanding_enrollment_codes_as_operator(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<u64> {
    Ok(sqlx::query(
        r#"
        UPDATE kival.passkey_enrollment_codes
        SET revoked_at = now(), revoked_by = NULL, revoked_by_operator = true
        WHERE user_id = $1
            AND consumed_at IS NULL
            AND revoked_at IS NULL
        "#,
    )
    .bind(user_id)
    .execute(&mut **tx)
    .await?
    .rows_affected())
}

/// Creates an enrollment code for an operator workflow.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn create_operator_enrollment_code(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    code_hash: &[u8],
    purpose: PasskeyEnrollmentPurpose,
) -> Result<Uuid> {
    Ok(sqlx::query_scalar(
        r#"
        INSERT INTO kival.passkey_enrollment_codes (
            user_id, code_hash, created_by, created_by_operator, purpose, expires_at
        )
        VALUES ($1, $2, NULL, true, $3, now() + interval '30 minutes')
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(code_hash)
    .bind(purpose.as_str())
    .fetch_one(&mut **tx)
    .await?)
}

/// Records completion of an operator passkey-recovery action.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn record_operator_passkey_recovery(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    enrollment_code_id: Uuid,
    revocations: OperatorRecoveryRevocations,
    superseded_code_count: u64,
    revoke_api_keys: bool,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO kival.events (event_kind, target_user_id, payload)
        VALUES (
            $1,
            $2,
            jsonb_build_object(
                'user_id', $2::text,
                'enrollment_code_id', $3::text,
                'revoked_passkey_count', $4::bigint,
                'revoked_session_count', $5::bigint,
                'revoked_api_key_count', $6::bigint,
                'superseded_code_count', $7::bigint,
                'api_keys_preserved', NOT $8::boolean,
                'revoke_api_keys_requested', $8::boolean,
                'operator_recovery', true
            )
        )
        "#,
    )
    .bind(EventKind::AdminPasskeyRecoveryIssued.as_str())
    .bind(user_id)
    .bind(enrollment_code_id)
    .bind(i64::try_from(revocations.passkeys).unwrap_or(i64::MAX))
    .bind(i64::try_from(revocations.sessions).unwrap_or(i64::MAX))
    .bind(i64::try_from(revocations.api_keys).unwrap_or(i64::MAX))
    .bind(i64::try_from(superseded_code_count).unwrap_or(i64::MAX))
    .bind(revoke_api_keys)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
