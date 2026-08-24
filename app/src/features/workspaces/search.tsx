import { styles } from "../../shared/styles/index";
import type { SearchHit } from "../../shared/types";

export type WorkspaceSearchResult = {
  objectId: string;
  versionId: string;
  versionNumber: number;
  title: string;
  status: SearchHit["status"];
  metadata: SearchHit["metadata"];
  snippet: string;
  category: string;
  termCoverage?: SearchHit["term_coverage"];
};

function normalizeSearchCategory(category: string) {
  return category.replaceAll("_", " ");
}

export function mapWorkspaceSearchResults(hits: SearchHit[]): WorkspaceSearchResult[] {
  return hits.map((hit) => ({
    objectId: hit.object_id,
    versionId: hit.version_id,
    versionNumber: hit.version_number,
    title: hit.title,
    status: hit.status,
    metadata: hit.metadata,
    snippet: hit.snippet && hit.snippet !== hit.title ? hit.snippet : "",
    category: normalizeSearchCategory(hit.matched_category),
    termCoverage: hit.term_coverage,
  }));
}

type HighlightedSearchTextProps = {
  value: string;
  query: string;
  matchedTerms?: string[];
};

export function HighlightedSearchText({ value, query, matchedTerms }: HighlightedSearchTextProps) {
  const normalizedQuery = query.trim();

  if (!normalizedQuery) {
    return value;
  }

  const lowerValue = value.toLowerCase();
  const lowerQuery = normalizedQuery.toLowerCase();
  const needles = lowerValue.includes(lowerQuery)
    ? [normalizedQuery]
    : Array.from(
        new Set(
          (matchedTerms ?? [])
            .map((term) => term.trim())
            .filter((term) => term.length > 0),
        ),
      ).sort((left, right) => right.length - left.length);

  if (needles.length === 0) {
    return value;
  }

  const parts: Array<string | { match: string; key: string }> = [];
  let cursor = 0;

  while (cursor < value.length) {
    let nextIndex = -1;
    let nextNeedle = "";

    for (const needle of needles) {
      const index = lowerValue.indexOf(needle.toLowerCase(), cursor);
      if (index !== -1 && (nextIndex === -1 || index < nextIndex)) {
        nextIndex = index;
        nextNeedle = needle;
      }
    }

    if (nextIndex === -1) {
      break;
    }

    if (nextIndex > cursor) {
      parts.push(value.slice(cursor, nextIndex));
    }

    const match = value.slice(nextIndex, nextIndex + nextNeedle.length);
    parts.push({ match, key: `${nextIndex}-${match}` });
    cursor = nextIndex + nextNeedle.length;
  }

  if (cursor < value.length) {
    parts.push(value.slice(cursor));
  }

  return parts.map((part) =>
    typeof part === "string" ? (
      part
    ) : (
      <mark key={part.key} style={styles.searchHighlight}>
        {part.match}
      </mark>
    ),
  );
}
