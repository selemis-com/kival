//! File, stdin, and structured JSON input helpers for write commands.

use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    str::FromStr,
};

use argx::Args;
use eyre::Result;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::{Map, Value, error::Category, json};

use crate::utils::error::CliFailure;

/// File or standard-input source selected by a path-bearing CLI option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputPath {
    /// Read from a filesystem path.
    File(PathBuf),
    /// Read from standard input.
    Stdin,
}

impl std::fmt::Display for InputPath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(path) => write!(formatter, "{}", path.display()),
            Self::Stdin => formatter.write_str("-"),
        }
    }
}

impl FromStr for InputPath {
    type Err = std::convert::Infallible;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        Ok(if value == "-" { Self::Stdin } else { Self::File(PathBuf::from(value)) })
    }
}

/// Shared `--input` CLI option for commands that accept structured JSON input.
#[derive(Debug, Clone, Args)]
pub struct StructuredInputArgs {
    /// Read this command's structured JSON input from PATH, or from stdin when PATH is `-`.
    ///
    /// The accepted JSON properties are documented by the selected command. Cannot be combined
    /// with command-line fields represented by the structured input.
    #[argx(long)]
    pub input: Option<InputPath>,
}

/// Reads structured JSON input from a file or stdin.
///
/// # Errors
///
/// Returns a stable CLI input error when the input cannot be read, is malformed JSON, or does not
/// deserialize into the requested command input type.
pub fn read_json_input<T>(source: InputPath) -> Result<T>
where
    T: DeserializeOwned,
{
    read_input(source, |reader, path| decode_json(reader, path))
}

/// Reads UTF-8 text input from a file or stdin without normalising its contents.
///
/// # Errors
///
/// Returns a stable input-read error when the source cannot be opened, read, or decoded as UTF-8.
pub fn read_text_input(source: InputPath) -> Result<String> {
    read_input(source, |reader, path| decode_text(reader, path))
}

/// Opens a file or stdin once and delegates decoding to the supplied function.
///
/// # Errors
///
/// Returns an input-read error if the source cannot be opened, or propagates an error returned by
/// `decode`.
fn read_input<T>(
    source: InputPath,
    decode: impl FnOnce(&mut dyn std::io::Read, Option<&Path>) -> Result<T>,
) -> Result<T> {
    match source {
        InputPath::File(path) => {
            let file =
                File::open(&path).map_err(|_error| CliFailure::input_read_failed(Some(&path)))?;
            let mut reader = BufReader::new(file);
            decode(&mut reader, Some(&path))
        }
        InputPath::Stdin => {
            let stdin = std::io::stdin();
            let mut reader = BufReader::new(stdin.lock());
            decode(&mut reader, None)
        }
    }
}

/// Decodes UTF-8 text from `reader` without altering line endings or trailing whitespace.
///
/// # Errors
///
/// Returns a stable input-read error when reading fails or the input is not valid UTF-8.
fn decode_text(reader: &mut dyn std::io::Read, path: Option<&Path>) -> Result<String> {
    let mut value = String::new();
    reader.read_to_string(&mut value).map_err(|_error| CliFailure::input_read_failed(path))?;
    Ok(value)
}

/// Rejects `--input` combined with semantic command payload fields.
///
/// # Errors
///
/// Returns `input.conflicting_sources` when `input` is present and any payload field is present.
pub fn reject_conflicting_input(input: &Option<InputPath>, fields: &[(&str, bool)]) -> Result<()> {
    if input.is_none() {
        return Ok(());
    }

    let conflicts = fields
        .iter()
        .filter_map(|(field, present)| present.then_some(Value::String((*field).to_owned())))
        .collect::<Vec<_>>();

    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(CliFailure::input_conflicting_sources(&conflicts).into())
    }
}

/// Deserializes an optional nullable field while preserving explicit JSON null.
///
/// # Errors
///
/// Returns the underlying Serde error when the concrete value cannot deserialize as `T`.
pub fn deserialize_optional_nullable<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Deserializes an optional field that may be omitted but may not be null.
///
/// # Errors
///
/// Returns the underlying Serde error when the present value cannot deserialize as `T`.
pub fn deserialize_optional_non_null<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Builds stable details for an invalid structured input value.
#[must_use]
pub fn invalid_value_details(details: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Object(details.into_iter().map(|(key, value)| (key.to_owned(), value)).collect())
}

