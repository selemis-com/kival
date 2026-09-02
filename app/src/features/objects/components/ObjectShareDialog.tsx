import { KivalTransportError } from "kival-sdk";
import { useEffect, useMemo, useRef, useState } from "react";
import { kival } from "../../../shared/api";
import { styles } from "../../../shared/styles/index";
import type {
  ObjectGrant,
  ObjectRole,
  User,
  WorkspaceGroup,
  WorkspaceMembership,
} from "../../../shared/types";
import { AnimatedSelect } from "../../../shared/ui/AnimatedSelect";
import { ConfirmationDialog } from "../../../shared/ui/ConfirmationDialog";
import { CopyableId } from "../../../shared/ui/CopyableId";
import { LoadingIndicator } from "../../../shared/ui/LoadingIndicator";

type Props = {
  user: User;
  workspaceId: string;
  objectId: string;
  objectTitle: string;
  onClose: () => void;
  onAccessChanged: () => Promise<void>;
};

function memberLabel(membership: WorkspaceMembership) {
  return `${membership.user_display_name} (${membership.user_username})`;
}

export function ObjectShareDialog({
  user,
  workspaceId,
  objectId,
  objectTitle,
  onClose,
  onAccessChanged,
}: Props) {
  const [memberships, setMemberships] = useState<WorkspaceMembership[]>([]);
  const [membershipsNextCursor, setMembershipsNextCursor] = useState<string | null>(null);
  const [groups, setGroups] = useState<WorkspaceGroup[]>([]);
  const [groupsNextCursor, setGroupsNextCursor] = useState<string | null>(null);
  const [grants, setGrants] = useState<ObjectGrant[]>([]);
  const [grantsNextCursor, setGrantsNextCursor] = useState<string | null>(null);
  const [principalType, setPrincipalType] = useState<"user" | "group">("user");
  const [selectedPrincipalId, setSelectedPrincipalId] = useState("");
  const [role, setRole] = useState<ObjectRole>("viewer");
  const [loading, setLoading] = useState(true);
  const [loadingMoreMembers, setLoadingMoreMembers] = useState(false);
  const [loadingMoreGroups, setLoadingMoreGroups] = useState(false);
  const [loadingMoreGrants, setLoadingMoreGrants] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [revokeTarget, setRevokeTarget] = useState<{
    grant: ObjectGrant;
    identity: string;
  } | null>(null);
  const [revokingGrantId, setRevokingGrantId] = useState<string | null>(null);
  const [revokeError, setRevokeError] = useState<string | null>(null);
  const [updatingGrantId, setUpdatingGrantId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [discardOpen, setDiscardOpen] = useState(false);
  const loadGenerationRef = useRef(0);
  const loadedScopeRef = useRef<string | null>(null);
  const scope = `${workspaceId}:${objectId}`;

  useEffect(() => {
    const controller = new AbortController();
    const generation = ++loadGenerationRef.current;
    loadedScopeRef.current = null;
    setLoading(true);
    setLoadingMoreMembers(false);
    setLoadingMoreGroups(false);
    setLoadingMoreGrants(false);
    setError(null);
    setMembershipsNextCursor(null);
    setGroupsNextCursor(null);
    setGrantsNextCursor(null);

    void Promise.all([
      kival.listWorkspaceMemberships({ workspaceId, signal: controller.signal }),
      kival.listWorkspaceGroups({ workspaceId, signal: controller.signal }),
      kival.listObjectGrants({ workspaceId, objectId, signal: controller.signal }),
    ])
      .then(([membershipResponse, groupResponse, grantResponse]) => {
        if (generation !== loadGenerationRef.current) {
          return;
        }
        loadedScopeRef.current = scope;
        setMemberships(membershipResponse.items);
        setMembershipsNextCursor(membershipResponse.next_cursor ?? null);
        setGroups(groupResponse.items);
        setGroupsNextCursor(groupResponse.next_cursor ?? null);
        setGrants(grantResponse.items);
        setGrantsNextCursor(grantResponse.next_cursor ?? null);
      })
      .catch((cause: unknown) => {
        if (
          (cause instanceof KivalTransportError && cause.kind === "abort") ||
          generation !== loadGenerationRef.current
        ) {
          return;
        }

        setError(cause instanceof Error ? cause.message : "Could not load object access.");
      })
      .finally(() => {
        if (!controller.signal.aborted && generation === loadGenerationRef.current) {
          setLoading(false);
        }
      });

    return () => controller.abort();
  }, [objectId, scope, workspaceId]);

  const membershipsByUserId = useMemo(
    () => new Map(memberships.map((membership) => [membership.user_id, membership])),
    [memberships],
  );
  const groupsById = useMemo(
    () => new Map(groups.map((group) => [group.group_id, group])),
    [groups],
  );
  const directlyGrantedUserIds = new Set(
    grants.flatMap((grant) => (grant.principal_user_id ? [grant.principal_user_id] : [])),
  );
  const availableMemberships = memberships.filter(
    (membership) => !directlyGrantedUserIds.has(membership.user_id),
  );
  const directlyGrantedGroupIds = new Set(
    grants.flatMap((grant) => (grant.principal_group_id ? [grant.principal_group_id] : [])),
  );
  const availableGroups = groups.filter((group) => !directlyGrantedGroupIds.has(group.group_id));
  const availablePrincipals = principalType === "user" ? availableMemberships : availableGroups;
  const loadedAdminGrantCount = grants.filter((grant) => grant.object_role === "admin").length;
  const busy = submitting || revokingGrantId !== null || updatingGrantId !== null;

  async function loadMoreMembers() {
    if (
      !membershipsNextCursor ||
      loading ||
      loadingMoreMembers ||
      loadedScopeRef.current !== scope
    ) {
      return;
    }

    const generation = loadGenerationRef.current;
    const cursor = membershipsNextCursor;

    setLoadingMoreMembers(true);
    setError(null);

    try {
      const response = await kival.listWorkspaceMemberships({ workspaceId, cursor });
      if (generation !== loadGenerationRef.current) {
        return;
      }
      setMemberships((current) => [
        ...current,
        ...response.items.filter(
          (membership) => !current.some((candidate) => candidate.user_id === membership.user_id),
        ),
      ]);
      setMembershipsNextCursor(response.next_cursor ?? null);
    } catch (cause) {
      if (generation === loadGenerationRef.current) {
        setError(cause instanceof Error ? cause.message : "Could not load more workspace members.");
      }
    } finally {
      if (generation === loadGenerationRef.current) {
        setLoadingMoreMembers(false);
      }
    }
  }

  async function loadMoreGrants() {
    if (!grantsNextCursor || loading || loadingMoreGrants || loadedScopeRef.current !== scope) {
      return;
    }

    const generation = loadGenerationRef.current;
    const cursor = grantsNextCursor;

    setLoadingMoreGrants(true);
    setError(null);

    try {
      const response = await kival.listObjectGrants({ workspaceId, objectId, cursor });
      if (generation !== loadGenerationRef.current) {
        return;
      }
      setGrants((current) => [
        ...current,
        ...response.items.filter(
          (grant) => !current.some((candidate) => candidate.id === grant.id),
        ),
      ]);
      setGrantsNextCursor(response.next_cursor ?? null);
    } catch (cause) {
      if (generation === loadGenerationRef.current) {
        setError(cause instanceof Error ? cause.message : "Could not load more object access.");
      }
    } finally {
      if (generation === loadGenerationRef.current) {
        setLoadingMoreGrants(false);
      }
    }
  }

  async function loadMoreGroups() {
    if (!groupsNextCursor || loading || loadingMoreGroups || loadedScopeRef.current !== scope) {
      return;
    }

    const generation = loadGenerationRef.current;
    const cursor = groupsNextCursor;

    setLoadingMoreGroups(true);
    setError(null);

    try {
      const response = await kival.listWorkspaceGroups({ workspaceId, cursor });
      if (generation !== loadGenerationRef.current) {
        return;
      }
      setGroups((current) => [
        ...current,
        ...response.items.filter(
          (group) => !current.some((candidate) => candidate.group_id === group.group_id),
        ),
      ]);
      setGroupsNextCursor(response.next_cursor ?? null);
    } catch (cause) {
      if (generation === loadGenerationRef.current) {
        setError(cause instanceof Error ? cause.message : "Could not load more workspace groups.");
      }
    } finally {
      if (generation === loadGenerationRef.current) {
        setLoadingMoreGroups(false);
      }
    }
  }

  async function refreshAccessAfterMutation() {
    try {
      await onAccessChanged();
    } catch (cause) {
      const detail = cause instanceof Error ? `: ${cause.message}` : ".";
      setError(`Access changed, but current permissions could not be refreshed${detail}`);
    }
  }

  async function handleShare(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();

    if (!selectedPrincipalId) {
      setError(
        principalType === "user" ? "Choose a workspace member." : "Choose a workspace group.",
      );
      return;
    }

    setSubmitting(true);
    setError(null);

    try {
      const grant = await kival.createObjectGrant({
        workspaceId,
        objectId,
        input: {
          principal: { type: principalType, id: selectedPrincipalId },
          object_role: role,
        },
      });
      setGrants((current) => [grant, ...current.filter((candidate) => candidate.id !== grant.id)]);
      setPrincipalType("user");
      setSelectedPrincipalId("");
      setRole("viewer");
      await refreshAccessAfterMutation();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not share this object.");
    } finally {
      setSubmitting(false);
    }
  }

  async function handleRevoke() {
    if (!revokeTarget || revokingGrantId) {
      return;
    }

    const { grant } = revokeTarget;
    setRevokingGrantId(grant.id);
    setRevokeError(null);

    try {
      await kival.revokeObjectGrant({ workspaceId, objectId, grantId: grant.id });
      setGrants((current) => current.filter((candidate) => candidate.id !== grant.id));
      await refreshAccessAfterMutation();
      setRevokeTarget(null);
    } catch (cause) {
      setRevokeError(cause instanceof Error ? cause.message : "Could not remove this access.");
    } finally {
      setRevokingGrantId(null);
    }
  }

  async function handleRoleChange(grant: ObjectGrant, objectRole: ObjectRole) {
    if (grant.object_role === objectRole) {
      return;
    }

    setUpdatingGrantId(grant.id);
    setError(null);

    try {
      const updatedGrant = await kival.updateObjectGrant({
        workspaceId,
        objectId,
        grantId: grant.id,
        input: { object_role: objectRole },
      });
      setGrants((current) =>
        current.map((candidate) => (candidate.id === grant.id ? updatedGrant : candidate)),
      );
      await refreshAccessAfterMutation();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not change this access level.");
    } finally {
      setUpdatingGrantId(null);
    }
  }

  function requestClose() {
    if (busy) {
      return;
    }

    if (principalType !== "user" || selectedPrincipalId.length > 0 || role !== "viewer") {
      setDiscardOpen(true);
      return;
    }

    onClose();
  }

  return (
    <div style={styles.modalBackdrop} role="presentation">
      <button
        type="button"
        aria-label="Close object sharing dialog"
        style={styles.modalBackdropDismiss}
        onClick={requestClose}
      />

      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="share-object-title"
        style={{
          ...styles.modalDialog,
          width: "min(100%, 620px)",
          maxHeight: "min(780px, calc(100vh - 48px))",
          overflowY: "auto",
        }}
      >
        <div style={styles.modalCopy}>
          <h2 id="share-object-title" style={styles.modalTitle}>
            Share “{objectTitle}”
          </h2>
          <p style={styles.muted}>
            Give an existing workspace member or linked group access to this object.
          </p>
        </div>

        {loading && <LoadingIndicator label="Loading access…" compact />}

        {!loading && (
          <>
            <form
              style={{ display: "flex", flexDirection: "column", gap: 14 }}
              onSubmit={(event) => void handleShare(event)}
            >
              <label htmlFor="object-share-principal-type" style={styles.field}>
                <span style={styles.fieldLabel}>Share with</span>
                <AnimatedSelect
                  id="object-share-principal-type"
                  value={principalType}
                  style={styles.input}
                  disabled={submitting}
                  onChange={(event) => {
                    setPrincipalType(event.target.value as "user" | "group");
                    setSelectedPrincipalId("");
                  }}
                >
                  <option value="user">Person</option>
                  <option value="group">Group</option>
                </AnimatedSelect>
              </label>

              <label htmlFor="object-share-principal" style={styles.field}>
                <span style={styles.fieldLabel}>
                  {principalType === "user" ? "Workspace member" : "Workspace group"}
                </span>
                <AnimatedSelect
                  id="object-share-principal"
                  autoFocus
                  value={selectedPrincipalId}
                  style={styles.input}
                  disabled={submitting || availablePrincipals.length === 0}
                  onChange={(event) => setSelectedPrincipalId(event.target.value)}
                >
                  <option value="">
                    {principalType === "user" ? "Choose a member…" : "Choose a group…"}
                  </option>
                  {principalType === "user"
                    ? availableMemberships.map((membership) => (
                        <option key={membership.user_id} value={membership.user_id}>
                          {memberLabel(membership)}
                        </option>
                      ))
                    : availableGroups.map((group) => (
                        <option key={group.group_id} value={group.group_id}>
                          {group.group_name}
                        </option>
                      ))}
                </AnimatedSelect>
              </label>

              {principalType === "user" && membershipsNextCursor && (
                <button
                  type="button"
                  style={styles.secondaryButton}
                  disabled={loadingMoreMembers}
                  onClick={() => void loadMoreMembers()}
                >
                  {loadingMoreMembers ? "Loading members…" : "Load more members"}
                </button>
              )}

              {principalType === "group" && groupsNextCursor && (
                <button
                  type="button"
                  style={styles.secondaryButton}
                  disabled={loadingMoreGroups}
                  onClick={() => void loadMoreGroups()}
                >
                  {loadingMoreGroups ? "Loading groups…" : "Load more groups"}
                </button>
              )}

              <label htmlFor="object-share-role" style={styles.field}>
                <span style={styles.fieldLabel}>Access level</span>
                <AnimatedSelect
                  id="object-share-role"
                  value={role}
                  style={styles.input}
                  disabled={submitting}
                  onChange={(event) => setRole(event.target.value as ObjectRole)}
                >
                  <option value="viewer">Viewer — can read and comment</option>
                  <option value="editor">Editor — can read, comment, and edit</option>
                  <option value="admin">Admin — can read, comment, edit, and manage access</option>
                </AnimatedSelect>
              </label>

              <div style={styles.modalActions}>
                <button
                  type="submit"
                  style={styles.primaryButtonCompact}
                  disabled={submitting || !selectedPrincipalId}
                >
                  {submitting ? "Sharing…" : "Share"}
                </button>
              </div>
            </form>

            <section aria-label="People and groups with direct access">
              <div style={styles.sectionHeader}>
                <h3 style={styles.sectionTitle}>Direct access</h3>
                <span style={styles.muted}>{grants.length}</span>
              </div>

              <div className="kival-row-list" style={styles.directoryList}>
                {grants.map((grant) => {
                  const membership = grant.principal_user_id
                    ? membershipsByUserId.get(grant.principal_user_id)
                    : null;
                  const group = grant.principal_group_id
                    ? groupsById.get(grant.principal_group_id)
                    : null;
                  const isCurrentUser = grant.principal_user_id === user.id;
                  const identity = membership
                    ? memberLabel(membership)
                    : group
                      ? group.group_name
                      : grant.principal_group_id
                        ? "Unknown group"
                        : "Unknown user";
                  const principalLabel = grant.principal_group_id ? "Group grant" : "Person grant";
                  const hasImplicitWorkspaceAdminAccess = membership?.workspace_role === "admin";
                  const isKnownLastAdminGrant =
                    grant.object_role === "admin" &&
                    loadedAdminGrantCount === 1 &&
                    !grantsNextCursor;

                  return (
                    <div key={grant.id} style={styles.directoryRow}>
                      <div style={styles.directoryMain}>
                        <strong>
                          {identity}
                          {isCurrentUser ? " (you)" : ""}
                        </strong>
                        <span style={styles.objectMeta}>
                          {principalLabel}
                          {hasImplicitWorkspaceAdminAccess
                            ? " · Workspace administrator has implicit access"
                            : ""}
                        </span>
                        {!membership && grant.principal_user_id ? (
                          <CopyableId
                            value={grant.principal_user_id}
                            displayValue={`User ID: ${grant.principal_user_id}`}
                            label="user ID"
                          />
                        ) : null}
                        {!group && grant.principal_group_id ? (
                          <CopyableId
                            value={grant.principal_group_id}
                            displayValue={`Group ID: ${grant.principal_group_id}`}
                            label="group ID"
                          />
                        ) : null}
                      </div>
                      <div style={styles.directoryHeaderActions}>
                        <AnimatedSelect
                          wrapperStyle={{ minWidth: 104 }}
                          aria-label={`Access level for ${identity}`}
                          value={grant.object_role}
                          style={styles.input}
                          disabled={busy}
                          title={
                            isKnownLastAdminGrant
                              ? "Assign another admin before demoting this grant."
                              : "Change access level"
                          }
                          onChange={(event) =>
                            void handleRoleChange(grant, event.target.value as ObjectRole)
                          }
                        >
                          <option value="viewer" disabled={isKnownLastAdminGrant}>
                            Viewer
                          </option>
                          <option value="editor" disabled={isKnownLastAdminGrant}>
                            Editor
                          </option>
                          <option value="admin">Admin</option>
                        </AnimatedSelect>
                        <button
                          type="button"
                          style={styles.secondaryButtonCompact}
                          disabled={busy || isKnownLastAdminGrant}
                          title={
                            isKnownLastAdminGrant
                              ? "An object must retain at least one admin."
                              : undefined
                          }
                          onClick={() => {
                            setRevokeTarget({ grant, identity });
                            setRevokeError(null);
                          }}
                        >
                          {revokingGrantId === grant.id
                            ? "Removing…"
                            : isKnownLastAdminGrant
                              ? "Required"
                              : "Remove grant"}
                        </button>
                      </div>
                    </div>
                  );
                })}

                {grants.length === 0 && (
                  <div style={styles.emptyState}>
                    <strong>No direct access grants</strong>
                    <span>Workspace administrators may still have implicit access.</span>
                  </div>
                )}
              </div>

              {grantsNextCursor && (
                <button
                  type="button"
                  style={styles.secondaryButton}
                  disabled={loadingMoreGrants}
                  onClick={() => void loadMoreGrants()}
                >
                  {loadingMoreGrants ? "Loading access…" : "Load more access"}
                </button>
              )}
            </section>
          </>
        )}

        {error && (
          <div style={styles.loginError} role="alert">
            {error}
          </div>
        )}

        <div style={styles.modalActions}>
          <button
            type="button"
            disabled={busy}
            style={styles.secondaryButton}
            onClick={requestClose}
          >
            Done
          </button>
        </div>
      </div>

      {revokeTarget ? (
        <ConfirmationDialog
          title={`Remove direct grant for ${revokeTarget.identity}?`}
          description={
            revokeTarget.grant.principal_user_id &&
            membershipsByUserId.get(revokeTarget.grant.principal_user_id)?.workspace_role ===
              "admin"
              ? `This removes the direct grant, but ${revokeTarget.identity} will retain full access to “${objectTitle}” while they are a workspace administrator.`
              : `This removes the direct grant. Access to “${objectTitle}” may remain through another group grant or an administrator role.`
          }
          confirmLabel="Remove grant"
          pendingLabel="Removing…"
          pending={revokingGrantId === revokeTarget.grant.id}
          error={revokeError}
          errorTitle="Could not remove direct grant"
          closeLabel="Cancel direct grant removal"
          zIndex={120}
          onCancel={() => {
            setRevokeTarget(null);
            setRevokeError(null);
          }}
          onConfirm={() => void handleRevoke()}
        />
      ) : null}

      {discardOpen ? (
        <ConfirmationDialog
          title="Discard sharing changes?"
          description="The selected person or group and access level have not been shared."
          confirmLabel="Discard changes"
          pendingLabel="Discarding…"
          closeLabel="Keep editing object access"
          zIndex={130}
          onCancel={() => setDiscardOpen(false)}
          onConfirm={onClose}
        />
      ) : null}
    </div>
  );
}
