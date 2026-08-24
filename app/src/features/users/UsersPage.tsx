import { useCallback, useState } from "react";
import { kival } from "../../shared/api";
import { usePaginatedResource } from "../../shared/hooks/usePaginatedResource";
import { KivalSideBar } from "../../shared/navigation/KivalSideBar";
import { TopBar } from "../../shared/navigation/TopBar";
import { styles } from "../../shared/styles/index";
import type { User, Workspace } from "../../shared/types";
import { AnimatedSelect } from "../../shared/ui/AnimatedSelect";
import { ConfirmationDialog } from "../../shared/ui/ConfirmationDialog";
import { InfiniteScrollSentinel } from "../../shared/ui/InfiniteScrollSentinel";
import { LoadingIndicator } from "../../shared/ui/LoadingIndicator";

type UserStatusFilter = "active" | "disabled" | "all";
type ManagedUserExit = "close" | "disable" | "enable";

type Props = {
  user: User;
  workspaces: Workspace[];
  workspacesNextCursor: string | null;
  workspacesLoadingMore: boolean;
  onLoadMoreWorkspaces: () => void;
  onHome: () => void;
  onInboxClick: () => void;
  unreadInboxCount: number;
  onWorkspaceSelect: (workspaceId: string) => void;
  onUsersClick: () => void;
  onGroupsClick?: () => void;
  onEventsClick: () => void;
  onSecurityClick: () => void;
  onApiKeysClick: () => void;
  onLogout: () => Promise<void>;
  onCurrentUserUpdate: (user: User) => void;
};

function formatTimestamp(value: string | null) {
  if (!value) {
    return "Never";
  }

  const timestamp = new Date(value);
  if (Number.isNaN(timestamp.getTime())) {
    return "Unknown";
  }

  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(timestamp);
}

function userInitial(user: User) {
  return (user.display_name || user.username).slice(0, 1).toUpperCase();
}

