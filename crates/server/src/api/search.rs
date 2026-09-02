//! Workspace search handlers.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{SearchDocumentCursor, SearchDocumentRow, SearchDocuments, search_documents};
use kival_sdk::{
    DEFAULT_LIMIT, MAX_LIMIT, SearchHit, SearchParams, SearchResponse, SearchTermCoverage,
};
use kival_types::{ArchiveListStatus, SearchCategory, SearchMode};
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        error::{ApiError, ApiResult},
        metrics::SearchMetrics,
        pagination::{decode_search, filtered_kind, search_page},
        query::QueryParams,
    },
};

/// Maximum snippet context characters on either side of a match.
const MAX_CONTEXT: usize = 240;

/// Searches visible workspace content.
pub(crate) async fn handle_search_workspace(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
    QueryParams(params): QueryParams<SearchParams>,
) -> ApiResult<Json<SearchResponse>> {
    let q = params.q.trim();
    if q.is_empty() {
        return Err(ApiError::bad_request("search query must not be empty"));
    }

    let categories = normalize_categories(params.categories.as_deref())?;
    let limit = match params.limit {
        Some(limit) if limit < 1 => return Err(ApiError::bad_request("limit must be at least 1")),
        Some(limit) => limit.min(MAX_LIMIT),
        None => DEFAULT_LIMIT,
    };
    let mode = params.mode.unwrap_or(SearchMode::Auto);
    let status = params.status.unwrap_or(ArchiveListStatus::Active);
    let case_sensitive = params.case_sensitive.unwrap_or(false);
    let include_history = params.include_history.unwrap_or(false);
    let mut category_names =
        categories.iter().copied().map(SearchCategory::as_str).collect::<Vec<_>>();
    category_names.sort_unstable();
    let cursor_kind = filtered_kind(
        "search",
        &(q, &category_names, status.as_str(), mode.as_str(), case_sensitive, include_history),
    )?;
    let cursor = decode_search(params.cursor.as_deref(), &cursor_kind, workspace_id)?;
    let search_scope = if include_history { "history" } else { "current" };
    let mut metrics = SearchMetrics::start(mode.as_str(), status.as_str(), search_scope);

    let rows = search_documents(
        state.db(),
        SearchDocuments {
            workspace_id,
            query: q,
            categories: &categories,
            user_id: actor.id,
            mode,
            case_sensitive,
            status,
            include_history,
            cursor: cursor.map(|cursor| SearchDocumentCursor {
                rank: cursor.rank,
                object_id: cursor.object_id,
                version_number: cursor.version_number,
                version_id: cursor.version_id,
            }),
            limit: limit.saturating_add(1),
        },
    )
    .await?;

    let page = search_page(rows, limit, &cursor_kind, workspace_id, |row| {
        (row.rank, row.object_id, row.version_number, row.version_id)
    })?;
    let context = params.context.unwrap_or(80).min(MAX_CONTEXT);
    let items = page
        .items
        .into_iter()
        .map(|row| row.into_hit(q, case_sensitive, context))
        .collect::<Vec<_>>();

    metrics.complete(items.len());

    Ok(Json(SearchResponse { items, next_cursor: page.next_cursor }))
}

/// Normalizes the comma-separated search categories accepted by the CLI and API.
fn normalize_categories(categories: Option<&str>) -> ApiResult<Vec<SearchCategory>> {
    let Some(categories) = categories else {
        return Ok(Vec::new());
    };

    let mut normalized = Vec::new();

    for category in categories.split(',') {
        let category = category.trim();

        if category.is_empty() {
            return Err(ApiError::bad_request("search category must not be empty"));
        }

        let category = category
            .parse::<SearchCategory>()
            .map_err(|()| ApiError::bad_request(format!("unknown search category: {category}")))?;

        if !normalized.contains(&category) {
            normalized.push(category);
        }
    }

    Ok(normalized)
}

/// Converts kernel search rows into API hits with request-specific snippets.
trait SearchRowExt {
    /// Converts this row into an API search hit.
    fn into_hit(self, search_text: &str, case_sensitive: bool, context: usize) -> SearchHit;
}

