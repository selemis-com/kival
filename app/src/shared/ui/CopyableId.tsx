import type { CSSProperties } from "react";
import { useEffect, useRef, useState } from "react";
import { styles } from "../styles/index";

type Props = {
  value: string;
  displayValue?: string;
  label?: string;
  style?: CSSProperties;
};

type CopyStatus = "idle" | "copied" | "failed";

export function CopyableId({ value, displayValue = value, label = "ID", style }: Props) {
  const [status, setStatus] = useState<CopyStatus>("idle");
  const resetTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (resetTimeout.current) {
        clearTimeout(resetTimeout.current);
      }
    },
    [],
  );

  async function copyId() {
    try {
      await navigator.clipboard.writeText(value);
      setStatus("copied");
    } catch {
      setStatus("failed");
    }

    if (resetTimeout.current) {
      clearTimeout(resetTimeout.current);
    }
    resetTimeout.current = setTimeout(() => setStatus("idle"), 1400);
  }

  const feedback = status === "copied" ? "Copied" : status === "failed" ? "Copy failed" : null;

  return (
    <button
      type="button"
      style={{ ...styles.copyableId, ...style }}
      onClick={() => void copyId()}
      title={`Copy ${label.toLowerCase()}`}
      aria-label={`Copy ${label}: ${value}`}
    >
      <span>{displayValue}</span>
      {feedback && (
        <span style={styles.copyableIdFeedback} aria-live="polite">
          {feedback}
        </span>
      )}
    </button>
  );
}
