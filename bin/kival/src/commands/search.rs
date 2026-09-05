//! Search command.

use argx::{Args, ValueEnum, argx};
use kival_cli::runner::CliContext;
use kival_sdk::{SearchCategory, SearchHit, SearchMode, SearchParams};
use serde::Serialize;
use uuid::Uuid;

use crate::utils::{
    args::CliArchiveListStatus,
    credentials::authenticated_client,
    error::{CommandError, command_error_codes},
    output::{OutputMode, print_empty_list, print_output, quote_human_string},
};

command_error_codes! {
    pub(crate) enum SearchErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
        InvalidCursor => ("invalid.cursor", InvalidCursor),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
    }
}

/// Error returned by the corresponding command handler.
type SearchError = CommandError<SearchErrorCode>;

/// Search response enriched with browser links for each matched object.
#[derive(Debug, Serialize)]
#[argx(schema)]
pub struct SearchOutput {
    /// Search hits.
    pub items: Vec<SearchOutputHit>,
    /// Opaque cursor for the next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Search hit enriched with a canonical browser URL.
#[derive(Debug, Serialize)]
#[argx(schema)]
pub struct SearchOutputHit {
    /// Search hit returned by Kival.
    #[serde(flatten)]
    pub hit: SearchHit,
    /// Browser URL for the matched object.
    pub url: String,
}

/// Arguments for `kival search`.
///
/// Leading and trailing whitespace is trimmed from the query before matching. Indexed categories
/// are `title`, `body`, and `metadata`; metadata is searched as one serialized JSON value, so paths
/// such as `metadata.kind` are not supported. Omit `--categories` to search every category.
///
/// Search modes:
///
/// - `auto` matches normalized full-text tokens or a literal substring and is the default.
/// - `text` uses PostgreSQL web-search syntax with the `simple` text-search configuration.
/// - `literal` matches one contiguous substring without tokenization.
/// - `exact` matches only the complete stored category value.
///
/// `--case-sensitive` affects literal and exact comparisons, including those performed by `auto`,
/// but not text matching. By default only current object versions are searched;
/// `--include-history` also searches previous immutable versions. `--status` filters objects before
/// matching, while `--context` only changes the returned snippet.
///
/// Examples:
///
/// `kival search <WORKSPACE_ID> '"release notes"' --mode text --categories body`
///
/// `kival search <WORKSPACE_ID> 'release notes' --mode literal --categories title,body`
///
/// `kival search <WORKSPACE_ID> 'superseded decision' --include-history`
#[derive(Debug, Args)]

pub struct SearchCommand {
    /// Workspace ID.
    pub workspace_id: Uuid,

    /// Search query. Leading and trailing whitespace is trimmed before matching.
    pub query: String,

    /// Restrict matching to one or more search categories.
    ///
    /// Omit this option to search all indexed categories. Accepted values are `title`, `body`,
    /// and `metadata`. Values may be repeated or comma-delimited.
    ///
    /// These values select where the query may match; they are not JSON output fields or property
    /// paths. `metadata` searches the complete serialized JSON value. Nested paths such as
    /// `metadata.kind` are not supported. Every search mode uses the same selected categories.
    #[argx(long, value_enum, delimited, help = "Restrict matching to search categories")]
    pub categories: Vec<CliSearchCategory>,

    /// Archive status filter applied before search matching. Defaults to active content.
    #[argx(long, value_enum)]
    pub status: Option<CliArchiveListStatus>,

    /// Maximum number of hits to return per page.
    #[argx(long)]
    pub limit: Option<i64>,

    /// Opaque `response.next_cursor` from the previous page; reuse it with the same search.
    #[argx(long)]
    pub cursor: Option<String>,

    /// Matching model. Defaults to `auto`. See the mode descriptions for matching semantics.
    #[argx(long, value_enum, help = "Select the search matching model")]
    pub mode: Option<CliSearchMode>,

    /// Make literal and exact comparisons case-sensitive.
    ///
    /// This affects `literal`, `exact`, and the literal/exact checks performed by `auto`.
    /// Full-text matching remains case-insensitive.
    #[argx(long)]
    pub case_sensitive: bool,

    /// Number of context characters around snippets. This does not affect matching.
    #[argx(long)]
    pub context: Option<usize>,

    /// Include previous immutable object versions in search results.
    ///
    /// By default search is scoped to each object's current version.
    #[argx(long)]
    pub include_history: bool,
}

/// CLI search category values.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliSearchCategory {
    /// Object-version title.
    Title,
    /// Object-version body.
    Body,
    /// Serialized object-version metadata.
    Metadata,
}

