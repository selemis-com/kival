import { useCallback, useEffect, useState } from "react";
import { useSearchParams } from "react-router";
import { kival } from "../../shared/api";
import { submitFormOnEnter } from "../../shared/forms";
import { usePaginatedResource } from "../../shared/hooks/usePaginatedResource";
import { KivalSideBar } from "../../shared/navigation/KivalSideBar";
import { TopBar } from "../../shared/navigation/TopBar";
import { comparePinOrder } from "../../shared/pins";
import { styles } from "../../shared/styles/index";
import type { User, Workspace } from "../../shared/types";
import { AnimatedSelect } from "../../shared/ui/AnimatedSelect";
import { ConfirmationDialog } from "../../shared/ui/ConfirmationDialog";
import { CopyableId } from "../../shared/ui/CopyableId";
import { InfiniteScrollSentinel } from "../../shared/ui/InfiniteScrollSentinel";
import { LoadingIndicator } from "../../shared/ui/LoadingIndicator";
import { PinIcon } from "../../shared/ui/PinIcon";

type Props = {
  user: User;
  workspaces: Workspace[];
  pinnedWorkspaces: Workspace[];
  workspacesNextCursor: string | null;
  workspacesLoadingMore: boolean;
  error: string | null;
  onLoadMoreWorkspaces: () => void;
  onInboxClick: () => void;
  unreadInboxCount: number;
  onOpenWorkspace: (workspace: Workspace) => void;
  onCreateWorkspace: (name: string, description?: string) => Promise<void>;
  onRestoreWorkspace: (workspaceId: string) => Promise<void>;
  onSetWorkspacePin: (workspaceId: string, pinned: boolean) => Promise<void>;
  onSecurityClick: () => void;
  onApiKeysClick: () => void;
  onUsersClick?: () => void;
  onGroupsClick?: () => void;
  onEventsClick?: () => void;
  onLogout: () => Promise<void>;
};

