import { KivalApiError, KivalTransportError } from "kival-sdk";
import { useCallback, useEffect, useRef, useState } from "react";
import { matchPath, Outlet, useLocation, useNavigate } from "react-router";
import { LoginPage } from "../features/auth/LoginPage";
import {
  createWorkspace,
  finishPasskeyAuthentication,
  getCurrentIdentity,
  kival,
  logout,
  startPasskeyAuthentication,
} from "../shared/api";
import { authenticationCredential, decodeBase64Url } from "../shared/auth/webauthn";
import { publishRealtimeMessage, realtimeWebSocketUrl } from "../shared/realtime";
import type { AuthState, RealtimeMessage, User, Workspace } from "../shared/types";
import type { KivalAppContext } from "./context";
import { DocumentTitleProvider } from "./documentTitle";

const REALTIME_CREDENTIAL_INACTIVE_CLOSE_CODE = 1008;

function defaultPageTitle(pathname: string, workspaces: Workspace[]) {
  const normalizedPathname = pathname.replace(/\/+$/, "") || "/";
  const staticTitles: Record<string, string> = {
    "/": "Workspaces",
    "/inbox": "Inbox",
    "/users": "Users",
    "/groups": "Groups",
    "/events": "Events",
    "/settings/security": "Security",
    "/settings/api-keys": "API keys",
  };
  const staticTitle = staticTitles[normalizedPathname];
  if (staticTitle) {
    return staticTitle;
  }

  const workspaceRoute = matchPath("/w/:workspaceId/*", normalizedPathname);
  if (workspaceRoute?.params.workspaceId) {
    return (
      workspaces.find((workspace) => workspace.id === workspaceRoute.params.workspaceId)?.name ??
      "Workspace"
    );
  }

  return "Kival";
}

async function listAllPinnedWorkspaces(signal?: AbortSignal): Promise<Workspace[]> {
  const pinned: Workspace[] = [];
  let cursor: string | null = null;

  do {
    const response = await kival.listWorkspaces({ pinned: true, cursor, signal });
    pinned.push(...response.items);
    cursor = response.next_cursor ?? null;
  } while (cursor);

  return pinned;
}