impl SearchRowExt for SearchDocumentRow {
    /// Converts this row into a wire hit.
    fn into_hit(self, search_text: &str, case_sensitive: bool, context: usize) -> SearchHit {
        let query_term_count = self.query_term_count;
        let term_coverage = self.matched_terms.map(|matched_terms| SearchTermCoverage {
            matched_terms,
            query_term_count: query_term_count.expect("matched terms require a query term count"),
        });
        let snippet_terms = term_coverage
            .as_ref()
            .map(|coverage| coverage.matched_terms.as_slice())
            .unwrap_or_default();
        let snippet = snippet(&self.text, search_text, snippet_terms, case_sensitive, context);

        SearchHit {
            workspace_id: self.workspace_id,
            object_id: self.object_id,
            version_id: self.version_id,
            version_number: self.version_number,
            title: self.title,
            status: self.status,
            metadata: self.metadata,
            matched_category: self.category,
            match_kind: self.match_kind,
            term_coverage,
            snippet,
            rank: Some(self.rank),
        }
    }
}

/// Returns whether Unicode lowercasing preserves UTF-8 byte offsets.
fn lowercase_preserves_offsets(value: &str) -> bool {
    value.chars().all(|character| {
        let mut lowercase = character.to_lowercase();
        matches!(
            (lowercase.next(), lowercase.next()),
            (Some(lowercase), None) if lowercase.len_utf8() == character.len_utf8()
        )
    })
}

/// Builds a compact context snippet around the first literal search-term occurrence.
fn snippet(
    text: &str,
    search_text: &str,
    matched_terms: &[String],
    case_sensitive: bool,
    context: usize,
) -> String {
    if search_text.is_empty() {
        return truncate_on_char_boundary(text, context.saturating_mul(2));
    }

    if !case_sensitive
        && (!lowercase_preserves_offsets(text)
            || !lowercase_preserves_offsets(search_text)
            || matched_terms
                .iter()
                .any(|term| !lowercase_preserves_offsets(term)))
    {
        return truncate_on_char_boundary(text, context.saturating_mul(2));
    }

    let haystack = if case_sensitive { text.to_owned() } else { text.to_lowercase() };
    let search_needle =
        if case_sensitive { search_text.to_owned() } else { search_text.to_lowercase() };
    let matched_term = haystack.find(&search_needle).map_or_else(
        || {
            matched_terms
                .iter()
                .filter_map(|term| {
                    let needle = if case_sensitive { term.clone() } else { term.to_lowercase() };
                    haystack.find(&needle).map(|byte_index| (byte_index, term.len()))
                })
                .min_by_key(|(byte_index, _)| *byte_index)
        },
        |byte_index| Some((byte_index, search_text.len())),
    );
    let Some((byte_index, match_len)) = matched_term else {
        return truncate_on_char_boundary(text, context.saturating_mul(2));
    };

    let start = if context == 0 {
        byte_index
    } else {
        text[..byte_index].char_indices().rev().nth(context - 1).map_or(0, |(idx, _)| idx)
    };
    let end_seed = byte_index.saturating_add(match_len).min(text.len());
    let end = if context == 0 {
        end_seed
    } else {
        text[end_seed..].char_indices().nth(context).map_or(text.len(), |(idx, _)| end_seed + idx)
    };
    let prefix = if start > 0 { "..." } else { "" };
    let suffix = if end < text.len() { "..." } else { "" };
    format!("{prefix}{}{suffix}", text[start..end].replace('\n', " "))
}

/// Truncates text on a UTF-8 boundary.
fn truncate_on_char_boundary(text: &str, max_chars: usize) -> String {
    let end =
        if let Some((idx, _)) = text.char_indices().nth(max_chars) { idx } else { text.len() };
    let suffix = if end < text.len() { "..." } else { "" };
    format!("{}{suffix}", text[..end].replace('\n', " "))
}

#[cfg(test)]
mod tests {
    use super::{lowercase_preserves_offsets, snippet};

    #[test]
    fn case_insensitive_snippet_supports_accented_text() {
        let text = "José wrote the introduction. The deployment failed in Zürich.";
        let result = snippet(text, "DEPLOYMENT", &[], false, 4);

        assert!(result.contains("deployment"));
    }

    #[test]
    fn lowercase_offset_check_accepts_common_accents_and_rejects_expansion() {
        assert!(lowercase_preserves_offsets("José naïve Zürich 🙂"));
        assert!(!lowercase_preserves_offsets("İstanbul"));
    }
}
