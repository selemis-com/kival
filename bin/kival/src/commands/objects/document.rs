//! External-editor projection for versioned Kival object content.

use eyre::Result;
use serde_json::{Map, Value};

use crate::utils::error::CliError;

/// Reserved top-level front-matter field containing the version title.
const TITLE_FIELD: &str = "title";
/// Reserved top-level front-matter field containing user-defined metadata.
const METADATA_FIELD: &str = "metadata";
/// Indentation used for metadata members.
const METADATA_INDENT: &str = "  ";
/// Indentation and marker used for metadata list members.
const LIST_ITEM_PREFIX: &str = "    - ";

/// Editable object state parsed from an external-editor document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObjectDocument {
    /// Editable version title.
    pub title: String,
    /// Editable flat metadata.
    pub metadata: Map<String, Value>,
    /// Editable Markdown body.
    pub body: String,
}

/// Renders editable object state as Markdown with deterministic YAML front matter.
///
/// The top-level namespace mirrors Kival's version model: `title` is a string and `metadata` is a
/// mapping. Metadata remains flat, but one-dimensional scalar arrays are rendered using ordinary
/// indented YAML list items. Scalar strings are quoted as JSON strings, which are also valid YAML
/// and preserve their exact type without implicit conversions.
#[must_use]
pub(super) fn render_object_document(document: &ObjectDocument) -> String {
    let mut output = String::new();
    output.push_str("---\n");
    output.push_str(TITLE_FIELD);
    output.push_str(": ");
    output.push_str(&Value::String(document.title.clone()).to_string());
    output.push('\n');

    if document.metadata.is_empty() {
        output.push_str("metadata: {}\n");
    } else {
        output.push_str("metadata:\n");
        let mut metadata = document.metadata.iter().collect::<Vec<_>>();
        metadata.sort_by_key(|(left, _)| *left);
        for (key, value) in metadata {
            push_metadata_member(&mut output, key, value);
        }
    }

    output.push_str("---\n\n");
    output.push_str(&document.body);
    output
}

/// Parses Kival's constrained YAML object-document format.
///
/// # Errors
///
/// Returns an invalid-argument error when front matter is absent or malformed, required fields
/// are absent or duplicated, metadata is not a flat mapping, or metadata contains unsupported
/// nested values.
pub(super) fn parse_object_document(input: &str) -> Result<ObjectDocument> {
    let (front_matter, body) = split_front_matter(input)?;
    let lines = front_matter.lines().enumerate().collect::<Vec<_>>();
    let mut index = 0;
    let mut title = None;
    let mut metadata = None;

    while index < lines.len() {
        let (line_index, line) = lines[index];
        let line_number = line_index + 2;
        index += 1;

        if line.trim().is_empty() {
            continue;
        }
        reject_tab_indentation(line, line_number)?;
        if line.chars().next().is_some_and(char::is_whitespace) {
            return Err(CliError::invalid_argument(format!(
                "unexpected indentation in object front matter on line {line_number}"
            ))
            .into());
        }

        let (encoded_key, encoded_value) = split_front_matter_field(line, line_number)?;
        let key = parse_front_matter_key(encoded_key, line_number)?;
        let value = encoded_value.trim();

        match key.as_str() {
            TITLE_FIELD => {
                if title.is_some() {
                    return Err(CliError::invalid_argument(
                        "duplicate object front-matter field `title`",
                    )
                    .into());
                }
                title = Some(parse_title(value, line_number)?);
            }
            METADATA_FIELD => {
                if metadata.is_some() {
                    return Err(CliError::invalid_argument(
                        "duplicate object front-matter field `metadata`",
                    )
                    .into());
                }
                metadata = Some(if value == "{}" {
                    Map::new()
                } else if value.is_empty() {
                    parse_metadata_block(&lines, &mut index)?
                } else {
                    return Err(CliError::invalid_argument(format!(
                        "object front-matter field `metadata` on line {line_number} must be an indented mapping or `{{}}`"
                    ))
                    .into());
                });
            }
            _ => {
                return Err(CliError::invalid_argument(format!(
                    "unknown object front-matter field {key:?} on line {line_number}; expected `title` or `metadata`"
                ))
                .into());
            }
        }
    }

    let title = title.ok_or_else(|| {
        CliError::invalid_argument("missing required object front-matter field `title`")
    })?;
    let metadata = metadata.ok_or_else(|| {
        CliError::invalid_argument("missing required object front-matter field `metadata`")
    })?;

    Ok(ObjectDocument { title, metadata, body: body.to_owned() })
}

