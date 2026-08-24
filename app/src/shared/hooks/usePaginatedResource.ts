import { KivalTransportError } from "kival-sdk";
import { useEffect, useRef, useState } from "react";

type Page<T, Cursor> = {
  items: T[];
  nextCursor: Cursor | null;
};

type Options<T, Cursor> = {
  queryKey: string;
  loadPage: (cursor: Cursor | null, signal: AbortSignal) => Promise<Page<T, Cursor>>;
  enabled?: boolean;
  debounceMs?: number;
  errorMessage: string;
  itemKey?: (item: T) => string;
  clearItemsOnError?: boolean;
};

type RequestIdentity = {
  queryKey: string;
  reloadToken: number;
  enabled: boolean;
};

export function usePaginatedResource<T, Cursor = string>({
  queryKey,
  loadPage,
  enabled = true,
  debounceMs = 0,
  errorMessage,
  itemKey,
  clearItemsOnError = false,
}: Options<T, Cursor>) {
  const [items, setItems] = useState<T[]>([]);
  const [nextCursor, setNextCursor] = useState<Cursor | null>(null);
  const [loading, setLoading] = useState(enabled);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const [settledRequest, setSettledRequest] = useState<RequestIdentity | null>(null);
  const activeRequestRef = useRef<RequestIdentity | null>(null);
  const loadMoreControllerRef = useRef<AbortController | null>(null);

  useEffect(() => {
    const request = { queryKey, reloadToken, enabled };
    activeRequestRef.current = request;
    loadMoreControllerRef.current?.abort();
    setLoadingMore(false);

    if (!enabled) {
      setItems([]);
      setNextCursor(null);
      setLoading(false);
      setError(null);
      setSettledRequest(request);
      return;
    }

    const controller = new AbortController();
    setLoading(true);
    setError(null);
    setNextCursor(null);

    const timeout = window.setTimeout(() => {
      void loadPage(null, controller.signal)
        .then((page) => {
          if (controller.signal.aborted || activeRequestRef.current !== request) {
            return;
          }

          setItems(page.items);
          setNextCursor(page.nextCursor);
        })
        .catch((cause: unknown) => {
          if (
            (cause instanceof KivalTransportError && cause.kind === "abort") ||
            activeRequestRef.current !== request
          ) {
            return;
          }

          if (clearItemsOnError) {
            setItems([]);
            setNextCursor(null);
          }
          setError(cause instanceof Error ? cause.message : errorMessage);
        })
        .finally(() => {
          if (!controller.signal.aborted && activeRequestRef.current === request) {
            setLoading(false);
            setSettledRequest(request);
          }
        });
    }, debounceMs);

    return () => {
      window.clearTimeout(timeout);
      controller.abort();
    };
  }, [clearItemsOnError, debounceMs, enabled, errorMessage, loadPage, queryKey, reloadToken]);

  useEffect(() => () => loadMoreControllerRef.current?.abort(), []);

  async function loadMore() {
    const request = activeRequestRef.current;
    const cursorBelongsToCurrentQuery =
      request !== null &&
      settledRequest !== null &&
      request.queryKey === queryKey &&
      request.reloadToken === reloadToken &&
      request.enabled === enabled &&
      settledRequest.queryKey === queryKey &&
      settledRequest.reloadToken === reloadToken &&
      settledRequest.enabled === enabled;

    if (!enabled || !cursorBelongsToCurrentQuery || nextCursor === null || loading || loadingMore) {
      return;
    }

    const cursor = nextCursor;
    const controller = new AbortController();
    loadMoreControllerRef.current?.abort();
    loadMoreControllerRef.current = controller;
    setLoadingMore(true);
    setError(null);

    try {
      const page = await loadPage(cursor, controller.signal);

      if (controller.signal.aborted || activeRequestRef.current !== request) {
        return;
      }

      setItems((current) => appendUnique(current, page.items, itemKey));
      setNextCursor(page.nextCursor);
    } catch (cause) {
      if (
        (cause instanceof KivalTransportError && cause.kind === "abort") ||
        activeRequestRef.current !== request
      ) {
        return;
      }

      setError(cause instanceof Error ? cause.message : errorMessage);
    } finally {
      if (activeRequestRef.current === request) {
        setLoadingMore(false);
      }
      if (loadMoreControllerRef.current === controller) {
        loadMoreControllerRef.current = null;
      }
    }
  }

  function reload() {
    setReloadToken((current) => current + 1);
  }

  const requestPending =
    enabled &&
    (settledRequest === null ||
      settledRequest.queryKey !== queryKey ||
      settledRequest.reloadToken !== reloadToken ||
      settledRequest.enabled !== enabled);

  return {
    items,
    setItems,
    nextCursor,
    loading: enabled && (loading || requestPending),
    loadingMore,
    error,
    setError,
    loadMore,
    reload,
  };
}

function appendUnique<T>(current: T[], incoming: T[], itemKey?: (item: T) => string) {
  if (!itemKey) {
    return [...current, ...incoming];
  }

  const existing = new Set(current.map(itemKey));
  return [...current, ...incoming.filter((item) => !existing.has(itemKey(item)))];
}
