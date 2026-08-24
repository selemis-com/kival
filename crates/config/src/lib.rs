//! Configuration for Kival.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]

use std::{io::ErrorKind::NotFound, path::PathBuf};

use eyre::{Result, eyre};
use kival_common::fs;
use serde::{Serialize, de::DeserializeOwned};

mod layer;

pub use layer::{ConfigDefaults, ConfigLayer};

/// The default filename for Kival configuration files.
pub const DEFAULT_CONFIG_FILENAME: &str = "kival.toml";

/// Loads configuration from disk, or creates a new file with defaults when missing.
///
/// Environment placeholders in the TOML payload are expanded before parsing using `${VAR_NAME}`
/// syntax.
///
/// # Type parameters
/// * `T`: The type of the configuration struct. Must implement `Default`, `Serialize`, and
///   `DeserializeOwned`.
///
/// # Arguments
/// * `path`: The path to the configuration file on disk.
///
/// # Returns
/// A `Result` containing the loaded configuration struct, or an error if loading or parsing fails.
///
/// # Errors
///
/// Returns an error if the config file cannot be read, created, written, serialized,
/// deserialized, or if an environment placeholder cannot be resolved.
pub fn load_config<T>(path: &PathBuf) -> Result<T>
where
    T: Default + Serialize + DeserializeOwned,
{
    match fs::read_to_string(path) {
        Ok(cfg_string) => {
            let cfg_string = replace_env_vars(&cfg_string)?;
            toml::from_str(&cfg_string)
                .map_err(|e| eyre!("failed to parse TOML at {}: {e}", path.display()))
        }
        Err(fs::FsPathError::Read { source, .. }) if source.kind() == NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let config = T::default();
            let contents = toml::to_string_pretty(&config)?;
            fs::write(path, contents)?;
            Ok(config)
        }
        Err(e) => Err(e.into()),
    }
}

/// Replaces `${VAR_NAME}` placeholders in the input string with the corresponding environment
/// variable values. Returns an error if a placeholder is malformed or if an environment variable is
/// not set.
fn replace_env_vars(input: &str) -> Result<String> {
    let mut result = String::with_capacity(input.len());
    let mut remainder = input;

    while let Some((before, after)) = remainder.split_once("${") {
        result.push_str(before);

        // An opening `${` without a closing `}` is malformed; surface it
        // instead of silently passing the input through.
        let (key, rest) =
            after.split_once('}').ok_or_else(|| eyre!("unterminated `${{...}}` placeholder"))?;

        // Treat a missing (or non-UTF-8) env var as a hard error so config
        // typos don't silently produce a broken value downstream.
        let value =
            std::env::var(key).map_err(|_| eyre!("environment variable `{key}` is not set"))?;
        result.push_str(&value);

        remainder = rest;
    }

    result.push_str(remainder);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::env;

    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    use super::*;

    #[derive(Debug, Default, Serialize, Deserialize)]
    struct TestConfig {
        key: String,
        value: i32,
    }

    /// Test that environment variables are replaced correctly.
    #[test]
    fn replace_env_vars_works() -> Result<()> {
        let input = r#"
            key = "${TEST_KEY_REPLACE}"
            value = ${TEST_VALUE_REPLACE}
        "#;
        let expected = r#"
            key = "env_value"
            value = 42
        "#;

        // SAFETY: This is required to set the environment variable for the test.
        unsafe {
            env::set_var("TEST_KEY_REPLACE", "env_value");
            env::set_var("TEST_VALUE_REPLACE", "42");
        };

        let output = replace_env_vars(input)?;
        assert_eq!(output.trim(), expected.trim());

        // SAFETY: This is required to unset the environment variable for the test.
        unsafe {
            env::remove_var("TEST_KEY_REPLACE");
            env::remove_var("TEST_VALUE_REPLACE");
        };

        Ok(())
    }

    /// Test that unset environment variables return an error.
    #[test]
    fn replace_env_vars_unset_var_errors() {
        let input = r#"
            key = "${TEST_KEY}"
            value = ${TEST_VALUE
        "#;

        let err = replace_env_vars(input).expect_err("unset env var should error");
        assert!(err.to_string().contains("environment variable `TEST_KEY` is not set"));
    }

    /// Test that unclosed environment variable syntax returns an error.
    #[test]
    fn replace_env_vars_unclosed_placeholder_errors() {
        let input = r#"
            key = "${TEST_KEY"
            value = ${TEST_VALUE
        "#;

        let err = replace_env_vars(input).expect_err("unterminated placeholder should error");
        assert!(err.to_string().contains("unterminated `${...}` placeholder"));
    }

    /// Test that loading a config from a non-existent file creates it with default values.
    #[test]
    fn load_config_write_to_disk() {
        let base = TempDir::new().unwrap();
        let nested = base.path().join("nested");
        let path = nested.join("test_config.toml");

        assert!(!path.exists());

        let config = load_config::<TestConfig>(&path);
        assert!(config.is_ok());

        let config = config.unwrap();
        assert_eq!(config.key, "");
        assert_eq!(config.value, 0);

        assert!(path.exists());

        // Test that the file contains the default values
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("key = \"\""));
        assert!(contents.contains("value = 0"));
    }

    /// Test that loading a config from disk works correctly.
    #[test]
    fn load_config_read_from_disk() {
        let base = TempDir::new().unwrap();
        let nested = base.path().join("nested");
        let path = nested.join("test_config.toml");

        assert!(!path.exists());

        // Create a config file with some values
        let contents = r#"
            key = "test_key"
            value = 42
        "#;
        fs::create_dir_all(&nested).unwrap();
        fs::write(&path, contents).unwrap();

        let config = load_config::<TestConfig>(&path);
        assert!(config.is_ok());

        let config = config.unwrap();
        assert_eq!(config.key, "test_key");
        assert_eq!(config.value, 42);
    }

    /// Test that environment variables in the config file are replaced correctly.
    #[test]
    fn load_config_with_env_vars() {
        let base = TempDir::new().unwrap();
        let nested = base.path().join("nested");
        let path = nested.join("test_config.toml");

        assert!(!path.exists());

        // SAFETY: This is required to set the environment variable for the test.
        unsafe {
            env::set_var("TEST_KEY_CONFIG", "env_value");
            env::set_var("TEST_VALUE_CONFIG", "42");
        };

        // Create a config file with an environment variable
        let contents = r#"
            key = "${TEST_KEY_CONFIG}"
            value = ${TEST_VALUE_CONFIG}
        "#;
        fs::create_dir_all(&nested).unwrap();
        fs::write(&path, contents).unwrap();

        let config = load_config::<TestConfig>(&path);
        assert!(config.is_ok());

        let config = config.unwrap();
        assert_eq!(config.key, "env_value");
        assert_eq!(config.value, 42);

        // SAFETY: This is required to unset the environment variable for the test.
        unsafe {
            env::remove_var("TEST_KEY_CONFIG");
            env::remove_var("TEST_VALUE_CONFIG");
        };
    }
}
