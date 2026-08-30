//! JSON field projection for successful CLI output.

use std::collections::BTreeMap;

use eyre::Result;
use serde_json::{Map, Value, json};

use crate::utils::error::CliError;

/// Parsed output projection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Projection {
    /// Whether the full value at this node should be included.
    include_all: bool,
    /// Nested field selections keyed by object field name.
    children: BTreeMap<String, Self>,
}

impl Projection {
    /// Inserts one parsed field path into the projection tree.
    fn insert(&mut self, segments: &[&str]) {
        if segments.is_empty() {
            self.include_all = true;
            self.children.clear();
            return;
        }
        if self.include_all {
            return;
        }
        self.children.entry(segments[0].to_owned()).or_default().insert(&segments[1..]);
    }
}

/// Parses dot-separated field selectors into a normalized projection.
///
/// # Errors
///
/// Returns an invalid-argument error for empty paths or empty path segments.
pub fn parse_projection(fields: &[String]) -> Result<Option<Projection>> {
    if fields.is_empty() {
        return Ok(None);
    }

    let mut projection = Projection::default();
    for field in fields {
        let field = field.trim();
        if field.is_empty() || field.split('.').any(str::is_empty) {
            return Err(CliError::invalid_argument(format!(
                "invalid field path `{field}`: paths must contain non-empty dot-separated segments"
            ))
            .into());
        }
        projection.insert(&field.split('.').collect::<Vec<_>>());
    }
    Ok(Some(projection))
}

/// Validates a projection against a Schemars-generated JSON Schema.
///
/// # Errors
///
/// Returns a stable output-field or projection error when a selected path is unavailable or cannot
/// be traversed.
pub fn validate_projection(schema: &Value, projection: &Projection) -> Result<()> {
    if projection.include_all {
        return Ok(());
    }

    let schema = traversable_schema(schema, schema, "$")?;
    for (field, child) in &projection.children {
        validate_child(schema, schema, field, child, field)?;
    }
    Ok(())
}

/// Validates one selected field and its nested projection against the schema.
fn validate_child(
    root: &Value,
    schema: &Value,
    field: &str,
    projection: &Projection,
    path: &str,
) -> Result<()> {
    let schema = traversable_schema(root, schema, path)?;
    let property = schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get(field))
        .ok_or_else(|| unknown_field_error(schema, path))?;

    if projection.include_all || projection.children.is_empty() {
        return Ok(());
    }

    let property = traversable_schema(root, property, path)?;
    for (child, child_projection) in &projection.children {
        let child_path = format!("{path}.{child}");
        validate_child(root, property, child, child_projection, &child_path)?;
    }
    Ok(())
}

/// Applies a validated projection while preserving arrays and nulls.
///
/// # Errors
///
/// Returns an output projection error if the runtime JSON shape cannot be traversed as requested.
pub fn project_value(value: &Value, projection: &Projection) -> Result<Value> {
    if projection.include_all {
        return Ok(value.clone());
    }

    match value {
        Value::Object(object) => {
            let mut projected = Map::new();
            for (field, child_projection) in &projection.children {
                if let Some(child) = object.get(field) {
                    projected.insert(field.clone(), project_value(child, child_projection)?);
                }
            }
            Ok(Value::Object(projected))
        }
        Value::Array(items) => items
            .iter()
            .map(|item| project_value(item, projection))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        Value::Null => Ok(Value::Null),
        _ => Err(CliError::invalid_projection(
            "Selected output field attempts to traverse a scalar value.",
            None,
        )
        .into()),
    }
}

/// Resolves wrappers until the schema describes the traversable value at `path`.
fn traversable_schema<'a>(root: &'a Value, schema: &'a Value, path: &str) -> Result<&'a Value> {
    let mut schema = resolve_schema(root, schema, path)?;

    loop {
        if let Some(items) = schema.get("items")
            && schema_allows_type(schema, "array")
        {
            schema = resolve_schema(root, items, path)?;
            continue;
        }

        if let Some(branches) =
            schema.get("anyOf").or_else(|| schema.get("oneOf")).and_then(Value::as_array)
        {
            schema = non_null_branch(root, branches, path)?;
            continue;
        }

        return resolve_schema(root, schema, path);
    }
}

