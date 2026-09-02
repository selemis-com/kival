import { useEffect, useState } from "react";

type LoadingIndicatorProps = {
  label?: string;
  compact?: boolean;
  delayMs?: number;
};

export function LoadingIndicator({
  label = "Loading…",
  compact = false,
  delayMs = 300,
}: LoadingIndicatorProps) {
  const [visible, setVisible] = useState(delayMs <= 0);

  useEffect(() => {
    if (delayMs <= 0) {
      setVisible(true);
      return;
    }

    setVisible(false);
    const timer = window.setTimeout(() => setVisible(true), delayMs);
    return () => window.clearTimeout(timer);
  }, [delayMs]);

  if (!visible) {
    return null;
  }

  return (
    <div
      className={`kival-loading-indicator${compact ? " kival-loading-indicator-compact" : ""}`}
      role="status"
    >
      <span className="kival-loading-dots" aria-hidden="true">
        <span />
        <span />
        <span />
      </span>
      <span className="kival-visually-hidden">{label}</span>
    </div>
  );
}
