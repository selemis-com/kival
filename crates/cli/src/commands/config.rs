//! Shared configuration loading and the `config` command.

use std::path::{Path, PathBuf};

use argx::{Args, ConfigLoader, Defaults, Environment, Toml, config::Config};
use kival_tracing::trace;
use serde::Serialize;
use thiserror::Error;

use crate::args::datadir::DatadirArgs;

/// The default filename for Kival configuration files.
pub const DEFAULT_CONFIG_FILENAME: &str = "kival.toml";

/// Result type for the `config` command.
pub type Result<T> = std::result::Result<T, ConfigError>;

/// Loads effective configuration from defaults, an optional TOML file, and the process environment.
///
/// The configuration file is optional. When present, it overrides declared defaults. Environment
/// values have the highest precedence of these shared sources. Command-specific argv overrides are
/// applied by the binary after parsing so subcommand-local options remain part of the command tree.
///
/// # Errors
///
/// Returns an error if Argx cannot read, interpolate, parse, or resolve the configuration.
pub fn load_config<T>(path: &Path) -> Result<T>
where
    T: Config,
{
    let loader = ConfigLoader::<T>::default().layer(Defaults);
    let loader = if path.exists() { loader.layer(Toml::new(path)) } else { loader };

    loader.layer(Environment).resolve().map_err(ConfigError::Argx)
}

/// Errors for the `config` command.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Argx configuration loading or resolution failed.
    #[error(transparent)]
    Argx(#[from] argx::ConfigError),

    /// TOML serialization failed.
    #[error(transparent)]
    TomlSer(#[from] toml::ser::Error),
}

/// The command arguments for the `config` command.
#[derive(Args, Debug)]
pub struct ConfigCommand {
    /// The path to the configuration file to use.
    #[argx(long)]
    pub config: Option<PathBuf>,

    /// Show only declared defaults.
    #[argx(long, conflicts = "config")]
    default: bool,

    /// The data directory to use.
    #[argx(flatten)]
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
        T: Config + Serialize,
    {
        let config = self.run_inner::<T>().await?;
        println!("{}", toml::to_string_pretty(&config)?);
        Ok(())
    }

    /// Resolve the configuration selected by this command.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration cannot be loaded or resolved.
    pub async fn run_inner<T>(&self) -> Result<T>
    where
        T: Config,
    {
        let Self { config, default, datadir } = self;

        if *default {
            trace!("Using declared configuration defaults");
            return ConfigLoader::<T>::default()
                .layer(Defaults)
                .resolve()
                .map_err(ConfigError::Argx);
        }

        let config_path = config.as_ref().cloned().unwrap_or_else(|| {
            trace!("No config file provided, using default location");
            datadir.resolve_path().join(DEFAULT_CONFIG_FILENAME)
        });

        trace!(path = %config_path.display(), "Loading configuration");
        load_config(&config_path)
    }
}
