import { useState } from "react";
import { styles } from "../../../shared/styles/index";
import type { MembershipRole, User, Workspace, WorkspaceMembership } from "../../../shared/types";
import { AnimatedSelect } from "../../../shared/ui/AnimatedSelect";
import { ConfirmationDialog } from "../../../shared/ui/ConfirmationDialog";
import { CopyableId } from "../../../shared/ui/CopyableId";
import { InfiniteScrollSentinel } from "../../../shared/ui/InfiniteScrollSentinel";
import { LoadingIndicator } from "../../../shared/ui/LoadingIndicator";
import { useWorkspaceDirectory } from "../hooks/useWorkspaceDirectory";
import { AddWorkspaceMemberDialog } from "./AddWorkspaceMemberDialog";
import { WorkspaceGroupsPanel } from "./WorkspaceGroupsPanel";

type Props = {
  user: User;
  isGlobalAdmin: boolean;
  workspace: Workspace;
  canManageWorkspace: boolean;
  onCurrentUserRemoved: () => void;
  onCurrentUserRoleChanged: () => Promise<void>;
  onToast: (message: string) => void;
};

export function WorkspaceMembersPanel({
  user,
  isGlobalAdmin,
  workspace,
  canManageWorkspace,
  onCurrentUserRemoved,
  onCurrentUserRoleChanged,
  onToast,
}: Props) {
  const directory = useWorkspaceDirectory(workspace.id);
  const [addMemberOpen, setAddMemberOpen] = useState(false);
  const [removalTarget, setRemovalTarget] = useState<WorkspaceMembership | null>(null);
  const [roleChangeTarget, setRoleChangeTarget] = useState<WorkspaceMembership | null>(null);
  const [removingMembershipId, setRemovingMembershipId] = useState<string | null>(null);
  const [updatingMembershipId, setUpdatingMembershipId] = useState<string | null>(null);
  const [memberActionError, setMemberActionError] = useState<string | null>(null);

  async function changeMemberRole(membership: WorkspaceMembership, workspaceRole: MembershipRole) {
    if (membership.workspace_role === workspaceRole || updatingMembershipId) {
      return false;
    }

    setUpdatingMembershipId(membership.id);
    setMemberActionError(null);

    try {
      const updated = await directory.updateMemberRole(membership.id, workspaceRole);
      onToast(
        `${updated.user_display_name} is now ${workspaceRole === "admin" ? "an administrator" : "a member"}`,
      );

      if (updated.user_id === user.id && workspaceRole !== "admin") {
        try {
          await onCurrentUserRoleChanged();
        } catch {
          onToast("Role changed. Refresh the page to update your workspace controls.");
        }
      }
      return true;
    } catch (cause) {
      setMemberActionError(
        cause instanceof Error ? cause.message : "Could not change this member's role.",
      );
      return false;
    } finally {
      setUpdatingMembershipId(null);
    }
  }

  function requestMemberRoleChange(membership: WorkspaceMembership, workspaceRole: MembershipRole) {
    if (
      membership.user_id === user.id &&
      membership.workspace_role === "admin" &&
      workspaceRole === "member"
    ) {
      setRoleChangeTarget(membership);
      setMemberActionError(null);
      return;
    }

    void changeMemberRole(membership, workspaceRole);
  }

  async function removeMember() {
    if (!removalTarget || removingMembershipId) {
      return;
    }

    const target = removalTarget;
    setRemovingMembershipId(target.id);
    setMemberActionError(null);

    try {
      await directory.removeMember(target.id);
      setRemovalTarget(null);
      onToast("Workspace member removed");
      if (target.user_id === user.id && !isGlobalAdmin) {
        onCurrentUserRemoved();
      }
    } catch (cause) {
      setMemberActionError(
        cause instanceof Error ? cause.message : "Could not remove this member.",
      );
    } finally {
      setRemovingMembershipId(null);
    }
  }

  return (
    <>
      {addMemberOpen && (
        <AddWorkspaceMemberDialog
          workspaceName={workspace.name}
          onClose={() => setAddMemberOpen(false)}
          onAdd={async (input) => {
            const membership = await directory.addMember(input);
            setAddMemberOpen(false);
            onToast(`${membership.user_display_name} added to ${workspace.name}`);
            return membership;
          }}
        />
      )}

      <div style={styles.pageHeader}>
        <p style={styles.eyebrow}>Workspace</p>
        <h1 style={styles.pageTitle}>Members</h1>
        <p style={styles.muted}>People and groups that belong to this workspace.</p>
      </div>

      {directory.loading && <LoadingIndicator label="Loading members…" />}

      {!directory.loading && directory.error && (
        <div style={styles.errorBox}>
          <strong>Could not load members</strong>
          <span>{directory.error}</span>
        </div>
      )}

      {!directory.loading && !directory.error && (
        <section>
          <div style={styles.sectionHeader}>
            <h2 style={styles.sectionTitle}>People</h2>
            <div style={styles.directoryHeaderActions}>
              <span style={styles.muted}>
                {directory.memberships.length}
                {directory.membershipsNextCursor ? "+" : ""}{" "}
                {directory.memberships.length === 1 ? "member" : "members"}
              </span>
              {canManageWorkspace && (
                <button
                  type="button"
                  style={styles.primaryButtonCompact}
                  onClick={() => setAddMemberOpen(true)}
                >
                  Add member
                </button>
              )}
            </div>
          </div>

          {memberActionError && !removalTarget && !roleChangeTarget ? (
            <div style={styles.errorBox} role="alert">
              <strong>Could not update workspace member</strong>
              <span>{memberActionError}</span>
            </div>
          ) : null}

          <div className="kival-row-list" style={styles.directoryList}>
            {directory.memberships.map((membership) => {
              const isCurrentUser = membership.user_id === user.id;

              return (
                <div key={membership.id} style={styles.directoryRow}>
                  <div style={styles.directoryIdentity}>
                    <div style={styles.directoryAvatar}>
                      {membership.user_display_name.slice(0, 1).toUpperCase() || "R"}
                    </div>

                    <div style={styles.directoryMain}>
                      <strong>
                        {membership.user_display_name}
                        {isCurrentUser ? " (you)" : ""}
                      </strong>
                      <span style={styles.objectMeta}>{membership.user_username}</span>
                      <CopyableId
                        value={membership.user_id}
                        displayValue={`ID: ${membership.user_id}`}
                        label="user ID"
                      />
                    </div>
                  </div>

                  <div style={styles.directoryHeaderActions}>
                    {canManageWorkspace && !(isCurrentUser && isGlobalAdmin) ? (
                      <AnimatedSelect
                        wrapperStyle={{ minWidth: 132 }}
                        aria-label={`Workspace role for ${membership.user_display_name}`}
                        title="Direct workspace role"
                        value={membership.workspace_role}
                        style={styles.input}
                        disabled={updatingMembershipId !== null || removingMembershipId !== null}
                        onChange={(event) =>
                          requestMemberRoleChange(membership, event.target.value as MembershipRole)
                        }
                      >
                        <option value="member">Member</option>
                        <option value="admin">Administrator</option>
                      </AnimatedSelect>
                    ) : (
                      <span style={styles.directoryRole}>
                        {isCurrentUser && isGlobalAdmin
                          ? "Administrator"
                          : membership.workspace_role}
                      </span>
                    )}
                    {canManageWorkspace && (
                      <button
                        type="button"
                        style={styles.apiKeyDangerButton}
                        disabled={removingMembershipId !== null || updatingMembershipId !== null}
                        onClick={() => {
                          setRemovalTarget(membership);
                          setMemberActionError(null);
                        }}
                      >
                        Remove
                      </button>
                    )}
                  </div>
                </div>
              );
            })}

            {directory.memberships.length === 0 && (
              <div style={styles.emptyState}>
                <strong>No people found</strong>
                <span>This workspace has no active direct members.</span>
              </div>
            )}
          </div>

          <InfiniteScrollSentinel
            hasMore={Boolean(directory.membershipsNextCursor)}
            loading={directory.loadingMore}
            onLoadMore={directory.loadMore}
            label="Loading more members…"
          />
        </section>
      )}

      <WorkspaceGroupsPanel
        workspace={workspace}
        canManageWorkspace={canManageWorkspace}
        onToast={onToast}
      />

      {removalTarget ? (
        <ConfirmationDialog
          title={`Remove ${removalTarget.user_display_name}?`}
          description={
            removalTarget.user_id === user.id
              ? isGlobalAdmin
                ? `This removes your direct membership. You will retain full access to ${workspace.name} while you are a global administrator.`
                : `You will immediately lose access to ${workspace.name}.`
              : `${removalTarget.user_display_name} will immediately lose direct access to ${workspace.name}.`
          }
          confirmLabel="Remove member"
          pendingLabel="Removing…"
          pending={removingMembershipId === removalTarget.id}
          error={memberActionError}
          errorTitle="Could not remove member"
          closeLabel="Cancel workspace member removal"
          onCancel={() => {
            setRemovalTarget(null);
            setMemberActionError(null);
          }}
          onConfirm={() => void removeMember()}
        />
      ) : null}

      {roleChangeTarget ? (
        <ConfirmationDialog
          title="Change your direct role to Member?"
          description={
            isGlobalAdmin
              ? `Your direct workspace role will become Member. You will retain full administrative access to ${workspace.name} while you are a global administrator.`
              : `You will remain a member of ${workspace.name}, but you will immediately lose workspace administration rights.`
          }
          confirmLabel="Change to Member"
          pendingLabel="Changing role…"
          pending={updatingMembershipId === roleChangeTarget.id}
          error={memberActionError}
          errorTitle="Could not change your role"
          closeLabel="Cancel workspace role change"
          onCancel={() => {
            setRoleChangeTarget(null);
            setMemberActionError(null);
          }}
          onConfirm={() => {
            void changeMemberRole(roleChangeTarget, "member").then((changed) => {
              if (changed) {
                setRoleChangeTarget(null);
              }
            });
          }}
        />
      ) : null}
    </>
  );
}
