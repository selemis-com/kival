//! Database connection configuration shared by `kivald` commands.

use eyre::{Result, eyre};
use kival_common::env::require_env_var;
use sqlx::postgres::PgConnectOptions;

/// Parses the operator-provided PostgreSQL connection URL.
///
/// The URL is intentionally read from the environment rather than the regular
/// Kival config file because it commonly contains database credentials.
///
/// # Errors
///
/// Returns an error when `DATABASE_URL` is missing or is not a valid PostgreSQL
/// connection URL.
pub(crate) fn connect_options_from_env() -> Result<PgConnectOptions> {
    let database_url = require_env_var("DATABASE_URL")?;
    database_url
        .parse()
        .map_err(|_| eyre!("DATABASE_URL must be a valid PostgreSQL connection URL"))
}
