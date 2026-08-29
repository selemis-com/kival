//! JSON field projection for successful CLI output.

use std::collections::BTreeMap;

use eyre::Result;
use serde_json::{Map, Value};

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

/// Parses repeated `--field` values into a normalized projection.
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

#[cfg(test)]
mod tests {
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

    fn fields(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }
}
