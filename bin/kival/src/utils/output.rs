//! Output helpers for the `kival` CLI.

use eyre::Result;
use serde::Serialize;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Connector for a non-final item in a tree branch.
pub(crate) const TREE_BRANCH: &str = "├─";
/// Connector for the final item in a tree branch.
pub(crate) const TREE_LAST: &str = "└─";

/// Output format for command results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, argx::ValueEnum)]
pub enum OutputMode {
    /// Human-readable text.
    Text,
    /// JSON.
    Json,
}

/// Prints a serializable value as JSON.
///
/// # Errors
///
/// Returns an error if the value cannot be serialized.
pub fn print_json<T>(value: &T) -> Result<()>
where
    T: Serialize,
{
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Prints either human-readable text or JSON.
///
/// The `text` closure is evaluated only in text mode.
///
/// # Errors
///
/// Returns an error if the value cannot be serialized as JSON.
pub fn print_output<T, F>(mode: &OutputMode, value: &T, text: F) -> Result<()>
where
    T: Serialize,
    F: FnOnce(),
{
    match mode {
        OutputMode::Text => {
            text();
            Ok(())
        }
        OutputMode::Json => print_json(value),
    }
}

/// Prints a standard empty-list message for human-readable list commands.
pub fn print_empty_list(resource: &str) {
    println!("No {resource} found");
}

/// Returns the connector for an item at `index` in a branch of length `len`.
#[must_use]
pub(crate) const fn tree_connector(index: usize, len: usize) -> &'static str {
    if index + 1 == len { TREE_LAST } else { TREE_BRANCH }
}

/// Prints an empty root-level tree branch.
pub(crate) fn print_tree_none() {
    println!("{TREE_LAST} none");
}

/// Prints an empty tree branch with a caller-provided prefix.
pub(crate) fn print_indented_tree_none(prefix: &str) {
    println!("{prefix}{TREE_LAST} none");
}

/// Formats a timestamp for compact human-readable output.
#[must_use]
pub fn format_human_timestamp(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Formats user-controlled text as a quoted single-line string value.
#[must_use]
pub fn quote_human_string(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');

    for character in value.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character if character.is_control() => quoted.push(' '),
            character => quoted.push(character),
        }
    }

    quoted.push('"');
    quoted
}

/// Adds a UUID field to compact human output when it is present.
pub(crate) fn push_optional_uuid_field(fields: &mut Vec<String>, name: &str, value: Option<Uuid>) {
    if let Some(value) = value {
        fields.push(format!("{name}={value}"));
    }
}