/// Selects the single non-null branch from an optional-value schema union.
fn non_null_branch<'a>(root: &'a Value, branches: &'a [Value], path: &str) -> Result<&'a Value> {
    let branches =
        branches.iter().filter(|branch| !schema_allows_type(branch, "null")).collect::<Vec<_>>();

    match branches.as_slice() {
        [branch] => resolve_schema(root, branch, path),
        [] => Err(CliError::invalid_projection(
            format!("Output field `{path}` only permits null."),
            Some(path.to_owned()),
        )
        .into()),
        _ => Err(CliError::invalid_projection(
            format!("Output schema for `{path}` uses an unsupported union."),
            Some(path.to_owned()),
        )
        .into()),
    }
}

/// Resolves a local `$defs` reference, returning non-reference schemas unchanged.
fn resolve_schema<'a>(root: &'a Value, schema: &'a Value, path: &str) -> Result<&'a Value> {
    let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
        return Ok(schema);
    };
    let Some(name) = reference.strip_prefix("#/$defs/") else {
        return Err(CliError::invalid_projection(
            format!("Output schema for `{path}` uses unsupported reference `{reference}`."),
            Some(path.to_owned()),
        )
        .into());
    };

    root.get("$defs")
        .and_then(Value::as_object)
        .and_then(|definitions| definitions.get(name))
        .ok_or_else(|| {
            CliError::invalid_projection(
                format!("Output schema reference `{reference}` could not be resolved."),
                Some(path.to_owned()),
            )
            .into()
        })
}

/// Returns whether a schema's `type` declaration permits `expected`.
fn schema_allows_type(schema: &Value, expected: &str) -> bool {
    match schema.get("type") {
        Some(Value::String(kind)) => kind == expected,
        Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind.as_str() == Some(expected)),
        _ => false,
    }
}

/// Builds a stable error for a field missing from the current object schema.
fn unknown_field_error(schema: &Value, field: &str) -> eyre::Report {
    let available = schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().map(Value::String).collect::<Vec<_>>());

    let details = available.map_or_else(
        || json!({ "field": field }),
        |available| json!({ "field": field, "available": available }),
    );

    CliError::invalid_field(format!("Unknown output field `{field}`."), details).into()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::utils::error::{CliErrorBody, CliErrorCode};

    fn fields(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_and_normalizes_field_paths() {
        let projection =
            parse_projection(&fields(&["id", "current_version.id", "current_version"]))
                .unwrap()
                .unwrap();

        assert!(projection.children["id"].include_all);
        assert!(projection.children["current_version"].include_all);
        assert!(projection.children["current_version"].children.is_empty());
    }

    #[test]
    fn rejects_invalid_field_paths() {
        for value in ["", ".id", "id.", "id..title"] {
            let error = parse_projection(&fields(&[value])).unwrap_err();
            let body = CliErrorBody::from_report(&error);
            assert_eq!(body.code, CliErrorCode::InvalidArgument);
        }
    }

    #[test]
    fn projects_nested_objects_and_arrays() {
        let value = json!({
            "items": [
                { "id": "1", "title": "First", "content": "..." },
                { "id": "2", "title": "Second", "content": "..." }
            ],
            "next_cursor": "abc",
            "ignored": true
        });
        let projection = parse_projection(&fields(&["items.id", "items.title", "next_cursor"]))
            .unwrap()
            .unwrap();

        assert_eq!(
            project_value(&value, &projection).unwrap(),
            json!({
                "items": [
                    { "id": "1", "title": "First" },
                    { "id": "2", "title": "Second" }
                ],
                "next_cursor": "abc"
            })
        );
    }

    #[test]
    fn validates_refs_arrays_nullable_and_unknown_fields() {
        let schema = json!({
            "type": "object",
            "properties": {
                "items": { "type": "array", "items": { "$ref": "#/$defs/Item" } },
                "next_cursor": { "type": ["string", "null"] }
            },
            "$defs": {
                "Item": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "title": { "anyOf": [{ "type": "string" }, { "type": "null" }] }
                    }
                }
            }
        });

        let projection = parse_projection(&fields(&["items.id", "items.title", "next_cursor"]))
            .unwrap()
            .unwrap();
        validate_projection(&schema, &projection).unwrap();

        let error = validate_projection(
            &schema,
            &parse_projection(&fields(&["items.nope"])).unwrap().unwrap(),
        )
        .unwrap_err();
        let body = CliErrorBody::from_report(&error);
        assert_eq!(body.code, CliErrorCode::InvalidField);
        assert_eq!(body.details.as_ref().unwrap()["field"], "items.nope");
    }
}