/// Appends one metadata member using nested YAML mapping and list syntax.
fn push_metadata_member(output: &mut String, key: &str, value: &Value) {
    output.push_str(METADATA_INDENT);
    output.push_str(&render_front_matter_key(key));

    if let Value::Array(values) = value {
        if values.is_empty() {
            output.push_str(": []\n");
            return;
        }

        output.push_str(":\n");
        for value in values {
            output.push_str(LIST_ITEM_PREFIX);
            output.push_str(&render_scalar(value));
            output.push('\n');
        }
        return;
    }

    output.push_str(": ");
    output.push_str(&render_scalar(value));
    output.push('\n');
}

/// Renders one JSON scalar as valid, type-stable YAML.
fn render_scalar(value: &Value) -> String {
    debug_assert!(!matches!(value, Value::Array(_) | Value::Object(_)));
    value.to_string()
}

/// Parses the nested `metadata` mapping and advances `index` to the next top-level field.
fn parse_metadata_block(lines: &[(usize, &str)], index: &mut usize) -> Result<Map<String, Value>> {
    let mut metadata = Map::new();

    while *index < lines.len() {
        let (line_index, line) = lines[*index];
        let line_number = line_index + 2;

        if line.trim().is_empty() {
            *index += 1;
            continue;
        }
        reject_tab_indentation(line, line_number)?;
        if !line.chars().next().is_some_and(char::is_whitespace) {
            break;
        }
        let Some(member) = line.strip_prefix(METADATA_INDENT) else {
            return Err(CliError::invalid_argument(format!(
                "metadata field on line {line_number} must use exactly two spaces of indentation"
            ))
            .into());
        };
        if member.chars().next().is_some_and(char::is_whitespace) {
            return Err(CliError::invalid_argument(format!(
                "metadata field on line {line_number} must use exactly two spaces of indentation"
            ))
            .into());
        }

        let (encoded_key, encoded_value) = split_front_matter_field(member, line_number)?;
        let key = parse_front_matter_key(encoded_key, line_number)?;
        let encoded_value = encoded_value.trim();
        *index += 1;

        let value = if encoded_value.is_empty() {
            parse_metadata_list(lines, index, &key, line_number)?
        } else {
            parse_metadata_value(encoded_value, &key, line_number)?
        };

        if metadata.insert(key.clone(), value).is_some() {
            return Err(CliError::invalid_argument(format!(
                "duplicate object metadata field {key:?}"
            ))
            .into());
        }
    }

    Ok(metadata)
}

/// Parses an indented scalar list belonging to one metadata key.
fn parse_metadata_list(
    lines: &[(usize, &str)],
    index: &mut usize,
    key: &str,
    key_line_number: usize,
) -> Result<Value> {
    let mut values = Vec::new();

    while *index < lines.len() {
        let (line_index, line) = lines[*index];
        let line_number = line_index + 2;

        if line.trim().is_empty() {
            *index += 1;
            continue;
        }
        reject_tab_indentation(line, line_number)?;
        if !line.starts_with(LIST_ITEM_PREFIX) {
            break;
        }

        let encoded = line[LIST_ITEM_PREFIX.len()..].trim();
        if encoded.is_empty() {
            return Err(CliError::invalid_argument(format!(
                "metadata list item for {key:?} on line {line_number} must contain a scalar value"
            ))
            .into());
        }
        values.push(parse_metadata_scalar(encoded, key, line_number)?);
        *index += 1;
    }

    if values.is_empty() {
        return Err(CliError::invalid_argument(format!(
            "metadata field {key:?} on line {key_line_number} must contain a scalar value or an indented scalar list; nested mappings are not supported"
        ))
        .into());
    }

    Ok(Value::Array(values))
}

