//! API-key identity commands.

use argx::{Args, argx};
use kival_cli::runner::CliContext;
use kival_sdk::WhoamiResponse;

use crate::utils::{
    credentials::authenticated_client,
    error::{CommandError, command_error_codes},
    output::{OutputMode, print_output, quote_human_string},
};

command_error_codes! {
    pub(crate) enum WhoamiErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
    }
}

/// Error returned by the corresponding command handler.
type WhoamiError = CommandError<WhoamiErrorCode>;

/// Arguments for `kival whoami`.
#[derive(Debug, Clone, Copy, Args)]
pub struct WhoamiCommand {}

#[argx(handler = run)]
impl WhoamiCommand {
    /// Resolves the configured API key and fetches its user.
    ///
    /// # Errors
    ///
    /// Returns an error when API-key resolution or the identity request fails.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> Result<WhoamiResponse, WhoamiError> {
        let client = authenticated_client(&ctx)?;
        let identity = client.whoami().await?;

        print_output(&output, &identity, || {
            println!(
                "{} username={} display_name={} global_admin={} can_manage_groups={} scopes={}",
                identity.user.id,
                quote_human_string(&identity.user.username),
                quote_human_string(&identity.user.display_name),
                identity.is_global_admin,
                identity.can_manage_groups,
                identity
                    .scopes
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|scope| scope.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            );
        })?;
        Ok(identity)
    }
}
