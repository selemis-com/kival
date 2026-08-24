import type { ArchiveListStatus, UUID } from "./common.js";

/** Search match kind. */
export type SearchMatchKind = "text" | "literal" | "exact";

/**
 * Search matching model.
 *
 * - `auto` combines normalized full-text matching with literal and exact checks.
 * - `text` uses normalized tokens and PostgreSQL web-search syntax.
 * - `literal` matches one contiguous substring.
 * - `exact` matches the complete stored category value.
 */
export type SearchMode = "auto" | SearchMatchKind;

/** Query parameters for workspace search. */
export type SearchParams = {
  /** Search string. Leading and trailing whitespace is trimmed before matching. */
  q: string;
  /**
   * Comma-separated categories. Omit to search all indexed categories.
   *
   * Accepted values are `title`, `body`, and `metadata`.
   * Categories select where matching occurs; nested metadata paths are not supported.
   */
  categories?: string | null;
  /** Archive status filter. Defaults to active content. */
  status?: ArchiveListStatus | null;
  /** Maximum hits to return. */
  limit?: number | null;
  /** Matching model. Defaults to `auto`. */
  mode?: SearchMode | null;
  /**
   * Use case-sensitive literal and exact comparisons.
   *
   * Full-text matching remains case-insensitive.
   */
  case_sensitive?: boolean | null;
  /** Context characters around snippets. Does not affect matching. */
  context?: number | null;
  /** Include previous immutable object versions. Defaults to current versions only. */
  include_history?: boolean | null;
};

/** One actionable search hit. */
export type SearchHit = {
  /** Workspace ID. */
  workspace_id: UUID;
  /** Object ID. */
  object_id: UUID;
  /** Immutable object version containing the match. */
  version_id: UUID;
  /** Monotonic version number within the object. */
  version_number: number;
  /** Title of the matched version. */
  title: string;
  /** Search category in which the match occurred. */
  matched_category: string;
  /** Match kind. */
  match_kind: SearchMatchKind;
  /** Context snippet. */
  snippet: string;
  /** Relevance score. Higher is better. */
  rank?: number;
};

/** Search response envelope. */
export type SearchResponse = { items: SearchHit[] };
