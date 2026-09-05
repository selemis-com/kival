//! Deployment-operator user provisioning.

use argx::{Args, Subcommand};
use eyre::{Context, Result, bail};
use kival_kernel::{
    EventKind, PasskeyEnrollmentPurpose, create_user, is_bootstrapped, lock_admin_provisioning,
    lock_user_for_operator, record_operator_user_created, record_operator_user_lifecycle,
    set_user_disabled_as_operator,
};
use kival_tracing::info;
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::recovery::{issue_operator_enrollment_code, print_enrollment_link};

/// Arguments for `kivald admin users`.
#[derive(Debug, Args, Serialize)]
pub(crate) struct AdminUsersCommand {
    /// User-administration operation to run.
    #[argx(subcommand)]
    pub command: AdminUsersSubcommand,
}

/// Deployment-operator user-administration operations.
#[derive(Debug, Subcommand, Serialize)]
pub(crate) enum AdminUsersSubcommand {
    /// Create a user and issue their first passkey enrollment link.
    Create(AdminUserCreateCommand),

    /// Disable a user while preserving their credentials and access assignments.
    Disable(AdminUserLifecycleCommand),

    /// Enable a disabled user without changing their credentials or access assignments.
    Enable(AdminUserLifecycleCommand),
}

/// Arguments for `kivald admin users create`.
#[derive(Debug, Args, Serialize)]
pub(crate) struct AdminUserCreateCommand {
    /// Username for the new user.
    #[argx(long)]
    pub username: String,

    /// Display name for the new user.
    #[argx(long)]
    pub display_name: String,
}

/// Arguments for deployment-operator user lifecycle changes.
#[derive(Debug, Args, Serialize)]
pub(crate) struct AdminUserLifecycleCommand {
    /// User ID or username whose lifecycle state should change.
    pub user: String,
}

/// Reversible user lifecycle transition performed by a deployment operator.
#[derive(Debug, Clone, Copy)]
enum UserLifecycleAction {
    /// Prevent the user from authenticating or accessing Kival.
    Disable,
    /// Restore the user's ability to authenticate and access Kival.
    Enable,
}

impl UserLifecycleAction {
    /// Returns the event kind emitted for this lifecycle transition.
    const fn event_kind(self) -> EventKind {
        match self {
            Self::Disable => EventKind::UserDisabled,
            Self::Enable => EventKind::UserEnabled,
        }
    }

    /// Returns the transition name used in operator-facing output.
    const fn past_tense(self) -> &'static str {
        match self {
            Self::Disable => "disabled",
            Self::Enable => "enabled",
        }
    }
}

impl AdminUsersCommand {
    /// Runs the selected deployment-operator user operation.
    pub(crate) async fn run(&self, db_pool: PgPool, origin: &str) -> Result<()> {
        match &self.command {
            AdminUsersSubcommand::Create(command) => command.run(db_pool, origin).await,
            AdminUsersSubcommand::Disable(command) => {
                command.run(db_pool, UserLifecycleAction::Disable).await
            }
            AdminUsersSubcommand::Enable(command) => {
                command.run(db_pool, UserLifecycleAction::Enable).await
            }
        }
    }
}

impl AdminUserCreateCommand {
    /// Creates a user and prints a one-time passkey enrollment link.
    async fn run(&self, db_pool: PgPool, origin: &str) -> Result<()> {
        let username = self.username.trim();
        let display_name = self.display_name.trim();

        if username.is_empty() {
            bail!("username must not be empty");
        }
        if display_name.is_empty() {
            bail!("display name must not be empty");
        }

        let mut tx =
            db_pool.begin().await.wrap_err("failed to begin operator user creation transaction")?;
        lock_admin_provisioning(&mut tx)
            .await
            .wrap_err("failed to acquire admin provisioning lock")?;

        let bootstrapped =
            is_bootstrapped(&mut tx).await.wrap_err("failed to inspect Kival bootstrap state")?;
        if !bootstrapped {
            bail!("Kival is not bootstrapped; run `kivald admin bootstrap` first");
        }

        if let Some((_, existing_username, _)) = lock_user_for_operator(&mut tx, username)
            .await
            .wrap_err("failed to inspect existing users")?
        {
            bail!("user `{existing_username}` already exists");
        }

        let created =
            create_user(&mut tx, username, display_name).await.wrap_err("failed to create user")?;

        let issued = issue_operator_enrollment_code(
            &mut tx,
            created.id,
            created.username,
            PasskeyEnrollmentPurpose::Enrollment,
            origin,
        )
        .await?;

        record_operator_user_created(&mut tx, created.id, username, issued.id)
            .await
            .wrap_err("failed to record operator user creation event")?;

        tx.commit().await.wrap_err("failed to commit operator user creation transaction")?;

        info!(
            target: "kival::cli",
            user_id = %created.id,
            username,
            "Created Kival user",
        );
        print_enrollment_link(&issued, "Kival user passkey enrollment link");
        Ok(())
    }
}

