import { useEffect, useRef } from "react";
import { LoadingIndicator } from "./LoadingIndicator";

type InfiniteScrollSentinelProps = {
  hasMore: boolean;
  loading: boolean;
  onLoadMore: () => void;
  label?: string;
};

export function InfiniteScrollSentinel({
  hasMore,
  loading,
  onLoadMore,
  label = "Loading…",
}: InfiniteScrollSentinelProps) {
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const element = ref.current;

    if (!element || !hasMore || loading) {
      return;
    }

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting) {
          onLoadMore();
        }
      },
      {
        rootMargin: "400px 0px",
      },
    );

    observer.observe(element);

    return () => {
      observer.disconnect();
    };
  }, [hasMore, loading, onLoadMore]);

  if (!hasMore) {
    return null;
  }

  return <div ref={ref}>{loading ? <LoadingIndicator label={label} compact /> : null}</div>;
}
