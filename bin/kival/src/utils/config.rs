//! Client configuration helpers.

use eyre::{Context, Result};
use kival_cli::{commands::config::load_config_for_command, runner::CliContext};
use kival_config::DEFAULT_CONFIG_FILENAME;
use serde::Serialize;

use crate::ClientConfig;

/// Loads the `kival` config for a specific command.
///
/// Command fields such as `--url` are merged over the config file, and built-in defaults are
/// resolved by the generated accessor methods on [`ClientConfig`].
///
/// # Errors
///
/// Returns an error if the command cannot be converted into a config layer or the config file
/// cannot be loaded.
pub fn load_client_config_for_command<P>(ctx: &CliContext, command: &P) -> Result<ClientConfig>
where
    P: Serialize,
{
    let config_path = ctx.datadir.join(DEFAULT_CONFIG_FILENAME);

    load_config_for_command::<ClientConfig, _>(&config_path, command)
        .wrap_err_with(|| format!("failed to load kival config `{}`", config_path.display()))
}
