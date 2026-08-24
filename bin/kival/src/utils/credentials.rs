//! Deterministic API-key credential resolution for `kival`.

use std::sync::OnceLock;

use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::KivalClient;
use serde::Serialize;
use url::Url;

use crate::utils::{config::load_client_config_for_command, error::CliError};

/// Environment variable used for ephemeral API-key overrides.
const API_KEY_ENV: &str = "KIVAL_API_KEY";

/// Top-level command-line credential overrides.
#[derive(Debug)]
struct CommandOverrides {
    /// Explicit API key, when supplied.
    api_key: Option<String>,
    /// Explicit Kival server URL, when supplied.
    url: Option<Url>,
}

/// Process-wide command overrides initialized before async command dispatch.
static COMMAND_OVERRIDES: OnceLock<CommandOverrides> = OnceLock::new();

/// Empty command configuration layer used to load only file/default values.
#[derive(Debug, Clone, Copy, Serialize)]
struct EmptyOverlay {}

/// Records top-level command-line overrides once, before command dispatch.
///
/// # Errors
///
/// Returns an error for ambiguous key values or repeated initialization.
pub fn set_command_overrides(api_key: Option<String>, url: Option<Url>) -> Result<()> {
    if let Some(api_key) = api_key.as_deref() {
        validate_api_key(api_key, "--api-key")?;
    }
    COMMAND_OVERRIDES
        .set(CommandOverrides { api_key, url })
        .map_err(|_| CliError::invalid_argument("credential overrides were already initialized"))?;
    Ok(())
}

/// Creates an API-key-authenticated client using flag, environment, then config precedence.
///
/// # Errors
///
/// Returns an error when no key is available or configuration is invalid.
pub fn authenticated_client(ctx: &CliContext) -> Result<KivalClient> {
    let config = load_client_config_for_command::<EmptyOverlay>(ctx, &EmptyOverlay {})?;
    let overrides = COMMAND_OVERRIDES.get();
    let api_key = if let Some(value) = overrides.and_then(|value| value.api_key.clone()) {
        value
    } else if let Some(value) = api_key_from_environment()? {
        value
    } else {
        config.api_key().ok_or_else(|| {
            CliError::invalid_argument(
                "no API key configured; use --api-key, KIVAL_API_KEY, or set api_key in the kival config",
            )
        })?
    };
    validate_api_key(&api_key, "configured API key")?;
    let url = overrides.and_then(|value| value.url.clone()).unwrap_or_else(|| config.url());
    Ok(KivalClient::new(url)?.with_api_key(api_key))
}

/// Reads and validates the optional environment API key.
fn api_key_from_environment() -> Result<Option<String>> {
    match std::env::var(API_KEY_ENV) {
        Ok(value) => {
            validate_api_key(&value, API_KEY_ENV)?;
            Ok(Some(value))
        }
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(CliError::invalid_argument("KIVAL_API_KEY must contain valid UTF-8").into())
        }
    }
}

/// Rejects empty or whitespace-padded bearer credentials.
fn validate_api_key(value: &str, source: &str) -> Result<()> {
    if value.is_empty() {
        return Err(CliError::invalid_argument(format!("{source} must not be empty")).into());
    }
    if value.trim() != value {
        return Err(CliError::invalid_argument(format!(
            "{source} must not start or end with whitespace"
        ))
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ambiguous_api_key_values() {
        assert!(validate_api_key("", "test").is_err());
        assert!(validate_api_key(" leading", "test").is_err());
        assert!(validate_api_key("trailing ", "test").is_err());
        assert!(validate_api_key("kvl_valid", "test").is_ok());
    }
}
