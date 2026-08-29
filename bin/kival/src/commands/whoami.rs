//! API-key identity commands.

use argx::Args;
use eyre::Result;
use kival_cli::runner::CliContext;
use schemars::JsonSchema;
use serde::Serialize;
use uuid::Uuid;

use crate::utils::error::CliResult;
use crate::utils::{
    credentials::authenticated_client,
    output::{OutputMode, print_output, quote_human_string},
};

/// Arguments for `kival whoami`.
#[derive(Debug, Clone, Copy, Args)]
pub struct WhoamiCommand {}

/// Resolved API-key identity output.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WhoamiOutput {
    /// User associated with the resolved API key.
    user: WhoamiUserOutput,
}

/// User fields returned by `kival whoami`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
struct WhoamiUserOutput {
    /// Stable user identifier.
    id: Uuid,
    /// User display name.
    display_name: String,
    /// Username.
    username: String,
}

#[argx(handler = run)]
impl WhoamiCommand {
    /// Resolves the configured API key and fetches its user.
    ///
    /// # Errors
    ///
    /// Returns an error when API-key resolution or the identity request fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<WhoamiOutput> {
        let client = authenticated_client(&ctx)?;
        let user = client.whoami().await?;
        let value = WhoamiOutput {
            user: WhoamiUserOutput {
                id: user.id,
                display_name: user.display_name,
                username: user.username,
            },
        };

        print_output(output, &value, || {
            println!(
                "{} username={} display_name={}",
                value.user.id,
                quote_human_string(&value.user.username),
                quote_human_string(&value.user.display_name)
            );
        })?;
        Ok(value)
    }
}
