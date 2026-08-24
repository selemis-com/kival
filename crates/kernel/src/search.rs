//! Search projection bindings.

use sqlx::PgPool;
use uuid::Uuid;

use crate::{ArchiveListStatus, Result, SearchCategory, SearchMatchKind, SearchMode, parse_stored};

/// Search-document match projected from `PostgreSQL`.
#[derive(Debug, Clone)]
pub struct SearchDocumentRow {
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// Object identifier.
    pub object_id: Uuid,
    /// Object-version identifier.
    pub version_id: Uuid,
    /// Monotonic version number within the object.
    pub version_number: i64,
    /// Object title.
    pub title: String,
    /// Search-document category.
    pub category: SearchCategory,
    /// Indexed document text.
    pub text: String,
    /// Classification of the strongest match.
    pub match_kind: SearchMatchKind,
    /// Computed search rank, when available.
    pub rank: Option<f32>,
}

/// Parameters for a workspace search query.
#[derive(Debug, Clone, Copy)]
pub struct SearchDocuments<'a> {
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// Search query text.
    pub query: &'a str,
    /// Search-document categories to include.
    pub categories: &'a [SearchCategory],
    /// User identifier.
    pub user_id: Uuid,
    /// Search matching mode.
    pub mode: SearchMode,
    /// Whether literal and exact matching is case-sensitive.
    pub case_sensitive: bool,
    /// Lifecycle status.
    pub status: ArchiveListStatus,
    /// Whether historical object versions are eligible.
    pub include_history: bool,
    /// Maximum number of rows to return.
    pub limit: i64,
}

/// Searches visible object-version documents in a workspace.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn search_documents(
    pool: &PgPool,
    input: SearchDocuments<'_>,
) -> Result<Vec<SearchDocumentRow>> {
    let categories: Vec<String> = input.categories.iter().map(ToString::to_string).collect();

    #[derive(sqlx::FromRow)]
    struct StoredSearchDocumentRow {
        workspace_id: Uuid,
        object_id: Uuid,
        version_id: Uuid,
        version_number: i64,
        title: String,
        category: String,
        text: String,
        match_kind: String,
        rank: Option<f32>,
    }

    let rows = sqlx::query_as::<_, StoredSearchDocumentRow>(
        r#"
        WITH workspace_access AS MATERIALIZED (
            SELECT kival.require_read_workspace($1, $4) AS allowed
        ),
        search_query AS (
            SELECT websearch_to_tsquery('simple', $2) AS tsq
        ),
        candidates AS (
            SELECT
                sd.workspace_id,
                sd.object_id,
                sd.version_id,
                version.title,
                version.version_number,
                sd.category,
                sd.text,
                (sd.search_vector @@ search_query.tsq) AS matched_text,
                CASE
                    WHEN $6::bool THEN position($2 in sd.text) > 0
                    ELSE position(lower($2) in lower(sd.text)) > 0
                END AS matched_literal,
                CASE
                    WHEN sd.search_vector @@ search_query.tsq
                        THEN ts_rank(sd.search_vector, search_query.tsq)
                    ELSE 0.0::real
                END AS text_rank,
                CASE
                    WHEN $6::bool THEN sd.text = $2
                    ELSE lower(sd.text) = lower($2)
                END AS matched_exact
            FROM kival.search_documents sd
            CROSS JOIN search_query
            JOIN kival.objects object
                ON object.workspace_id = sd.workspace_id
                AND object.id = sd.object_id
            JOIN kival.object_versions version
                ON version.object_id = sd.object_id
                AND version.id = sd.version_id
            WHERE sd.workspace_id = $1
                AND (cardinality($3::text[]) = 0 OR sd.category = ANY($3::text[]))
                AND (
                    $7 = 'all'
                    OR ($7 = 'active' AND object.archived_at IS NULL)
                    OR ($7 = 'archived' AND object.archived_at IS NOT NULL)
                )
                AND ($8::bool OR object.current_version_id = sd.version_id)
                AND kival.has_object_permission(
                    sd.workspace_id,
                    sd.object_id,
                    $4,
                    CASE
                        WHEN object.archived_at IS NULL THEN 'viewer'::kival.object_role
                        ELSE 'admin'::kival.object_role
                    END
                )
        ),
        matched AS (
            SELECT
                candidates.*,
                CASE candidates.category
                    WHEN 'title' THEN 6.0::real
                    WHEN 'body' THEN 2.0::real
                    WHEN 'metadata' THEN 1.0::real
                    ELSE 0.0::real
                END AS category_weight,
                CASE
                    WHEN $5 = 'exact' THEN 3.0::real
                    WHEN $5 = 'literal' THEN 2.0::real
                    WHEN $5 = 'text' THEN 1.0::real
                    WHEN candidates.matched_exact THEN 3.0::real
                    WHEN candidates.matched_literal THEN 2.0::real
                    WHEN candidates.matched_text THEN 1.0::real
                    ELSE 0.0::real
                END AS match_weight
            FROM candidates
            WHERE CASE $5
                WHEN 'text' THEN candidates.matched_text
                WHEN 'exact' THEN candidates.matched_exact
                WHEN 'literal' THEN candidates.matched_literal
                ELSE candidates.matched_text OR candidates.matched_literal
            END
        ),
        ranked AS (
            SELECT
                workspace_id,
                object_id,
                version_id,
                version_number,
                title,
                category,
                text,
                CASE
                    WHEN $5 = 'exact' THEN 'exact'
                    WHEN $5 = 'literal' THEN 'literal'
                    WHEN $5 = 'text' THEN 'text'
                    WHEN matched_exact THEN 'exact'
                    WHEN matched_literal THEN 'literal'
                    ELSE 'text'
                END AS match_kind,
                category_weight + match_weight + text_rank AS rank
            FROM matched
        ),
        deduplicated AS (
            SELECT DISTINCT ON (object_id, version_id)
                workspace_id,
                object_id,
                version_id,
                version_number,
                title,
                category,
                text,
                match_kind,
                rank
            FROM ranked
            ORDER BY object_id, version_id, rank DESC, category
        )
        SELECT
            result.workspace_id,
            result.object_id,
            result.version_id,
            result.version_number,
            result.title,
            result.category,
            result.text,
            result.match_kind,
            result.rank
        FROM workspace_access
        CROSS JOIN LATERAL (
            SELECT
                workspace_id,
                object_id,
                version_id,
                version_number,
                title,
                category,
                text,
                match_kind,
                rank
            FROM deduplicated
            WHERE workspace_access.allowed
            ORDER BY rank DESC, object_id, version_number DESC, version_id
            LIMIT $9
        ) result
        "#,
    )
    .bind(input.workspace_id)
    .bind(input.query)
    .bind(categories)
    .bind(input.user_id)
    .bind(input.mode.as_str())
    .bind(input.case_sensitive)
    .bind(input.status.as_str())
    .bind(input.include_history)
    .bind(input.limit)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(SearchDocumentRow {
                workspace_id: row.workspace_id,
                object_id: row.object_id,
                version_id: row.version_id,
                version_number: row.version_number,
                title: row.title,
                category: parse_stored("search category", row.category)?,
                text: row.text,
                match_kind: parse_stored("search match kind", row.match_kind)?,
                rank: row.rank,
            })
        })
        .collect()
}
