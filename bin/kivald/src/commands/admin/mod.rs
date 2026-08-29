//! The `admin` command for the `kivald` CLI.

use std::path::PathBuf;

use argx::{Args, Subcommand};
use eyre::{Context, Result};
use kival_cli::{
    commands::config::{DEFAULT_CONFIG_FILENAME, load_config},
    runner::CliContext,
};
use kival_kernel::open_pool_with_options;
use kival_server::WebAuthnConfig;
use kival_tracing::trace;
use sqlx::PgPool;

use crate::{
    ServerConfig,
    commands::admin::{
        bootstrap::AdminBootstrapCommand, recovery::AdminRecoverCommand, users::AdminUsersCommand,
        workspaces::AdminWorkspacesCommand,
    },
    database::connect_options_from_env,
};

mod bootstrap;
mod recovery;
mod users;
mod workspaces;

/// The `admin` command arguments.
#[derive(Debug, Args)]
pub struct AdminCommand {
    /// The path to the configuration file to use.
    #[argx(long, global)]
    pub config: Option<PathBuf>,

    /// Canonical URL used to generate passkey enrollment links.
    #[argx(long, global)]
    pub canonical_url: Option<String>,

    /// The admin subcommand to run.
    #[argx(subcommand)]
    pub(crate) command: AdminSubcommand,
}

/// The available `admin` subcommands.
#[derive(Debug, Subcommand)]
pub(crate) enum AdminSubcommand {
    /// Bootstrap the first global admin user.
    Bootstrap(AdminBootstrapCommand),

    /// Reset a user's interactive credentials and issue a one-time passkey enrollment link.
    Recover(AdminRecoverCommand),

    /// Provision, disable, and enable users.
    Users(AdminUsersCommand),

    /// Create workspaces with optional one-shot administrative initialization.
    Workspaces(AdminWorkspacesCommand),
}


impl AdminCommand {
    /// Run the `admin` command.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration loading, database setup, or the selected
    /// admin operation fails.
    pub async fn run(&self, ctx: CliContext) -> Result<()> {
        let Self { config, canonical_url, command } = self;

        let config_path =
            config.clone().unwrap_or_else(|| ctx.datadir.join(DEFAULT_CONFIG_FILENAME));
        let mut config = load_config::<ServerConfig>(&config_path)?;
        if let Some(canonical_url) = canonical_url {
            config.canonical_url.clone_from(canonical_url);
        }

        let canonical_url = config.canonical_url;
        let webauthn = WebAuthnConfig::from_canonical_url(&canonical_url)?;
        let origin = webauthn.origin();
        let db_pool = open_admin_pool().await?;

        match command {
            AdminSubcommand::Bootstrap(command) => command.run(db_pool, origin).await,
            AdminSubcommand::Recover(command) => command.run(db_pool, origin).await,
            AdminSubcommand::Users(command) => command.run(db_pool, origin).await,
            AdminSubcommand::Workspaces(command) => command.run(db_pool).await,
        }
    }
}

/// Opens the Kival database using the same env-based connection convention as `serve`.
async fn open_admin_pool() -> Result<PgPool> {
    let db_pool = open_pool_with_options(connect_options_from_env()?)
        .await
        .wrap_err("failed to open Kival database pool")?;

    trace!(target: "kival::cli", "Database connection established");

    Ok(db_pool)
}
