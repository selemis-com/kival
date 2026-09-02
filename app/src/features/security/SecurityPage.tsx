import { KivalTransportError } from "kival-sdk";
import { useEffect, useState } from "react";
import {
  finishPasskeyRegistration,
  listPasskeys,
  listSessions,
  revokePasskey,
  revokeSession,
  startPasskeyRegistration,
} from "../../shared/api";
import { freshAuthenticate, passkeyActionError } from "../../shared/auth/freshAuthentication";
import { registrationCredential, registrationRequestOptions } from "../../shared/auth/webauthn";
import { formatTimestampOr } from "../../shared/format";
import { KivalSideBar } from "../../shared/navigation/KivalSideBar";
import { TopBar } from "../../shared/navigation/TopBar";
import { styles } from "../../shared/styles/index";
import type { Passkey, Session, User, Workspace } from "../../shared/types";
import { LoadingIndicator } from "../../shared/ui/LoadingIndicator";

type Props = {
  user: User;
  workspaces: Workspace[];
  workspacesNextCursor: string | null;
  workspacesLoadingMore: boolean;
  onLoadMoreWorkspaces: () => void;
  onHome: () => void;
  onInboxClick: () => void;
  unreadInboxCount: number;
  onSecurityClick: () => void;
  onApiKeysClick: () => void;
  onUsersClick?: () => void;
  onGroupsClick?: () => void;
  onEventsClick?: () => void;
  onLogout: () => Promise<void>;
  onCurrentSessionRevoked: () => void;
};

type RevokeTarget = { type: "passkey"; passkey: Passkey } | { type: "session"; session: Session };

function formatTimestamp(value: string | null) {
  return value ? formatTimestampOr(value, "Unknown") : "Never";
}

function deviceLabel(userAgent: string | null) {
  if (!userAgent) {
    return "Unknown browser";
  }

  if (userAgent.includes("Firefox/")) {
    return "Firefox";
  }
  if (userAgent.includes("Edg/")) {
    return "Microsoft Edge";
  }
  if (userAgent.includes("Chrome/")) {
    return "Chrome";
  }
  if (userAgent.includes("Safari/")) {
    return "Safari";
  }

  return "Browser session";
}

