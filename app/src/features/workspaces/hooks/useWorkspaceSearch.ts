import { useCallback } from "react";
import { kival } from "../../../shared/api";
import { usePaginatedResource } from "../../../shared/hooks/usePaginatedResource";

export function useWorkspaceSearch(workspaceId: string, query: string, includeHistory = false) {
  const normalizedQuery = query.trim();
  const active = normalizedQuery.length > 0;
  const searchKey = `${workspaceId}:${normalizedQuery}:${includeHistory ? "history" : "current"}`;
  const loadSearchPage = useCallback(
    async (cursor: string | null, signal: AbortSignal) => {
      const response = await kival.searchWorkspace({
        workspaceId,
        q: normalizedQuery,
        ...(includeHistory ? { include_history: true } : {}),
        ...(cursor ? { cursor } : {}),
        signal,
      });
      return { items: response.items, nextCursor: response.next_cursor ?? null };
    },
    [includeHistory, normalizedQuery, workspaceId],
  );
  const {
    items: results,
    nextCursor,
    loading,
    loadingMore,
    error,
    loadMore,
  } = usePaginatedResource({
    queryKey: searchKey,
    loadPage: loadSearchPage,
    enabled: active,
    debounceMs: 150,
    errorMessage: "Could not search this workspace.",
  });

  return {
    active,
    normalizedQuery,
    results,
    nextCursor,
    loading,
    loadingMore,
    error,
    loadMore,
  };
}
