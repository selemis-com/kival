//! JSON field projection for successful CLI output.

use std::collections::BTreeMap;

use eyre::Result;
use serde_json::{Map, Value, json};

use crate::utils::error::CliError;

/// Parsed dot-separated output field path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldPath {
    /// Ordered path segments split on `.`.
    segments: Vec<String>,
}

impl FieldPath {
    /// Parses a single dot-separated path.
    ///
    /// # Errors
    ///
    /// Returns an invalid argument error when the path is empty or contains empty segments.
    fn parse(value: &str) -> Result<Self> {
        if value.is_empty() || value.split('.').any(str::is_empty) {
            return Err(CliError::invalid_argument(format!(
                "invalid field path `{value}`: paths must contain non-empty dot-separated segments"
            ))
            .into());
        }

        Ok(Self { segments: value.split('.').map(str::to_owned).collect() })
    }
}

/// Projection tree for selected fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Projection {
    /// Whether this node selects the complete current value.
    include_all: bool,
    /// Child selections keyed by object field name.
    children: BTreeMap<String, Self>,
}

impl Projection {
    /// Builds a normalized projection from parsed field paths.
    #[must_use]
    fn from_paths(paths: &[FieldPath]) -> Self {
        let mut projection = Self::default();
        for path in paths {
            projection.insert(&path.segments);
        }
        projection
    }

    /// Inserts one path into the tree, dropping children when a parent is selected.
    fn insert(&mut self, segments: &[String]) {
        if segments.is_empty() {
            self.include_all = true;
            self.children.clear();
            return;
        }
        if self.include_all {
            return;
        }

        self.children.entry(segments[0].clone()).or_default().insert(&segments[1..]);
    }

    /// Returns true when no field selection is active.
    #[must_use]
    fn is_empty(&self) -> bool {
        !self.include_all && self.children.is_empty()
    }
}

/// Parses comma-delimited clap values into a normalized projection.
///
/// # Errors
///
/// Returns an invalid argument error for malformed path syntax.
pub fn parse_projection(values: &[String]) -> Result<Option<Projection>> {
    if values.is_empty() {
        return Ok(None);
    }

    let paths = values.iter().map(|value| FieldPath::parse(value)).collect::<Result<Vec<_>>>()?;
    let projection = Projection::from_paths(&paths);
    Ok((!projection.is_empty()).then_some(projection))
}

/// Validates a projection against a Schemars-generated output schema.
///
/// # Errors
///
/// Returns a stable CLI projection error for unknown fields or unsupported schema structures.
pub fn validate_projection(schema: &Value, projection: &Projection) -> Result<()> {
    for (field, child) in &projection.children {
        validate_child(schema, schema, field, child, field)?;
    }
    Ok(())
}