/// Builds stable details for an at-least-one input constraint.
#[must_use]
pub fn at_least_one_input_field(fields: &[&str]) -> Value {
    invalid_value_details([
        ("constraint", Value::String("at_least_one".to_owned())),
        (
            "fields",
            Value::Array(fields.iter().map(|field| Value::String((*field).to_owned())).collect()),
        ),
    ])
}

/// Decodes JSON and maps syntax/data/read failures to stable input error codes.
fn decode_json<T>(reader: impl std::io::Read, path: Option<&Path>) -> Result<T>
where
    T: DeserializeOwned,
{
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    let value = serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        let field_path = error.path().to_string();
        map_json_error(&error.into_inner(), path, field_path)
    })?;
    deserializer.end().map_err(|error| map_json_error(&error, path, String::new()))?;
    Ok(value)
}

/// Maps a `serde_json` error to a stable CLI input error.
fn map_json_error(
    error: &serde_json::Error,
    path: Option<&Path>,
    field_path: String,
) -> CliFailure {
    let details = error_details(error, field_path);
    match error.classify() {
        Category::Io => CliFailure::input_read_failed(path),
        Category::Data => CliFailure::input_invalid_value(details),
        Category::Syntax | Category::Eof => CliFailure::input_invalid_json(details),
    }
}

