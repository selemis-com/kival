//! Human-readable object and version output helpers.

use kival_sdk::{ObjectResource, ObjectResponse, ObjectVersion};
use serde_json::Value;

use crate::utils::output::{format_human_timestamp, push_optional_uuid_field, quote_human_string};

/// Prints a human-readable full object response.
pub(super) fn print_object_response(response: &ObjectResponse, action: Option<&str>) {
    let object = &response.object;

    match action {
        Some(action) => println!("{} action={action}", object.id),
        None => println!("{}", object.id),
    }
    println!(
        "status={} created={} updated={}",
        object.status,
        format_human_timestamp(object.created_at),
        format_human_timestamp(object.updated_at),
    );
    println!("workspace={}", object.workspace_id);

    if let Some(version) = &response.current_version {
        println!(
            "current_version={} version={} created={}",
            version.id,
            version.version_number,
            format_human_timestamp(version.created_at),
        );

        if object.title == version.title {
            print_version_content(version);
        } else {
            println!();
            println!("Object title:");
            println!("{}", object.title);

            print_version_content_with_title_label(version, "Current version title");
        }
    } else {
        println!("current_version=none");

        println!();
        println!("Title:");
        println!("{}", object.title);
    }
}

/// Prints title, metadata, and body for a single object version.
fn print_version_content(version: &ObjectVersion) {
    print_version_content_with_title_label(version, "Title");
}

/// Prints title, metadata, and body for a single object version with a custom title label.
fn print_version_content_with_title_label(version: &ObjectVersion, title_label: &str) {
    println!();
    println!("{title_label}:");
    println!("{}", version.title);

    if !is_empty_json_object(&version.metadata) {
        println!();
        println!("Metadata:");

        match serde_json::to_string_pretty(&version.metadata) {
            Ok(metadata) => println!("{metadata}"),
            Err(_) => println!("{}", version.metadata),
        }
    }

    println!();
    println!("Body:");

    if version.body.is_empty() {
        println!("<empty>");
    } else {
        println!("{}", version.body);
    }
}

/// Returns true if a JSON value is an empty object.
fn is_empty_json_object(value: &Value) -> bool {
    matches!(value, Value::Object(map) if map.is_empty())
}

/// Prints a compact object line.
pub(super) fn print_object_line(object: &ObjectResource, action: Option<&str>) {
    let mut fields = vec![object.id.to_string()];
    if let Some(action) = action {
        fields.push(format!("action={action}"));
    }
    fields.extend([
        format!("created={}", format_human_timestamp(object.created_at)),
        format!("updated={}", format_human_timestamp(object.updated_at)),
        format!("status={}", object.status),
    ]);

    push_optional_uuid_field(&mut fields, "current_version", object.current_version_id);
    fields.push(format!("title={}", quote_human_string(&object.title)));

    println!("{}", fields.join(" "));
}

/// Prints a compact version line.
pub(super) fn print_version_line(version: &ObjectVersion) {
    println!(
        "{} object={} version={} created={} title={}",
        version.id,
        version.object_id,
        version.version_number,
        format_human_timestamp(version.created_at),
        quote_human_string(&version.title)
    );
}

/// Prints a human-readable full object version response.
pub(super) fn print_version_response(version: &ObjectVersion) {
    println!("{}", version.id);
    println!(
        "object={} version={} created={}",
        version.object_id,
        version.version_number,
        format_human_timestamp(version.created_at),
    );

    print_version_content(version);
}
