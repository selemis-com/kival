//! Client configuration helpers.

use eyre::{Context, Result};
use kival_cli::{
    commands::config::{DEFAULT_CONFIG_FILENAME, load_config},
    runner::CliContext,
};

use crate::ClientConfig;

/// Loads effective client configuration.
///
/// # Errors
///
/// Returns an error if the configuration file or environment cannot be resolved.
pub fn load_client_config(ctx: &CliContext) -> Result<ClientConfig> {
    let config_path = ctx.datadir.join(DEFAULT_CONFIG_FILENAME);
    load_config::<ClientConfig>(&config_path)
        .wrap_err_with(|| format!("failed to load kival config `{}`", config_path.display()))
}
