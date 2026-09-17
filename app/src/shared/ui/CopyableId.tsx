import type { CSSProperties } from "react";
import { useEffect, useRef, useState } from "react";
import { styles } from "../styles/index";

type Props = {
  value: string;
  displayValue?: string;
  label?: string;
  style?: CSSProperties;
  iconOnly?: boolean;
};

type CopyStatus = "idle" | "copied" | "failed";

export function CopyableId({
  value,
  displayValue = value,
  label = "ID",
  style,
  iconOnly = false,
}: Props) {
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
  const shortValue = value.slice(0, 8);
  const renderedValue = displayValue.includes(value)
    ? displayValue.replaceAll(value, value.toUpperCase())
    : displayValue.includes(shortValue)
      ? displayValue.replaceAll(shortValue, shortValue.toUpperCase())
      : displayValue;

  return (
    <button
      type="button"
      style={{ ...styles.copyableId, ...(iconOnly ? styles.copyableIdIcon : {}), ...style }}
      onClick={() => void copyId()}
      title={`Copy ${label.toLowerCase()}`}
      aria-label={`Copy ${label}: ${value}`}
    >
      {iconOnly ? (
        status === "copied" ? (
          <svg
            aria-hidden="true"
            width="16"
            height="16"
            viewBox="0 0 20 20"
            fill="none"
            xmlns="http://www.w3.org/2000/svg"
          >
            <path
              d="m4 10.3 3.8 3.8 8.4-8.2"
              stroke="currentColor"
              strokeWidth="1.35"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        ) : (
          <svg
            aria-hidden="true"
            width="16"
            height="16"
            viewBox="0 0 20 20"
            fill="none"
            xmlns="http://www.w3.org/2000/svg"
          >
            <rect
              x="6.5"
              y="6.5"
              width="9.5"
              height="9.5"
              rx="1.8"
              stroke="currentColor"
              strokeWidth="1.35"
            />
            <path
              d="M13.5 6.5V4.25A1.75 1.75 0 0 0 11.75 2.5h-7.5A1.75 1.75 0 0 0 2.5 4.25v7.5a1.75 1.75 0 0 0 1.75 1.75H6.5"
              stroke="currentColor"
              strokeWidth="1.35"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        )
      ) : (
        <span>{renderedValue}</span>
      )}
      {!iconOnly && feedback && (
        <span style={styles.copyableIdFeedback} aria-live="polite">
          {feedback}
        </span>
      )}
      {iconOnly && feedback && (
        <span style={styles.visuallyHidden} aria-live="polite">
          {feedback}
        </span>
      )}
    </button>
  );
}