/// Projects a JSON value according to a validated projection.
///
/// # Errors
///
/// Returns a stable CLI projection error when a runtime value cannot be traversed as requested.
pub fn project_value(value: &Value, projection: &Projection) -> Result<Value> {
    if projection.include_all || projection.children.is_empty() {
        return Ok(value.clone());
    }

    match value {
        Value::Object(object) => {
            let mut projected = Map::new();
            for (key, value) in object {
                if let Some(child) = projection.children.get(key) {
                    projected.insert(key.clone(), project_value(value, child)?);
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

/// Validates one selected object field and recurses into any child selections.
fn validate_child(
    root: &Value,
    schema: &Value,
    field: &str,
    projection: &Projection,
    path: &str,
) -> Result<()> {
    let schema = non_null_schema(root, schema, path)?;

    if projection.include_all || projection.children.is_empty() {
        if object_property(root, schema, field, path)?.is_some() {
            return Ok(());
        }
        return Err(unknown_field_error(schema, path));
    }

    let property = object_property(root, schema, field, path)?
        .ok_or_else(|| unknown_field_error(schema, path))?;
    let property = array_item_schema(root, property, path)?;

    for (child_field, child_projection) in &projection.children {
        let child_path = format!("{path}.{child_field}");
        validate_child(root, property, child_field, child_projection, &child_path)?;
    }

    Ok(())
}

/// Returns the schema for an object property or a projection error for non-objects.
fn object_property<'a>(
    root: &'a Value,
    schema: &'a Value,
    field: &str,
    path: &str,
) -> Result<Option<&'a Value>> {
    let schema = non_null_schema(root, schema, path)?;

    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        return Ok(properties.get(field));
    }

    if schema_allows_type(schema, "object") {
        return Err(CliError::invalid_projection(
            format!("Output schema for `{path}` does not declare selectable fields."),
            Some(path.to_owned()),
        )
        .into());
    }

    Err(CliError::invalid_projection(
        format!("Output field `{path}` cannot be traversed."),
        Some(path.to_owned()),
    )
    .into())
}

/// Descends through array item schemas until a non-array schema is reached.
fn array_item_schema<'a>(root: &'a Value, mut schema: &'a Value, path: &str) -> Result<&'a Value> {
    loop {
        schema = non_null_schema(root, schema, path)?;
        if !schema_allows_type(schema, "array") {
            return Ok(schema);
        }
        schema = schema.get("items").ok_or_else(|| {
            CliError::invalid_projection(
                format!("Output array field `{path}` does not declare item schema."),
                Some(path.to_owned()),
            )
        })?;
    }
}

/// Resolves references and unwraps nullable unions to their non-null branch.
fn non_null_schema<'a>(root: &'a Value, schema: &'a Value, path: &str) -> Result<&'a Value> {
    let schema = resolve_schema(root, schema, path)?;

    if let Some(any_of) = schema.get("anyOf").and_then(Value::as_array) {
        return non_null_branch(root, any_of, path);
    }
    if let Some(one_of) = schema.get("oneOf").and_then(Value::as_array) {
        return non_null_branch(root, one_of, path);
    }

    Ok(schema)
}

/// Returns the single non-null branch from an `anyOf` or `oneOf` schema.
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

/// Resolves local `$defs` references produced by Schemars.
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

/// Returns whether a schema declares the requested JSON Schema type.
fn schema_allows_type(schema: &Value, expected: &str) -> bool {
    match schema.get("type") {
        Some(Value::String(kind)) => kind == expected,
        Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind.as_str() == Some(expected)),
        _ => false,
    }
}

/// Builds the stable unknown-field error with immediately selectable sibling fields.
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
    use clap_schema::CliSchema;
    use serde_json::json;

    use super::*;
    use crate::utils::error::{CliErrorBody, CliErrorCode};

    #[test]
    fn parses_field_paths() {
        assert_eq!(FieldPath::parse("id").unwrap().segments, ["id"]);
        assert_eq!(
            FieldPath::parse("current_version.id").unwrap().segments,
            ["current_version", "id"]
        );
        assert_eq!(
            parse_projection(&fields(&["id", "title"])).unwrap().unwrap(),
            Projection {
                include_all: false,
                children: [
                    (
                        "id".to_owned(),
                        Projection { include_all: true, children: Default::default() }
                    ),
                    (
                        "title".to_owned(),
                        Projection { include_all: true, children: Default::default() }
                    )
                ]
                .into(),
            }
        );
    }

    #[test]
    fn rejects_invalid_field_paths() {
        for value in ["", ".id", "id.", "id..title"] {
            let error = FieldPath::parse(value).unwrap_err();
            let body = CliErrorBody::from_report(&error);
            assert_eq!(body.code, CliErrorCode::InvalidArgument);
        }
    }

    #[test]
    fn parent_selection_overrides_children() {
        let projection =
            parse_projection(&fields(&["current_version.id", "current_version"])).unwrap().unwrap();
        assert!(projection.children["current_version"].include_all);
        assert!(projection.children["current_version"].children.is_empty());
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
    fn omits_absent_optional_fields_and_preserves_null() {
        let projection =
            parse_projection(&fields(&["id", "optional", "nullable.value"])).unwrap().unwrap();
        let value = json!({ "id": "1", "nullable": null });

        assert_eq!(
            project_value(&value, &projection).unwrap(),
            json!({ "id": "1", "nullable": null })
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

    #[test]
    fn reports_available_fields_for_nullable_root_object() {
        let schema = json!({
            "anyOf": [
                {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "title": { "type": "string" }
                    }
                },
                {
                    "type": "null"
                }
            ]
        });
        let projection = parse_projection(&fields(&["nope"])).unwrap().unwrap();

        let error = validate_projection(&schema, &projection).unwrap_err();
        let body = CliErrorBody::from_report(&error);

        assert_eq!(body.code, CliErrorCode::InvalidField);
        assert_eq!(
            body.details,
            Some(json!({
                "field": "nope",
                "available": ["id", "title"]
            }))
        );
    }

    #[test]
    fn validates_actual_object_response_schema() {
        let contract = crate::Cli::schema().expect("CLI schema should build");
        let schema = contract
            .command(&["objects", "get"])
            .expect("objects get should be discoverable")
            .output
            .expect("objects get has JSON output");
        let projection =
            parse_projection(&fields(&["object.id", "current_version.id"])).unwrap().unwrap();

        validate_projection(&schema, &projection).unwrap();
    }

    fn fields(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }
}
