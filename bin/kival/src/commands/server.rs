//! Public commands.

use clap::{Args, Parser, Subcommand};
use clap_schema::{CommandSchema, schema_handler};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{KivalClient, StatusResponse};
use serde::Serialize;
use url::Url;

use crate::utils::{
    config,
    output::{OutputMode, print_output},
};

/// Kival server status commands.
#[derive(Debug, Args, CommandSchema)]
pub struct ServerCommand {
    /// The server command to run.
    #[command(subcommand)]
    pub command: ServerSubcommand,
}

/// Commands for inspecting Kival server status.
#[derive(Debug, Subcommand, CommandSchema)]
pub enum ServerSubcommand {
    /// Check Kival server health.
    #[command(name = "health")]
    Health(HealthCommand),

    /// Check Kival server readiness.
    #[command(name = "ready")]
    Ready(ReadyCommand),
}

impl ServerCommand {
    /// Execute the selected server command.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected command fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            ServerSubcommand::Health(command) => {
                command.run(ctx, output).await?;
            }
            ServerSubcommand::Ready(command) => {
                command.run(ctx, output).await?;
            }
        }
        Ok(())
    }
}

/// Arguments for `kival health`.
#[derive(Debug, Parser, Serialize)]
pub struct HealthCommand {
    /// Override the configured Kival server root URL.
    #[arg(long, value_name = "URL")]
    pub url: Option<Url>,
}

/// Arguments for `kival ready`.
#[derive(Debug, Parser, Serialize)]
pub struct ReadyCommand {
    /// Override the configured Kival server root URL.
    #[arg(long, value_name = "URL")]
    pub url: Option<Url>,
}

#[schema_handler(run)]
impl HealthCommand {
    /// Run `kival health`.
    ///
    /// # Errors
    ///
    /// Returns an error if the server cannot be reached or the health response cannot be decoded.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<StatusResponse> {
        let config = config::load_client_config_for_command(&ctx, &self)?;
        let client = KivalClient::new(config.url())?;
        let health = client.health().await?;

        print_output(output, &health, || {
            println!("{}", health.status);
        })?;
        Ok(health)
    }
}

#[schema_handler(run)]
impl ReadyCommand {
    /// Run `kival ready`.
    ///
    /// # Errors
    ///
    /// Returns an error if the server cannot be reached, is not ready, or the readiness response
    /// cannot be decoded.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<StatusResponse> {
        let config = config::load_client_config_for_command(&ctx, &self)?;
        let client = KivalClient::new(config.url())?;
        let ready = client.ready().await?;

        print_output(output, &ready, || {
            println!("{}", ready.status);
        })?;
        Ok(ready)
    }
}
