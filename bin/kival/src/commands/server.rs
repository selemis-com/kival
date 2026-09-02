//! Public commands.

use argx::{Args, Subcommand, argx};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{KivalClient, StatusResponse};
use serde::Serialize;
use url::Url;

use crate::utils::{
    config,
    error::{CommandError, command_error_codes, erase_command_error},
    output::{OutputMode, print_output},
};

command_error_codes! {
    pub(crate) enum ServerStatusErrorCode {
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
    }
}

/// Error returned by the corresponding command handler.
type ServerStatusError = CommandError<ServerStatusErrorCode>;

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
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
        })
    )]
    Health(HealthCommand),

    /// Check Kival server readiness.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
        })
    )]
    Ready(ReadyCommand),
}

impl ServerCommand {
    /// Execute the selected server command.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected command fails.
    pub(crate) async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            ServerSubcommand::Health(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            ServerSubcommand::Ready(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
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
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<StatusResponse, ServerStatusError> {
        let mut config = config::load_client_config(&ctx)?;
        if let Some(url) = self.url {
            config.url = url;
        }
        let client = KivalClient::new(config.url)?;
        let health = client.health().await?;

        print_output(&output, &health, || {
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
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<StatusResponse, ServerStatusError> {
        let mut config = config::load_client_config(&ctx)?;
        if let Some(url) = self.url {
            config.url = url;
        }
        let client = KivalClient::new(config.url)?;
        let ready = client.ready().await?;

        print_output(&output, &ready, || {
            println!("{}", ready.status);
        })?;
        Ok(ready)
    }
}
