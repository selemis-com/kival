import { useCallback, useEffect, useState } from "react";
import { createApiKey, listApiKeys, revokeApiKey, updateApiKey } from "../../shared/api";
import { freshAuthenticate, passkeyActionError } from "../../shared/auth/freshAuthentication";
import { usePaginatedResource } from "../../shared/hooks/usePaginatedResource";
import { KivalSideBar } from "../../shared/navigation/KivalSideBar";
import { TopBar } from "../../shared/navigation/TopBar";
import { styles } from "../../shared/styles/index";
import type { ApiKey, ApiKeyScope, User, Workspace } from "../../shared/types";
import { AnimatedSelect } from "../../shared/ui/AnimatedSelect";
import { ConfirmationDialog } from "../../shared/ui/ConfirmationDialog";
import { InfiniteScrollSentinel } from "../../shared/ui/InfiniteScrollSentinel";
import { LoadingIndicator } from "../../shared/ui/LoadingIndicator";

import { ApiKeyCards } from "./ApiKeyCards";
import { apiKeyScopeOptions, type ExpirationOption, expirationOptions } from "./model";

function containsSameValues(left: string[], right: string[]) {
  return left.length === right.length && left.every((value) => right.includes(value));
}

type Props = {
  user: User;
  workspaces: Workspace[];
  workspacesNextCursor: string | null;
  workspacesLoadingMore: boolean;
  onLoadMoreWorkspaces: () => void;
  onHome: () => void;
  onInboxClick: () => void;
  unreadInboxCount: number;
  onLogout: () => Promise<void>;
  onUsersClick?: () => void;
  onGroupsClick?: () => void;
  onEventsClick?: () => void;
  onSecurityClick: () => void;
};

