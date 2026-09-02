//! Database connection configuration shared by `kivald` commands.

use std::path::Path;

use argx::{Dotenv, Environment};
use eyre::{Result, eyre};
use sqlx::postgres::PgConnectOptions;

use crate::DEFAULT_DOTENV_FILENAME;

/// Database configuration kept separate from the regular Kival config because the connection URL
/// commonly contains credentials.
#[derive(Debug, argx::Config)]
struct DatabaseConfig {
    /// PostgreSQL connection URL.
    #[argx(env = "DATABASE_URL")]
    database_url: Option<String>,
}

/// Resolves and parses the operator-provided PostgreSQL connection URL.
///
/// A local `.env` file participates when present, with the process environment taking precedence.
///
/// # Errors
///
/// Returns an error when `DATABASE_URL` is missing or is not a valid PostgreSQL connection URL, or
/// when the selected dotenv source cannot be loaded.
pub(crate) fn connect_options() -> Result<PgConnectOptions> {
    let mut loader = DatabaseConfig::loader();
    if Path::new(DEFAULT_DOTENV_FILENAME).try_exists()? {
        loader = loader.layer(Dotenv::new(DEFAULT_DOTENV_FILENAME));
    }
    let config = loader.layer(Environment).resolve()?;

    config
        .database_url
        .ok_or_else(|| eyre!("Missing required environment variable: DATABASE_URL"))?
        .parse()
        .map_err(|_| eyre!("DATABASE_URL must be a valid PostgreSQL connection URL"))
}
