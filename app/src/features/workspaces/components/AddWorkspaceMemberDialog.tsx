import { useState } from "react";
import { styles } from "../../../shared/styles/index";
import type { CreateWorkspaceMembershipRequest, WorkspaceMembership } from "../../../shared/types";
import { AnimatedSelect } from "../../../shared/ui/AnimatedSelect";
import { ConfirmationDialog } from "../../../shared/ui/ConfirmationDialog";

type Props = {
  workspaceName: string;
  onClose: () => void;
  onAdd: (input: CreateWorkspaceMembershipRequest) => Promise<WorkspaceMembership>;
};

export function AddWorkspaceMemberDialog({ workspaceName, onClose, onAdd }: Props) {
  const [username, setUsername] = useState("");
  const [role, setRole] = useState<"member" | "admin">("member");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [discardOpen, setDiscardOpen] = useState(false);

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const normalizedUsername = username.trim();

    if (!normalizedUsername) {
      setError("Enter the Kival account username to add.");
      return;
    }

    setSubmitting(true);
    setError(null);

    try {
      await onAdd({ username: normalizedUsername, workspace_role: role });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not add this member.");
      setSubmitting(false);
    }
  }

  function requestClose() {
    if (submitting) {
      return;
    }

    if (username.length > 0 || role !== "member") {
      setDiscardOpen(true);
      return;
    }

    onClose();
  }

  return (
    <div style={styles.modalBackdrop} role="presentation">
      <button
        type="button"
        aria-label="Close add workspace member dialog"
        style={styles.modalBackdropDismiss}
        onClick={requestClose}
      />

      <form
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-workspace-member-title"
        style={styles.modalDialog}
        onSubmit={(event) => void handleSubmit(event)}
      >
        <div style={styles.modalCopy}>
          <h2 id="add-workspace-member-title" style={styles.modalTitle}>
            Add to {workspaceName}
          </h2>
          <p style={styles.muted}>
            Add an existing active Kival account by username. A Kival administrator must create the
            account first if it does not exist.
          </p>
        </div>

        <label style={styles.field}>
          <span style={styles.fieldLabel}>Account username</span>
          <input
            data-1p-ignore="true"
            autoFocus
            required
            type="text"
            autoComplete="off"
            maxLength={30}
            value={username}
            placeholder="alice"
            style={styles.input}
            onChange={(event) => setUsername(event.target.value)}
          />
        </label>

        <label htmlFor="workspace-member-role" style={styles.field}>
          <span style={styles.fieldLabel}>Workspace role</span>
          <AnimatedSelect
            id="workspace-member-role"
            value={role}
            style={styles.input}
            onChange={(event) => setRole(event.target.value as "member" | "admin")}
          >
            <option value="member">Member</option>
            <option value="admin">Administrator</option>
          </AnimatedSelect>
        </label>

        {error && (
          <div style={styles.loginError} role="alert">
            {error}
          </div>
        )}

        <div style={styles.modalActions}>
          <button
            type="button"
            disabled={submitting}
            style={styles.secondaryButton}
            onClick={requestClose}
          >
            Cancel
          </button>
          <button type="submit" disabled={submitting} style={styles.primaryButtonCompact}>
            {submitting ? "Adding…" : "Add member"}
          </button>
        </div>
      </form>

      {discardOpen ? (
        <ConfirmationDialog
          title="Discard new member?"
          description="The selected account and workspace role have not been added."
          confirmLabel="Discard changes"
          pendingLabel="Discarding…"
          closeLabel="Keep editing member invitation"
          zIndex={120}
          onCancel={() => setDiscardOpen(false)}
          onConfirm={onClose}
        />
      ) : null}
    </div>
  );
}
