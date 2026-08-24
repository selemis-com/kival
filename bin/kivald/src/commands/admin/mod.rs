//! The `admin` command for the `kivald` CLI.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use eyre::{Context, Result};
use kival_cli::{commands::config::load_config_for_command, runner::CliContext};
use kival_config::DEFAULT_CONFIG_FILENAME;
use kival_kernel::open_pool_with_options;
use kival_server::WebAuthnConfig;
use kival_tracing::trace;
use serde::Serialize;
use sqlx::PgPool;

use crate::{
    ServerConfig,
    commands::admin::{
        bootstrap::AdminBootstrapCommand, recovery::AdminRecoverCommand, users::AdminUsersCommand,
    },
    database::connect_options_from_env,
};

mod bootstrap;
mod recovery;
mod users;

/// The `admin` command arguments.
#[derive(Debug, Parser, Serialize)]
pub struct AdminCommand {
    /// The path to the configuration file to use.
    #[arg(long, value_name = "FILE", global = true)]
    #[serde(skip)]
    pub config: Option<PathBuf>,

    /// Canonical URL used to generate passkey enrollment links.
    #[arg(long, env = "KIVAL_CANONICAL_URL", value_name = "URL", global = true)]
    pub canonical_url: Option<String>,

    /// The admin subcommand to run.
    #[command(subcommand)]
    pub(crate) command: AdminSubcommand,
}

/// The available `admin` subcommands.
#[derive(Debug, Subcommand, Serialize)]
pub(crate) enum AdminSubcommand {
    /// Bootstrap the first global admin user.
    #[command(name = "bootstrap")]
    Bootstrap(AdminBootstrapCommand),

    /// Reset a user's interactive credentials and issue a one-time passkey enrollment link.
    #[command(name = "recover")]
    Recover(AdminRecoverCommand),

    /// Provision, disable, and enable users.
    #[command(name = "users")]
    Users(AdminUsersCommand),
}

/// Configuration overrides accepted by deployment-operator commands.
#[derive(Serialize)]
struct AdminConfigOverrides<'a> {
    /// Optional canonical public URL used to generate enrollment links.
    canonical_url: &'a Option<String>,
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
        let config = load_config_for_command::<ServerConfig, _>(
            &config_path,
            &AdminConfigOverrides { canonical_url },
        )?;

        let canonical_url = config.canonical_url();
        let webauthn = WebAuthnConfig::from_canonical_url(&canonical_url)?;
        let origin = webauthn.origin();
        let db_pool = open_admin_pool().await?;

        match command {
            AdminSubcommand::Bootstrap(command) => command.run(db_pool, origin).await,
            AdminSubcommand::Recover(command) => command.run(db_pool, origin).await,
            AdminSubcommand::Users(command) => command.run(db_pool, origin).await,
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
