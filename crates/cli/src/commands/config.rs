//! The `config` command for the Kival CLI.

use std::path::PathBuf;

use clap::Parser;
use kival_config::{ConfigDefaults, ConfigLayer, DEFAULT_CONFIG_FILENAME, load_config};
use kival_tracing::trace;
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::args::datadir::DatadirArgs;

/// Result type for the `config` command.
pub type Result<T> = std::result::Result<T, ConfigError>;

/// Loads configuration from disk and resolves it for a parsed command.
///
/// CLI and environment values already resolved by Clap take precedence over
/// values from the configuration file.
///
/// # Errors
///
/// Returns an error if command options cannot be converted to a config layer or
/// if the config file cannot be loaded.
pub fn load_config_for_command<T, P>(path: &PathBuf, command: &P) -> eyre::Result<T>
where
    T: Default + Serialize + DeserializeOwned + ConfigLayer,
    P: Serialize,
{
    let value = toml::Value::try_from(command)?;
    let command_config: T = value.try_into()?;
    Ok(command_config.merge(load_config::<T>(path)?))
}

/// Errors for the `config` command.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Config load errors (whatever `load_config` returns).
    ///
    /// We keep it boxed so we don’t couple the CLI error surface to the config crate’s exact type.
    #[error("failed to load config from {path:?}: {source}")]
    Load {
        /// The config path we attempted.
        path: PathBuf,
        /// Underlying error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// TOML serialization errors.
    #[error(transparent)]
    TomlSer(#[from] toml::ser::Error),

    /// I/O errors.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// The command arguments for the `config` command.
#[derive(Parser, Debug)]
pub struct ConfigCommand {
    /// The path to the configuration file to use.
    #[arg(long, value_name = "FILE", verbatim_doc_comment)]
    pub config: Option<PathBuf>,

    /// Show the default config
    #[arg(long, verbatim_doc_comment, conflicts_with = "config")]
    default: bool,

    /// The data directory to use.
    #[command(flatten)]
    pub(crate) datadir: DatadirArgs,
}

impl ConfigCommand {
    /// Execute the `config` command.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration cannot be loaded or serialized for output.
    pub async fn run<T>(&self) -> Result<()>
    where
        T: ConfigDefaults + ConfigLayer + Default + Serialize + DeserializeOwned,
    {
        let config = self.run_inner::<T>().await?.merge(T::with_defaults());
        println!("{}", toml::to_string_pretty(&config)?);
        Ok(())
    }

    /// Run the inner logic of the `config` command.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration file cannot be loaded or parsed.
    pub async fn run_inner<T>(&self) -> Result<T>
    where
        T: ConfigDefaults + ConfigLayer + Default + Serialize + DeserializeOwned,
    {
        let Self { config, default, datadir } = self;

        if *default {
            trace!("Using default configuration");
            return Ok(T::with_defaults());
        }

        let config_path = config.as_ref().map_or_else(
            || {
                trace!("No config file provided, using default location.");
                  datadir.resolve_path().join(DEFAULT_CONFIG_FILENAME)
            },
            |p| {
                trace!(
                    "Loading config from {p:?}, if not found, will create a new one with default configuration."
                );
                p.clone()
            },
        );

        load_config::<T>(&config_path)
            .map_err(|err| ConfigError::Load { path: config_path, source: err.into() })
    }
}
