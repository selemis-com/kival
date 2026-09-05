//! Deployment-operator passkey recovery.

use argx::Args;
use eyre::{Context, Result, bail};
use kival_common::security;
use kival_kernel::{
    PasskeyEnrollmentPurpose, create_operator_enrollment_code, lock_active_user_for_operator,
    record_operator_passkey_recovery, revoke_credentials_for_operator_recovery,
    revoke_outstanding_enrollment_codes_as_operator,
};
use kival_tracing::info;
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

/// Prefix distinguishing passkey enrollment capabilities from other Kival secrets.
const ENROLLMENT_CODE_PREFIX: &str = "kvl_enroll_";

/// Arguments for the kivald admin recover command.
#[derive(Debug, Args, Serialize)]
pub(crate) struct AdminRecoverCommand {
    /// User ID or username whose passkeys should be reset.
    pub user: String,

    /// Also revoke every active API key owned by the user.
    #[argx(long)]
    pub revoke_api_keys: bool,
}

/// One-time enrollment capability returned after a committed operator action.
pub(super) struct IssuedEnrollmentCode {
    /// Stable code record identifier.
    pub id: Uuid,
    /// Target user identifier.
    pub user_id: Uuid,
    /// Target user's canonical username.
    pub username: String,
    /// Magic link containing the raw code in its URL fragment.
    pub url: String,
    /// Number of older outstanding codes invalidated by issuance.
    pub superseded_code_count: u64,
}

impl AdminRecoverCommand {
    /// Resets an active user's interactive credentials and issues a one-time enrollment link.
    pub(crate) async fn run(&self, db_pool: PgPool, origin: &str) -> Result<()> {
        let user = self.user.trim();
        if user.is_empty() {
            bail!("user must not be empty");
        }

        let mut tx =
            db_pool.begin().await.wrap_err("failed to begin passkey recovery transaction")?;
        let (user_id, username) = lock_active_user(&mut tx, user).await?;

        let revocations =
            revoke_credentials_for_operator_recovery(&mut tx, user_id, self.revoke_api_keys)
                .await
                .wrap_err("failed to revoke credentials during recovery")?;
        let revoked_api_key_count = revocations.api_keys;

        let issued = issue_operator_enrollment_code(
            &mut tx,
            user_id,
            username,
            PasskeyEnrollmentPurpose::PasskeyReset,
            origin,
        )
        .await?;

        record_operator_passkey_recovery(
            &mut tx,
            user_id,
            issued.id,
            revocations,
            issued.superseded_code_count,
            self.revoke_api_keys,
        )
        .await
        .wrap_err("failed to record passkey recovery event")?;

        tx.commit().await.wrap_err("failed to commit passkey recovery transaction")?;
        print_enrollment_link(&issued, "Kival passkey recovery link");
        if self.revoke_api_keys {
            println!("Revoked {revoked_api_key_count} active API key(s).");
        } else {
            println!("Existing API keys are not affected by passkey recovery.");
        }
        Ok(())
    }
}

/// Issues and stores one operator-attributed enrollment capability.
pub(super) async fn issue_operator_enrollment_code(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    username: String,
    purpose: PasskeyEnrollmentPurpose,
    origin: &str,
) -> Result<IssuedEnrollmentCode> {
    let superseded_code_count = revoke_outstanding_enrollment_codes_as_operator(tx, user_id)
        .await
        .wrap_err("failed to invalidate older enrollment codes")?;

    let raw_code = format!(
        "{ENROLLMENT_CODE_PREFIX}{}",
        security::generate_secret_token().wrap_err("failed to generate enrollment code")?
    );
    let code_hash = security::hash_token(&raw_code);
    let code_id = create_operator_enrollment_code(tx, user_id, code_hash.as_slice(), purpose)
        .await
        .wrap_err("failed to create operator enrollment code")?;

    let url = format!("{origin}/auth/enroll#code={raw_code}&username={username}");

    Ok(IssuedEnrollmentCode {
        id: code_id,
        user_id,
        username,
        url,
        superseded_code_count,
    })
}

