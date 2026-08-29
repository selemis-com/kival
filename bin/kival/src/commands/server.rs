//! Public commands.

use argx::{Args, Subcommand};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{KivalClient, StatusResponse};
use serde::Serialize;
use url::Url;

use crate::utils::error::CliResult;
use crate::utils::{
    config,
    output::{OutputMode, print_output},
};

/// Kival server status commands.
#[derive(Debug, Args)]
#[argx(schema)]
pub struct ServerCommand {
    /// The server command to run.
    #[argx(subcommand)]
    pub command: ServerSubcommand,
}

/// Commands for inspecting Kival server status.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum ServerSubcommand {
    /// Check Kival server health.
    #[argx(name = "health")]
    Health(HealthCommand),

    /// Check Kival server readiness.
    #[argx(name = "ready")]
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
#[derive(Debug, Args, Serialize)]
pub struct HealthCommand {
    /// Override the configured Kival server root URL.
    #[argx(long)]
    pub url: Option<Url>,
}

/// Arguments for `kival ready`.
#[derive(Debug, Args, Serialize)]
pub struct ReadyCommand {
    /// Override the configured Kival server root URL.
    #[argx(long)]
    pub url: Option<Url>,
}

#[argx(handler = run)]
impl HealthCommand {
    /// Run `kival health`.
    ///
    /// # Errors
    ///
    /// Returns an error if the server cannot be reached or the health response cannot be decoded.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<StatusResponse> {
        let mut config = config::load_client_config(&ctx)?;
        if let Some(url) = self.url {
            config.url = url;
        }
        let client = KivalClient::new(config.url)?;
        let health = client.health().await?;

        print_output(output, &health, || {
            println!("{}", health.status);
        })?;
        Ok(health)
    }
}

#[argx(handler = run)]
impl ReadyCommand {
    /// Run `kival ready`.
    ///
    /// # Errors
    ///
    /// Returns an error if the server cannot be reached, is not ready, or the readiness response
    /// cannot be decoded.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<StatusResponse> {
        let mut config = config::load_client_config(&ctx)?;
        if let Some(url) = self.url {
            config.url = url;
        }
        let client = KivalClient::new(config.url)?;
        let ready = client.ready().await?;

        print_output(output, &ready, || {
            println!("{}", ready.status);
        })?;
        Ok(ready)
    }
}
