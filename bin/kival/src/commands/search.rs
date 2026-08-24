//! Search command.

use clap::{Parser, ValueEnum};
use clap_schema::schema_handler;
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{SearchHit, SearchMode, SearchParams, SearchResponse};
use uuid::Uuid;

use crate::utils::{
    args::CliArchiveListStatus,
    credentials::authenticated_client,
    error::CliError,
    output::{OutputMode, print_empty_list, print_output, quote_human_string},
};

/// Arguments for `kival search`.
#[derive(Debug, Parser)]
#[command(after_long_help = "\
Leading and trailing whitespace is trimmed from QUERY before matching.

Indexed categories:
  title     Version title
  body      Version body text
  metadata  Version metadata serialized as one JSON value

Every mode searches the same selected categories. Omit --categories to search every indexed
category. Metadata paths such as metadata.kind are not supported.

Search modes:
  auto     Match normalized full-text tokens or a literal substring. Classify each hit as exact
           for complete-value equality, otherwise literal for a substring, otherwise text. This is
           the default.
  text     Match normalized tokens using PostgreSQL web-search syntax and the simple text-search
           configuration. Quoted phrases, OR, and -term use PostgreSQL web-search syntax. Matching
           is case-insensitive, does not stem words, and does not match arbitrary substrings inside
           a token.
  literal  Match the query as one contiguous substring of the stored category value, without
           tokenization or text normalization. Matching is case-insensitive by default.
  exact    Match only when the query equals the complete stored category value. Matching is
           case-insensitive by default. For metadata, the query must equal the complete serialized
           JSON value.

--case-sensitive affects literal and exact comparisons, including those performed by auto. It does
not affect text matching. By default only current object versions are searched; --include-history
searches previous immutable versions as well. --status filters objects before matching; --context
changes only the returned snippet.

Examples:
  kival search <WORKSPACE_ID> '\"release notes\"' --mode text --categories body
  kival search <WORKSPACE_ID> 'release notes' --mode literal --categories title,body
  kival search <WORKSPACE_ID> 'superseded decision' --include-history
")]
pub struct SearchCommand {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,

    /// Search query. Leading and trailing whitespace is trimmed before matching.
    #[arg(value_name = "QUERY")]
    pub query: String,

    /// Restrict matching to comma-separated search categories.
    ///
    /// Omit this option to search all indexed categories. Accepted values are `title`, `body`,
    /// and `metadata`.
    ///
    /// These values select where the query may match; they are not JSON output fields or property
    /// paths. `metadata` searches the complete serialized JSON value. Nested paths such as
    /// `metadata.kind` are not supported. Every search mode uses the same selected categories.
    #[arg(
        long,
        value_name = "CATEGORY[,CATEGORY]",
        help = "Restrict matching to comma-separated search categories"
    )]
    pub categories: Option<String>,

    /// Archive status filter applied before search matching. Defaults to active content.
    #[arg(long, value_name = "STATUS", value_enum)]
    pub status: Option<CliArchiveListStatus>,

    /// Maximum number of hits to return.
    #[arg(long, value_name = "N")]
    pub limit: Option<i64>,

    /// Matching model. Defaults to `auto`. See the mode descriptions for matching semantics.
    #[arg(long, value_name = "MODE", value_enum, help = "Select the search matching model")]
    pub mode: Option<CliSearchMode>,

    /// Make literal and exact comparisons case-sensitive.
    ///
    /// This affects `literal`, `exact`, and the literal/exact checks performed by `auto`.
    /// Full-text matching remains case-insensitive.
    #[arg(long)]
    pub case_sensitive: bool,

    /// Number of context characters around snippets. This does not affect matching.
    #[arg(long, value_name = "N")]
    pub context: Option<usize>,

    /// Include previous immutable object versions in search results.
    ///
    /// By default search is scoped to each object's current version.
    #[arg(long)]
    pub include_history: bool,
}

/// CLI search mode values.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliSearchMode {
    /// Match normalized full-text tokens or a literal substring.
    ///
    /// Each hit is classified as exact for complete-value equality, otherwise literal for a
    /// substring, otherwise text. Case sensitivity affects only the literal and exact checks.
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

#[schema_handler(run)]
impl SearchCommand {
    /// Run `kival search`.
    ///
    /// # Errors
    ///
    /// Returns an error if search fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<SearchResponse> {
        let query = self.query.trim();
        if query.is_empty() {
            return Err(CliError::invalid_argument("search query must not be empty").into());
        }
        if matches!(self.limit, Some(limit) if limit < 1) {
            return Err(CliError::invalid_argument("limit must be at least 1").into());
        }

        let params = SearchParams {
            q: query.to_owned(),
            categories: self.categories,
            status: self.status.map(Into::into),
            limit: self.limit,
            mode: self.mode.map(SearchMode::from),
            case_sensitive: Some(self.case_sensitive).filter(|value| *value),
            context: self.context,
            include_history: Some(self.include_history).filter(|value| *value),
        };

        let client = authenticated_client(&ctx)?;
        let response = client.search_workspace(self.workspace_id, &params).await?;

        print_output(output, &response, || {
            if response.items.is_empty() {
                print_empty_list("search hits");
            } else {
                for hit in &response.items {
                    print_search_hit(hit);
                }
            }
        })?;
        Ok(response)
    }
}

/// Prints a compact search hit.
fn print_search_hit(hit: &SearchHit) {
    let mut parts = Vec::new();

    parts.push(format!("object={}", hit.object_id));
    parts.push(format!("version={} number={}", hit.version_id, hit.version_number));
    parts.push(format!("category={}", hit.matched_category));

    parts.push(format!("title={}", quote_human_string(&hit.title)));
    if let Some(rank) = hit.rank {
        parts.push(format!("rank={rank:.3}"));
    }

    println!("{}", parts.join(" "));
    println!("  {}", hit.snippet);
}