/// Parses one metadata scalar or inline scalar array.
fn parse_metadata_value(encoded: &str, key: &str, line_number: usize) -> Result<Value> {
    if encoded == "[]" {
        return Ok(Value::Array(Vec::new()));
    }
    if encoded.starts_with('[') {
        let value: Value = serde_json::from_str(encoded).map_err(|error| {
            CliError::invalid_argument(format!(
                "invalid inline metadata array for {key:?} on line {line_number}: {error}"
            ))
        })?;
        let Value::Array(values) = value else {
            unreachable!("JSON values beginning with `[` parse as arrays");
        };
        if values.iter().any(|value| matches!(value, Value::Array(_) | Value::Object(_))) {
            return Err(CliError::invalid_argument(format!(
                "metadata field {key:?} on line {line_number} must contain only scalar list items"
            ))
            .into());
        }
        return Ok(Value::Array(values));
    }
    if encoded.starts_with('{') {
        return Err(CliError::invalid_argument(format!(
            "metadata field {key:?} on line {line_number} must not contain a nested mapping"
        ))
        .into());
    }

    parse_metadata_scalar(encoded, key, line_number)
}

/// Parses one YAML-compatible scalar while preserving explicit JSON scalar types.
fn parse_metadata_scalar(encoded: &str, key: &str, line_number: usize) -> Result<Value> {
    if encoded.starts_with('"') {
        let value: Value = serde_json::from_str(encoded).map_err(|error| {
            CliError::invalid_argument(format!(
                "invalid quoted metadata string for {key:?} on line {line_number}: {error}"
            ))
        })?;
        let Some(value) = value.as_str() else {
            return Err(CliError::invalid_argument(format!(
                "metadata value for {key:?} on line {line_number} must be a scalar"
            ))
            .into());
        };
        return Ok(Value::String(value.to_owned()));
    }

    match encoded {
        "null" => return Ok(Value::Null),
        "true" => return Ok(Value::Bool(true)),
        "false" => return Ok(Value::Bool(false)),
        _ => {}
    }

    if let Ok(Value::Number(number)) = serde_json::from_str::<Value>(encoded) {
        return Ok(Value::Number(number));
    }

    if encoded.starts_with('-') && encoded.len() == 1 {
        return Err(CliError::invalid_argument(format!(
            "metadata value for {key:?} on line {line_number} must not be an empty list marker"
        ))
        .into());
    }

    Ok(Value::String(encoded.to_owned()))
}

/// Parses the top-level title as a quoted or plain YAML string.
fn parse_title(encoded: &str, line_number: usize) -> Result<String> {
    if encoded.is_empty() {
        return Err(CliError::invalid_argument(format!(
            "object front-matter field `title` on line {line_number} must not be empty"
        ))
        .into());
    }
    if encoded.starts_with('"') {
        let value: Value = serde_json::from_str(encoded).map_err(|error| {
            CliError::invalid_argument(format!(
                "invalid quoted object title on line {line_number}: {error}"
            ))
        })?;
        return value.as_str().map(str::to_owned).ok_or_else(|| {
            CliError::invalid_argument(format!(
                "object front-matter field `title` on line {line_number} must be a string"
            ))
            .into()
        });
    }

    Ok(encoded.to_owned())
}

/// Returns a plain key when safe and a JSON-quoted key otherwise.
fn render_front_matter_key(key: &str) -> String {
    if is_plain_front_matter_key(key) {
        key.to_owned()
    } else {
        Value::String(key.to_owned()).to_string()
    }
}

/// Returns whether a key is unambiguous in Kival's constrained front-matter dialect.
fn is_plain_front_matter_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

/// Splits one front-matter field at the first colon outside a JSON-quoted key.
fn split_front_matter_field(line: &str, line_number: usize) -> Result<(&str, &str)> {
    let mut quoted = false;
    let mut escaped = false;

    for (index, character) in line.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
        } else if character == '"' {
            quoted = true;
        } else if character == ':' {
            return Ok((&line[..index], &line[index + character.len_utf8()..]));
        }
    }

    Err(CliError::invalid_argument(format!(
        "invalid object front matter on line {line_number}: expected `key: value`"
    ))
    .into())
}