export function WorkspaceChooser({
  user,
  workspaces,
  pinnedWorkspaces,
  workspacesNextCursor,
  workspacesLoadingMore,
  error,
  onLoadMoreWorkspaces,
  onInboxClick,
  unreadInboxCount,
  onOpenWorkspace,
  onCreateWorkspace,
  onRestoreWorkspace,
  onSetWorkspacePin,
  onSecurityClick,
  onApiKeysClick,
  onUsersClick,
  onGroupsClick,
  onEventsClick,
  onLogout,
}: Props) {
  const [searchParams, setSearchParams] = useSearchParams();
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [discardCreateOpen, setDiscardCreateOpen] = useState(false);
  const [statusFilter, setStatusFilter] = useState<"active" | "archived">("active");
  const [restoringId, setRestoringId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const normalizedSearchQuery = searchQuery.trim();
  const searchActive = normalizedSearchQuery.length > 0;
  const searchKey = `${statusFilter}:${normalizedSearchQuery}`;
  const createWorkspaceRequested = searchParams.get("create") === "workspace";

  useEffect(() => {
    if (createWorkspaceRequested) {
      setCreateOpen(true);
    }
  }, [createWorkspaceRequested]);

  const loadArchivedPage = useCallback(async (cursor: string | null, signal: AbortSignal) => {
    const response = await kival.listWorkspaces({ cursor, signal, status: "archived" });
    return { items: response.items, nextCursor: response.next_cursor ?? null };
  }, []);
  const {
    items: archivedWorkspaces,
    setItems: setArchivedWorkspaces,
    nextCursor: archivedNextCursor,
    loading: archivedLoading,
    loadingMore: archivedLoadingMore,
    error: archivedError,
    setError: setArchivedError,
    loadMore: loadMoreArchived,
  } = usePaginatedResource({
    queryKey: "archived-workspaces",
    loadPage: loadArchivedPage,
    enabled: statusFilter === "archived",
    errorMessage: "Could not load workspaces.",
    itemKey: (workspace: Workspace) => workspace.id,
  });

  const loadSearchPage = useCallback(
    async (cursor: string | null, signal: AbortSignal) => {
      const response = await kival.listWorkspaces({
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
    items: searchResults,
    setItems: setSearchResults,
    nextCursor: searchNextCursor,
    loading: searchLoading,
    loadingMore: searchLoadingMore,
    error: searchError,
    loadMore: loadMoreSearchResults,
  } = usePaginatedResource({
    queryKey: searchKey,
    loadPage: loadSearchPage,
    enabled: searchActive,
    debounceMs: 150,
    errorMessage: "Could not search workspaces.",
    itemKey: (workspace: Workspace) => workspace.id,
  });

  const unsortedVisibleWorkspaces = searchActive
    ? searchResults
    : statusFilter === "active"
      ? workspaces
      : archivedWorkspaces;
  const visibleWorkspaces = unsortedVisibleWorkspaces;
  const visiblePinnedWorkspaces = pinnedWorkspaces
    .filter((workspace) => workspace.status === "active" && workspace.pinned)
    .sort(comparePinOrder);
  const directoryLoading =
    searchLoading || (!searchActive && statusFilter === "archived" && archivedLoading);
  const directoryError = searchError ?? archivedError ?? error;

  async function restoreWorkspace(workspace: Workspace) {
    setRestoringId(workspace.id);
    setArchivedError(null);

    try {
      await onRestoreWorkspace(workspace.id);
      setArchivedWorkspaces((current) =>
        current.filter((candidate) => candidate.id !== workspace.id),
      );
      setSearchResults((current) => current.filter((candidate) => candidate.id !== workspace.id));
    } catch (cause) {
      setArchivedError(cause instanceof Error ? cause.message : "Could not restore workspace.");
    } finally {
      setRestoringId(null);
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
    setDiscardCreateOpen(false);

    setCreateOpen(false);
    setCreateError(null);
    clearCreateWorkspaceRequest();
  }

  function clearCreateWorkspaceRequest() {
    if (!createWorkspaceRequested) {
      return;
    }

    const next = new URLSearchParams(searchParams);
    next.delete("create");
    setSearchParams(next, { replace: true });
  }

  async function handleCreate(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();

    const normalizedName = name.trim();
    const normalizedDescription = description.trim();

    if (!normalizedName) {
      setCreateError("Enter a workspace name.");
      return;
    }

    setCreating(true);
    setCreateError(null);

    try {
      await onCreateWorkspace(normalizedName, normalizedDescription || undefined);
      setName("");
      setDescription("");
      setCreateOpen(false);
      clearCreateWorkspaceRequest();
    } catch (error) {
      setCreateError(error instanceof Error ? error.message : String(error));
    } finally {
      setCreating(false);
    }
  }

  return (
    <div style={styles.app}>
      <TopBar
        user={user}
        workspaces={workspaces}
        workspacesNextCursor={workspacesNextCursor}
        workspacesLoadingMore={workspacesLoadingMore}
        onLoadMoreWorkspaces={onLoadMoreWorkspaces}
        onInboxClick={onInboxClick}
        unreadInboxCount={unreadInboxCount}
        onSecurityClick={onSecurityClick}
        onApiKeysClick={onApiKeysClick}
        onLogout={onLogout}
      />

      <div style={styles.kivalShell}>
        <KivalSideBar
          active="workspaces"
          onWorkspacesClick={() => undefined}
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
              <h1 style={styles.pageTitle}>Workspaces</h1>
              <p style={styles.muted}>Open a workspace to explore its objects and connections.</p>
            </div>

            {visiblePinnedWorkspaces.length > 0 && (
              <section style={styles.pinnedCardSection}>
                <div style={styles.pinnedCardHeader}>
                  <h2 style={styles.sectionTitle}>Pinned</h2>
                </div>

                <div style={styles.pinnedCardGrid}>
                  {visiblePinnedWorkspaces.map((workspace) => (
                    <article key={workspace.id} style={styles.pinnedCard}>
                      <button
                        type="button"
                        style={styles.pinnedCardAction}
                        aria-label={`Open ${workspace.name}`}
                        onClick={() => onOpenWorkspace(workspace)}
                      />
                      <span style={styles.pinnedCardPinAction}>
                        <button
                          type="button"
                          style={styles.pinButtonActive}
                          aria-label={`Unpin ${workspace.name}`}
                          title="Unpin workspace"
                          onClick={() => void onSetWorkspacePin(workspace.id, false)}
                        >
                          <PinIcon active />
                        </button>
                      </span>
                      <div style={styles.pinnedCardMain}>
                        <strong style={styles.workspaceName}>{workspace.name}</strong>
                        {workspace.description && (
                          <span style={styles.workspaceDescription}>{workspace.description}</span>
                        )}
                      </div>
                      <div style={styles.pinnedCardFooter}>
                        <CopyableId
                          value={workspace.id}
                          displayValue={`ID: ${workspace.id}`}
                          label="workspace ID"
                        />
                      </div>
                    </article>
                  ))}
                </div>
              </section>
            )}

            <div style={styles.sectionHeader}>
              <h2 style={styles.sectionTitle}>All workspaces</h2>
            </div>

            <div
              style={{
                ...styles.workspaceChooserActions,
                display: "flex",
                alignItems: "flex-end",
                justifyContent: "space-between",
                gap: 16,
                marginTop: 0,
              }}
            >
              <div
                style={{
                  display: "flex",
                  alignItems: "flex-end",
                  flexWrap: "wrap",
                  gap: 12,
                }}
              >
                <label htmlFor="workspace-status-filter" style={{ ...styles.field, minWidth: 180 }}>
                  <span style={styles.fieldLabel}>Show workspaces</span>
                  <AnimatedSelect
                    id="workspace-status-filter"
                    value={statusFilter}
                    style={styles.input}
                    onChange={(event) =>
                      setStatusFilter(event.target.value as "active" | "archived")
                    }
                  >
                    <option value="active">Active</option>
                    <option value="archived">Archived</option>
                  </AnimatedSelect>
                </label>
                <label htmlFor="workspace-name-search" style={{ ...styles.field, minWidth: 260 }}>
                  <span style={styles.fieldLabel}>Search by name</span>
                  <input
                    data-1p-ignore="true"
                    id="workspace-name-search"
                    type="search"
                    value={searchQuery}
                    placeholder="Search workspaces…"
                    autoComplete="off"
                    style={styles.input}
                    onChange={(event) => setSearchQuery(event.target.value)}
                  />
                </label>
              </div>
              <button
                type="button"
                style={styles.primaryButtonCompact}
                onClick={() => setCreateOpen(true)}
              >
                Create workspace
              </button>
            </div>

            {directoryLoading && (
              <LoadingIndicator
                label={searchActive ? "Searching workspaces…" : "Loading archived workspaces…"}
              />
            )}

            {!directoryLoading && directoryError && (
              <div style={styles.errorBox} role="alert">
                <strong>Could not load workspaces</strong>
                <span>{directoryError}</span>
              </div>
            )}

            {!directoryLoading && !directoryError && (
              <div className="kival-row-list" style={styles.directoryList}>
                {visibleWorkspaces.map((workspace) =>
                  workspace.status === "active" ? (
                    <div
                      key={workspace.id}
                      style={{ ...styles.directoryRow, position: "relative", cursor: "pointer" }}
                    >
                      <button
                        type="button"
                        style={styles.directoryRowAction}
                        aria-label={`Open ${workspace.name}`}
                        onClick={() => onOpenWorkspace(workspace)}
                      />
                      <div
                        style={{
                          ...styles.directoryMain,
                          position: "relative",
                          zIndex: 1,
                          pointerEvents: "none",
                        }}
                      >
                        <strong>{workspace.name}</strong>
                        {workspace.description && (
                          <span style={styles.muted}>{workspace.description}</span>
                        )}
                        <CopyableId
                          value={workspace.id}
                          displayValue={`ID: ${workspace.id}`}
                          label="workspace ID"
                        />
                      </div>
                      <div
                        style={{
                          ...styles.directoryHeaderActions,
                          position: "relative",
                          zIndex: 1,
                          pointerEvents: "none",
                        }}
                      >
                        <button
                          type="button"
                          style={workspace.pinned ? styles.pinButtonActive : styles.pinButton}
                          aria-label={
                            workspace.pinned ? `Unpin ${workspace.name}` : `Pin ${workspace.name}`
                          }
                          aria-pressed={Boolean(workspace.pinned)}
                          title={workspace.pinned ? "Unpin workspace" : "Pin workspace"}
                          onClick={() => void onSetWorkspacePin(workspace.id, !workspace.pinned)}
                        >
                          <PinIcon active={Boolean(workspace.pinned)} />
                        </button>
                      </div>
                    </div>
                  ) : (
                    <div key={workspace.id} style={styles.directoryRow}>
                      <div style={styles.directoryMain}>
                        <strong>{workspace.name}</strong>
                        {workspace.description && (
                          <span style={styles.muted}>{workspace.description}</span>
                        )}
                        <CopyableId
                          value={workspace.id}
                          displayValue={`ID: ${workspace.id}`}
                          label="workspace ID"
                        />
                      </div>
                      <div style={styles.directoryHeaderActions}>
                        <button
                          type="button"
                          style={styles.secondaryButtonCompact}
                          disabled={restoringId === workspace.id}
                          onClick={() => void restoreWorkspace(workspace)}
                        >
                          {restoringId === workspace.id ? "Restoring…" : "Restore"}
                        </button>
                      </div>
                    </div>
                  ),
                )}

                {!searchActive &&
                  statusFilter === "active" &&
                  visibleWorkspaces.length === 0 &&
                  !workspacesNextCursor && (
                    <div style={styles.emptyState}>
                      <strong>No workspaces found</strong>
                      <span>Create your first workspace to get started.</span>
                    </div>
                  )}
                {!searchActive &&
                  statusFilter === "archived" &&
                  visibleWorkspaces.length === 0 &&
                  !archivedNextCursor && (
                    <div style={styles.emptyState}>
                      <strong>No archived workspaces</strong>
                      <span>Archived workspaces you can access will appear here.</span>
                    </div>
                  )}
                {searchActive && visibleWorkspaces.length === 0 && (
                  <div style={styles.emptyState}>
                    <strong>No matching workspaces</strong>
                    <span>No workspace name matches “{normalizedSearchQuery}”.</span>
                  </div>
                )}
              </div>
            )}

            {searchActive ? (
              <InfiniteScrollSentinel
                hasMore={Boolean(searchNextCursor)}
                loading={searchLoadingMore}
                onLoadMore={() => void loadMoreSearchResults()}
                label="Loading more matches…"
              />
            ) : statusFilter === "active" ? (
              <InfiniteScrollSentinel
                hasMore={Boolean(workspacesNextCursor)}
                loading={workspacesLoadingMore}
                onLoadMore={onLoadMoreWorkspaces}
                label="Loading more workspaces…"
              />
            ) : (
              <InfiniteScrollSentinel
                hasMore={Boolean(archivedNextCursor)}
                loading={archivedLoadingMore}
                onLoadMore={() => void loadMoreArchived()}
                label="Loading more archived workspaces…"
              />
            )}
          </div>
        </main>
      </div>

      {createOpen && (
        <div style={styles.modalBackdrop} role="presentation">
          <button
            type="button"
            aria-label="Close create workspace dialog"
            style={styles.modalBackdropDismiss}
            onClick={requestCloseCreateDialog}
          />

          <form
            aria-labelledby="create-workspace-title"
            aria-modal="true"
            role="dialog"
            style={styles.modalDialog}
            onSubmit={handleCreate}
          >
            <div style={styles.modalCopy}>
              <h2 id="create-workspace-title" style={styles.modalTitle}>
                Create workspace
              </h2>

              <p style={styles.muted}>
                You will be added as an administrator of the new workspace.
              </p>
            </div>

            <label style={styles.field}>
              <span>Name</span>
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
              <span>Description</span>
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

            {createError && <p style={styles.error}>{createError}</p>}

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
                {creating ? "Creating…" : "Create workspace"}
              </button>
            </div>
          </form>
        </div>
      )}

      {discardCreateOpen ? (
        <ConfirmationDialog
          title="Discard workspace draft?"
          description="The workspace has not been created. Your entered name and description will be lost."
          confirmLabel="Discard draft"
          pendingLabel="Discarding…"
          closeLabel="Keep editing workspace draft"
          zIndex={101}
          onCancel={() => setDiscardCreateOpen(false)}
          onConfirm={closeCreateDialog}
        />
      ) : null}
    </div>
  );
}
