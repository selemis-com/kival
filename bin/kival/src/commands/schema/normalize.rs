//! JSON Schema generation for Kival-owned schema fragments.

use schemars::JsonSchema;
use serde_json::{Value, json};

/// Output metadata fields known to carry JSON objects.
const METADATA_OBJECT_SCHEMA_PATHS: &[&[&str]] = &[
    &["properties", "metadata"],
    &["$defs", "ObjectVersion", "properties", "metadata"],
    &["$defs", "ObjectAttachment", "properties", "metadata"],
];

/// Known RFC 3339 timestamp property names in Kival's public wire contract.
const TIMESTAMP_PROPERTY_NAMES: &[&str] = &[
    "archived_at",
    "created_at",
    "disabled_at",
    "expires_at",
    "last_seen_at",
    "revoked_at",
    "updated_at",
];

/// Returns a normalized Schemars schema for a Kival-owned type.
///
/// # Panics
///
/// Panics if Schemars produces a schema that cannot be serialized to JSON.
pub fn schema_for<T>() -> Value
where
    T: JsonSchema,
{
    let value =
        serde_json::to_value(schemars::schema_for!(T)).expect("Schemars schemas must serialize");
    normalize_schema(value)
}

/// Applies Kival-specific JSON Schema normalization to an existing schema value.
pub(super) fn normalize_schema(mut value: Value) -> Value {
    strip_schema_titles(&mut value);
    strip_schema_meta(&mut value);
    add_timestamp_formats(&mut value);
    add_metadata_object_types(&mut value);
    value
}

/// Emits known metadata output fields as JSON objects rather than unconstrained values.
fn add_metadata_object_types(value: &mut Value) {
    for path in METADATA_OBJECT_SCHEMA_PATHS {
        rewrite_property_schema(value, path, json!({ "type": "object" }));
    }
}

/// Replaces a property schema while preserving its description.
fn rewrite_property_schema(value: &mut Value, path: &[&str], replacement: Value) {
    let Some(property) = value_at_path_mut(value, path).and_then(Value::as_object_mut) else {
        return;
    };
    let description = property.get("description").cloned();
    property.clear();
    if let Value::Object(replacement) = replacement {
        property.extend(replacement);
    }
    if let Some(description) = description {
        property.insert("description".to_owned(), description);
    }
}

/// Returns a mutable JSON value at a static object path.
fn value_at_path_mut<'a>(value: &'a mut Value, path: &[&str]) -> Option<&'a mut Value> {
    let mut current = value;
    for segment in path {
        current = current.as_object_mut()?.get_mut(*segment)?;
    }
    Some(current)
}

/// Adds JSON Schema date-time formats to known RFC 3339 timestamp fields.
fn add_timestamp_formats(value: &mut Value) {
    walk_schema_mut(value, &mut |node| {
        let Value::Object(object) = node else {
            return;
        };
        let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) else {
            return;
        };

        for (name, property) in properties {
            if TIMESTAMP_PROPERTY_NAMES.contains(&name.as_str())
                && let Some(property) = property.as_object_mut()
            {
                property
                    .entry("format".to_owned())
                    .or_insert_with(|| Value::String("date-time".to_owned()));
            }
        }
    });
}

/// Removes repeated schema-document metadata from nested command schemas.
fn strip_schema_meta(value: &mut Value) {
    walk_schema_mut(value, &mut |node| {
        if let Value::Object(object) = node {
            object.remove("$schema");
        }
    });
}

/// Removes Rust type-name title annotations from generated schemas before exposing them publicly.
fn strip_schema_titles(value: &mut Value) {
    walk_schema_mut(value, &mut |node| {
        if let Value::Object(object) = node {
            object.remove("title");
        }
    });
}

/// Walks every mutable JSON Schema node, skipping instance-value keywords.
fn walk_schema_mut(schema: &mut Value, visit: &mut impl FnMut(&mut Value)) {
    visit(schema);

    match schema {
        Value::Object(object) => {
            for (key, value) in object {
                if is_instance_value_keyword(key) {
                    continue;
                }

                if is_schema_map_keyword(key) {
                    walk_schema_map_mut(value, visit);
                } else {
                    walk_schema_mut(value, visit);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                walk_schema_mut(value, visit);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Walks mutable schema values inside a named-schema map.
fn walk_schema_map_mut(value: &mut Value, visit: &mut impl FnMut(&mut Value)) {
    match value {
        Value::Object(object) => {
            for value in object.values_mut() {
                walk_schema_mut(value, visit);
            }
        }
        Value::Array(values) => {
            for value in values {
                walk_schema_mut(value, visit);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Returns true for JSON Schema keywords whose value is a map of named schemas.
fn is_schema_map_keyword(key: &str) -> bool {
    matches!(key, "$defs" | "definitions" | "properties" | "patternProperties" | "dependentSchemas")
}

/// Returns true for JSON Schema keywords whose values are data, not schemas.
fn is_instance_value_keyword(key: &str) -> bool {
    matches!(key, "default" | "const" | "enum" | "examples")
}
