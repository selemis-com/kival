import { useEffect } from "react";
import { styles } from "../styles/index";

type Props = {
  message: string | null;
  onDismiss: () => void;
};

export function Toast({ message, onDismiss }: Props) {
  useEffect(() => {
    if (!message) {
      return;
    }

    const timeout = window.setTimeout(onDismiss, 3200);
    return () => window.clearTimeout(timeout);
  }, [message, onDismiss]);

  if (!message) {
    return null;
  }

  return (
    <div className="kival-dark-panel" role="status" aria-live="polite" style={styles.toast}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <path
          d="m5 12 4 4L19 6"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
      <span>{message}</span>
      <button
        type="button"
        style={styles.toastDismiss}
        aria-label="Dismiss notification"
        onClick={onDismiss}
      >
        ×
      </button>
    </div>
  );
}