/// Parses one plain or JSON-quoted front-matter key.
fn parse_front_matter_key(encoded: &str, line_number: usize) -> Result<String> {
    let encoded = encoded.trim();
    if encoded.starts_with('"') {
        let value: Value = serde_json::from_str(encoded).map_err(|error| {
            CliError::invalid_argument(format!(
                "invalid quoted object front-matter key on line {line_number}: {error}"
            ))
        })?;
        return value.as_str().map(str::to_owned).ok_or_else(|| {
            CliError::invalid_argument(format!(
                "object front-matter key on line {line_number} must be a string"
            ))
            .into()
        });
    }

    if is_plain_front_matter_key(encoded) {
        Ok(encoded.to_owned())
    } else {
        Err(CliError::invalid_argument(format!(
            "invalid object front-matter key {encoded:?} on line {line_number}; quote keys containing spaces or punctuation"
        ))
        .into())
    }
}

/// Rejects tab indentation, which is not valid in YAML mappings.
fn reject_tab_indentation(line: &str, line_number: usize) -> Result<()> {
    if line
        .chars()
        .take_while(|character| character.is_whitespace())
        .any(|character| character == '\t')
    {
        return Err(CliError::invalid_argument(format!(
            "object front matter on line {line_number} must use spaces, not tabs, for indentation"
        ))
        .into());
    }
    Ok(())
}