/// Prints the raw one-time link only to the invoking operator's stdout.
pub(super) fn print_enrollment_link(issued: &IssuedEnrollmentCode, heading: &str) {
    info!(
        target: "kival::cli",
        user_id = %issued.user_id,
        username = %issued.username,
        enrollment_code_id = %issued.id,
        "Issued one-time passkey enrollment link",
    );
    println!("{heading} (shown once; expires in 30 minutes):");
    println!("{}", issued.url);
}

/// Locks and resolves one active user by UUID or normalized username.
async fn lock_active_user(
    tx: &mut Transaction<'_, Postgres>,
    user: &str,
) -> Result<(Uuid, String)> {
    lock_active_user_for_operator(tx, user)
        .await
        .wrap_err("failed to resolve recovery user")?
        .ok_or_else(|| eyre::eyre!("active recovery user not found"))
}

#[cfg(test)]
mod tests {
    use kival_tests::{TestFixtureExt, TestKival};

    use super::*;

    #[sqlx::test(migrations = "../../crates/kernel/migrations")]
    async fn recovery_resolves_an_ordinary_username_case_insensitively(pool: PgPool) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let user = kival.create_user("operator-recovery-username").await?;
        let mut tx = kival.pool.begin().await?;

        let resolved = lock_active_user(&mut tx, &user.username.to_uppercase()).await?;