export function SecurityPage({
  user,
  workspaces,
  workspacesNextCursor,
  workspacesLoadingMore,
  onLoadMoreWorkspaces,
  onHome,
  onInboxClick,
  unreadInboxCount,
  onSecurityClick,
  onApiKeysClick,
  onUsersClick,
  onGroupsClick,
  onEventsClick,
  onLogout,
  onCurrentSessionRevoked,
}: Props) {
  const [passkeys, setPasskeys] = useState<Passkey[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [label, setLabel] = useState("");
  const [addingPasskey, setAddingPasskey] = useState(false);
  const [revokeTarget, setRevokeTarget] = useState<RevokeTarget | null>(null);
  const [revoking, setRevoking] = useState(false);

  useEffect(() => {
    const controller = new AbortController();

    void Promise.all([listPasskeys(controller.signal), listSessions(controller.signal)])
      .then(([passkeyResponse, sessionResponse]) => {
        setPasskeys(passkeyResponse.items);
        setSessions(sessionResponse.items);
      })
      .catch((cause: unknown) => {
        if (cause instanceof KivalTransportError && cause.kind === "abort") {
          return;
        }
        setError(cause instanceof Error ? cause.message : "Could not load account security.");
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setLoading(false);
        }
      });

    return () => controller.abort();
  }, []);

  async function freshAuthenticateAndRefreshSessions() {
    await freshAuthenticate();
    const response = await listSessions();
    setSessions(response.items);
  }

  async function handleAddPasskey(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const normalizedLabel = label.trim();

    if (!normalizedLabel) {
      setError("Enter a name for this passkey.");
      return;
    }

    setAddingPasskey(true);
    setError(null);

    try {
      if (!window.PublicKeyCredential || !navigator.credentials?.create) {
        throw new Error("This browser or device does not support passkeys.");
      }

      await freshAuthenticateAndRefreshSessions();
      const options = await startPasskeyRegistration();
      const created = await navigator.credentials.create({
        publicKey: registrationRequestOptions(options.publicKey),
      });

      if (!(created instanceof PublicKeyCredential)) {
        throw new Error("The authenticator did not create a passkey.");
      }

      const response = await finishPasskeyRegistration({
        ceremonyId: options.ceremonyId,
        label: normalizedLabel,
        credential: registrationCredential(created),
      });
      setPasskeys((current) => [
        response.passkey,
        ...current.filter((passkey) => passkey.id !== response.passkey.id),
      ]);
      setLabel("");
    } catch (cause) {
      setError(passkeyActionError(cause, "Could not add this passkey."));
    } finally {
      setAddingPasskey(false);
    }
  }

  async function handleRevoke() {
    if (!revokeTarget) {
      return;
    }

    setRevoking(true);
    setError(null);

    try {
      if (revokeTarget.type === "passkey") {
        await freshAuthenticateAndRefreshSessions();
        const response = await revokePasskey(revokeTarget.passkey.id);
        setPasskeys((current) => current.filter((passkey) => passkey.id !== response.passkey.id));
      } else {
        const response = await revokeSession(revokeTarget.session.id);

        if (revokeTarget.session.is_current) {
          onCurrentSessionRevoked();
          return;
        }

        setSessions((current) => current.filter((session) => session.id !== response.session.id));
      }

      setRevokeTarget(null);
    } catch (cause) {
      setError(passkeyActionError(cause, "Could not revoke this credential."));
    } finally {
      setRevoking(false);
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
        onHomeClick={onHome}
        onInboxClick={onInboxClick}
        unreadInboxCount={unreadInboxCount}
        onSecurityClick={onSecurityClick}
        onApiKeysClick={onApiKeysClick}
        onLogout={onLogout}
      />

      <div style={styles.kivalShell}>
        <KivalSideBar
          active="security"
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
              <p style={styles.eyebrow}>Account</p>
              <h1 style={styles.pageTitle}>Security</h1>
              <p style={styles.muted}>
                Manage the passkeys and browser sessions that can access your Kival account.
              </p>
            </div>

            {error && (
              <div style={styles.errorBox} role="alert">
                <strong>Something went wrong</strong>
                <span>{error}</span>
              </div>
            )}

            {loading ? (
              <LoadingIndicator label="Loading account security…" />
            ) : (
              <>
                <section style={styles.apiKeyCreateCard}>
                  <div style={styles.apiKeySectionHeader}>
                    <div>
                      <h2 style={styles.apiKeySectionTitle}>Passkeys</h2>
                      <p style={styles.muted}>
                        Add a passkey for another device or remove one you no longer use.
                      </p>
                    </div>
                  </div>

                  <form
                    style={styles.apiKeyFormGrid}
                    onSubmit={(event) => void handleAddPasskey(event)}
                  >
                    <label style={styles.field}>
                      <span>Passkey name</span>
                      <input
                        data-1p-ignore="true"
                        autoComplete="off"
                        value={label}
                        style={styles.input}
                        placeholder="For example, Work laptop"
                        maxLength={64}
                        required
                        disabled={addingPasskey}
                        onChange={(event) => setLabel(event.target.value)}
                      />
                    </label>
                    <div style={{ display: "flex", alignItems: "flex-end" }}>
                      <button
                        type="submit"
                        style={styles.primaryButtonCompact}
                        disabled={addingPasskey}
                      >
                        {addingPasskey ? "Adding passkey…" : "Add passkey"}
                      </button>
                    </div>
                  </form>

                  <div style={{ ...styles.apiKeyList, marginTop: 22 }}>
                    {passkeys.map((passkey) => (
                      <article key={passkey.id} style={styles.apiKeyCard}>
                        <div style={styles.apiKeyCardHeader}>
                          <div style={styles.apiKeyCardTitle}>
                            <strong>{passkey.label || "Unnamed passkey"}</strong>
                          </div>
                          <button
                            type="button"
                            style={styles.apiKeyDangerButton}
                            disabled={passkeys.length <= 1}
                            title={
                              passkeys.length <= 1
                                ? "Your last passkey cannot be removed."
                                : undefined
                            }
                            onClick={() => setRevokeTarget({ type: "passkey", passkey })}
                          >
                            {passkeys.length <= 1 ? "Required" : "Revoke"}
                          </button>
                        </div>
                        <dl style={{ ...styles.apiKeyMetadata, marginBottom: 0 }}>
                          <div>
                            <dt>Created</dt>
                            <dd>{formatTimestamp(passkey.createdAt)}</dd>
                          </div>
                          <div>
                            <dt>Last used</dt>
                            <dd>{formatTimestamp(passkey.lastUsedAt)}</dd>
                          </div>
                        </dl>
                      </article>
                    ))}
                  </div>
                </section>

                <section style={{ ...styles.apiKeyListSection, marginTop: 32 }}>
                  <div style={styles.apiKeySectionHeader}>
                    <div>
                      <h2 style={styles.apiKeySectionTitle}>Active sessions</h2>
                      <p style={styles.muted}>
                        Revoke browser sessions you do not recognize or no longer use.
                      </p>
                    </div>
                  </div>

                  <div style={styles.apiKeyList}>
                    {sessions.map((session) => (
                      <article key={session.id} style={styles.apiKeyCard}>
                        <div style={styles.apiKeyCardHeader}>
                          <div style={styles.apiKeyCardTitle}>
                            <strong>{deviceLabel(session.user_agent)}</strong>
                            {session.is_current && (
                              <span style={styles.apiKeyStatus}>This session</span>
                            )}
                          </div>
                          <button
                            type="button"
                            style={styles.apiKeyDangerButton}
                            onClick={() => setRevokeTarget({ type: "session", session })}
                          >
                            {session.is_current ? "Log out" : "Revoke"}
                          </button>
                        </div>
                        <dl style={{ ...styles.apiKeyMetadata, marginBottom: 0 }}>
                          <div>
                            <dt>IP address</dt>
                            <dd>{session.ip_address ?? "Unknown"}</dd>
                          </div>
                          <div>
                            <dt>Created</dt>
                            <dd>{formatTimestamp(session.created_at)}</dd>
                          </div>
                          <div>
                            <dt>Last active</dt>
                            <dd>{formatTimestamp(session.last_seen_at)}</dd>
                          </div>
                          <div>
                            <dt>Expires</dt>
                            <dd>{formatTimestamp(session.expires_at)}</dd>
                          </div>
                        </dl>
                      </article>
                    ))}
                  </div>
                </section>
              </>
            )}
          </div>
        </main>
      </div>

      {revokeTarget && (
        <div style={styles.modalBackdrop} role="presentation">
          <button
            type="button"
            aria-label="Cancel revocation"
            style={styles.modalBackdropDismiss}
            onClick={() => !revoking && setRevokeTarget(null)}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="security-revoke-title"
            style={styles.modalDialog}
          >
            <div style={styles.modalCopy}>
              <h2 id="security-revoke-title" style={styles.modalTitle}>
                {revokeTarget.type === "passkey"
                  ? `Revoke ${revokeTarget.passkey.label || "this passkey"}?`
                  : revokeTarget.session.is_current
                    ? "Log out this session?"
                    : "Revoke this session?"}
              </h2>
              <p style={styles.muted}>
                {revokeTarget.type === "passkey"
                  ? "You will verify with another passkey before it is removed."
                  : revokeTarget.session.is_current
                    ? "You will need to sign in again on this browser."
                    : "That browser will need to sign in again."}
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
                {revoking
                  ? "Revoking…"
                  : revokeTarget.type === "session" && revokeTarget.session.is_current
                    ? "Log out"
                    : "Revoke"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