export function UsersPage({
  user,
  workspaces,
  workspacesNextCursor,
  workspacesLoadingMore,
  onLoadMoreWorkspaces,
  onHome,
  onInboxClick,
  unreadInboxCount,
  onWorkspaceSelect,
  onUsersClick,
  onGroupsClick,
  onEventsClick,
  onSecurityClick,
  onApiKeysClick,
  onLogout,
  onCurrentUserUpdate,
}: Props) {
  const [statusFilter, setStatusFilter] = useState<UserStatusFilter>("active");
  const [searchQuery, setSearchQuery] = useState("");
  const normalizedSearchQuery = searchQuery.trim();
  const searchActive = normalizedSearchQuery.length > 0;
  const searchKey = `${statusFilter}:${normalizedSearchQuery}`;
  const loadUserPage = useCallback(
    async (cursor: string | null, signal: AbortSignal) => {
      const response = await kival.listUsers({
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
    items: users,
    setItems: setUsers,
    nextCursor,
    loading,
    loadingMore,
    error,
    setError,
    loadMore,
  } = usePaginatedResource({
    queryKey: searchKey,
    loadPage: loadUserPage,
    debounceMs: searchActive ? 150 : 0,
    errorMessage: "Could not load users.",
    itemKey: (candidate: User) => candidate.id,
  });
  const [managedUser, setManagedUser] = useState<User | null>(null);
  const [openingUserId, setOpeningUserId] = useState<string | null>(null);
  const [displayName, setDisplayName] = useState("");
  const [saving, setSaving] = useState(false);
  const [manageError, setManageError] = useState<string | null>(null);
  const [pendingManagedUserExit, setPendingManagedUserExit] = useState<ManagedUserExit | null>(
    null,
  );
  const [disableTarget, setDisableTarget] = useState<User | null>(null);
  const [disabling, setDisabling] = useState(false);
  const [enableTarget, setEnableTarget] = useState<User | null>(null);
  const [enabling, setEnabling] = useState(false);

  async function openUser(target: User) {
    setOpeningUserId(target.id);
    setError(null);
    try {
      const latestUser = await kival.getUser({ userId: target.id });
      setUsers((current) =>
        current.map((candidate) => (candidate.id === latestUser.id ? latestUser : candidate)),
      );
      setManagedUser(latestUser);
      setDisplayName(latestUser.display_name);
      setManageError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not load this user.");
    } finally {
      setOpeningUserId(null);
    }
  }

  async function handleUpdate(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!managedUser) {
      return;
    }

    const normalizedName = displayName.trim();
    if (!normalizedName) {
      setManageError("Enter a display name.");
      return;
    }

    setSaving(true);
    setManageError(null);
    try {
      const updatedUser = await kival.updateUser({
        userId: managedUser.id,
        input: { display_name: normalizedName },
      });
      setUsers((current) =>
        current.map((candidate) => (candidate.id === updatedUser.id ? updatedUser : candidate)),
      );
      setManagedUser(updatedUser);
      setDisplayName(updatedUser.display_name);
      if (updatedUser.id === user.id) {
        onCurrentUserUpdate(updatedUser);
      }
    } catch (cause) {
      setManageError(cause instanceof Error ? cause.message : "Could not update this user.");
    } finally {
      setSaving(false);
    }
  }

  function completeManagedUserExit(action: ManagedUserExit) {
    if (!managedUser) {
      return;
    }

    const target = managedUser;
    setManagedUser(null);
    setManageError(null);
    setPendingManagedUserExit(null);

    if (action === "disable") {
      setDisableTarget(target);
    } else if (action === "enable") {
      setEnableTarget(target);
    }
  }

  function requestManagedUserExit(action: ManagedUserExit) {
    if (!managedUser || saving) {
      return;
    }

    if (displayName !== managedUser.display_name) {
      setPendingManagedUserExit(action);
      return;
    }

    completeManagedUserExit(action);
  }

  async function handleDisable() {
    if (!disableTarget) {
      return;
    }

    setDisabling(true);
    setError(null);
    try {
      const disabledUser = await kival.disableUser({ userId: disableTarget.id });
      setUsers((current) =>
        statusFilter === "active"
          ? current.filter((candidate) => candidate.id !== disabledUser.id)
          : current.map((candidate) =>
              candidate.id === disabledUser.id ? disabledUser : candidate,
            ),
      );
      setDisableTarget(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not disable this user.");
    } finally {
      setDisabling(false);
    }
  }

  async function handleEnable() {
    if (!enableTarget) {
      return;
    }

    setEnabling(true);
    setError(null);
    try {
      const enabledUser = await kival.enableUser({ userId: enableTarget.id });
      setUsers((current) =>
        statusFilter === "disabled"
          ? current.filter((candidate) => candidate.id !== enabledUser.id)
          : current.map((candidate) => (candidate.id === enabledUser.id ? enabledUser : candidate)),
      );
      setEnableTarget(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not enable this user.");
    } finally {
      setEnabling(false);
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
          active="users"
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
              <h1 style={styles.pageTitle}>Users</h1>
              <p style={styles.muted}>Manage Kival accounts.</p>
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
                <label htmlFor="user-status-filter" style={{ ...styles.field, minWidth: 180 }}>
                  <span style={styles.fieldLabel}>Show users</span>
                  <AnimatedSelect
                    id="user-status-filter"
                    value={statusFilter}
                    style={styles.input}
                    disabled={loading}
                    onChange={(event) => setStatusFilter(event.target.value as UserStatusFilter)}
                  >
                    <option value="active">Active</option>
                    <option value="disabled">Disabled</option>
                    <option value="all">All</option>
                  </AnimatedSelect>
                </label>
                <label htmlFor="user-name-search" style={{ ...styles.field, minWidth: 260 }}>
                  <span style={styles.fieldLabel}>Search by name</span>
                  <input
                    data-1p-ignore="true"
                    id="user-name-search"
                    type="search"
                    value={searchQuery}
                    placeholder="Search users…"
                    autoComplete="off"
                    style={styles.input}
                    onChange={(event) => setSearchQuery(event.target.value)}
                  />
                </label>
              </div>
            </div>

            {loading && (
              <LoadingIndicator label={searchActive ? "Searching users…" : "Loading users…"} />
            )}
            {!loading && error && (
              <div style={styles.errorBox} role="alert">
                <strong>Could not load users</strong>
                <span>{error}</span>
              </div>
            )}

            {!loading && !error && (
              <div className="kival-row-list" style={styles.directoryList}>
                {users.map((candidate) => (
                  <div key={candidate.id} style={styles.directoryRow}>
                    <div style={styles.directoryIdentity}>
                      <div style={styles.directoryAvatar}>{userInitial(candidate)}</div>
                      <div style={styles.directoryMain}>
                        <strong>{candidate.display_name}</strong>
                        <span style={styles.muted}>{candidate.username}</span>
                        <span style={styles.objectMeta}>
                          Created {formatTimestamp(candidate.created_at)}
                        </span>
                      </div>
                    </div>
                    <div style={styles.directoryHeaderActions}>
                      {candidate.id === user.id && <span style={styles.directoryRole}>You</span>}
                      {statusFilter === "all" ? (
                        <span style={styles.directoryRole}>{candidate.status}</span>
                      ) : null}
                      <button
                        type="button"
                        style={styles.secondaryButtonCompact}
                        disabled={openingUserId === candidate.id}
                        onClick={() => void openUser(candidate)}
                      >
                        {openingUserId === candidate.id ? "Opening…" : "Manage"}
                      </button>
                    </div>
                  </div>
                ))}

                {users.length === 0 && (
                  <div style={styles.emptyState}>
                    <strong>
                      {searchActive
                        ? "No matching users"
                        : `No ${statusFilter === "all" ? "" : `${statusFilter} `}users found`}
                    </strong>
                    <span>
                      {searchActive
                        ? `No users match “${normalizedSearchQuery}”.`
                        : "Accounts matching this status will appear here."}
                    </span>
                  </div>
                )}
              </div>
            )}

            <InfiniteScrollSentinel
              hasMore={Boolean(nextCursor)}
              loading={loadingMore}
              onLoadMore={() => void loadMore()}
              label="Loading more users…"
            />
          </div>
        </main>
      </div>

      {managedUser && (
        <div style={styles.modalBackdrop} role="presentation">
          <button
            type="button"
            aria-label="Close user management"
            style={styles.modalBackdropDismiss}
            onClick={() => requestManagedUserExit("close")}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="manage-user-title"
            style={{ ...styles.modalDialog, width: "min(100%, 580px)" }}
          >
            <div style={styles.modalCopy}>
              <p style={styles.eyebrow}>{managedUser.username}</p>
              <h2 id="manage-user-title" style={styles.modalTitle}>
                Manage {managedUser.display_name}
              </h2>
              <p style={styles.muted}>Account created {formatTimestamp(managedUser.created_at)}.</p>
            </div>

            {managedUser.status === "active" ? (
              <>
                <form style={styles.settingsSection} onSubmit={(event) => void handleUpdate(event)}>
                  <label style={styles.field}>
                    <span style={styles.fieldLabel}>Display name</span>
                    <input
                      data-1p-ignore="true"
                      autoComplete="off"
                      autoFocus
                      required
                      value={displayName}
                      style={styles.input}
                      disabled={saving}
                      onChange={(event) => setDisplayName(event.target.value)}
                    />
                  </label>
                  <div style={styles.settingsActions}>
                    <button
                      type="submit"
                      disabled={saving || displayName.trim() === managedUser.display_name}
                      style={styles.primaryButtonCompact}
                    >
                      {saving ? "Saving…" : "Save display name"}
                    </button>
                  </div>
                </form>

                <section style={styles.settingsSection}>
                  <div style={styles.settingsSectionHeader}>
                    <strong>Disable account</strong>
                    <span style={styles.muted}>
                      Disabled users can no longer authenticate or access Kival.
                    </span>
                  </div>
                  {managedUser.id === user.id ? (
                    <span style={styles.muted}>
                      You cannot disable your own account from this session.
                    </span>
                  ) : (
                    <div>
                      <button
                        type="button"
                        style={styles.apiKeyDangerButton}
                        onClick={() => requestManagedUserExit("disable")}
                      >
                        Disable user
                      </button>
                    </div>
                  )}
                </section>
              </>
            ) : (
              <section style={styles.settingsSection}>
                <div style={styles.settingsSectionHeader}>
                  <strong>This account is disabled</strong>
                  <span style={styles.muted}>
                    Disabled {formatTimestamp(managedUser.disabled_at)}. Enabling the account
                    restores access using its existing credentials.
                  </span>
                </div>
                <div>
                  <button
                    type="button"
                    style={styles.primaryButtonCompact}
                    onClick={() => requestManagedUserExit("enable")}
                  >
                    Enable user
                  </button>
                </div>
              </section>
            )}

            {manageError && (
              <div style={styles.loginError} role="alert">
                {manageError}
              </div>
            )}
            <div style={styles.modalActions}>
              <button
                type="button"
                disabled={saving}
                style={styles.secondaryButton}
                onClick={() => requestManagedUserExit("close")}
              >
                Close
              </button>
            </div>
          </div>
        </div>
      )}

      {pendingManagedUserExit ? (
        <ConfirmationDialog
          title="Discard user changes?"
          description="The updated display name has not been saved."
          confirmLabel="Discard changes"
          pendingLabel="Discarding…"
          closeLabel="Keep editing user"
          zIndex={120}
          onCancel={() => setPendingManagedUserExit(null)}
          onConfirm={() => completeManagedUserExit(pendingManagedUserExit)}
        />
      ) : null}

      {disableTarget && (
        <div style={styles.modalBackdrop} role="presentation">
          <button
            type="button"
            aria-label="Cancel disabling user"
            style={styles.modalBackdropDismiss}
            onClick={() => !disabling && setDisableTarget(null)}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="disable-user-title"
            style={styles.modalDialog}
          >
            <div style={styles.modalCopy}>
              <h2 id="disable-user-title" style={styles.modalTitle}>
                Disable {disableTarget.display_name}?
              </h2>
              <p style={styles.muted}>
                {disableTarget.username} will immediately lose access to Kival. Their credentials,
                memberships, and roles will be preserved so access can be restored later.
              </p>
            </div>
            <div style={styles.modalActions}>
              <button
                type="button"
                disabled={disabling}
                style={styles.secondaryButton}
                onClick={() => setDisableTarget(null)}
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={disabling}
                style={styles.apiKeyDangerButtonSolid}
                onClick={() => void handleDisable()}
              >
                {disabling ? "Disabling…" : "Disable user"}
              </button>
            </div>
          </div>
        </div>
      )}

      {enableTarget && (
        <div style={styles.modalBackdrop} role="presentation">
          <button
            type="button"
            aria-label="Cancel enabling user"
            style={styles.modalBackdropDismiss}
            onClick={() => !enabling && setEnableTarget(null)}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="enable-user-title"
            style={styles.modalDialog}
          >
            <div style={styles.modalCopy}>
              <h2 id="enable-user-title" style={styles.modalTitle}>
                Enable {enableTarget.display_name}?
              </h2>
              <p style={styles.muted}>
                {enableTarget.username} will be able to access Kival again using their existing
                passkeys, sessions, and API keys. Memberships and roles are unchanged.
              </p>
            </div>
            <div style={styles.modalActions}>
              <button
                type="button"
                disabled={enabling}
                style={styles.secondaryButton}
                onClick={() => setEnableTarget(null)}
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={enabling}
                style={styles.primaryButtonCompact}
                onClick={() => void handleEnable()}
              >
                {enabling ? "Enabling…" : "Enable user"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
