import { KivalTransportError } from "kival-sdk";
import { useEffect, useState } from "react";
import { kival } from "../../../shared/api";
import { styles } from "../../../shared/styles/index";
import type { Group, Workspace, WorkspaceGroup } from "../../../shared/types";
import { AnimatedSelect } from "../../../shared/ui/AnimatedSelect";
import { ConfirmationDialog } from "../../../shared/ui/ConfirmationDialog";
import { CopyableId } from "../../../shared/ui/CopyableId";
import { LoadingIndicator } from "../../../shared/ui/LoadingIndicator";

type Props = {
  workspace: Workspace;
  canManageWorkspace: boolean;
  onToast: (message: string) => void;
};

export function WorkspaceGroupsPanel({ workspace, canManageWorkspace, onToast }: Props) {
  const [workspaceGroups, setWorkspaceGroups] = useState<WorkspaceGroup[]>([]);
  const [removedWorkspaceGroups, setRemovedWorkspaceGroups] = useState<WorkspaceGroup[]>([]);
  const [availableGroups, setAvailableGroups] = useState<Group[]>([]);
  const [selectedGroupId, setSelectedGroupId] = useState("");
  const [addDialogOpen, setAddDialogOpen] = useState(false);
  const [discardAddOpen, setDiscardAddOpen] = useState(false);
  const [addError, setAddError] = useState<string | null>(null);
  const [groupsLoading, setGroupsLoading] = useState(true);
  const [groupsError, setGroupsError] = useState<string | null>(null);
  const [groupActionId, setGroupActionId] = useState<string | null>(null);
  const [removalTarget, setRemovalTarget] = useState<WorkspaceGroup | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    setGroupsLoading(true);
    setGroupsError(null);

    void (async () => {
      const linkedGroups: WorkspaceGroup[] = [];
      let linkedCursor: string | null = null;

      do {
        const response = await kival.listWorkspaceGroups({
          workspaceId: workspace.id,
          cursor: linkedCursor,
          signal: controller.signal,
          status: "active",
        });
        linkedGroups.push(...response.items);
        linkedCursor = response.next_cursor ?? null;
      } while (linkedCursor && !controller.signal.aborted);

      const removedGroups: WorkspaceGroup[] = [];
      let removedCursor: string | null = null;

      if (canManageWorkspace) {
        do {
          const response = await kival.listWorkspaceGroups({
            workspaceId: workspace.id,
            cursor: removedCursor,
            signal: controller.signal,
            status: "archived",
          });
          removedGroups.push(...response.items);
          removedCursor = response.next_cursor ?? null;
        } while (removedCursor && !controller.signal.aborted);
      }

      const manageableGroups: Group[] = [];
      let groupCursor: string | null = null;

      if (canManageWorkspace) {
        do {
          const response = await kival.listGroups({
            cursor: groupCursor,
            signal: controller.signal,
          });
          manageableGroups.push(...response.items);
          groupCursor = response.next_cursor ?? null;
        } while (groupCursor && !controller.signal.aborted);
      }

      setWorkspaceGroups(linkedGroups);
      setRemovedWorkspaceGroups(removedGroups);
      setAvailableGroups(manageableGroups);
    })()
      .catch((cause: unknown) => {
        if (cause instanceof KivalTransportError && cause.kind === "abort") {
          return;
        }
        setGroupsError(cause instanceof Error ? cause.message : "Could not load workspace groups.");
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setGroupsLoading(false);
        }
      });

    return () => controller.abort();
  }, [canManageWorkspace, workspace.id]);

  const linkedGroupIds = new Set(workspaceGroups.map((group) => group.group_id));
  const addableGroups = availableGroups.filter((group) => !linkedGroupIds.has(group.id));

  async function addGroup(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const groupId = selectedGroupId.trim();

    if (!groupId) {
      setAddError("Choose a group to add.");
      return;
    }

    setGroupActionId(groupId);
    setAddError(null);

    try {
      const removedWorkspaceGroup = removedWorkspaceGroups.find(
        (group) => group.group_id === groupId,
      );
      const workspaceGroup = removedWorkspaceGroup
        ? await kival.unarchiveWorkspaceGroup({
            workspaceId: workspace.id,
            groupId,
          })
        : await kival.createWorkspaceGroup({
            workspaceId: workspace.id,
            input: { group_id: groupId },
          });
      setWorkspaceGroups((current) => [
        workspaceGroup,
        ...current.filter((group) => group.group_id !== groupId),
      ]);
      setRemovedWorkspaceGroups((current) => current.filter((group) => group.group_id !== groupId));
      setSelectedGroupId("");
      setAddDialogOpen(false);
      onToast(`${workspaceGroup.group_name} added to ${workspace.name}`);
    } catch (cause) {
      setAddError(cause instanceof Error ? cause.message : "Could not add this group.");
    } finally {
      setGroupActionId(null);
    }
  }

  function requestAddDialogClose() {
    if (groupActionId) {
      return;
    }

    if (selectedGroupId) {
      setDiscardAddOpen(true);
      return;
    }

    setAddDialogOpen(false);
    setAddError(null);
  }

  async function removeGroup(group: WorkspaceGroup) {
    setGroupActionId(group.group_id);
    setGroupsError(null);

    try {
      const removedGroup = await kival.archiveWorkspaceGroup({
        workspaceId: workspace.id,
        groupId: group.group_id,
      });
      setRemovedWorkspaceGroups((current) => [
        removedGroup,
        ...current.filter((candidate) => candidate.group_id !== group.group_id),
      ]);
      onToast(`${group.group_name} removed from ${workspace.name}`);
      setWorkspaceGroups((current) => current.filter((candidate) => candidate.id !== group.id));
      setRemovalTarget(null);
    } catch (cause) {
      setGroupsError(
        cause instanceof Error ? cause.message : "Could not change this workspace group.",
      );
    } finally {
      setGroupActionId(null);
    }
  }

  return (
    <>
      <section style={{ marginTop: 32 }}>
        <div style={styles.sectionHeader}>
          <div style={styles.directoryMain}>
            <h2 style={styles.sectionTitle}>Groups</h2>
            <p style={styles.muted}>Groups whose members can receive access in this workspace.</p>
          </div>
          <div style={styles.directoryHeaderActions}>
            <span style={styles.muted}>
              {workspaceGroups.length} {workspaceGroups.length === 1 ? "group" : "groups"}
            </span>
            {canManageWorkspace && !groupsLoading && addableGroups.length > 0 ? (
              <button
                type="button"
                style={styles.primaryButtonCompact}
                onClick={() => {
                  setAddError(null);
                  setAddDialogOpen(true);
                }}
              >
                Add group
              </button>
            ) : null}
          </div>
        </div>

        {groupsLoading ? <LoadingIndicator label="Loading workspace groups…" compact /> : null}
        {!groupsLoading && groupsError ? (
          <div style={styles.errorBox} role="alert">
            <strong>Could not manage workspace groups</strong>
            <span>{groupsError}</span>
          </div>
        ) : null}
        {!groupsLoading ? (
          <div className="kival-row-list" style={styles.directoryList}>
            {workspaceGroups.map((group) => (
              <div key={group.id} style={styles.directoryRow}>
                <div style={styles.directoryIdentity}>
                  <div style={styles.directoryAvatar}>
                    {group.group_name.slice(0, 1).toUpperCase() || "G"}
                  </div>
                  <div style={styles.directoryMain}>
                    <strong>{group.group_name}</strong>
                    {group.group_description ? (
                      <span style={styles.objectMeta}>{group.group_description}</span>
                    ) : null}
                    <CopyableId
                      value={group.group_id}
                      displayValue={`ID: ${group.group_id}`}
                      label="group ID"
                    />
                  </div>
                </div>
                {canManageWorkspace ? (
                  <div style={styles.directoryHeaderActions}>
                    <button
                      type="button"
                      style={styles.apiKeyDangerButton}
                      disabled={groupActionId === group.group_id}
                      onClick={() => {
                        setRemovalTarget(group);
                        setGroupsError(null);
                      }}
                    >
                      {groupActionId === group.group_id ? "Removing…" : "Remove"}
                    </button>
                  </div>
                ) : null}
              </div>
            ))}
            {workspaceGroups.length === 0 ? (
              <div style={styles.emptyState}>
                <strong>No groups</strong>
                <span>
                  {canManageWorkspace
                    ? "Add a group to make it available for object sharing."
                    : "No groups are linked to this workspace."}
                </span>
              </div>
            ) : null}
          </div>
        ) : null}
      </section>

      {addDialogOpen ? (
        <div style={styles.modalBackdrop} role="presentation">
          <button
            type="button"
            aria-label="Close add workspace group dialog"
            style={styles.modalBackdropDismiss}
            onClick={requestAddDialogClose}
          />

          <form
            role="dialog"
            aria-modal="true"
            aria-labelledby="add-workspace-group-title"
            style={styles.modalDialog}
            onSubmit={(event) => void addGroup(event)}
          >
            <div style={styles.modalCopy}>
              <h2 id="add-workspace-group-title" style={styles.modalTitle}>
                Add to {workspace.name}
              </h2>
              <p style={styles.muted}>
                Add an existing Kival group to make it available for object sharing in this
                workspace.
              </p>
            </div>

            <label htmlFor="workspace-group-add" style={styles.field}>
              <span style={styles.fieldLabel}>Group</span>
              <AnimatedSelect
                id="workspace-group-add"
                value={selectedGroupId}
                style={styles.input}
                disabled={Boolean(groupActionId)}
                onChange={(event) => setSelectedGroupId(event.target.value)}
              >
                <option value="">Choose a group</option>
                {addableGroups.map((group) => (
                  <option key={group.id} value={group.id}>
                    {group.name}
                  </option>
                ))}
              </AnimatedSelect>
            </label>

            {addError ? (
              <div style={styles.loginError} role="alert">
                {addError}
              </div>
            ) : null}

            <div style={styles.modalActions}>
              <button
                type="button"
                disabled={Boolean(groupActionId)}
                style={styles.secondaryButton}
                onClick={requestAddDialogClose}
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={!selectedGroupId || Boolean(groupActionId)}
                style={styles.primaryButtonCompact}
              >
                {groupActionId === selectedGroupId ? "Adding…" : "Add group"}
              </button>
            </div>
          </form>

          {discardAddOpen ? (
            <ConfirmationDialog
              title="Discard group selection?"
              description="The selected group has not been added to this workspace."
              confirmLabel="Discard selection"
              pendingLabel="Discarding…"
              closeLabel="Keep editing group selection"
              zIndex={120}
              onCancel={() => setDiscardAddOpen(false)}
              onConfirm={() => {
                setDiscardAddOpen(false);
                setSelectedGroupId("");
                setAddError(null);
                setAddDialogOpen(false);
              }}
            />
          ) : null}
        </div>
      ) : null}

      {removalTarget ? (
        <ConfirmationDialog
          title={`Remove ${removalTarget.group_name} from ${workspace.name}?`}
          description={`Members of ${removalTarget.group_name} will immediately lose access inherited through this group. You can restore the group later.`}
          confirmLabel="Remove group"
          pendingLabel="Removing…"
          pending={groupActionId === removalTarget.group_id}
          error={groupsError}
          errorTitle="Could not remove group"
          closeLabel="Cancel workspace group removal"
          onCancel={() => {
            setRemovalTarget(null);
            setGroupsError(null);
          }}
          onConfirm={() => void removeGroup(removalTarget)}
        />
      ) : null}
    </>
  );
}