        assert_eq!(resolved, (user.id, user.username));
        tx.rollback().await?;
        Ok(())
    }

    #[sqlx::test(migrations = "../../crates/kernel/migrations")]
    async fn recovery_resolves_a_uuid_as_a_user_id(pool: PgPool) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let user = kival.create_user("operator-recovery-id").await?;
        let mut tx = kival.pool.begin().await?;

        let resolved = lock_active_user(&mut tx, &user.id.to_string()).await?;

        assert_eq!(resolved, (user.id, user.username));
        tx.rollback().await?;
        Ok(())
    }

    #[sqlx::test(migrations = "../../crates/kernel/migrations")]
    async fn unknown_recovery_identifiers_fail_without_mutation(pool: PgPool) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let target = kival.create_user("operator-recovery-unchanged").await?;
        let before = recovery_state(&kival, target.id).await?;
        let unknown_id = Uuid::now_v7().to_string();
        let suffix = Uuid::now_v7().simple().to_string();
        let unknown_username = format!("missing-{}", &suffix[suffix.len() - 12..]);

        for identifier in [unknown_id, unknown_username] {
            let error = AdminRecoverCommand { user: identifier, revoke_api_keys: false }
                .run(kival.pool.clone(), "https://kival.example")
                .await
                .expect_err("unknown recovery identifiers must fail");
            assert!(error.to_string().contains("active recovery user not found"));
        }

        assert_eq!(recovery_state(&kival, target.id).await?, before);
        Ok(())
    }

    #[sqlx::test(migrations = "../../crates/kernel/migrations")]
    async fn recovery_preserves_api_keys_by_default(pool: PgPool) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let user = kival.create_user("operator-recovery-preserve-key").await?;
        let api_key_id = insert_api_key(&kival, user.id, "preserved-key").await?;

        AdminRecoverCommand { user: user.username, revoke_api_keys: false }
            .run(kival.pool.clone(), "https://kival.example")
            .await?;

        let key_state: (bool, Option<Uuid>, bool) = sqlx::query_as(
            "SELECT revoked_at IS NULL, revoked_by, revoked_by_operator FROM kival.api_keys WHERE id = $1",
        )
        .bind(api_key_id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(key_state, (true, None, false));

        let audit: (bool, bool, i64) = sqlx::query_as(
            r#"
            SELECT
                (payload ->> 'api_keys_preserved')::boolean,
                (payload ->> 'revoke_api_keys_requested')::boolean,
                (payload ->> 'revoked_api_key_count')::bigint
            FROM kival.events
            WHERE event_kind = 'admin.passkey_recovery_issued'
                AND target_user_id = $1
            "#,
        )
        .bind(user.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(audit, (true, false, 0));

        Ok(())
    }

    #[sqlx::test(migrations = "../../crates/kernel/migrations")]
    async fn recovery_can_revoke_all_active_api_keys_as_the_operator(pool: PgPool) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let user = kival.create_user("operator-recovery-revoke-keys").await?;
        let first = insert_api_key(&kival, user.id, "first-key").await?;
        let second = insert_api_key(&kival, user.id, "second-key").await?;
        let already_revoked = insert_api_key(&kival, user.id, "already-revoked-key").await?;
        sqlx::query("UPDATE kival.api_keys SET revoked_at = now(), revoked_by = $2 WHERE id = $1")
            .bind(already_revoked)
            .bind(kival.admin.id)
            .execute(&kival.pool)
            .await?;

        AdminRecoverCommand { user: user.id.to_string(), revoke_api_keys: true }
            .run(kival.pool.clone(), "https://kival.example")
            .await?;

        let operator_revoked: Vec<(Uuid, Option<Uuid>, bool)> = sqlx::query_as(
            r#"
            SELECT id, revoked_by, revoked_by_operator
            FROM kival.api_keys
            WHERE id = ANY($1)
            ORDER BY id
            "#,
        )
        .bind(vec![first, second])
        .fetch_all(&kival.pool)
        .await?;
        assert_eq!(operator_revoked.len(), 2);
        assert!(
            operator_revoked
                .iter()
                .all(|(_, revoked_by, by_operator)| revoked_by.is_none() && *by_operator)
        );

        let previously_revoked: (Option<Uuid>, bool) = sqlx::query_as(
            "SELECT revoked_by, revoked_by_operator FROM kival.api_keys WHERE id = $1",
        )
        .bind(already_revoked)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(previously_revoked, (Some(kival.admin.id), false));

        let audit: (bool, bool, i64) = sqlx::query_as(
            r#"
            SELECT
                (payload ->> 'api_keys_preserved')::boolean,
                (payload ->> 'revoke_api_keys_requested')::boolean,
                (payload ->> 'revoked_api_key_count')::bigint
            FROM kival.events
            WHERE event_kind = 'admin.passkey_recovery_issued'
                AND target_user_id = $1
            "#,
        )
        .bind(user.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(audit, (false, true, 2));

        Ok(())
    }

    async fn insert_api_key(kival: &TestKival, user_id: Uuid, label: &str) -> Result<Uuid> {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        let token_hash = [first.as_bytes().as_slice(), second.as_bytes().as_slice()].concat();

        Ok(sqlx::query_scalar(
            r#"
            INSERT INTO kival.api_keys (user_id, label, token_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
        )
        .bind(user_id)
        .bind(label)
        .bind(token_hash)
        .fetch_one(&kival.pool)
        .await?)
    }

    async fn recovery_state(kival: &TestKival, user_id: Uuid) -> Result<(i64, i64, i64, i64, i64)> {
        Ok(sqlx::query_as(
            r#"
            SELECT
                (SELECT count(*) FROM kival.sessions WHERE user_id = $1 AND revoked_at IS NULL),
                (SELECT count(*) FROM kival.passkey_credentials
                    WHERE user_id = $1
                        AND revoked_at IS NULL),
                (SELECT count(*) FROM kival.api_keys
                    WHERE user_id = $1
                        AND revoked_at IS NULL),
                (SELECT count(*) FROM kival.passkey_enrollment_codes WHERE user_id = $1),
                (SELECT count(*) FROM kival.events WHERE target_user_id = $1)
            "#,
        )
        .bind(user_id)
        .fetch_one(&kival.pool)
        .await?)
    }
}