/// Builds stable JSON error details for a `serde_json` error and optional path.
fn error_details(error: &serde_json::Error, field_path: String) -> Value {
    let mut details = Map::new();
    details.insert("line".to_owned(), json!(error.line()));
    details.insert("column".to_owned(), json!(error.column()));
    if error.classify() == Category::Data && !field_path.is_empty() && field_path != "." {
        details.insert("path".to_owned(), Value::String(field_path));
    }
    Value::Object(details)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde::Deserialize;

    use super::*;
    use crate::utils::error::FailureCode;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    struct TestInput {
        title: String,
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    struct NestedInput {
        properties: NestedProperties,
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    struct NestedProperties {
        status: String,
    }

    struct ErrorReader;

    impl std::io::Read for ErrorReader {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("read failed"))
        }
    }

    #[test]
    fn input_path_parses_stdin_only_for_exact_dash() {
        assert_eq!(InputPath::from_str("-").unwrap(), InputPath::Stdin);
        assert_eq!(InputPath::from_str("--").unwrap(), InputPath::File("--".into()));
        assert_eq!(InputPath::from_str("payload").unwrap(), InputPath::File("payload".into()));
    }

    #[test]
    fn read_json_input_reads_file() {
        let path = temp_input_path("valid");
        fs::write(&path, r#"{"title":"Hello"}"#).unwrap();

        let input: TestInput = read_json_input(InputPath::File(path.clone())).unwrap();

        assert_eq!(input, TestInput { title: "Hello".to_owned() });
        let _ = fs::remove_file(path);
    }

    /// Verifies that Markdown input is read byte-for-byte as UTF-8 text.
    #[test]
    fn read_text_input_preserves_content_exactly() {
        let path = temp_input_path("markdown-exact");
        let expected = "# Héading\r\n\r\nbody with trailing spaces  \r\nno-final-newline";
        fs::write(&path, expected.as_bytes()).unwrap();

        let content = read_text_input(InputPath::File(path.clone())).unwrap();

        assert_eq!(content, expected);
        let _ = fs::remove_file(path);
    }

    /// Verifies that an empty Markdown source remains an explicitly empty body.
    #[test]
    fn read_text_input_preserves_empty_content() {
        let path = temp_input_path("markdown-empty");
        fs::write(&path, b"").unwrap();

        let content = read_text_input(InputPath::File(path.clone())).unwrap();

        assert!(content.is_empty());
        let _ = fs::remove_file(path);
    }

    /// Verifies that missing Markdown files use the stable input-read error.
    #[test]
    fn read_text_input_reports_missing_file() {
        let error = read_text_input(InputPath::File(temp_input_path("markdown-missing")))
            .unwrap_err()
            .downcast::<CliFailure>()
            .unwrap();

        assert_eq!(error.code, FailureCode::InputReadFailed);
    }

    /// Verifies that non-UTF-8 Markdown sources are rejected as unreadable input.
    #[test]
    fn read_text_input_rejects_invalid_utf8() {
        let path = temp_input_path("markdown-invalid-utf8");
        fs::write(&path, [0xff, 0xfe]).unwrap();

        let error = read_text_input(InputPath::File(path.clone()))
            .unwrap_err()
            .downcast::<CliFailure>()
            .unwrap();

        assert_eq!(error.code, FailureCode::InputReadFailed);
        let _ = fs::remove_file(path);
    }

    /// Verifies that reader failures while decoding Markdown use the stable input-read error.
    #[test]
    fn decode_text_reports_reader_io_errors_as_read_failed() {
        let mut reader = ErrorReader;
        let error = decode_text(&mut reader, None).unwrap_err().downcast::<CliFailure>().unwrap();

        assert_eq!(error.code, FailureCode::InputReadFailed);
    }

    #[test]
    fn read_json_input_reports_missing_file() {
        let error = read_json_input::<TestInput>(InputPath::File(temp_input_path("missing")))
            .unwrap_err()
            .downcast::<CliFailure>()
            .unwrap();

        assert_eq!(error.code, FailureCode::InputReadFailed);
    }

    #[test]
    fn decode_json_rejects_trailing_json_values() {
        let error =
            decode_json::<TestInput>(br#"{"title":"First"} {"title":"Second"}"#.as_slice(), None)
                .unwrap_err()
                .downcast::<CliFailure>()
                .unwrap();

        assert_eq!(error.code, FailureCode::InputInvalidJson);
    }

    #[test]
    fn decode_json_allows_trailing_whitespace() {
        let input: TestInput = decode_json(
            br#"{"title":"First"}
	"#
            .as_slice(),
            None,
        )
        .unwrap();

        assert_eq!(input, TestInput { title: "First".to_owned() });
    }

    #[test]
    fn read_json_input_reports_invalid_json() {
        let path = temp_input_path("invalid-json");
        fs::write(&path, "{").unwrap();

        let error = read_json_input::<TestInput>(InputPath::File(path.clone()))
            .unwrap_err()
            .downcast::<CliFailure>()
            .unwrap();

        assert_eq!(error.code, FailureCode::InputInvalidJson);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn read_json_input_reports_invalid_value() {
        let path = temp_input_path("invalid-value");
        fs::write(&path, r#"{"title":1}"#).unwrap();

        let error = read_json_input::<TestInput>(InputPath::File(path.clone()))
            .unwrap_err()
            .downcast::<CliFailure>()
            .unwrap();

        assert_eq!(error.code, FailureCode::InputInvalidValue);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn read_json_input_reports_invalid_nested_value_path() {
        let path = temp_input_path("invalid-nested-value");
        fs::write(&path, r#"{"properties":{"status":1}}"#).unwrap();

        let error = read_json_input::<NestedInput>(InputPath::File(path.clone()))
            .unwrap_err()
            .downcast::<CliFailure>()
            .unwrap();

        assert_eq!(error.code, FailureCode::InputInvalidValue);
        assert_eq!(error.details.unwrap()["path"], "properties.status");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn read_json_input_reports_unknown_field_path() {
        let path = temp_input_path("unknown-field");
        fs::write(&path, r#"{"properties":{"status":"ok","extra":true}}"#).unwrap();

        let error = read_json_input::<NestedInput>(InputPath::File(path.clone()))
            .unwrap_err()
            .downcast::<CliFailure>()
            .unwrap();

        assert_eq!(error.code, FailureCode::InputInvalidValue);
        assert_eq!(error.details.unwrap()["path"], "properties.extra");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn decode_json_reports_reader_io_errors_as_read_failed() {
        let error = decode_json::<TestInput>(ErrorReader, None)
            .unwrap_err()
            .downcast::<CliFailure>()
            .unwrap();

        assert_eq!(error.code, FailureCode::InputReadFailed);
    }

    #[test]
    fn at_least_one_input_field_reports_constraint_details() {
        let details = at_least_one_input_field(&["name", "description"]);

        assert_eq!(details["constraint"], "at_least_one");
        assert_eq!(details["fields"], serde_json::json!(["name", "description"]));
    }

    #[test]
    fn reject_conflicting_input_reports_payload_fields() {
        let error = reject_conflicting_input(
            &Some(InputPath::Stdin),
            &[("title", true), ("description", false)],
        )
        .unwrap_err()
        .downcast::<CliFailure>()
        .unwrap();

        assert_eq!(error.code, FailureCode::InputConflictingSources);
        assert_eq!(error.details.unwrap()["fields"], serde_json::json!(["title"]));
    }

    fn temp_input_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kival-input-{name}-{}.json", std::process::id()))
    }
}
