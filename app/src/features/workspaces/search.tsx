import { styles } from "../../shared/styles/index";
import type { SearchHit } from "../../shared/types";

export type GroupedSearchResult = {
  objectId: string;
  versionId: string;
  versionNumber: number;
  title: string;
  snippets: string[];
  categories: string[];
  matchCount: number;
};

const categoryPriority = ["title", "body", "metadata"];

function normalizeSearchCategory(category: string) {
  return category.replaceAll("_", " ");
}

export function groupWorkspaceSearchResults(hits: SearchHit[]): GroupedSearchResult[] {
  const grouped = new Map<string, GroupedSearchResult>();

  for (const hit of hits) {
    const objectId = hit.object_id;
    const resultKey = `${objectId}:${hit.version_id}`;

    const category = normalizeSearchCategory(hit.matched_category);
    const existing = grouped.get(resultKey);

    if (existing) {
      existing.matchCount += 1;

      if (!existing.categories.includes(category)) {
        existing.categories.push(category);
      }

      if (existing.title === existing.snippets[0]) {
        existing.title = hit.title;
      }

      if (hit.snippet && hit.snippet !== hit.title && !existing.snippets.includes(hit.snippet)) {
        existing.snippets.push(hit.snippet);
      }

      continue;
    }

    grouped.set(resultKey, {
      objectId,
      versionId: hit.version_id,
      versionNumber: hit.version_number,
      title: hit.title,
      snippets: hit.snippet && hit.snippet !== hit.title ? [hit.snippet] : [],
      categories: [category],
      matchCount: 1,
    });
  }

  return Array.from(grouped.values()).map((result) => ({
    ...result,
    categories: result.categories.sort((left, right) => {
      const leftPriority = categoryPriority.indexOf(left);
      const rightPriority = categoryPriority.indexOf(right);

      if (leftPriority === -1 && rightPriority === -1) {
        return left.localeCompare(right);
      }

      if (leftPriority === -1) {
        return 1;
      }

      if (rightPriority === -1) {
        return -1;
      }

      return leftPriority - rightPriority;
    }),
  }));
}

type HighlightedSearchTextProps = {
  value: string;
  query: string;
};

export function HighlightedSearchText({ value, query }: HighlightedSearchTextProps) {
  const normalizedQuery = query.trim();

  if (!normalizedQuery) {
    return value;
  }

  const lowerValue = value.toLowerCase();
  const lowerQuery = normalizedQuery.toLowerCase();
  const parts: Array<string | { match: string; key: string }> = [];
  let cursor = 0;
  let matchIndex = lowerValue.indexOf(lowerQuery);

  while (matchIndex !== -1) {
    if (matchIndex > cursor) {
      parts.push(value.slice(cursor, matchIndex));
    }

    const match = value.slice(matchIndex, matchIndex + normalizedQuery.length);
    parts.push({ match, key: `${matchIndex}-${match}` });
    cursor = matchIndex + normalizedQuery.length;
    matchIndex = lowerValue.indexOf(lowerQuery, cursor);
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