export function ApiKeysPage({
  user,
  workspaces,
  workspacesNextCursor,
  workspacesLoadingMore,
  onLoadMoreWorkspaces,
  onHome,
  onInboxClick,
  unreadInboxCount,
  onLogout,
  onUsersClick,
  onGroupsClick,
  onEventsClick,
  onSecurityClick,
}: Props) {
  const loadApiKeyPage = useCallback(async (cursor: string | null, signal: AbortSignal) => {
    const response = await listApiKeys(cursor, signal);
    return { items: response.items, nextCursor: response.next_cursor ?? null };
  }, []);
  const {
    items: apiKeys,
    setItems: setApiKeys,
    nextCursor,
    loading,
    loadingMore,
    error,
    setError,
    loadMore,
  } = usePaginatedResource({
    queryKey: "api-keys",
    loadPage: loadApiKeyPage,
    errorMessage: "Could not load API keys.",
    itemKey: (apiKey: ApiKey) => apiKey.id,
  });
  const [label, setLabel] = useState("");
  const [scopes, setScopes] = useState<ApiKeyScope[]>([]);
  const [workspaceIds, setWorkspaceIds] = useState<string[]>([]);
  const [selectAllListedWorkspaces, setSelectAllListedWorkspaces] = useState(false);
  const [expiration, setExpiration] = useState<ExpirationOption>("30");
  const [customExpirationDays, setCustomExpirationDays] = useState("");
  const [creating, setCreating] = useState(false);
  const [createdSecret, setCreatedSecret] = useState<{ label: string; token: string } | null>(null);
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState<string | null>(null);
  const [revokeTarget, setRevokeTarget] = useState<ApiKey | null>(null);
  const [revoking, setRevoking] = useState(false);
  const [editTarget, setEditTarget] = useState<ApiKey | null>(null);
  const [editScopes, setEditScopes] = useState<ApiKeyScope[]>([]);
  const [editWorkspaceIds, setEditWorkspaceIds] = useState<string[]>([]);
  const [selectAllListedEditWorkspaces, setSelectAllListedEditWorkspaces] = useState(false);
  const [savingEdit, setSavingEdit] = useState(false);
  const [editError, setEditError] = useState<string | null>(null);
  const [discardEditOpen, setDiscardEditOpen] = useState(false);

  useEffect(() => {
    if (selectAllListedWorkspaces) {
      setWorkspaceIds((current) => {
        const newlyListed = workspaces
          .map((workspace) => workspace.id)
          .filter((workspaceId) => !current.includes(workspaceId));

        return newlyListed.length > 0 ? [...current, ...newlyListed] : current;
      });
    }
  }, [selectAllListedWorkspaces, workspaces]);

  useEffect(() => {
    if (selectAllListedEditWorkspaces) {
      setEditWorkspaceIds((current) => {
        const newlyListed = workspaces
          .map((workspace) => workspace.id)
          .filter((workspaceId) => !current.includes(workspaceId));

        return newlyListed.length > 0 ? [...current, ...newlyListed] : current;
      });
    }
  }, [selectAllListedEditWorkspaces, workspaces]);

  function toggleScope(scope: ApiKeyScope) {
    setScopes((current) =>
      current.includes(scope)
        ? current.filter((candidate) => candidate !== scope)
        : [...current, scope],
    );
  }

  function toggleWorkspace(workspaceId: string) {
    setSelectAllListedWorkspaces(false);
    setWorkspaceIds((current) =>
      current.includes(workspaceId)
        ? current.filter((candidate) => candidate !== workspaceId)
        : [...current, workspaceId],
    );
  }

  function openEdit(apiKey: ApiKey) {
    setEditTarget(apiKey);
    setEditScopes(apiKey.scopes);
    setEditWorkspaceIds(apiKey.workspace_ids);
    setSelectAllListedEditWorkspaces(false);
    setEditError(null);
    setDiscardEditOpen(false);
  }

  function closeEdit() {
    setEditTarget(null);
    setEditError(null);
    setDiscardEditOpen(false);
  }

  function requestCloseEdit() {
    if (!editTarget || savingEdit) {
      return;
    }

    if (
      !containsSameValues(editScopes, editTarget.scopes) ||
      !containsSameValues(editWorkspaceIds, editTarget.workspace_ids)
    ) {
      setDiscardEditOpen(true);
      return;
    }

    closeEdit();
  }

  function toggleEditScope(scope: ApiKeyScope) {
    setEditScopes((current) =>
      current.includes(scope)
        ? current.filter((candidate) => candidate !== scope)
        : [...current, scope],
    );
  }

  function toggleEditWorkspace(workspaceId: string) {
    setSelectAllListedEditWorkspaces(false);
    setEditWorkspaceIds((current) =>
      current.includes(workspaceId)
        ? current.filter((candidate) => candidate !== workspaceId)
        : [...current, workspaceId],
    );
  }

  async function handleUpdateApiKey() {
    if (!editTarget) {
      return;
    }
    if (editScopes.length === 0) {
      setEditError("Select at least one API key scope.");
      return;
    }

    setSavingEdit(true);
    setEditError(null);
    try {
      await freshAuthenticate();
      const response = await updateApiKey(editTarget.id, {
        authorization_revision: editTarget.authorization_revision,
        scopes: editScopes,
        workspace_ids: editWorkspaceIds,
      });
      setApiKeys((current) =>
        current.map((apiKey) => (apiKey.id === response.api_key.id ? response.api_key : apiKey)),
      );
      closeEdit();
    } catch (cause) {
      setEditError(passkeyActionError(cause, "Could not update this API key."));
    } finally {
      setSavingEdit(false);
    }
  }

  const workspaceNames = new Map(workspaces.map((workspace) => [workspace.id, workspace.name]));
  const unrevokedApiKeys = apiKeys.filter((apiKey) => !apiKey.revoked_at);
  const revokedApiKeys = apiKeys.filter((apiKey) => Boolean(apiKey.revoked_at));

  return (
    <div style={styles.app}>
      <TopBar
        user={user}
        workspaces={workspaces}
        workspacesNextCursor={workspacesNextCursor}
        workspacesLoadingMore={workspacesLoadingMore}
        onLoadMoreWorkspaces={onLoadMoreWorkspaces}
        onHomeClick={onHome}
        onInboxClick={onInboxClick}
        unreadInboxCount={unreadInboxCount}
        onSecurityClick={onSecurityClick}
        onApiKeysClick={() => undefined}
        onLogout={onLogout}
      />

      <div style={styles.kivalShell}>
        <KivalSideBar
          active="api-keys"
          onWorkspacesClick={onHome}
          onUsersClick={onUsersClick}
          onGroupsClick={onGroupsClick}
          onEventsClick={onEventsClick}
          onSecurityClick={onSecurityClick}
          onApiKeysClick={() => undefined}
        />

        <main style={styles.apiKeysPage}>
          <div style={styles.contentPaneInner}>
            <div style={styles.pageHeader}>
              <p style={styles.eyebrow}>Account settings</p>
              <h1 style={styles.pageTitle}>API keys</h1>
              <p style={styles.muted}>
                Create bearer credentials for the Kival CLI, agents, and automation. Keys can only
                exercise the scopes and workspaces selected here.
              </p>
            </div>

            {error && (
              <div style={styles.errorBox} role="alert">
                <strong>Something went wrong</strong>
                <span>{error}</span>
              </div>
            )}

            <section style={styles.apiKeyCreateCard}>
              <div style={styles.apiKeySectionHeader}>
                <div>
                  <h2 style={styles.apiKeySectionTitle}>Create an API key</h2>
                  <p style={styles.muted}>
                    The secret is displayed once and cannot be recovered later.
                  </p>
                </div>
              </div>

              <form
                style={styles.apiKeyForm}
                onSubmit={async (event) => {
                  event.preventDefault();
                  const normalizedLabel = label.trim();

                  if (!normalizedLabel) {
                    setError("API key label is required.");
                    return;
                  }

                  if (scopes.length === 0) {
                    setError("Select at least one API key scope.");
                    return;
                  }

                  let expirationDays: number | null = null;

                  if (expiration === "custom") {
                    expirationDays = Number(customExpirationDays);

                    if (!Number.isSafeInteger(expirationDays) || expirationDays <= 0) {
                      setError("Custom expiration must be a positive whole number of days.");
                      return;
                    }
                  } else if (expiration !== "none") {
                    expirationDays = Number(expiration);
                  }

                  const expirationTime =
                    expirationDays === null ? null : Date.now() + expirationDays * 86_400_000;

                  if (expirationTime !== null && !Number.isFinite(expirationTime)) {
                    setError("Custom expiration is too large.");
                    return;
                  }

                  setCreating(true);
                  setError(null);

                  try {
                    await freshAuthenticate();
                    const response = await createApiKey({
                      label: normalizedLabel,
                      scopes,
                      workspace_ids: workspaceIds,
                      expires_at:
                        expirationTime === null ? null : new Date(expirationTime).toISOString(),
                    });
                    setApiKeys((current) => [
                      response.api_key,
                      ...current.filter((apiKey) => apiKey.id !== response.api_key.id),
                    ]);
                    setCreatedSecret({ label: response.api_key.label, token: response.token });
                    setCopied(false);
                    setCopyError(null);
                    setLabel("");
                    setScopes([]);
                    setWorkspaceIds([]);
                    setSelectAllListedWorkspaces(false);
                    setExpiration("30");
                    setCustomExpirationDays("");
                  } catch (cause) {
                    setError(passkeyActionError(cause, "Could not create API key."));
                  } finally {
                    setCreating(false);
                  }
                }}
              >
                <div style={styles.apiKeyFormGrid}>
                  <label style={styles.field}>
                    <span style={styles.fieldLabel}>Label</span>
                    <input
                      data-1p-ignore="true"
                      value={label}
                      onChange={(event) => setLabel(event.target.value)}
                      maxLength={64}
                      placeholder="Personal CLI"
                      autoComplete="off"
                      required
                      style={styles.input}
                    />
                  </label>

                  <label htmlFor="api-key-expiration" style={styles.field}>
                    <span style={styles.fieldLabel}>Expiration</span>
                    <AnimatedSelect
                      id="api-key-expiration"
                      value={expiration}
                      onChange={(event) => setExpiration(event.target.value as ExpirationOption)}
                      style={styles.input}
                    >
                      {expirationOptions.map(([value, optionLabel]) => (
                        <option key={value} value={value}>
                          {optionLabel}
                        </option>
                      ))}
                    </AnimatedSelect>
                    {expiration === "custom" && (
                      <input
                        type="number"
                        data-1p-ignore="true"
                        autoComplete="off"
                        min="1"
                        step="1"
                        value={customExpirationDays}
                        onChange={(event) => setCustomExpirationDays(event.target.value)}
                        placeholder="Number of days"
                        aria-label="Custom expiration in days"
                        required
                        style={styles.input}
                      />
                    )}
                  </label>
                </div>

                <fieldset style={styles.apiKeyFieldset}>
                  <legend style={styles.apiKeyLegend}>Scopes</legend>
                  <label style={styles.apiKeySelectAllOption}>
                    <input
                      type="checkbox"
                      checked={apiKeyScopeOptions.every(([scope]) => scopes.includes(scope))}
                      onChange={(event) =>
                        setScopes(
                          event.target.checked ? apiKeyScopeOptions.map(([scope]) => scope) : [],
                        )
                      }
                    />
                    <span>
                      <strong>All scopes</strong>
                      <span style={styles.apiKeyHelpInline}>
                        Select every scope explicitly, then remove anything this key should not
                        have.
                      </span>
                    </span>
                  </label>
                  <p style={styles.apiKeyHelp}>
                    Select only the capabilities this key needs. Write scopes also satisfy their
                    corresponding read scope.
                  </p>
                  <div style={styles.apiKeyOptionGrid}>
                    {apiKeyScopeOptions.map(([scope, title, description]) => {
                      const selected = scopes.includes(scope);

                      return (
                        <label
                          key={scope}
                          style={selected ? styles.apiKeyOptionSelected : styles.apiKeyOption}
                        >
                          <input
                            type="checkbox"
                            checked={selected}
                            onChange={() => toggleScope(scope)}
                          />
                          <span style={styles.apiKeyOptionCopy}>
                            <strong>{title}</strong>
                            <span>{description}</span>
                          </span>
                        </label>
                      );
                    })}
                  </div>
                </fieldset>

                <fieldset style={styles.apiKeyFieldset}>
                  <legend style={styles.apiKeyLegend}>Workspace access</legend>
                  <label style={styles.apiKeySelectAllOption}>
                    <input
                      type="checkbox"
                      checked={selectAllListedWorkspaces && workspaces.length > 0}
                      onChange={(event) => {
                        setSelectAllListedWorkspaces(event.target.checked);
                        setWorkspaceIds(
                          event.target.checked ? workspaces.map((workspace) => workspace.id) : [],
                        );
                      }}
                    />
                    <span>
                      <strong>All listed workspaces</strong>
                      <span style={styles.apiKeyHelpInline}>
                        Newly loaded workspace pages remain selected until you uncheck an item.
                      </span>
                    </span>
                  </label>
                  <p style={styles.apiKeyHelp}>
                    An empty selection grants no workspace-scoped access.
                  </p>
                  <div style={styles.apiKeyWorkspaceGrid}>
                    {workspaces.map((workspace) => {
                      const selected = workspaceIds.includes(workspace.id);
                      return (
                        <label
                          key={workspace.id}
                          style={
                            selected
                              ? styles.apiKeyWorkspaceOptionSelected
                              : styles.apiKeyWorkspaceOption
                          }
                        >
                          <input
                            type="checkbox"
                            checked={selected}
                            onChange={() => toggleWorkspace(workspace.id)}
                          />
                          <span>{workspace.name}</span>
                        </label>
                      );
                    })}
                  </div>
                  <InfiniteScrollSentinel
                    hasMore={Boolean(workspacesNextCursor)}
                    loading={workspacesLoadingMore}
                    onLoadMore={onLoadMoreWorkspaces}
                    label="Loading more workspaces…"
                  />
                </fieldset>

                <div style={styles.apiKeyFormActions}>
                  <button type="submit" disabled={creating} style={styles.primaryButtonCompact}>
                    {creating ? "Verifying and creating…" : "Create API key"}
                  </button>
                </div>
              </form>
            </section>

            <section style={styles.apiKeyListSection}>
              <div style={styles.apiKeySectionHeader}>
                <div>
                  <h2 style={styles.apiKeySectionTitle}>Your API keys</h2>
                  <p style={styles.muted}>Secrets are never shown again after creation.</p>
                </div>
              </div>

              {loading ? (
                <LoadingIndicator label="Loading API keys…" />
              ) : apiKeys.length === 0 ? (
                <div style={styles.apiKeyEmpty}>You have not created any API keys.</div>
              ) : (
                <div style={styles.apiKeyGroupedLists}>
                  {unrevokedApiKeys.length === 0 ? (
                    <div style={styles.apiKeyEmpty}>No current API keys.</div>
                  ) : (
                    <ApiKeyCards
                      apiKeys={unrevokedApiKeys}
                      workspaceNames={workspaceNames}
                      onEdit={openEdit}
                      onRevoke={setRevokeTarget}
                    />
                  )}

                  {revokedApiKeys.length > 0 && (
                    <details style={styles.apiKeyRevokedDetails}>
                      <summary style={styles.apiKeyRevokedSummary}>
                        <span>Revoked API keys</span>
                        <span style={styles.apiKeyRevokedCount}>{revokedApiKeys.length}</span>
                      </summary>
                      <div style={styles.apiKeyRevokedContent}>
                        <ApiKeyCards
                          apiKeys={revokedApiKeys}
                          workspaceNames={workspaceNames}
                          onEdit={openEdit}
                          onRevoke={setRevokeTarget}
                        />
                      </div>
                    </details>
                  )}
                </div>
              )}

              <InfiniteScrollSentinel
                hasMore={Boolean(nextCursor)}
                loading={loadingMore}
                onLoadMore={() => void loadMore()}
                label="Loading more API keys…"
              />
            </section>
          </div>
        </main>
      </div>

      {createdSecret && (
        <div style={styles.modalBackdrop} role="presentation">
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="api-key-created-title"
            style={styles.apiKeySecretDialog}
          >
            <div style={styles.modalCopy}>
              <p style={styles.eyebrow}>Created successfully</p>
              <h2 id="api-key-created-title" style={styles.modalTitle}>
                Copy {createdSecret.label}
              </h2>
              <p style={styles.muted}>
                This is the only time Kival will display this API key. Store it securely before
                closing this dialog.
              </p>
            </div>
            <code style={styles.apiKeySecret}>{createdSecret.token}</code>
            {copyError && (
              <div style={styles.loginError} role="alert">
                {copyError}
              </div>
            )}
            <div style={styles.modalActions}>
              <button
                type="button"
                style={styles.secondaryButton}
                onClick={async () => {
                  try {
                    await navigator.clipboard.writeText(createdSecret.token);
                    setCopied(true);
                    setCopyError(null);
                  } catch {
                    setCopyError("Could not copy the API key. Select and copy it manually.");
                  }
                }}
              >
                {copied ? "Copied" : "Copy"}
              </button>
              <button
                type="button"
                style={styles.primaryButtonCompact}
                onClick={() => {
                  setCreatedSecret(null);
                  setCopied(false);
                  setCopyError(null);
                }}
              >
                I have stored it
              </button>
            </div>
          </div>
        </div>
      )}

      {editTarget && (
        <div style={styles.modalBackdrop} role="presentation">
          <button
            type="button"
            aria-label="Close API key access editor"
            style={styles.modalBackdropDismiss}
            onClick={requestCloseEdit}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="edit-api-key-title"
            style={{
              ...styles.modalDialog,
              width: "min(100%, 760px)",
              maxHeight: "calc(100vh - 48px)",
              overflowY: "auto",
            }}
          >
            <div style={styles.modalCopy}>
              <p style={styles.eyebrow}>Session-controlled change</p>
              <h2 id="edit-api-key-title" style={styles.modalTitle}>
                Edit access for {editTarget.label}
              </h2>
              <p style={styles.muted}>
                The existing token remains valid. Your passkey will be requested before these
                changes are applied.
              </p>
            </div>

            <fieldset style={styles.apiKeyFieldset}>
              <legend style={styles.apiKeyLegend}>Scopes</legend>
              <label style={styles.apiKeySelectAllOption}>
                <input
                  type="checkbox"
                  checked={apiKeyScopeOptions.every(([scope]) => editScopes.includes(scope))}
                  onChange={(event) =>
                    setEditScopes(
                      event.target.checked ? apiKeyScopeOptions.map(([scope]) => scope) : [],
                    )
                  }
                />
                <span>
                  <strong>All scopes</strong>
                  <span style={styles.apiKeyHelpInline}>
                    Select everything, then uncheck scopes this key should not have.
                  </span>
                </span>
              </label>
              <div style={styles.apiKeyOptionGrid}>
                {apiKeyScopeOptions.map(([scope, title, description]) => {
                  const selected = editScopes.includes(scope);
                  return (
                    <label
                      key={scope}
                      style={selected ? styles.apiKeyOptionSelected : styles.apiKeyOption}
                    >
                      <input
                        type="checkbox"
                        checked={selected}
                        onChange={() => toggleEditScope(scope)}
                      />
                      <span style={styles.apiKeyOptionCopy}>
                        <strong>{title}</strong>
                        <span>{description}</span>
                      </span>
                    </label>
                  );
                })}
              </div>
            </fieldset>

            <fieldset style={styles.apiKeyFieldset}>
              <legend style={styles.apiKeyLegend}>Workspace access</legend>
              <label style={styles.apiKeySelectAllOption}>
                <input
                  type="checkbox"
                  checked={selectAllListedEditWorkspaces && workspaces.length > 0}
                  onChange={(event) => {
                    setSelectAllListedEditWorkspaces(event.target.checked);
                    setEditWorkspaceIds(
                      event.target.checked ? workspaces.map((workspace) => workspace.id) : [],
                    );
                  }}
                />
                <span>
                  <strong>All listed workspaces</strong>
                  <span style={styles.apiKeyHelpInline}>
                    Newly loaded workspace pages remain selected until you uncheck an item.
                  </span>
                </span>
              </label>
              <p style={styles.apiKeyHelp}>An empty selection grants no workspace-scoped access.</p>
              <div style={styles.apiKeyWorkspaceGrid}>
                {workspaces.map((workspace) => {
                  const selected = editWorkspaceIds.includes(workspace.id);
                  return (
                    <label
                      key={workspace.id}
                      style={
                        selected
                          ? styles.apiKeyWorkspaceOptionSelected
                          : styles.apiKeyWorkspaceOption
                      }
                    >
                      <input
                        type="checkbox"
                        checked={selected}
                        onChange={() => toggleEditWorkspace(workspace.id)}
                      />
                      <span>{workspace.name}</span>
                    </label>
                  );
                })}
              </div>
              <InfiniteScrollSentinel
                hasMore={Boolean(workspacesNextCursor)}
                loading={workspacesLoadingMore}
                onLoadMore={onLoadMoreWorkspaces}
                label="Loading more workspaces…"
              />
            </fieldset>

            {editError && (
              <div style={styles.loginError} role="alert">
                {editError}
              </div>
            )}

            <div style={styles.modalActions}>
              <button
                type="button"
                disabled={savingEdit}
                style={styles.secondaryButton}
                onClick={requestCloseEdit}
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={savingEdit}
                style={styles.primaryButtonCompact}
                onClick={() => void handleUpdateApiKey()}
              >
                {savingEdit ? "Verifying and saving…" : "Save access"}
              </button>
            </div>
          </div>
        </div>
      )}

      {discardEditOpen ? (
        <ConfirmationDialog
          title="Discard API key changes?"
          description="The updated scopes and workspace access have not been saved."
          confirmLabel="Discard changes"
          pendingLabel="Discarding…"
          closeLabel="Keep editing API key access"
          zIndex={120}
          onCancel={() => setDiscardEditOpen(false)}
          onConfirm={closeEdit}
        />
      ) : null}

      {revokeTarget && (
        <div style={styles.modalBackdrop} role="presentation">
          <button
            type="button"
            aria-label="Cancel API key revocation"
            style={styles.modalBackdropDismiss}
            onClick={() => !revoking && setRevokeTarget(null)}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="revoke-api-key-title"
            style={styles.modalDialog}
          >
            <div style={styles.modalCopy}>
              <h2 id="revoke-api-key-title" style={styles.modalTitle}>
                Revoke {revokeTarget.label}?
              </h2>
              <p style={styles.muted}>
                Any CLI, agent, or automation using this key will lose access immediately.
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
                onClick={async () => {
                  setRevoking(true);
                  setError(null);

                  try {
                    const response = await revokeApiKey(revokeTarget.id);
                    setApiKeys((current) =>
                      current.map((apiKey) =>
                        apiKey.id === response.api_key.id ? response.api_key : apiKey,
                      ),
                    );
                    setRevokeTarget(null);
                  } catch (cause) {
                    setError(cause instanceof Error ? cause.message : "Could not revoke API key.");
                  } finally {
                    setRevoking(false);
                  }
                }}
              >
                {revoking ? "Revoking…" : "Revoke API key"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
