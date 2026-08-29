//! The `admin bootstrap` command for the `kivald` CLI.

use argx::Args;
use eyre::{Context, Result, bail};
use kival_kernel::{
    PasskeyEnrollmentPurpose, create_user, enabled_global_admin_count,
    grant_global_admin_as_operator, lock_admin_provisioning, record_bootstrap_completed,
    user_count,
};
use kival_tracing::info;
use serde::Serialize;
use sqlx::PgPool;

use super::recovery::{issue_operator_enrollment_code, print_enrollment_link};

/// Arguments for `kivald admin bootstrap`.
#[derive(Debug, Args, Serialize)]
pub(crate) struct AdminBootstrapCommand {
    /// The username for the initial global admin.
    #[argx(long)]
    pub username: String,

    /// The display name for the initial global admin.
    #[argx(long)]
    pub display_name: String,
}

impl AdminBootstrapCommand {
    /// Run `kivald admin bootstrap`.
    pub(crate) async fn run(&self, db_pool: PgPool, origin: &str) -> Result<()> {
        let username = self.username.trim();
        let display_name = self.display_name.trim();

        if username.is_empty() {
            bail!("username must not be empty");
        }

        if display_name.is_empty() {
            bail!("display name must not be empty");
        }

        let mut tx =
            db_pool.begin().await.wrap_err("failed to begin admin bootstrap transaction")?;

        lock_admin_provisioning(&mut tx)
            .await
            .wrap_err("failed to acquire admin bootstrap lock")?;

        let global_admin_count = enabled_global_admin_count(&mut tx)
            .await
            .wrap_err("failed to inspect global admin state")?;
        if global_admin_count > 0 {
            info!(
                target: "kival::cli",
                global_admin_count,
                "Kival is already bootstrapped; no changes made",
            );
            tx.rollback().await.wrap_err("failed to rollback no-op admin bootstrap transaction")?;
            return Ok(());
        }

        let user_count = user_count(&mut tx).await.wrap_err("failed to inspect user state")?;
        if user_count > 0 {
            bail!(
                "Kival cannot be bootstrapped because users already exist; bootstrap is only for fresh instances"
            );
        }

        let user_id = create_user(&mut tx, username, display_name)
            .await
            .wrap_err("failed to create bootstrap user")?
            .id;
        info!(
            target: "kival::cli",
            user_id = %user_id,
            username,
            "Created bootstrap user",
        );
        grant_global_admin_as_operator(&mut tx, user_id)
            .await
            .wrap_err("failed to grant global admin")?;
        let issued = issue_operator_enrollment_code(
            &mut tx,
            user_id,
            username.to_owned(),
            PasskeyEnrollmentPurpose::Enrollment,
            origin,
        )
        .await?;
        record_bootstrap_completed(&mut tx, user_id, username, issued.id)
            .await
            .wrap_err("failed to emit admin bootstrap event")?;

        tx.commit().await.wrap_err("failed to commit admin bootstrap transaction")?;

        info!(
            target: "kival::cli",
            user_id = %user_id,
            username,
            "Kival admin bootstrap completed",
        );
        print_enrollment_link(&issued, "Initial Kival admin passkey enrollment link");

        Ok(())
    }
}
