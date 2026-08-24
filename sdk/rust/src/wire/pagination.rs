//! Pagination and collection response types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Default collection page size.
pub const DEFAULT_LIMIT: i64 = 50;

/// Maximum collection page size.
pub const MAX_LIMIT: i64 = 200;

/// Standard collection response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ListResponse<T> {
    /// Collection items.
    pub items: Vec<T>,

    /// Opaque cursor for the next page. Omitted on the final page; list responses do not include a
    /// total count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl<T> ListResponse<T> {
    /// Creates a collection response with no next page.
    pub const fn new(items: Vec<T>) -> Self {
        Self { items, next_cursor: None }
    }
}

/// Collection query parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ListParams {
    /// Maximum number of items to return per page. Defaults to 50 and is capped at 200.
    pub limit: Option<i64>,

    /// Opaque pagination cursor from a previous `response.next_cursor`.
    pub cursor: Option<String>,
}

impl ListParams {
    /// Returns the normalized page limit.
    pub fn limit(&self) -> i64 {
        self.checked_limit().unwrap_or(1)
    }

    /// Returns the validated page limit.
    ///
    /// Low values are invalid. High values are capped to the maximum page size.
    ///
    /// # Errors
    ///
    /// Returns an error if the requested limit is less than 1.
    pub fn checked_limit(&self) -> Result<i64, &'static str> {
        match self.limit {
            Some(limit) if limit < 1 => Err("limit must be at least 1"),
            Some(limit) => Ok(limit.min(MAX_LIMIT)),
            None => Ok(DEFAULT_LIMIT),
        }
    }
}