impl AdminUserLifecycleCommand {
    /// Changes one user's reversible account lifecycle state.
    async fn run(&self, db_pool: PgPool, action: UserLifecycleAction) -> Result<()> {
        let user = self.user.trim();
        if user.is_empty() {
            bail!("user must not be empty");
        }

        let mut tx = db_pool
            .begin()
            .await
            .wrap_err("failed to begin operator user lifecycle transaction")?;
        let (user_id, username, is_disabled) = lock_user(&mut tx, user).await?;

        match action {
            UserLifecycleAction::Disable if is_disabled => {
                bail!("user is already disabled");
            }
            UserLifecycleAction::Enable if !is_disabled => {
                bail!("user is already active");
            }
            UserLifecycleAction::Disable => {
                set_user_disabled_as_operator(&mut tx, user_id, true)
                    .await
                    .wrap_err("failed to disable user")?;
            }
            UserLifecycleAction::Enable => {
                set_user_disabled_as_operator(&mut tx, user_id, false)
                    .await
                    .wrap_err("failed to enable user")?;
            }
        }

        record_operator_user_lifecycle(&mut tx, action.event_kind(), user_id, &username)
            .await
            .wrap_err("failed to record operator user lifecycle event")?;

        tx.commit().await.wrap_err("failed to commit operator user lifecycle transaction")?;
        info!(
            target: "kival::cli",
            user_id = %user_id,
            username,
            action = action.past_tense(),
            "Changed Kival user lifecycle state",
        );
        println!("User {username} {}.", action.past_tense());
        Ok(())
    }
}

/// Locks and resolves one user by UUID or normalized username.
async fn lock_user(tx: &mut Transaction<'_, Postgres>, user: &str) -> Result<(Uuid, String, bool)> {
    lock_user_for_operator(tx, user)
        .await
        .wrap_err("failed to resolve user")?
        .ok_or_else(|| eyre::eyre!("user not found"))
}

#[cfg(test)]
mod tests {
    use argx::Parser;
    use eyre::Result;
    use kival_tests::{TestFixtureExt, TestKival};

    use super::{AdminUserLifecycleCommand, AdminUsersSubcommand, UserLifecycleAction};

    #[derive(Parser)]
    struct TestCli {
        #[argx(subcommand)]
        command: AdminUsersSubcommand,
    }

    #[test]
    fn lifecycle_commands_accept_username_or_user_id() {
        let disable = TestCli::try_parse_from(["test", "disable", "alice"])
            .expect("disable command should parse")
            .command;
        let AdminUsersSubcommand::Disable(disable) = disable else {
            panic!("disable subcommand should parse");
        };
        assert_eq!(disable.user, "alice");

        let user_id = uuid::Uuid::now_v7();
        let user_id_text = user_id.to_string();
        let enable = TestCli::try_parse_from(["test", "enable", &user_id_text])
            .expect("enable command should parse")
            .command;
        let AdminUsersSubcommand::Enable(enable) = enable else {
            panic!("enable subcommand should parse");
        };
        assert_eq!(enable.user, user_id.to_string());
    }

    #[sqlx::test(migrations = "../../crates/kernel/migrations")]
    async fn operator_create_reports_existing_username(pool: sqlx::PgPool) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let existing = kival.create_user("operator-create-existing").await?;

        let error = super::AdminUserCreateCommand {
            username: existing.username.clone(),
            display_name: "Duplicate User".to_owned(),
        }
        .run(kival.pool.clone(), "http://localhost:3000")
        .await
        .expect_err("duplicate username should be rejected");

        assert_eq!(error.to_string(), format!("user `{}` already exists", existing.username));
        Ok(())
    }

    #[sqlx::test(migrations = "../../crates/kernel/migrations")]
    async fn operator_disable_and_enable_are_reversible_and_operator_attributed(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let user = kival.create_user("operator-lifecycle").await?;

        AdminUserLifecycleCommand { user: user.username.clone() }
            .run(kival.pool.clone(), UserLifecycleAction::Disable)
            .await?;

        let disabled: (String, Option<uuid::Uuid>, bool) = sqlx::query_as(
            "SELECT status, disabled_by, disabled_by_operator FROM kival.users WHERE id = $1",
        )
        .bind(user.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(disabled, ("disabled".to_owned(), None, true));

        let active_sessions: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM kival.sessions WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(active_sessions, 1);

        AdminUserLifecycleCommand { user: user.id.to_string() }
            .run(kival.pool.clone(), UserLifecycleAction::Enable)
            .await?;

        let enabled: (String, bool, Option<uuid::Uuid>, bool) = sqlx::query_as(
            "SELECT status, disabled_at IS NULL, disabled_by, disabled_by_operator FROM kival.users WHERE id = $1",
        )
            .bind(user.id)
            .fetch_one(&kival.pool)
            .await?;
        assert_eq!(enabled, ("active".to_owned(), true, None, false));

        let operator_events: i64 = sqlx::query_scalar(
            r#"
            SELECT count(*)
            FROM kival.events
            WHERE target_user_id = $1
                AND event_kind IN ('user.disabled', 'user.enabled')
                AND actor_user_id IS NULL
                AND payload @> '{"operator_lifecycle": true}'::jsonb
            "#,
        )
        .bind(user.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(operator_events, 2);

        Ok(())
    }
}
