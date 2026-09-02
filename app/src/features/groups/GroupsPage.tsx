import { useCallback, useState } from "react";
import { kival } from "../../shared/api";
import { submitFormOnEnter } from "../../shared/forms";
import { usePaginatedResource } from "../../shared/hooks/usePaginatedResource";
import { KivalSideBar } from "../../shared/navigation/KivalSideBar";
import { TopBar } from "../../shared/navigation/TopBar";
import { styles } from "../../shared/styles/index";
import type { Group, User, Workspace } from "../../shared/types";
import { AnimatedSelect } from "../../shared/ui/AnimatedSelect";
import { ConfirmationDialog } from "../../shared/ui/ConfirmationDialog";
import { CopyableId } from "../../shared/ui/CopyableId";
import { InfiniteScrollSentinel } from "../../shared/ui/InfiniteScrollSentinel";
import { LoadingIndicator } from "../../shared/ui/LoadingIndicator";
import { ManageGroupMembersDialog } from "./components/ManageGroupMembersDialog";

type GroupStatusFilter = "active" | "archived";

type Props = {
  user: User;
  isGlobalAdmin: boolean;
  workspaces: Workspace[];
  workspacesNextCursor: string | null;
  workspacesLoadingMore: boolean;
  onLoadMoreWorkspaces: () => void;
  onHome: () => void;
  onInboxClick: () => void;
  unreadInboxCount: number;
  onWorkspaceSelect: (workspaceId: string) => void;
  onSecurityClick: () => void;
  onApiKeysClick: () => void;
  onUsersClick?: () => void;
  onGroupsClick?: () => void;
  onEventsClick?: () => void;
  onLogout: () => Promise<void>;
  onCurrentUserAuthorityChanged: () => Promise<boolean>;
};

