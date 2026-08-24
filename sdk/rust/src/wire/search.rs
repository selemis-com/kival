//! Search wire protocol types.

use kival_types::ArchiveStatus;
pub use kival_types::{SearchCategory, SearchMatchKind, SearchMode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::ArchiveListStatus;

/// Query parameters for workspace search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// Search query string. Leading and trailing whitespace is trimmed before matching.
    pub q: String,

    /// Optional comma-separated search categories. Omit to search all indexed categories.
    ///
    /// Accepted values are `title`, `body`, and `metadata`.
    ///
    /// These values select where the query may match; they are not JSON output fields or property
    /// paths. `metadata` searches the complete serialized JSON value. Nested paths such as
    /// `metadata.kind` are not supported. Every search mode uses the same selected categories.
    pub categories: Option<String>,

    /// Archive status filter. Defaults to active content.
    pub status: Option<ArchiveListStatus>,

    /// Maximum number of hits to return per page.
    pub limit: Option<i64>,

    /// Opaque pagination cursor from a previous `response.next_cursor`.
    pub cursor: Option<String>,

    /// Matching model. Defaults to `auto`. Plain multi-word `auto` queries may include
    /// lower-ranked partial-term full-text matches.
    pub mode: Option<SearchMode>,

    /// Case-sensitive literal and exact comparisons.
    ///
    /// Applies to `literal`, `exact`, and the literal/exact checks performed by `auto`. Full-text
    /// matching remains case-insensitive.
    pub case_sensitive: Option<bool>,

    /// Number of context characters around snippets. This does not affect matching.
    pub context: Option<usize>,

    /// Include previous immutable object versions. Defaults to current versions only.
    pub include_history: Option<bool>,
}

/// Search response envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchResponse {
    /// Search hits.
    pub items: Vec<SearchHit>,

    /// Opaque cursor for the next page. Omitted on the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Term coverage for a plain multi-term `auto` search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchTermCoverage {
    /// Query terms matched by the selected search document.
    pub matched_terms: Vec<String>,

    /// Number of terms in the broadened query.
    pub query_term_count: usize,
}

/// One actionable search hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHit {
    /// Workspace ID.
    pub workspace_id: Uuid,

    /// Object ID.
    pub object_id: Uuid,

    /// Immutable object version containing the match.
    pub version_id: Uuid,

    /// Monotonic version number within the object.
    pub version_number: i64,

    /// Title of the matched version.
    pub title: String,

    /// Object lifecycle status.
    pub status: ArchiveStatus,

    /// Flat metadata from the matched immutable version.
    pub metadata: Value,

    /// Search category in which the match occurred.
    pub matched_category: SearchCategory,

    /// Match kind.
    pub match_kind: SearchMatchKind,

    /// Term coverage for plain multi-term `auto` searches.
    ///
    /// Omitted for strict modes and queries that do not use the partial-term fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub term_coverage: Option<SearchTermCoverage>,

    /// Context snippet.
    pub snippet: String,

    /// Relevance score. Higher is better.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<f32>,
}
