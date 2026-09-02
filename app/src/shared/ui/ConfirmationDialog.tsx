import { type ReactNode, useEffect, useId } from "react";
import { styles } from "../styles";

type Props = {
  title: ReactNode;
  description: ReactNode;
  confirmLabel: string;
  pendingLabel: string;
  pending?: boolean;
  error?: string | null;
  errorTitle?: string;
  closeLabel?: string;
  zIndex?: number;
  onCancel: () => void;
  onConfirm: () => void;
};

export function ConfirmationDialog({
  title,
  description,
  confirmLabel,
  pendingLabel,
  pending = false,
  error,
  errorTitle = "Action failed",
  closeLabel = "Close confirmation",
  zIndex,
  onCancel,
  onConfirm,
}: Props) {
  const titleId = useId();
  const descriptionId = useId();

  useEffect(() => {
    if (pending) {
      return;
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onCancel();
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onCancel, pending]);

  return (
    <div
      style={zIndex === undefined ? styles.modalBackdrop : { ...styles.modalBackdrop, zIndex }}
      role="presentation"
    >
      <button
        type="button"
        style={styles.modalBackdropDismiss}
        aria-label={closeLabel}
        disabled={pending}
        onClick={onCancel}
      />
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
        style={styles.modalDialog}
      >
        <div style={styles.modalCopy}>
          <h2 id={titleId} style={styles.modalTitle}>
            {title}
          </h2>
          <p id={descriptionId} style={styles.muted}>
            {description}
          </p>
        </div>

        {error ? (
          <div style={styles.errorBox} role="alert">
            <strong>{errorTitle}</strong>
            <span>{error}</span>
          </div>
        ) : null}

        <div style={styles.modalActions}>
          <button
            type="button"
            style={styles.secondaryButton}
            disabled={pending}
            onClick={onCancel}
          >
            Cancel
          </button>
          <button type="button" style={styles.dangerButton} disabled={pending} onClick={onConfirm}>
            {pending ? pendingLabel : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