export function GroupsPage({
  user,
  isGlobalAdmin,
  workspaces,
  workspacesNextCursor,
  workspacesLoadingMore,
  onLoadMoreWorkspaces,
  onHome,
  onInboxClick,
  unreadInboxCount,
  onWorkspaceSelect,
  onSecurityClick,
  onApiKeysClick,
  onUsersClick,
  onGroupsClick,
  onEventsClick,
  onLogout,
  onCurrentUserAuthorityChanged,
}: Props) {
  const [statusFilter, setStatusFilter] = useState<GroupStatusFilter>("active");
  const [searchQuery, setSearchQuery] = useState("");
  const normalizedSearchQuery = searchQuery.trim();
  const searchActive = normalizedSearchQuery.length > 0;
  const searchKey = `${statusFilter}:${normalizedSearchQuery}`;
  const loadGroupPage = useCallback(
    async (cursor: string | null, signal: AbortSignal) => {
      const response = await kival.listGroups({
        cursor,
        signal,
        status: statusFilter,
        q: normalizedSearchQuery,
      });
      return { items: response.items, nextCursor: response.next_cursor ?? null };
    },
    [normalizedSearchQuery, statusFilter],
  );
  const {
    items: groups,
    setItems: setGroups,
    nextCursor,
    loading,
    loadingMore,
    error,
    setError,
    loadMore,
    reload,
  } = usePaginatedResource({
    queryKey: searchKey,
    loadPage: loadGroupPage,
    debounceMs: searchActive ? 150 : 0,
    errorMessage: "Could not load groups.",
    itemKey: (group: Group) => group.id,
  });
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [discardCreateOpen, setDiscardCreateOpen] = useState(false);
  const [managedGroup, setManagedGroup] = useState<Group | null>(null);
  const [editingGroup, setEditingGroup] = useState<Group | null>(null);
  const [editName, setEditName] = useState("");
  const [editDescription, setEditDescription] = useState("");
  const [savingGroup, setSavingGroup] = useState(false);
  const [editError, setEditError] = useState<string | null>(null);
  const [discardEditOpen, setDiscardEditOpen] = useState(false);
  const [lifecycleTarget, setLifecycleTarget] = useState<Group | null>(null);
  const [updatingLifecycle, setUpdatingLifecycle] = useState(false);
  const [openingGroupId, setOpeningGroupId] = useState<string | null>(null);

  async function handleCurrentUserAuthorityChanged() {
    setManagedGroup(null);

    try {
      const canStillManageGroups = await onCurrentUserAuthorityChanged();
      if (canStillManageGroups) {
        reload();
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not refresh group authority.");
    }
  }

  async function handleCreate(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const normalizedName = name.trim();
    const normalizedDescription = description.trim();

    if (!normalizedName) {
      setCreateError("Enter a group name.");
      return;
    }

    setCreating(true);
    setCreateError(null);
    try {
      const group = await kival.createGroup({
        input: {
          name: normalizedName,
          description: normalizedDescription || undefined,
        },
      });
      setGroups((current) => [group, ...current.filter((candidate) => candidate.id !== group.id)]);
      setName("");
      setDescription("");
      setCreateOpen(false);
    } catch (cause) {
      setCreateError(cause instanceof Error ? cause.message : "Could not create this group.");
    } finally {
      setCreating(false);
    }
  }

  function requestCloseCreateDialog() {
    if (creating) {
      return;
    }

    if (name.length > 0 || description.length > 0) {
      setDiscardCreateOpen(true);
      return;
    }

    closeCreateDialog();
  }

  function closeCreateDialog() {
    setName("");
    setDescription("");
    setCreateError(null);
    setDiscardCreateOpen(false);
    setCreateOpen(false);
  }

  function requestCloseEditDialog() {
    if (!editingGroup || savingGroup) {
      return;
    }

    if (editName !== editingGroup.name || editDescription !== (editingGroup.description ?? "")) {
      setDiscardEditOpen(true);
      return;
    }

    closeEditDialog();
  }

  function closeEditDialog() {
    setEditingGroup(null);
    setEditError(null);
    setDiscardEditOpen(false);
  }

  async function openLatestGroup(group: Group, destination: "members" | "edit") {
    setOpeningGroupId(group.id);
    setError(null);

    try {
      const latestGroup = await kival.getGroup({ groupId: group.id });
      setGroups((current) =>
        current.map((candidate) => (candidate.id === latestGroup.id ? latestGroup : candidate)),
      );

      if (destination === "members") {
        setManagedGroup(latestGroup);
      } else {
        setEditingGroup(latestGroup);
        setEditName(latestGroup.name);
        setEditDescription(latestGroup.description ?? "");
        setEditError(null);
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not load this group.");
    } finally {
      setOpeningGroupId(null);
    }
  }

  async function handleUpdateGroup(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();

    if (!editingGroup) {
      return;
    }

    const normalizedName = editName.trim();
    const normalizedDescription = editDescription.trim() || null;

    if (!normalizedName) {
      setEditError("Enter a group name.");
      return;
    }

    setSavingGroup(true);
    setEditError(null);

    try {
      const group = await kival.updateGroup({
        groupId: editingGroup.id,
        input: {
          name: normalizedName,
          description: normalizedDescription,
        },
      });
      setGroups((current) =>
        current.map((candidate) => (candidate.id === group.id ? group : candidate)),
      );
      setEditingGroup(null);
    } catch (cause) {
      setEditError(cause instanceof Error ? cause.message : "Could not update this group.");
    } finally {
      setSavingGroup(false);
    }
  }

  async function handleLifecycleChange() {
    if (!lifecycleTarget) {
      return;
    }

    setUpdatingLifecycle(true);
    setError(null);

    try {
      const group =
        lifecycleTarget.status === "active"
          ? await kival.archiveGroup({ groupId: lifecycleTarget.id })
          : await kival.unarchiveGroup({ groupId: lifecycleTarget.id });
      setGroups((current) => current.filter((candidate) => candidate.id !== group.id));
      setLifecycleTarget(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not change this group.");
    } finally {
      setUpdatingLifecycle(false);
    }
  }

  return (
    <div style={styles.app}>
      <TopBar
        user={user}
        workspaces={workspaces}
        workspacesNextCursor={workspacesNextCursor}
        workspacesLoadingMore={workspacesLoadingMore}
        onHomeClick={onHome}
        onWorkspaceSelect={onWorkspaceSelect}
        onLoadMoreWorkspaces={onLoadMoreWorkspaces}
        onInboxClick={onInboxClick}
        unreadInboxCount={unreadInboxCount}
        onSecurityClick={onSecurityClick}
        onApiKeysClick={onApiKeysClick}
        onLogout={onLogout}
      />

      <div style={styles.kivalShell}>
        <KivalSideBar
          active="groups"
          onWorkspacesClick={onHome}
          onUsersClick={onUsersClick}
          onGroupsClick={onGroupsClick}
          onEventsClick={onEventsClick}
          onSecurityClick={onSecurityClick}
          onApiKeysClick={onApiKeysClick}
        />

        <main style={styles.apiKeysPage}>
          <div style={styles.contentPaneInner}>
            <div style={styles.pageHeader}>
              <p style={styles.eyebrow}>Kival</p>
              <h1 style={styles.pageTitle}>Groups</h1>
              <p style={styles.muted}>
                Kival-wide groups you administer. Membership applies anywhere a group is granted
                access.
              </p>
            </div>

            <div
              style={{
                ...styles.workspaceChooserActions,
                display: "flex",
                alignItems: "flex-end",
                justifyContent: "space-between",
                gap: 16,
              }}
            >
              <div style={{ display: "flex", alignItems: "flex-end", flexWrap: "wrap", gap: 12 }}>
                <label htmlFor="group-status-filter" style={{ ...styles.field, minWidth: 180 }}>
                  <span style={styles.fieldLabel}>Show groups</span>
                  <AnimatedSelect
                    id="group-status-filter"
                    value={statusFilter}
                    style={styles.input}
                    disabled={loading}
                    onChange={(event) => setStatusFilter(event.target.value as GroupStatusFilter)}
                  >
                    <option value="active">Active</option>
                    <option value="archived">Archived</option>
                  </AnimatedSelect>
                </label>
                <label htmlFor="group-name-search" style={{ ...styles.field, minWidth: 260 }}>
                  <span style={styles.fieldLabel}>Search by name</span>
                  <input
                    data-1p-ignore="true"
                    id="group-name-search"
                    type="search"
                    value={searchQuery}
                    placeholder="Search groups…"
                    autoComplete="off"
                    style={styles.input}
                    onChange={(event) => setSearchQuery(event.target.value)}
                  />
                </label>
              </div>

              {isGlobalAdmin && (
                <button
                  type="button"
                  style={styles.primaryButtonCompact}
                  onClick={() => setCreateOpen(true)}
                >
                  Create group
                </button>
              )}
            </div>

            {loading && (
              <LoadingIndicator label={searchActive ? "Searching groups…" : "Loading groups…"} />
            )}
            {!loading && error && (
              <div style={styles.errorBox} role="alert">
                <strong>Could not load groups</strong>
                <span>{error}</span>
              </div>
            )}

            {!loading && !error && (
              <div className="kival-row-list" style={styles.directoryList}>
                {groups.map((group) => (
                  <div key={group.id} style={styles.directoryRow}>
                    <div style={styles.directoryMain}>
                      <strong>{group.name}</strong>
                      {group.description && <span style={styles.muted}>{group.description}</span>}
                      <CopyableId
                        value={group.id}
                        displayValue={`ID: ${group.id}`}
                        label="group ID"
                      />
                    </div>
                    <div style={styles.directoryHeaderActions}>
                      {group.status === "active" && (
                        <button
                          type="button"
                          style={styles.secondaryButtonCompact}
                          disabled={openingGroupId === group.id}
                          onClick={() => void openLatestGroup(group, "members")}
                        >
                          {openingGroupId === group.id ? "Opening…" : "Members"}
                        </button>
                      )}
                      {isGlobalAdmin && group.status === "active" && (
                        <button
                          type="button"
                          style={styles.secondaryButtonCompact}
                          disabled={openingGroupId === group.id}
                          onClick={() => void openLatestGroup(group, "edit")}
                        >
                          Edit
                        </button>
                      )}
                      {isGlobalAdmin && (
                        <button
                          type="button"
                          style={
                            group.status === "active"
                              ? styles.apiKeyDangerButton
                              : styles.secondaryButtonCompact
                          }
                          disabled={updatingLifecycle}
                          onClick={() => setLifecycleTarget(group)}
                        >
                          {group.status === "active" ? "Archive" : "Restore"}
                        </button>
                      )}
                    </div>
                  </div>
                ))}

                {groups.length === 0 && (
                  <div style={styles.emptyState}>
                    <strong>
                      {searchActive
                        ? "No matching groups"
                        : statusFilter === "archived"
                          ? "No archived groups"
                          : isGlobalAdmin
                            ? "No groups found"
                            : "No groups to manage"}
                    </strong>
                    <span>
                      {searchActive
                        ? `No groups match “${normalizedSearchQuery}”.`
                        : statusFilter === "archived"
                          ? "Archived groups will appear here."
                          : isGlobalAdmin
                            ? "Create the first Kival-wide group."
                            : "You are not an administrator of any Kival-wide groups."}
                    </span>
                  </div>
                )}
              </div>
            )}

            <InfiniteScrollSentinel
              hasMore={Boolean(nextCursor)}
              loading={loadingMore}
              onLoadMore={() => void loadMore()}
              label="Loading more groups…"
            />
          </div>
        </main>
      </div>

      {managedGroup && (
        <ManageGroupMembersDialog
          user={user}
          isGlobalAdmin={isGlobalAdmin}
          group={managedGroup}
          onClose={() => setManagedGroup(null)}
          onCurrentUserAuthorityChanged={handleCurrentUserAuthorityChanged}
        />
      )}

      {createOpen && (
        <div style={styles.modalBackdrop} role="presentation">
          <button
            type="button"
            aria-label="Close create group dialog"
            style={styles.modalBackdropDismiss}
            onClick={requestCloseCreateDialog}
          />
          <form
            role="dialog"
            aria-modal="true"
            aria-labelledby="create-group-title"
            style={styles.modalDialog}
            onSubmit={(event) => void handleCreate(event)}
          >
            <div style={styles.modalCopy}>
              <h2 id="create-group-title" style={styles.modalTitle}>
                Create group
              </h2>
              <p style={styles.muted}>
                Create a Kival-wide group. Kival administrator access is required.
              </p>
            </div>
            <label style={styles.field}>
              <span style={styles.fieldLabel}>Name</span>
              <input
                data-1p-ignore="true"
                autoComplete="off"
                autoFocus
                required
                value={name}
                style={styles.input}
                onChange={(event) => setName(event.target.value)}
              />
            </label>
            <label style={styles.field}>
              <span style={styles.fieldLabel}>Description</span>
              <textarea
                data-1p-ignore="true"
                autoComplete="off"
                value={description}
                placeholder="Optional"
                style={styles.settingsTextarea}
                onChange={(event) => setDescription(event.target.value)}
                onKeyDown={submitFormOnEnter}
              />
            </label>
            {createError && (
              <div style={styles.loginError} role="alert">
                {createError}
              </div>
            )}
            <div style={styles.modalActions}>
              <button
                type="button"
                disabled={creating}
                style={styles.secondaryButton}
                onClick={requestCloseCreateDialog}
              >
                Cancel
              </button>
              <button type="submit" disabled={creating} style={styles.primaryButtonCompact}>
                {creating ? "Creating…" : "Create group"}
              </button>
            </div>
          </form>
        </div>
      )}

      {editingGroup && (
        <div style={styles.modalBackdrop} role="presentation">
          <button
            type="button"
            aria-label="Close edit group dialog"
            style={styles.modalBackdropDismiss}
            onClick={requestCloseEditDialog}
          />
          <form
            role="dialog"
            aria-modal="true"
            aria-labelledby="edit-group-title"
            style={styles.modalDialog}
            onSubmit={(event) => void handleUpdateGroup(event)}
          >
            <div style={styles.modalCopy}>
              <h2 id="edit-group-title" style={styles.modalTitle}>
                Edit group
              </h2>
              <p style={styles.muted}>Update the name and description shown throughout Kival.</p>
            </div>
            <label style={styles.field}>
              <span style={styles.fieldLabel}>Name</span>
              <input
                data-1p-ignore="true"
                autoComplete="off"
                autoFocus
                required
                value={editName}
                style={styles.input}
                disabled={savingGroup}
                onChange={(event) => setEditName(event.target.value)}
              />
            </label>
            <label style={styles.field}>
              <span style={styles.fieldLabel}>Description</span>
              <textarea
                data-1p-ignore="true"
                autoComplete="off"
                value={editDescription}
                placeholder="Optional"
                style={styles.settingsTextarea}
                disabled={savingGroup}
                onChange={(event) => setEditDescription(event.target.value)}
                onKeyDown={submitFormOnEnter}
              />
            </label>
            {editError && (
              <div style={styles.loginError} role="alert">
                {editError}
              </div>
            )}
            <div style={styles.modalActions}>
              <button
                type="button"
                disabled={savingGroup}
                style={styles.secondaryButton}
                onClick={requestCloseEditDialog}
              >
                Cancel
              </button>
              <button type="submit" disabled={savingGroup} style={styles.primaryButtonCompact}>
                {savingGroup ? "Saving…" : "Save changes"}
              </button>
            </div>
          </form>
        </div>
      )}

      {discardCreateOpen ? (
        <ConfirmationDialog
          title="Discard group draft?"
          description="The group has not been created. Its name and description will be lost."
          confirmLabel="Discard draft"
          pendingLabel="Discarding…"
          closeLabel="Keep editing group draft"
          zIndex={120}
          onCancel={() => setDiscardCreateOpen(false)}
          onConfirm={closeCreateDialog}
        />
      ) : null}

      {discardEditOpen ? (
        <ConfirmationDialog
          title="Discard group changes?"
          description="The updated group name and description have not been saved."
          confirmLabel="Discard changes"
          pendingLabel="Discarding…"
          closeLabel="Keep editing group"
          zIndex={120}
          onCancel={() => setDiscardEditOpen(false)}
          onConfirm={closeEditDialog}
        />
      ) : null}

      {lifecycleTarget && (
        <div style={styles.modalBackdrop} role="presentation">
          <button
            type="button"
            aria-label="Cancel group lifecycle change"
            style={styles.modalBackdropDismiss}
            onClick={() => !updatingLifecycle && setLifecycleTarget(null)}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="group-lifecycle-title"
            style={styles.modalDialog}
          >
            <div style={styles.modalCopy}>
              <h2 id="group-lifecycle-title" style={styles.modalTitle}>
                {lifecycleTarget.status === "active"
                  ? `Archive ${lifecycleTarget.name}?`
                  : `Restore ${lifecycleTarget.name}?`}
              </h2>
              <p style={styles.muted}>
                {lifecycleTarget.status === "active"
                  ? "Members will immediately lose access inherited through this group until it is restored."
                  : "The group and its existing memberships will become active again."}
              </p>
            </div>
            <div style={styles.modalActions}>
              <button
                type="button"
                disabled={updatingLifecycle}
                style={styles.secondaryButton}
                onClick={() => setLifecycleTarget(null)}
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={updatingLifecycle}
                style={
                  lifecycleTarget.status === "active"
                    ? styles.apiKeyDangerButtonSolid
                    : styles.primaryButtonCompact
                }
                onClick={() => void handleLifecycleChange()}
              >
                {updatingLifecycle
                  ? lifecycleTarget.status === "active"
                    ? "Archiving…"
                    : "Restoring…"
                  : lifecycleTarget.status === "active"
                    ? "Archive group"
                    : "Restore group"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