impl From<CliSearchCategory> for SearchCategory {
    fn from(category: CliSearchCategory) -> Self {
        match category {
            CliSearchCategory::Title => Self::Title,
            CliSearchCategory::Body => Self::Body,
            CliSearchCategory::Metadata => Self::Metadata,
        }
    }
}

/// CLI search mode values.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliSearchMode {
    /// Match normalized full-text tokens or a literal substring.
    ///
    /// Plain multi-word queries also admit lower-ranked results matching only some terms. Each hit
    /// is classified as exact for complete-value equality, otherwise literal for a substring,
    /// otherwise text. Case sensitivity affects only the literal and exact checks.
    Auto,

    /// Match normalized tokens using `PostgreSQL` web-search syntax.
    ///
    /// Uses the `simple` text-search configuration. Quoted phrases, `OR`, and `-term` use
    /// `PostgreSQL` web-search syntax. Matching is case-insensitive, does not stem words, and does
    /// not match arbitrary substrings inside a token.
    Text,

    /// Match one contiguous substring of the complete stored category value.
    ///
    /// No tokenization or full-text normalization is applied. Matching is case-insensitive unless
    /// `--case-sensitive` is supplied.
    Literal,

    /// Match only when the query equals the complete stored category value.
    ///
    /// Matching is case-insensitive unless `--case-sensitive` is supplied. Metadata exact matching
    /// compares against the complete serialized JSON value, not an individual metadata property.
    Exact,
}

impl From<CliSearchMode> for SearchMode {
    fn from(mode: CliSearchMode) -> Self {
        match mode {
            CliSearchMode::Auto => Self::Auto,
            CliSearchMode::Text => Self::Text,
            CliSearchMode::Literal => Self::Literal,
            CliSearchMode::Exact => Self::Exact,
        }
    }
}

#[argx(handler = run)]
impl SearchCommand {
    /// Run `kival search`.
    ///
    /// # Errors
    ///
    /// Returns an error if search fails.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> Result<SearchOutput, SearchError> {
        let query = self.query.trim();
        if query.is_empty() {
            return Err(SearchError::invalid_argument("search query must not be empty"));
        }
        if matches!(self.limit, Some(limit) if limit < 1) {
            return Err(SearchError::invalid_argument("limit must be at least 1"));
        }

        let categories = (!self.categories.is_empty()).then(|| {
            self.categories
                .iter()
                .copied()
                .map(SearchCategory::from)
                .map(SearchCategory::as_str)
                .collect::<Vec<_>>()
                .join(",")
        });

        let params = SearchParams {
            q: query.to_owned(),
            categories,
            status: self.status.map(Into::into),
            limit: self.limit,
            cursor: self.cursor,
            mode: self.mode.map(SearchMode::from),
            case_sensitive: Some(self.case_sensitive).filter(|value| *value),
            context: self.context,
            include_history: Some(self.include_history).filter(|value| *value),
        };

        let client = authenticated_client(&ctx)?;
        let response = client.search_workspace(self.workspace_id, &params).await?;
        let response = SearchOutput {
            items: response
                .items
                .into_iter()
                .map(|hit| SearchOutputHit {
                    url: client.object_url(hit.workspace_id, hit.object_id).to_string(),
                    hit,
                })
                .collect(),
            next_cursor: response.next_cursor,
        };

        print_output(&output, &response, || {
            if response.items.is_empty() {
                print_empty_list("search hits");
            } else {
                for hit in &response.items {
                    print_search_hit(hit);
                }
            }
            if let Some(cursor) = &response.next_cursor {
                println!("\nNext cursor: {cursor}");
            }
        })?;
        Ok(response)
    }
}

/// Prints a compact search hit.
fn print_search_hit(output: &SearchOutputHit) {
    let hit = &output.hit;
    let mut parts = Vec::new();

    parts.push(format!("object={}", hit.object_id));
    parts.push(format!("version={} number={}", hit.version_id, hit.version_number));
    parts.push(format!("category={}", hit.matched_category));
    parts.push(format!("status={}", hit.status));
    if let Some(coverage) = &hit.term_coverage {
        parts.push(format!("terms={}/{}", coverage.matched_terms.len(), coverage.query_term_count));
    }

    parts.push(format!("title={}", quote_human_string(&hit.title)));
    if let Some(rank) = hit.rank {
        parts.push(format!("rank={rank:.3}"));
    }

    println!("{}", parts.join(" "));
    println!("  {}", hit.snippet);
    println!("  metadata={}", hit.metadata);
    println!("  url={}", output.url);
}