export function KivalApp() {
  const [authState, setAuthState] = useState<AuthState>("checking");
  const [user, setUser] = useState<User | null>(null);
  const [isGlobalAdmin, setIsGlobalAdmin] = useState(false);
  const [canManageGroups, setCanManageGroups] = useState(false);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [pinnedWorkspaces, setPinnedWorkspaces] = useState<Workspace[]>([]);
  const [workspacesNextCursor, setWorkspacesNextCursor] = useState<string | null>(null);
  const [workspacesLoadingMore, setWorkspacesLoadingMore] = useState(false);
  const [unreadInboxCount, setUnreadInboxCount] = useState(0);
  const [inboxRevision, setInboxRevision] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const workspaceLoadControllerRef = useRef<AbortController | null>(null);
  const workspaceGenerationRef = useRef(0);
  const navigate = useNavigate();
  const location = useLocation();

  const clearAuthenticatedState = useCallback(
    (message: string | null = null) => {
      workspaceLoadControllerRef.current?.abort();
      workspaceGenerationRef.current += 1;
      setUser(null);
      setIsGlobalAdmin(false);
      setCanManageGroups(false);
      setWorkspaces([]);
      setPinnedWorkspaces([]);
      setWorkspacesNextCursor(null);
      setWorkspacesLoadingMore(false);
      setUnreadInboxCount(0);
      setInboxRevision(0);
      setError(message);
      setAuthState("anonymous");
      navigate("/", { replace: true });
    },
    [navigate],
  );

  useEffect(() => {
    if (authState !== "authenticated") {
      document.title = authState === "anonymous" ? "Sign in · Kival" : "Kival";
    }
  }, [authState]);

  const refreshInbox = useCallback(async (signal?: AbortSignal) => {
    const response = await kival.getInboxUnreadCount({ signal });
    setUnreadInboxCount(response.unread_count);
    setInboxRevision((revision) => revision + 1);
  }, []);

  const refreshWorkspaceDirectory = useCallback(async () => {
    const controller = new AbortController();
    workspaceLoadControllerRef.current?.abort();
    workspaceLoadControllerRef.current = controller;
    workspaceGenerationRef.current += 1;
    const generation = workspaceGenerationRef.current;
    setWorkspacesLoadingMore(false);

    try {
      const [response, pinned] = await Promise.all([
        kival.listWorkspaces({ signal: controller.signal }),
        listAllPinnedWorkspaces(controller.signal),
      ]);

      if (controller.signal.aborted || workspaceGenerationRef.current !== generation) {
        return;
      }

      setWorkspaces(response.items);
      setPinnedWorkspaces(pinned);
      setWorkspacesNextCursor(response.next_cursor ?? null);
    } finally {
      if (workspaceLoadControllerRef.current === controller) {
        workspaceLoadControllerRef.current = null;
      }
    }
  }, []);

  useEffect(() => {
    const controller = new AbortController();

    async function restoreSession() {
      let identity: Awaited<ReturnType<typeof getCurrentIdentity>>;

      try {
        identity = await getCurrentIdentity(controller.signal);
      } catch (cause) {
        if (cause instanceof KivalTransportError && cause.kind === "abort") {
          return;
        }

        setAuthState("anonymous");
        return;
      }

      setUser(identity.user);
      setIsGlobalAdmin(identity.is_global_admin ?? false);
      setCanManageGroups(identity.can_manage_groups ?? false);
      setAuthState("authenticated");

      void refreshInbox(controller.signal).catch((cause: unknown) => {
        if (!(cause instanceof KivalTransportError && cause.kind === "abort")) {
          setUnreadInboxCount(0);
        }
      });

      try {
        await refreshWorkspaceDirectory();
      } catch (cause) {
        if (cause instanceof KivalTransportError && cause.kind === "abort") {
          return;
        }

        setWorkspaces([]);
        setPinnedWorkspaces([]);
        setWorkspacesNextCursor(null);
        setError(cause instanceof Error ? cause.message : "Could not load workspaces.");
      }
    }

    void restoreSession();
    return () => controller.abort();
  }, [refreshInbox, refreshWorkspaceDirectory]);

  useEffect(() => () => workspaceLoadControllerRef.current?.abort(), []);

  useEffect(() => {
    if (authState !== "authenticated") {
      return;
    }

    const retryDelays = [1_000, 2_000, 5_000, 10_000, 30_000];
    let stopped = false;
    let retryAttempt = 0;
    let retryTimer: number | null = null;
    let socket: WebSocket | null = null;

    const refreshAuthoritativeState = () => {
      void getCurrentIdentity()
        .then((identity) => {
          if (stopped) {
            return;
          }
          setUser(identity.user);
          setIsGlobalAdmin(identity.is_global_admin ?? false);
          setCanManageGroups(identity.can_manage_groups ?? false);
        })
        .catch((cause: unknown) => {
          if (!stopped && cause instanceof KivalApiError && cause.kind === "unauthorized") {
            stopped = true;
            clearAuthenticatedState("Your session is no longer active. Sign in again.");
          }
        });
      void refreshInbox().catch(() => undefined);
      void refreshWorkspaceDirectory().catch(() => undefined);
    };

    const scheduleReconnect = () => {
      if (stopped || retryTimer !== null) {
        return;
      }

      const delay = retryDelays[Math.min(retryAttempt, retryDelays.length - 1)] ?? 30_000;
      retryAttempt += 1;
      retryTimer = window.setTimeout(() => {
        retryTimer = null;
        void verifySessionAndConnect();
      }, delay);
    };

    const handleMessage = (event: MessageEvent<string>) => {
      let message: RealtimeMessage;

      try {
        message = JSON.parse(event.data) as RealtimeMessage;
      } catch {
        return;
      }

      if (!message || typeof message.type !== "string") {
        return;
      }

      publishRealtimeMessage(message);
      if (message.type === "inbox.updated") {
        void refreshInbox().catch(() => undefined);
      } else if (message.type === "realtime.resync_required") {
        refreshAuthoritativeState();
      }
    };

    function connect() {
      if (
        stopped ||
        socket?.readyState === WebSocket.OPEN ||
        socket?.readyState === WebSocket.CONNECTING
      ) {
        return;
      }

      socket = new WebSocket(realtimeWebSocketUrl());
      socket.addEventListener("open", () => {
        retryAttempt = 0;
        publishRealtimeMessage({
          type: "realtime.resync_required",
          workspace_id: null,
          object_id: null,
          event_id: null,
          inbox_entry_id: null,
        });
        refreshAuthoritativeState();
      });
      socket.addEventListener("message", handleMessage);
      socket.addEventListener("close", (event) => {
        socket = null;
        if (stopped) {
          return;
        }

        if (event.code === REALTIME_CREDENTIAL_INACTIVE_CLOSE_CODE) {
          // Fresh passkey authentication rotates the browser session. The existing
          // realtime connection is still bound to the replaced session and is
          // therefore closed as inactive even though the browser now has a valid
          // replacement session. Revalidate the current cookie before treating the
          // close as a logout.
          void verifySessionAndConnect();
          return;
        }

        scheduleReconnect();
      });
      socket.addEventListener("error", () => socket?.close());
    }

    async function verifySessionAndConnect() {
      if (stopped) {
        return;
      }

      try {
        await getCurrentIdentity();
      } catch (cause) {
        if (stopped) {
          return;
        }

        if (cause instanceof KivalApiError && cause.kind === "unauthorized") {
          stopped = true;
          if (retryTimer !== null) {
            window.clearTimeout(retryTimer);
            retryTimer = null;
          }
          clearAuthenticatedState("Your session is no longer active. Sign in again.");
          return;
        }

        scheduleReconnect();
        return;
      }

      connect();
    }

    const handleOnline = () => {
      if (retryTimer !== null) {
        window.clearTimeout(retryTimer);
        retryTimer = null;
      }
      void verifySessionAndConnect();
    };

    window.addEventListener("online", handleOnline);
    connect();

    return () => {
      stopped = true;
      window.removeEventListener("online", handleOnline);
      if (retryTimer !== null) {
        window.clearTimeout(retryTimer);
      }
      socket?.close();
    };
  }, [authState, clearAuthenticatedState, refreshInbox, refreshWorkspaceDirectory]);

  async function loadMoreWorkspaces() {
    if (!workspacesNextCursor || workspacesLoadingMore) {
      return;
    }

    const cursor = workspacesNextCursor;
    const generation = workspaceGenerationRef.current;
    const controller = new AbortController();
    workspaceLoadControllerRef.current?.abort();
    workspaceLoadControllerRef.current = controller;
    setWorkspacesLoadingMore(true);

    try {
      const response = await kival.listWorkspaces({ cursor, signal: controller.signal });

      if (controller.signal.aborted || workspaceGenerationRef.current !== generation) {
        return;
      }

      setWorkspaces((current) => {
        const existingIds = new Set(current.map((workspace) => workspace.id));
        return [
          ...current,
          ...response.items.filter((workspace) => !existingIds.has(workspace.id)),
        ];
      });
      setWorkspacesNextCursor(response.next_cursor ?? null);
    } catch (cause) {
      if (
        (cause instanceof KivalTransportError && cause.kind === "abort") ||
        workspaceGenerationRef.current !== generation
      ) {
        return;
      }

      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      if (workspaceGenerationRef.current === generation) {
        setWorkspacesLoadingMore(false);
      }
      if (workspaceLoadControllerRef.current === controller) {
        workspaceLoadControllerRef.current = null;
      }
    }
  }

  async function handleCreateWorkspace(name: string, description?: string) {
    const response = await createWorkspace({ name, description });
    setWorkspaces((current) => [
      response.workspace,
      ...current.filter((workspace) => workspace.id !== response.workspace.id),
    ]);
    setError(null);
    navigate(`/w/${response.workspace.id}`);
  }

  async function handleRestoreWorkspace(workspaceId: string) {
    const workspace = await kival.unarchiveWorkspace({ workspaceId });
    setWorkspaces((current) => [
      workspace,
      ...current.filter((candidate) => candidate.id !== workspace.id),
    ]);
    setError(null);

    void listAllPinnedWorkspaces()
      .then((pinned) => {
        setPinnedWorkspaces(pinned);
        const restored = pinned.find((candidate) => candidate.id === workspace.id);
        if (restored) {
          setWorkspaces((current) =>
            current.map((candidate) => (candidate.id === workspace.id ? restored : candidate)),
          );
        }
      })
      .catch(() => undefined);
  }

  async function handleSetWorkspacePin(workspaceId: string, pinned: boolean) {
    const state = await kival.setWorkspacePin({ workspaceId, pinned });
    const source =
      workspaces.find((workspace) => workspace.id === workspaceId) ??
      pinnedWorkspaces.find((workspace) => workspace.id === workspaceId);
    setWorkspaces((current) =>
      current.map((workspace) =>
        workspace.id === workspaceId
          ? { ...workspace, pinned: state.pinned, pinned_at: state.pinned_at }
          : workspace,
      ),
    );
    setPinnedWorkspaces((current) => {
      if (!state.pinned) {
        return current.filter((workspace) => workspace.id !== workspaceId);
      }

      if (!source) {
        return current;
      }

      const updated = { ...source, pinned: true, pinned_at: state.pinned_at };
      return [updated, ...current.filter((workspace) => workspace.id !== workspaceId)];
    });
  }

  const replaceWorkspace = useCallback((workspace: Workspace) => {
    setWorkspaces((current) => {
      const existingIndex = current.findIndex((candidate) => candidate.id === workspace.id);

      if (existingIndex === -1) {
        return [...current, workspace];
      }

      return current.map((candidate) =>
        candidate.id === workspace.id
          ? { ...candidate, ...workspace, pinned: workspace.pinned ?? candidate.pinned }
          : candidate,
      );
    });
    setPinnedWorkspaces((current) =>
      current.map((candidate) =>
        candidate.id === workspace.id
          ? { ...candidate, ...workspace, pinned: candidate.pinned, pinned_at: candidate.pinned_at }
          : candidate,
      ),
    );
  }, []);

  const removeWorkspace = useCallback((workspaceId: string) => {
    setWorkspaces((current) => current.filter((workspace) => workspace.id !== workspaceId));
    setPinnedWorkspaces((current) => current.filter((workspace) => workspace.id !== workspaceId));
  }, []);

  const replaceCurrentUser = useCallback((nextUser: User) => {
    setUser(nextUser);
  }, []);

  async function handleLogin(username: string) {
    setLoading(true);
    setError(null);

    try {
      if (!window.PublicKeyCredential || !navigator.credentials) {
        throw new Error("This browser or device does not support passkeys.");
      }

      const options = await startPasskeyAuthentication(username.trim());
      const publicKey: PublicKeyCredentialRequestOptions = {
        ...options.publicKey,
        challenge: decodeBase64Url(options.publicKey.challenge),
        allowCredentials: options.publicKey.allowCredentials.map((credential) => ({
          ...credential,
          id: decodeBase64Url(credential.id),
        })),
      };
      const assertion = await navigator.credentials.get({ publicKey });

      if (!(assertion instanceof PublicKeyCredential)) {
        throw new Error("The authenticator did not return a passkey.");
      }

      await finishPasskeyAuthentication({
        ceremonyId: options.ceremonyId,
        credential: authenticationCredential(assertion),
      });
      const identity = await getCurrentIdentity();

      setUser(identity.user);
      setIsGlobalAdmin(identity.is_global_admin ?? false);
      setCanManageGroups(identity.can_manage_groups ?? false);
      setAuthState("authenticated");
      void refreshInbox().catch(() => setUnreadInboxCount(0));

      try {
        await refreshWorkspaceDirectory();
      } catch (cause) {
        setWorkspaces([]);
        setPinnedWorkspaces([]);
        setWorkspacesNextCursor(null);
        setError(
          cause instanceof Error
            ? `Signed in, but workspaces could not be loaded: ${cause.message}`
            : "Signed in, but workspaces could not be loaded.",
        );
      }
    } catch (cause) {
      if (cause instanceof DOMException && cause.name === "NotAllowedError") {
        setError("Passkey sign-in was cancelled or was not allowed by this device.");
      } else {
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    } finally {
      setLoading(false);
    }
  }

  async function handleLogout() {
    setLoading(true);
    setError(null);

    try {
      await logout();
      clearAuthenticatedState();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      throw cause;
    } finally {
      setLoading(false);
    }
  }

  async function refreshCurrentIdentity() {
    const identity = await getCurrentIdentity();
    setUser(identity.user);
    setIsGlobalAdmin(identity.is_global_admin ?? false);
    setCanManageGroups(identity.can_manage_groups ?? false);
    return identity.can_manage_groups ?? false;
  }

  if (authState === "checking") {
    return null;
  }

  if (authState === "anonymous" || !user) {
    return <LoginPage loading={loading} error={error} onLogin={handleLogin} />;
  }

  const context: KivalAppContext = {
    user,
    isGlobalAdmin,
    canManageGroups,
    workspaces,
    pinnedWorkspaces,
    workspacesNextCursor,
    workspacesLoadingMore,
    unreadInboxCount,
    inboxRevision,
    error,
    setApplicationError: setError,
    replaceCurrentUser,
    loadMoreWorkspaces,
    createWorkspace: handleCreateWorkspace,
    restoreWorkspace: handleRestoreWorkspace,
    setWorkspacePin: handleSetWorkspacePin,
    replaceWorkspace,
    removeWorkspace,
    refreshCurrentIdentity,
    refreshInbox,
    logout: handleLogout,
  };

  return (
    <DocumentTitleProvider
      defaultTitle={defaultPageTitle(location.pathname, workspaces)}
      unreadCount={unreadInboxCount}
    >
      <Outlet context={context} />
    </DocumentTitleProvider>
  );
}
