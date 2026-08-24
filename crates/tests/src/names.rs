//! Unique test names and fixture helpers.

use uuid::Uuid;

/// Returns a unique human-readable name for a test resource.
#[must_use]
pub fn unique_name(prefix: &str) -> String {
    format!("{prefix} {}", Uuid::now_v7())
}
