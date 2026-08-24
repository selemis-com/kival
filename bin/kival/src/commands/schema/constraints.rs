//! Kival-specific constraints for structured JSON command input.

use serde_json::{Value, json};

use super::normalize::property_schema_mut;

/// Applies semantic constraints enforced by Kival's structured-input validation.
pub(super) fn apply_structured_input_constraints(schema: &mut Value, path: &[String]) {
    for field in ["query", "name", "display_name", "description", "title", "username", "media_type"]
    {
        add_non_whitespace_string(schema, field);
    }

    // Structured object metadata is an open JSON object whose values are validated by Kival.
    add_json_object_input(schema, "metadata");

    if path == ["objects", "update"] {
        add_at_least_one_value(schema, &["title", "body", "metadata"], &[]);
    } else if path == ["workspaces", "update"] || path == ["groups", "update"] {
        add_at_least_one_value(schema, &["name"], &["description"]);
    }
}

/// Adds trimmed-non-empty string constraints to one field when present.
fn add_non_whitespace_string(schema: &mut Value, field: &str) {
    if let Some(property) = property_schema_mut(schema, field) {
        property.insert("minLength".to_owned(), Value::Number(1_u64.into()));
        property.insert("pattern".to_owned(), Value::String("\\S".to_owned()));
    }
}

/// Marks one field as an open semantic JSON object input.
fn add_json_object_input(schema: &mut Value, field: &str) {
    if let Some(property) = property_schema_mut(schema, field) {
        let description = property.get("description").cloned();
        property.clear();
        property.insert("type".to_owned(), Value::String("object".to_owned()));
        if let Some(description) = description {
            property.insert("description".to_owned(), description);
        }
    }
}

/// Requires at least one update field, distinguishing nullable clear operations from non-null data.
fn add_at_least_one_value(schema: &mut Value, non_null: &[&str], nullable: &[&str]) {
    for field in non_null {
        if let Some(property) = property_schema_mut(schema, field) {
            let all_of =
                property.entry("allOf".to_owned()).or_insert_with(|| Value::Array(Vec::new()));
            if let Some(all_of) = all_of.as_array_mut() {
                all_of.push(json!({ "not": { "type": "null" } }));
            }
        }
    }

    let alternatives = non_null
        .iter()
        .chain(nullable.iter())
        .map(|field| json!({ "required": [field] }))
        .collect::<Vec<_>>();

    if let Some(object) = schema.as_object_mut() {
        let all_of = object.entry("allOf").or_insert_with(|| Value::Array(Vec::new()));
        if let Some(all_of) = all_of.as_array_mut() {
            all_of.push(json!({ "anyOf": alternatives }));
        }
    }
}
