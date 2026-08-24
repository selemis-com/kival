//! Environment variable utilities for Kival.

use eyre::{Result, eyre};

/// Retrieves the value of an environment variable, returning an error if it is not set.
///
/// # Errors
///
/// Returns an error if the environment variable is not set or cannot be read.
pub fn require_env_var(key: &str) -> Result<String> {
    std::env::var(key).map_err(|_| eyre!("Missing required environment variable: {key}"))
}
