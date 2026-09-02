import { KivalTransportError } from "kival-sdk";
import { useEffect, useRef, useState } from "react";
import { kival } from "../../../shared/api";
import { styles } from "../../../shared/styles/index";
import type { Group, GroupMembership, User } from "../../../shared/types";
import { AnimatedSelect } from "../../../shared/ui/AnimatedSelect";
import { ConfirmationDialog } from "../../../shared/ui/ConfirmationDialog";
import { LoadingIndicator } from "../../../shared/ui/LoadingIndicator";

type Props = {
  user: User;
  isGlobalAdmin: boolean;
  group: Pick<Group, "id" | "name">;
  onClose: () => void;
  onCurrentUserAuthorityChanged: () => Promise<void>;
};

export function ManageGroupMembersDialog({
  user,
  isGlobalAdmin,
  group,
  onClose,
  onCurrentUserAuthorityChanged,
}: Props) {
  const [memberships, setMemberships] = useState<GroupMembership[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [username, setUsername] = useState("");
  const [role, setRole] = useState<"member" | "admin">("member");
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [revokeTarget, setRevokeTarget] = useState<GroupMembership | null>(null);
  const [revoking, setRevoking] = useState(false);
  const [updatingMembershipId, setUpdatingMembershipId] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [discardOpen, setDiscardOpen] = useState(false);
  const loadGenerationRef = useRef(0);
  const loadedGroupIdRef = useRef<string | null>(null);
  const busy = submitting || revoking || updatingMembershipId !== null;

  useEffect(() => {
    const controller = new AbortController();
    const generation = ++loadGenerationRef.current;
    loadedGroupIdRef.current = null;
    setLoading(true);
    setLoadingMore(false);
    setLoadError(null);
    setNextCursor(null);

    void kival
      .listGroupMemberships({ groupId: group.id, signal: controller.signal })
      .then((response) => {
        if (generation !== loadGenerationRef.current) {
          return;
        }
        loadedGroupIdRef.current = group.id;
        setMemberships(response.items);
        setNextCursor(response.next_cursor ?? null);
      })
      .catch((cause: unknown) => {
        if (
          (cause instanceof KivalTransportError && cause.kind === "abort") ||
          generation !== loadGenerationRef.current
        ) {
          return;
        }

        setLoadError(cause instanceof Error ? cause.message : "Could not load group members.");
      })
      .finally(() => {
        if (!controller.signal.aborted && generation === loadGenerationRef.current) {
          setLoading(false);
        }
      });

    return () => controller.abort();
  }, [group.id]);

  async function handleAddMember(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const normalizedUsername = username.trim();

    if (!normalizedUsername) {
      setActionError("Enter the Kival account username to add.");
      return;
    }

    setSubmitting(true);
    setActionError(null);

    try {
      const membership = await kival.createGroupMembership({
        groupId: group.id,
        input: { username: normalizedUsername, group_role: role },
      });

      setMemberships((current) => [
        membership,
        ...current.filter((candidate) => candidate.id !== membership.id),
      ]);
      setUsername("");
      setRole("member");
    } catch (cause) {
      setActionError(cause instanceof Error ? cause.message : "Could not add this member.");
    } finally {
      setSubmitting(false);
    }
  }

  async function loadMore() {
    if (!nextCursor || loading || loadingMore || loadedGroupIdRef.current !== group.id) {
      return;
    }

    const generation = loadGenerationRef.current;
    const cursor = nextCursor;

    setLoadingMore(true);
    setLoadError(null);

    try {
      const response = await kival.listGroupMemberships({ groupId: group.id, cursor });
      if (generation !== loadGenerationRef.current) {
        return;
      }
      setMemberships((current) => [
        ...current,
        ...response.items.filter(
          (membership) => !current.some((candidate) => candidate.id === membership.id),
        ),
      ]);
      setNextCursor(response.next_cursor ?? null);
    } catch (cause) {
      if (generation === loadGenerationRef.current) {
        setLoadError(cause instanceof Error ? cause.message : "Could not load more group members.");
      }
    } finally {
      if (generation === loadGenerationRef.current) {
        setLoadingMore(false);
      }
    }
  }

  async function handleRevoke() {
    if (!revokeTarget) {
      return;
    }

    setRevoking(true);
    setActionError(null);

    try {
      const membership = await kival.revokeGroupMembership({
        groupId: group.id,
        membershipId: revokeTarget.id,
      });
      setMemberships((current) => current.filter((candidate) => candidate.id !== membership.id));
      setRevokeTarget(null);

      if (!isGlobalAdmin && membership.user_id === user.id) {
        await onCurrentUserAuthorityChanged();
      }
    } catch (cause) {
      setActionError(cause instanceof Error ? cause.message : "Could not remove this member.");
    } finally {
      setRevoking(false);
    }
  }

  async function handleRoleChange(membership: GroupMembership, groupRole: "member" | "admin") {
    if (membership.group_role === groupRole) {
      return;
    }

    setUpdatingMembershipId(membership.id);
    setActionError(null);

    try {
      const updatedMembership = await kival.updateGroupMembership({
        groupId: group.id,
        membershipId: membership.id,
        input: { group_role: groupRole },
      });
      setMemberships((current) =>
        current.map((candidate) =>
          candidate.id === membership.id ? updatedMembership : candidate,
        ),
      );

      if (
        !isGlobalAdmin &&
        updatedMembership.user_id === user.id &&
        updatedMembership.group_role !== "admin"
      ) {
        await onCurrentUserAuthorityChanged();
      }
    } catch (cause) {
      setActionError(
        cause instanceof Error ? cause.message : "Could not change this member's role.",
      );
    } finally {
      setUpdatingMembershipId(null);
    }
  }

  function requestClose() {
    if (busy) {
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
        aria-label="Close group members dialog"
        style={styles.modalBackdropDismiss}
        onClick={requestClose}
      />

      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="manage-group-members-title"
        style={{
          ...styles.modalDialog,
          width: "min(100%, 620px)",
          maxHeight: "min(760px, calc(100vh - 48px))",
          overflowY: "auto",
        }}
      >
        <div style={styles.modalCopy}>
          <h2 id="manage-group-members-title" style={styles.modalTitle}>
            Members of {group.name}
          </h2>
          <p style={styles.muted}>
            Add an existing active Kival account by username. Group administrators can manage
            membership and access.
          </p>
        </div>

        <form
          style={{ display: "flex", flexDirection: "column", gap: 14 }}
          onSubmit={(event) => void handleAddMember(event)}
        >
          <label style={styles.field}>
            <span style={styles.fieldLabel}>Account username</span>
            <input
              data-1p-ignore="true"
              autoFocus
              required
              autoComplete="off"
              value={username}
              maxLength={30}
              placeholder="alice"
              style={styles.input}
              disabled={busy}
              onChange={(event) => setUsername(event.target.value)}
            />
          </label>

          <label htmlFor="group-membership-role" style={styles.field}>
            <span style={styles.fieldLabel}>Group role</span>
            <AnimatedSelect
              id="group-membership-role"
              value={role}
              style={styles.input}
              disabled={busy}
              onChange={(event) => setRole(event.target.value as "member" | "admin")}
            >
              <option value="member">Member</option>
              <option value="admin">Administrator</option>
            </AnimatedSelect>
          </label>

          {actionError && (
            <div style={styles.loginError} role="alert">
              {actionError}
            </div>
          )}

          <div style={styles.modalActions}>
            <button type="submit" disabled={busy} style={styles.primaryButtonCompact}>
              {submitting ? "Adding…" : "Add member"}
            </button>
          </div>
        </form>

        <section aria-label={`${group.name} members`}>
          <div style={styles.sectionHeader}>
            <h3 style={styles.sectionTitle}>Current members</h3>
            <span style={styles.muted}>{memberships.length}</span>
          </div>

          {loading && <LoadingIndicator label="Loading members…" compact />}

          {!loading && loadError && (
            <div style={styles.errorBox} role="alert">
              <strong>Could not load members</strong>
              <span>{loadError}</span>
            </div>
          )}

          {!loading && !loadError && (
            <div className="kival-row-list" style={styles.directoryList}>
              {memberships.map((membership) => (
                <div key={membership.id} style={styles.directoryRow}>
                  <div style={styles.directoryIdentity}>
                    <div style={styles.directoryAvatar}>
                      {membership.user_display_name.slice(0, 1).toUpperCase() || "R"}
                    </div>
                    <div style={styles.directoryMain}>
                      <strong>{membership.user_display_name}</strong>
                      <span style={styles.objectMeta}>{membership.user_username}</span>
                    </div>
                  </div>
                  <div style={styles.directoryHeaderActions}>
                    <AnimatedSelect
                      wrapperStyle={{ minWidth: 116 }}
                      aria-label={`Group role for ${membership.user_display_name}`}
                      value={membership.group_role}
                      style={styles.input}
                      disabled={busy}
                      onChange={(event) =>
                        void handleRoleChange(membership, event.target.value as "member" | "admin")
                      }
                    >
                      <option value="member">Member</option>
                      <option value="admin">Administrator</option>
                    </AnimatedSelect>
                    <button
                      type="button"
                      style={styles.apiKeyDangerButton}
                      disabled={busy}
                      onClick={() => setRevokeTarget(membership)}
                    >
                      Remove
                    </button>
                  </div>
                </div>
              ))}

              {memberships.length === 0 && (
                <div style={styles.emptyState}>
                  <strong>No group members</strong>
                  <span>Add the first member using their Kival username.</span>
                </div>
              )}
            </div>
          )}

          {!loading && nextCursor && (
            <div style={{ display: "flex", justifyContent: "center", paddingTop: 12 }}>
              <button
                type="button"
                disabled={loadingMore}
                style={styles.secondaryButtonCompact}
                onClick={() => void loadMore()}
              >
                {loadingMore ? "Loading…" : "Load more"}
              </button>
            </div>
          )}
        </section>

        <div style={styles.modalActions}>
          <button
            type="button"
            disabled={busy}
            style={styles.secondaryButton}
            onClick={requestClose}
          >
            Close
          </button>
        </div>
      </div>

      {revokeTarget && (
        <div style={{ ...styles.modalBackdrop, zIndex: 120 }} role="presentation">
          <button
            type="button"
            aria-label="Cancel member removal"
            style={styles.modalBackdropDismiss}
            onClick={() => !revoking && setRevokeTarget(null)}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="remove-group-member-title"
            style={styles.modalDialog}
          >
            <div style={styles.modalCopy}>
              <h2 id="remove-group-member-title" style={styles.modalTitle}>
                Remove {revokeTarget.user_display_name}?
              </h2>
              <p style={styles.muted}>
                They will lose access inherited through {group.name} immediately.
              </p>
            </div>
            <div style={styles.modalActions}>
              <button
                type="button"
                disabled={revoking}
                style={styles.secondaryButton}
                onClick={() => setRevokeTarget(null)}
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={revoking}
                style={styles.apiKeyDangerButtonSolid}
                onClick={() => void handleRevoke()}
              >
                {revoking ? "Removing…" : "Remove member"}
              </button>
            </div>
          </div>
        </div>
      )}

      {discardOpen ? (
        <ConfirmationDialog
          title="Discard member changes?"
          description="The selected account and group role have not been added."
          confirmLabel="Discard changes"
          pendingLabel="Discarding…"
          closeLabel="Keep editing group member"
          zIndex={130}
          onCancel={() => setDiscardOpen(false)}
          onConfirm={onClose}
        />
      ) : null}
    </div>
  );
}