/// Splits the opening front-matter block from the exact Markdown body.
fn split_front_matter(input: &str) -> Result<(&str, &str)> {
    let (remainder, newline) = if let Some(remainder) = input.strip_prefix("---\r\n") {
        (remainder, "\r\n")
    } else if let Some(remainder) = input.strip_prefix("---\n") {
        (remainder, "\n")
    } else {
        return Err(CliError::invalid_argument(
            "edited object must begin with a `---` front-matter line",
        )
        .into());
    };

    let delimiter = format!("{newline}---{newline}");
    let end = remainder.find(&delimiter).ok_or_else(|| {
        CliError::invalid_argument("edited object front matter must end with a `---` line")
    })?;
    let body = &remainder[end + delimiter.len()..];
    let body = body.strip_prefix(newline).unwrap_or(body);
    Ok((&remainder[..end], body))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Verifies nested metadata front matter preserves typed values and exact Markdown.
    #[test]
    fn document_round_trip_preserves_body_and_values() {
        let document = ObjectDocument {
            title: "A title: with punctuation".to_owned(),
            metadata: json!({
                "status": "draft",
                "priority": 2,
                "explored": true,
                "aliases": ["one", 2, true, null],
            })
            .as_object()
            .unwrap()
            .clone(),
            body: "# Heading\n\nBody without final newline".to_owned(),
        };

        let rendered = render_object_document(&document);
        assert!(rendered.contains("title: \"A title: with punctuation\"\n"));
        assert!(rendered.contains("metadata:\n"));
        assert!(rendered.contains("  status: \"draft\"\n"));
        assert!(rendered.contains("  priority: 2\n"));
        assert!(rendered.contains("  aliases:\n    - \"one\"\n    - 2\n    - true\n    - null\n"));
        assert_eq!(parse_object_document(&rendered).unwrap(), document);
    }

    /// Verifies metadata keys cannot collide with reserved top-level fields.
    #[test]
    fn document_round_trip_preserves_reserved_names_inside_metadata() {
        let document = ObjectDocument {
            title: "Title".to_owned(),
            metadata: json!({
                "title": "metadata title",
                "metadata": "metadata member",
            })
            .as_object()
            .unwrap()
            .clone(),
            body: String::new(),
        };

        let rendered = render_object_document(&document);
        assert!(rendered.contains("metadata:\n  metadata: \"metadata member\"\n"));
        assert!(rendered.contains("  title: \"metadata title\"\n"));
        assert_eq!(parse_object_document(&rendered).unwrap(), document);
    }

    /// Verifies arbitrary metadata keys are quoted and retain their exact spelling.
    #[test]
    fn document_round_trip_preserves_quoted_metadata_keys() {
        let document = ObjectDocument {
            title: "Title".to_owned(),
            metadata: json!({" spaced:key ": "value"}).as_object().unwrap().clone(),
            body: "body".to_owned(),
        };

        let rendered = render_object_document(&document);
        assert!(rendered.contains("  \" spaced:key \": \"value\"\n"));
        assert_eq!(parse_object_document(&rendered).unwrap(), document);
    }

    /// Verifies users can write ordinary plain YAML strings and indented lists.
    #[test]
    fn parser_accepts_plain_strings_and_indented_lists() {
        let input = concat!(
            "---\n",
            "title: Database migration\n",
            "metadata:\n",
            "  status: proposed\n",
            "  priority: 2\n",
            "  reviewed: false\n",
            "  tags:\n",
            "    - postgres\n",
            "    - migration\n",
            "---\n\n",
            "body"
        );
        let parsed = parse_object_document(input).unwrap();

        assert_eq!(parsed.title, "Database migration");
        assert_eq!(
            parsed.metadata,
            json!({
                "status": "proposed",
                "priority": 2,
                "reviewed": false,
                "tags": ["postgres", "migration"],
            })
            .as_object()
            .unwrap()
            .clone()
        );
        assert_eq!(parsed.body, "body");
    }

    /// Verifies empty metadata and empty scalar arrays have explicit representations.
    #[test]
    fn parser_accepts_empty_metadata_and_arrays() {
        let empty_metadata = "---\ntitle: Title\nmetadata: {}\n---\n\nbody";
        assert!(parse_object_document(empty_metadata).unwrap().metadata.is_empty());

        let empty_array = "---\ntitle: Title\nmetadata:\n  tags: []\n---\n\nbody";
        assert_eq!(
            parse_object_document(empty_array).unwrap().metadata,
            json!({"tags": []}).as_object().unwrap().clone()
        );
    }

    /// Verifies required fields, duplicate fields, and unknown top-level fields are rejected.
    #[test]
    fn parser_rejects_invalid_top_level_fields() {
        let valid = "---\ntitle: Title\nmetadata: {}\n---\n\nbody";
        assert!(parse_object_document(valid).is_ok());
        assert!(parse_object_document(&valid.replace("title: Title\n", "")).is_err());
        assert!(parse_object_document(&valid.replace("metadata: {}\n", "")).is_err());
        assert!(
            parse_object_document(&valid.replace("title: Title", "title: First\ntitle: Second"))
                .is_err()
        );
        assert!(
            parse_object_document(&valid.replace("metadata: {}", "metadata: {}\nmetadata: {}"))
                .is_err()
        );
        assert!(
            parse_object_document(&valid.replace("metadata: {}", "unknown: true\nmetadata: {}"))
                .is_err()
        );
    }

    /// Verifies duplicate metadata keys and nested mappings are rejected.
    #[test]
    fn parser_rejects_invalid_metadata_shape() {
        let duplicate = concat!(
            "---\n",
            "title: Title\n",
            "metadata:\n",
            "  status: draft\n",
            "  status: final\n",
            "---\n"
        );
        assert!(parse_object_document(duplicate).is_err());

        let nested = concat!(
            "---\n",
            "title: Title\n",
            "metadata:\n",
            "  owner:\n",
            "    name: Tim\n",
            "---\n"
        );
        assert!(parse_object_document(nested).is_err());
    }

    /// Verifies CRLF front matter is accepted without changing the CRLF Markdown body.
    #[test]
    fn parser_accepts_crlf_front_matter() {
        let input = concat!(
            "---\r\n",
            "title: \"Title\"\r\n",
            "metadata:\r\n",
            "  status: true\r\n",
            "  tags:\r\n",
            "    - \"one\"\r\n",
            "---\r\n\r\n",
            "body\r\n"
        );
        let parsed = parse_object_document(input).unwrap();

        assert_eq!(parsed.title, "Title");
        assert_eq!(
            parsed.metadata,
            json!({"status": true, "tags": ["one"]}).as_object().unwrap().clone()
        );
        assert_eq!(parsed.body, "body\r\n");
    }
}
