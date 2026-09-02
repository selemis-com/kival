import { KivalApiError, KivalTransportError } from "kival-sdk";
import { useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { kival } from "../../../shared/api";
import { KIVAL_REALTIME_EVENT } from "../../../shared/realtime";
import type {
  CreateObjectRequest,
  ObjectContext,
  ObjectResponse,
  ObjectSummary,
  RealtimeMessage,
  RecentObject,
  UpdateObjectRequest,
  UpdateWorkspaceRequest,
  User,
  Workspace,
} from "../../../shared/types";

type Options = {
  user: User;
  workspaceId: string;
  objectId: string | null;
  workspaces: Workspace[];
  replaceWorkspace: (workspace: Workspace) => void;
  removeWorkspace: (workspaceId: string) => void;
  setApplicationError: (error: string | null) => void;
};

async function listAllPinnedObjects(
  workspaceId: string,
  favorited: boolean | undefined,
  signal?: AbortSignal,
): Promise<ObjectSummary[]> {
  const pinned: ObjectSummary[] = [];
  let cursor: string | null = null;

  do {
    const response = await kival.listObjects({
      workspaceId,
      favorited,
      pinned: true,
      cursor,
      signal,
    });
    pinned.push(...response.items);
    cursor = response.next_cursor ?? null;
  } while (cursor);

  return pinned;
}

export function useWorkspaceController({
  user,
  workspaceId: routeWorkspaceId,
  objectId: routeObjectId,
  workspaces,
  replaceWorkspace,
  removeWorkspace,
  setApplicationError,
}: Options) {
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [objects, setObjects] = useState<ObjectSummary[]>([]);
  const [pinnedObjects, setPinnedObjects] = useState<ObjectSummary[]>([]);
  const [recentObjects, setRecentObjects] = useState<RecentObject[]>([]);
  const [favoriteObjects, setFavoriteObjects] = useState<ObjectSummary[]>([]);
  const [pinnedFavoriteObjects, setPinnedFavoriteObjects] = useState<ObjectSummary[]>([]);
  const [archivedObjects, setArchivedObjects] = useState<ObjectSummary[]>([]);
  const [objectsNextCursor, setObjectsNextCursor] = useState<string | null>(null);
  const [archivedObjectsNextCursor, setArchivedObjectsNextCursor] = useState<string | null>(null);
  const [objectsLoadingMore, setObjectsLoadingMore] = useState(false);
  const [archivedObjectsLoadingMore, setArchivedObjectsLoadingMore] = useState(false);
  const [recentLoading, setRecentLoading] = useState(false);
  const [recentLoadingMore, setRecentLoadingMore] = useState(false);
  const [recentNextCursor, setRecentNextCursor] = useState<string | null>(null);
  const [favoritesNextCursor, setFavoritesNextCursor] = useState<string | null>(null);
  const [favoritesLoadingMore, setFavoritesLoadingMore] = useState(false);
  const [selectedObject, setSelectedObject] = useState<ObjectResponse | null>(null);
  const [objectContext, setObjectContext] = useState<ObjectContext | null>(null);
  const [workspaceLoading, setWorkspaceLoading] = useState(false);
  const [objectLoading, setObjectLoading] = useState(false);
  const workspaceLoadControllerRef = useRef<AbortController | null>(null);
  const workspaceAccessRefreshControllerRef = useRef<AbortController | null>(null);
  const directoryRefreshControllerRef = useRef<AbortController | null>(null);
  const objectLoadControllerRef = useRef<AbortController | null>(null);
  const objectRefreshControllerRef = useRef<AbortController | null>(null);
  const contextLoadControllerRef = useRef<AbortController | null>(null);
  const workspaceGenerationRef = useRef(0);
  const objectRequestIdRef = useRef(0);
  const objectRefreshRequestIdRef = useRef(0);
  const contextRequestIdRef = useRef(0);
  const recentLoadRef = useRef<{ workspaceId: string; loading: boolean } | null>(null);
  const routeWorkspaceIdRef = useRef(routeWorkspaceId);
  const routeObjectIdRef = useRef(routeObjectId);
  const selectedObjectIdRef = useRef(selectedObject?.object.id ?? null);
  const location = useLocation();
  const navigate = useNavigate();

  routeWorkspaceIdRef.current = routeWorkspaceId;
  routeObjectIdRef.current = routeObjectId;
  selectedObjectIdRef.current = selectedObject?.object.id ?? null;

  useEffect(() => {
    return () => {
      workspaceLoadControllerRef.current?.abort();
      workspaceAccessRefreshControllerRef.current?.abort();
      directoryRefreshControllerRef.current?.abort();
      objectLoadControllerRef.current?.abort();
      objectRefreshControllerRef.current?.abort();
      contextLoadControllerRef.current?.abort();
    };
  }, []);

  // The route is the source of truth; this effect resolves its workspace and starts its directory load.
  // biome-ignore lint/correctness/useExhaustiveDependencies: openWorkspace is a local command keyed by route identity.
  useEffect(() => {
    if (workspace?.id === routeWorkspaceId) {
      return;
    }

    const listedWorkspace = workspaces.find((candidate) => candidate.id === routeWorkspaceId);

    if (listedWorkspace) {
      void openWorkspace(listedWorkspace);
      return;
    }

    const controller = new AbortController();

    async function resolveWorkspace() {
      try {
        const resolvedWorkspace = await kival.getWorkspace({
          workspaceId: routeWorkspaceId,
          signal: controller.signal,
        });
        replaceWorkspace(resolvedWorkspace);
        await openWorkspace(resolvedWorkspace);
      } catch (cause) {
        if (cause instanceof KivalTransportError && cause.kind === "abort") {
          return;
        }

        cancelWorkspaceRequests();
        setWorkspace(null);
        setSelectedObject(null);
        setObjectContext(null);
        setApplicationError("Workspace not found or you no longer have access to it.");
        navigate("/", { replace: true });
      }
    }

    void resolveWorkspace();
    return () => controller.abort();
  }, [
    navigate,
    replaceWorkspace,
    routeWorkspaceId,
    setApplicationError,
    workspace?.id,
    workspaces,
  ]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: openObject is a local command keyed by route identity.
  useEffect(() => {
    if (!workspace || workspace.id !== routeWorkspaceId || !routeObjectId) {
      objectLoadControllerRef.current?.abort();
      objectRefreshControllerRef.current?.abort();
      contextLoadControllerRef.current?.abort();
      objectRequestIdRef.current += 1;
      objectRefreshRequestIdRef.current += 1;
      contextRequestIdRef.current += 1;
      setObjectLoading(false);

      if (selectedObject) {
        setSelectedObject(null);
        setObjectContext(null);
      }

      return;
    }

    if (selectedObject?.object.id === routeObjectId) {
      return;
    }

    void openObject(routeObjectId);
  }, [routeObjectId, routeWorkspaceId, selectedObject, workspace]);

  // Refresh directory summaries whenever the user returns to a directory-backed workspace view.
  // biome-ignore lint/correctness/useExhaustiveDependencies: refreshObjectDirectory is route-keyed.
  useEffect(() => {
    if (workspaceLoading || !workspace || workspace.id !== routeWorkspaceId) {
      return;
    }

    const workspaceBasePath = `/w/${workspace.id}`;
    if (
      location.pathname === workspaceBasePath ||
      location.pathname === `${workspaceBasePath}/graph`
    ) {
      void refreshObjectDirectory("active");
    } else if (location.pathname === `${workspaceBasePath}/archived`) {
      void refreshObjectDirectory("archived");
    } else if (location.pathname === `${workspaceBasePath}/favorites`) {
      void refreshFavoriteObjects();
    }
  }, [location.pathname, routeWorkspaceId, workspace?.id]);

  // Keep directory summaries current across browser-tab focus changes and realtime activity.
  // biome-ignore lint/correctness/useExhaustiveDependencies: refreshVisibleDirectory reads current route state.
  useEffect(() => {
    if (!workspace || workspace.id !== routeWorkspaceId) {
      return;
    }

    const refreshSelectedRouteObject = () => {
      const selectedId = routeObjectIdRef.current;
      if (!selectedId || location.pathname !== `/w/${workspace.id}/objects/${selectedId}`) {
        return;
      }

      void refreshSelectedObjectAccess(selectedId).catch((cause: unknown) => {
        setApplicationError(cause instanceof Error ? cause.message : String(cause));
      });
      void refreshObjectContext(selectedId).catch((cause: unknown) => {
        setApplicationError(cause instanceof Error ? cause.message : String(cause));
      });
    };

    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") {
        void refreshVisibleDirectory();
        refreshSelectedRouteObject();
      }
    };
    const handleRealtime = (event: Event) => {
      const message = (event as CustomEvent<RealtimeMessage>).detail;
      if (message.type === "realtime.resync_required" || message.workspace_id === workspace.id) {
        void refreshVisibleDirectory();
      }
      if (message.type === "realtime.resync_required") {
        void refreshWorkspaceAccess().catch((cause: unknown) => {
          setApplicationError(cause instanceof Error ? cause.message : String(cause));
        });
      }

      const selectedId = routeObjectIdRef.current;
      if (
        selectedId &&
        (message.type === "realtime.resync_required" || message.object_id === selectedId)
      ) {
        refreshSelectedRouteObject();
      }
    };

    window.addEventListener("focus", refreshWhenVisible);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    window.addEventListener(KIVAL_REALTIME_EVENT, handleRealtime);
    return () => {
      window.removeEventListener("focus", refreshWhenVisible);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
      window.removeEventListener(KIVAL_REALTIME_EVENT, handleRealtime);
    };
  }, [location.pathname, routeWorkspaceId, workspace?.id]);

  function mutationStillTargetsWorkspace(workspaceId: string, generation: number) {
    return (
      routeWorkspaceIdRef.current === workspaceId && workspaceGenerationRef.current === generation
    );
  }

  async function runWorkspaceMutation<T>(
    request: (workspaceId: string) => Promise<T>,
    onSuccess?: (result: T) => void,
  ): Promise<{ workspaceId: string; result: T } | null> {
    if (!workspace) {
      return null;
    }

    const workspaceId = workspace.id;
    const generation = workspaceGenerationRef.current;

    try {
      const result = await request(workspaceId);
      onSuccess?.(result);

      if (!mutationStillTargetsWorkspace(workspaceId, generation)) {
        return null;
      }

      return { workspaceId, result };
    } catch (cause) {
      if (!mutationStillTargetsWorkspace(workspaceId, generation)) {
        return null;
      }

      throw cause;
    }
  }

  function cancelWorkspaceRequests() {
    workspaceLoadControllerRef.current?.abort();
    workspaceAccessRefreshControllerRef.current?.abort();
    directoryRefreshControllerRef.current?.abort();
    objectLoadControllerRef.current?.abort();
    objectRefreshControllerRef.current?.abort();
    contextLoadControllerRef.current?.abort();
    workspaceGenerationRef.current += 1;
    objectRequestIdRef.current += 1;
    objectRefreshRequestIdRef.current += 1;
    contextRequestIdRef.current += 1;
    recentLoadRef.current = null;
  }

  async function refreshObjectDirectory(status: "active" | "archived") {
    if (!workspace || workspace.id !== routeWorkspaceIdRef.current) {
      return;
    }

    directoryRefreshControllerRef.current?.abort();
    const controller = new AbortController();
    directoryRefreshControllerRef.current = controller;
    const currentWorkspaceId = workspace.id;
    const generation = workspaceGenerationRef.current;

    try {
      const [response, pinned] = await Promise.all([
        kival.listObjects({
          workspaceId: currentWorkspaceId,
          status,
          signal: controller.signal,
        }),
        status === "active"
          ? listAllPinnedObjects(currentWorkspaceId, undefined, controller.signal)
          : Promise.resolve(null),
      ]);

      if (
        controller.signal.aborted ||
        workspaceGenerationRef.current !== generation ||
        routeWorkspaceIdRef.current !== currentWorkspaceId
      ) {
        return;
      }

      if (status === "active") {
        setObjects(response.items);
        setPinnedObjects(pinned ?? []);
        setObjectsNextCursor(response.next_cursor ?? null);
      } else {
        setArchivedObjects(response.items);
        setArchivedObjectsNextCursor(response.next_cursor ?? null);
      }
    } catch (cause) {
      if (!(cause instanceof KivalTransportError && cause.kind === "abort")) {
        setApplicationError(cause instanceof Error ? cause.message : String(cause));
      }
    } finally {
      if (directoryRefreshControllerRef.current === controller) {
        directoryRefreshControllerRef.current = null;
      }
    }
  }

  async function refreshVisibleDirectory() {
    if (!workspace || workspace.id !== routeWorkspaceIdRef.current) {
      return;
    }

    const workspaceBasePath = `/w/${workspace.id}`;
    if (location.pathname === `${workspaceBasePath}/favorites`) {
      await refreshFavoriteObjects();
    } else if (location.pathname === `${workspaceBasePath}/recent`) {
      await loadRecentObjects();
    } else if (location.pathname === `${workspaceBasePath}/archived`) {
      await refreshObjectDirectory("archived");
    } else if (
      location.pathname === workspaceBasePath ||
      location.pathname === `${workspaceBasePath}/graph`
    ) {
      await refreshObjectDirectory("active");
    }
  }

  async function refreshFavoriteObjects() {
    if (!workspace || workspace.id !== routeWorkspaceIdRef.current) return;
    const generation = workspaceGenerationRef.current;
    const [response, pinned] = await Promise.all([
      kival.listObjects({ workspaceId: workspace.id, favorited: true, pinned: false }),
      listAllPinnedObjects(workspace.id, true),
    ]);
    if (workspaceGenerationRef.current === generation) {
      setFavoriteObjects(response.items);
      setPinnedFavoriteObjects(pinned);
      setFavoritesNextCursor(response.next_cursor ?? null);
    }
  }

  async function openWorkspace(nextWorkspace: Workspace) {
    cancelWorkspaceRequests();

    const controller = new AbortController();
    workspaceLoadControllerRef.current = controller;
    const generation = workspaceGenerationRef.current;

    setWorkspace(nextWorkspace);
    setSelectedObject(null);
    setObjectContext(null);
    setObjects([]);
    setPinnedObjects([]);
    setRecentObjects([]);
    setFavoriteObjects([]);
    setPinnedFavoriteObjects([]);
    setRecentNextCursor(null);
    setFavoritesNextCursor(null);
    setRecentLoadingMore(false);
    setArchivedObjects([]);
    setObjectsNextCursor(null);
    setArchivedObjectsNextCursor(null);
    setWorkspaceLoading(true);
    setObjectLoading(false);
    setApplicationError(null);

    try {
      const [
        activeResponse,
        archivedResponse,
        favoritesResponse,
        pinnedResponse,
        pinnedFavoritesResponse,
      ] = await Promise.all([
        kival.listObjects({ workspaceId: nextWorkspace.id, signal: controller.signal }),
        kival.listObjects({
          workspaceId: nextWorkspace.id,
          status: "archived",
          signal: controller.signal,
        }),
        kival.listObjects({
          workspaceId: nextWorkspace.id,
          favorited: true,
          pinned: false,
          signal: controller.signal,
        }),
        listAllPinnedObjects(nextWorkspace.id, undefined, controller.signal),
        listAllPinnedObjects(nextWorkspace.id, true, controller.signal),
      ]);

      if (controller.signal.aborted || workspaceGenerationRef.current !== generation) {
        return;
      }

      setObjects(activeResponse.items);
      setPinnedObjects(pinnedResponse);
      setObjectsNextCursor(activeResponse.next_cursor ?? null);
      setArchivedObjects(archivedResponse.items);
      setArchivedObjectsNextCursor(archivedResponse.next_cursor ?? null);
      setFavoriteObjects(favoritesResponse.items);
      setPinnedFavoriteObjects(pinnedFavoritesResponse);
      setFavoritesNextCursor(favoritesResponse.next_cursor ?? null);
    } catch (cause) {
      if (
        (cause instanceof KivalTransportError && cause.kind === "abort") ||
        workspaceGenerationRef.current !== generation
      ) {
        return;
      }

      setApplicationError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      if (workspaceGenerationRef.current === generation) {
        setWorkspaceLoading(false);
      }
    }
  }

  async function loadMoreObjects(status: "active" | "archived") {
    const loadingMore = status === "active" ? objectsLoadingMore : archivedObjectsLoadingMore;

    if (!workspace || workspace.id !== routeWorkspaceIdRef.current || loadingMore) {
      return;
    }

    const generation = workspaceGenerationRef.current;
    const cursor = status === "active" ? objectsNextCursor : archivedObjectsNextCursor;

    if (!cursor) {
      return;
    }

    const setLoadingMore =
      status === "active" ? setObjectsLoadingMore : setArchivedObjectsLoadingMore;
    setLoadingMore(true);
    setApplicationError(null);

    try {
      const response = await kival.listObjects({ workspaceId: workspace.id, status, cursor });

      if (workspaceGenerationRef.current !== generation) {
        return;
      }

      if (status === "active") {
        setObjects((current) => [...current, ...response.items]);
        setObjectsNextCursor(response.next_cursor ?? null);
        setRecentObjects([]);
        setRecentNextCursor(null);
      } else {
        setArchivedObjects((current) => [...current, ...response.items]);
        setArchivedObjectsNextCursor(response.next_cursor ?? null);
      }
    } catch (cause) {
      if (workspaceGenerationRef.current === generation) {
        setApplicationError(cause instanceof Error ? cause.message : String(cause));
      }
    } finally {
      if (workspaceGenerationRef.current === generation) {
        setLoadingMore(false);
      }
    }
  }

  async function loadMoreFavoriteObjects() {
    if (!workspace || !favoritesNextCursor || favoritesLoadingMore) {
      return;
    }

    const generation = workspaceGenerationRef.current;
    const cursor = favoritesNextCursor;
    setFavoritesLoadingMore(true);
    try {
      const response = await kival.listObjects({
        workspaceId: workspace.id,
        favorited: true,
        pinned: false,
        cursor,
      });
      if (workspaceGenerationRef.current === generation) {
        setFavoriteObjects((current) => [...current, ...response.items]);
        setFavoritesNextCursor(response.next_cursor ?? null);
      }
    } finally {
      if (workspaceGenerationRef.current === generation) {
        setFavoritesLoadingMore(false);
      }
    }
  }

  async function loadRecentObjects() {
    if (!workspace) {
      return;
    }

    const currentWorkspaceId = workspace.id;
    const generation = workspaceGenerationRef.current;
    const currentLoad = recentLoadRef.current;

    if (currentLoad?.workspaceId === currentWorkspaceId && currentLoad.loading) {
      return;
    }

    recentLoadRef.current = { workspaceId: currentWorkspaceId, loading: true };
    setRecentLoading(true);
    setApplicationError(null);

    try {
      const response = await kival.listObjects({
        workspaceId: currentWorkspaceId,
        status: "active",
        order: "updated",
      });

      if (workspaceGenerationRef.current !== generation) {
        return;
      }

      setRecentObjects(
        response.items.map((object) => ({
          id: object.id,
          title: object.title,
          status: object.status,
          updated_at: object.updated_at,
        })),
      );
      setRecentNextCursor(response.next_cursor ?? null);
    } catch (cause) {
      if (workspaceGenerationRef.current === generation) {
        setApplicationError(cause instanceof Error ? cause.message : String(cause));
      }
    } finally {
      if (workspaceGenerationRef.current === generation) {
        recentLoadRef.current = { workspaceId: currentWorkspaceId, loading: false };
        setRecentLoading(false);
      }
    }
  }

  async function loadMoreRecentObjects() {
    if (
      !workspace ||
      workspace.id !== routeWorkspaceIdRef.current ||
      !recentNextCursor ||
      recentLoadingMore
    ) {
      return;
    }

    const currentWorkspaceId = workspace.id;
    const generation = workspaceGenerationRef.current;
    const cursor = recentNextCursor;
    setRecentLoadingMore(true);
    setApplicationError(null);

    try {
      const response = await kival.listObjects({
        workspaceId: currentWorkspaceId,
        status: "active",
        order: "updated",
        cursor,
      });

      if (workspaceGenerationRef.current !== generation) {
        return;
      }

      setRecentObjects((current) => {
        const existing = new Set(current.map((object) => object.id));
        return [
          ...current,
          ...response.items
            .filter((object) => !existing.has(object.id))
            .map((object) => ({
              id: object.id,
              title: object.title,
              status: object.status,
              updated_at: object.updated_at,
            })),
        ];
      });
      setRecentNextCursor(response.next_cursor ?? null);
    } catch (cause) {
      if (workspaceGenerationRef.current === generation) {
        setApplicationError(cause instanceof Error ? cause.message : String(cause));
      }
    } finally {
      if (workspaceGenerationRef.current === generation) {
        setRecentLoadingMore(false);
      }
    }
  }

  async function handleUpdateWorkspace(input: UpdateWorkspaceRequest): Promise<boolean> {
    const mutation = await runWorkspaceMutation(
      (workspaceId) => kival.updateWorkspace({ workspaceId, input }),
      replaceWorkspace,
    );

    if (!mutation) {
      return false;
    }

    setWorkspace(mutation.result);
    setApplicationError(null);
    return true;
  }

  async function handleArchiveWorkspace(): Promise<boolean> {
    const mutation = await runWorkspaceMutation(
      (workspaceId) => kival.archiveWorkspace({ workspaceId }),
      (archivedWorkspace) => removeWorkspace(archivedWorkspace.id),
    );

    if (!mutation) {
      return false;
    }

    cancelWorkspaceRequests();
    setWorkspace(null);
    setApplicationError(null);
    navigate("/", { replace: true });
    return true;
  }

  async function handleCreateObject(input: CreateObjectRequest): Promise<boolean> {
    const mutation = await runWorkspaceMutation((workspaceId) =>
      kival.createObject({ workspaceId, input }),
    );

    if (!mutation) {
      return false;
    }

    const response = mutation.result;
    objectRefreshControllerRef.current?.abort();
    objectRefreshRequestIdRef.current += 1;
    setObjects((current) => [
      {
        ...response.object,
        updated_by_username: user.username,
        updated_by_display_name: user.display_name,
        updated_by_workspace_role: workspace?.effective_role,
        updated_by_object_role: response.effective_role,
        connection_count: 0,
        unresolved_thread_count: 0,
        favorited: false,
        pinned: false,
        pinned_at: null,
      },
      ...current,
    ]);
    setRecentObjects([]);
    setRecentNextCursor(null);
    setSelectedObject(response);
    setObjectContext({
      backlinks: {
        object_id: response.object.id,
        incoming_edges: [],
        incoming_references: [],
      },
      edges: { items: [] },
      graph: {
        workspace_id: response.object.workspace_id,
        root_object_id: response.object.id,
        depth: 1,
        direction: "both",
        max_nodes: 100,
        max_edges: 250,
        truncated: false,
        truncation: { nodes: false, edges: false },
        nodes: [
          {
            id: response.object.id,
            workspace_id: response.object.workspace_id,
            current_version_id: response.object.current_version_id,
            title: response.current_version?.title ?? response.object.title,
            status: response.object.status,
            created_by: response.object.created_by,
            created_at: response.object.created_at,
            updated_at: response.object.updated_at,
            distance: 0,
            incoming_count: 0,
            outgoing_count: 0,
          },
        ],
        edges: [],
      },
    });
    setApplicationError(null);
    navigate(`/w/${mutation.workspaceId}/objects/${response.object.id}`);
    return true;
  }

  async function handleUpdateObject(
    objectId: string,
    input: UpdateObjectRequest,
  ): Promise<boolean> {
    const mutation = await runWorkspaceMutation((workspaceId) =>
      kival.updateObject({ workspaceId, objectId, input }),
    );

    if (!mutation) {
      return false;
    }

    const response = mutation.result;
    objectRefreshControllerRef.current?.abort();
    objectRefreshRequestIdRef.current += 1;
    setObjects((current) =>
      current.map((object) =>
        object.id === objectId
          ? {
              ...object,
              ...response.object,
              updated_by_username: user.username,
              updated_by_display_name: user.display_name,
              updated_by_workspace_role: workspace?.effective_role,
              updated_by_object_role: response.effective_role,
            }
          : object,
      ),
    );
    setPinnedObjects((current) =>
      current.map((object) =>
        object.id === objectId ? { ...object, ...response.object } : object,
      ),
    );
    setFavoriteObjects((current) =>
      current.map((object) =>
        object.id === objectId ? { ...object, ...response.object } : object,
      ),
    );
    setPinnedFavoriteObjects((current) =>
      current.map((object) =>
        object.id === objectId ? { ...object, ...response.object } : object,
      ),
    );
    setRecentObjects([]);
    setRecentNextCursor(null);
    setSelectedObject(response);
    setApplicationError(null);
    navigate(`/w/${mutation.workspaceId}/objects/${response.object.id}`);
    return true;
  }

  async function handleArchiveObject(objectId: string): Promise<boolean> {
    const source =
      objects.find((object) => object.id === objectId) ??
      pinnedObjects.find((object) => object.id === objectId) ??
      favoriteObjects.find((object) => object.id === objectId) ??
      pinnedFavoriteObjects.find((object) => object.id === objectId);
    const mutation = await runWorkspaceMutation((workspaceId) =>
      kival.archiveObject({ workspaceId, objectId }),
    );

    if (!mutation) {
      return false;
    }

    const response = mutation.result;
    objectRefreshControllerRef.current?.abort();
    objectRefreshRequestIdRef.current += 1;
    const archivedSummary: ObjectSummary = source
      ? { ...source, ...response.object }
      : response.object;
    setObjects((current) => current.filter((object) => object.id !== objectId));
    setPinnedObjects((current) => current.filter((object) => object.id !== objectId));
    setFavoriteObjects((current) => current.filter((object) => object.id !== objectId));
    setPinnedFavoriteObjects((current) => current.filter((object) => object.id !== objectId));
    setRecentObjects([]);
    setRecentNextCursor(null);
    setArchivedObjects((current) => [
      archivedSummary,
      ...current.filter((object) => object.id !== objectId),
    ]);
    setSelectedObject(null);
    setObjectContext(null);
    setApplicationError(null);
    navigate(`/w/${mutation.workspaceId}`);
    return true;
  }

  async function handleUnarchiveObject(objectId: string): Promise<boolean> {
    const source = archivedObjects.find((object) => object.id === objectId);
    const mutation = await runWorkspaceMutation((workspaceId) =>
      kival.unarchiveObject({ workspaceId, objectId }),
    );

    if (!mutation) {
      return false;
    }

    const response = mutation.result;
    objectRefreshControllerRef.current?.abort();
    objectRefreshRequestIdRef.current += 1;
    const restoredSummary: ObjectSummary = source
      ? { ...source, ...response.object }
      : response.object;
    setArchivedObjects((current) => current.filter((object) => object.id !== objectId));
    setRecentObjects([]);
    setRecentNextCursor(null);
    setObjects((current) => [
      restoredSummary,
      ...current.filter((object) => object.id !== objectId),
    ]);
    if (restoredSummary.pinned) {
      setPinnedObjects((current) => [
        restoredSummary,
        ...current.filter((object) => object.id !== objectId),
      ]);
    }
    if (restoredSummary.favorited && !restoredSummary.pinned) {
      setFavoriteObjects((current) => [
        restoredSummary,
        ...current.filter((object) => object.id !== objectId),
      ]);
    }
    if (restoredSummary.pinned && restoredSummary.favorited) {
      setPinnedFavoriteObjects((current) => [
        restoredSummary,
        ...current.filter((object) => object.id !== objectId),
      ]);
    }
    setSelectedObject(null);
    setObjectContext(null);
    setApplicationError(null);

    if (location.pathname !== `/w/${mutation.workspaceId}/archived`) {
      navigate(`/w/${mutation.workspaceId}`);
    }

    return true;
  }

  async function refreshObjectContext(objectId: string) {
    if (!workspace) {
      return;
    }

    contextLoadControllerRef.current?.abort();
    const controller = new AbortController();
    contextLoadControllerRef.current = controller;
    const requestId = ++contextRequestIdRef.current;
    const currentWorkspaceId = workspace.id;
    const generation = workspaceGenerationRef.current;

    try {
      const [backlinks, edges, graph] = await Promise.all([
        kival.getObjectBacklinks({
          workspaceId: currentWorkspaceId,
          objectId,
          signal: controller.signal,
        }),
        kival.listObjectEdges({
          workspaceId: currentWorkspaceId,
          objectId,
          signal: controller.signal,
        }),
        kival.getObjectGraph({
          workspaceId: currentWorkspaceId,
          objectId,
          depth: 1,
          direction: "both",
          signal: controller.signal,
        }),
      ]);

      if (
        controller.signal.aborted ||
        workspaceGenerationRef.current !== generation ||
        contextRequestIdRef.current !== requestId ||
        routeObjectIdRef.current !== objectId ||
        selectedObjectIdRef.current !== objectId ||
        backlinks.object_id !== objectId
      ) {
        return;
      }

      setObjectContext({ backlinks, edges, graph });
    } catch (cause) {
      if (cause instanceof KivalTransportError && cause.kind === "abort") {
        return;
      }

      throw cause;
    }
  }

  async function setObjectFavorite(objectId: string, favorited: boolean) {
    if (!workspace) {
      return;
    }

    const state = await kival.setObjectFavorite({
      workspaceId: workspace.id,
      objectId,
      favorited,
    });
    const update = (object: ObjectSummary) =>
      object.id === objectId ? { ...object, favorited: state.favorited } : object;
    const source =
      objects.find((object) => object.id === objectId) ??
      pinnedObjects.find((object) => object.id === objectId) ??
      favoriteObjects.find((object) => object.id === objectId) ??
      pinnedFavoriteObjects.find((object) => object.id === objectId);

    setObjects((current) => current.map(update));
    setRecentObjects((current) => current.map(update));
    setArchivedObjects((current) => current.map(update));
    setPinnedObjects((current) => current.map(update));

    if (!state.favorited) {
      setFavoriteObjects((current) => current.filter((object) => object.id !== objectId));
      setPinnedFavoriteObjects((current) => current.filter((object) => object.id !== objectId));
      return;
    }

    if (source?.pinned) {
      setFavoriteObjects((current) => current.filter((object) => object.id !== objectId));
      setPinnedFavoriteObjects((current) => [
        { ...source, favorited: true },
        ...current.filter((object) => object.id !== objectId),
      ]);
    } else if (source) {
      setPinnedFavoriteObjects((current) => current.filter((object) => object.id !== objectId));
      setFavoriteObjects((current) => [
        { ...source, favorited: true },
        ...current.filter((object) => object.id !== objectId),
      ]);
    }
  }

  async function setObjectPin(objectId: string, pinned: boolean) {
    if (!workspace) return;

    const state = await kival.setObjectPin({ workspaceId: workspace.id, objectId, pinned });
    const update = (object: ObjectSummary) =>
      object.id === objectId
        ? { ...object, pinned: state.pinned, pinned_at: state.pinned_at }
        : object;
    const source =
      objects.find((object) => object.id === objectId) ??
      pinnedObjects.find((object) => object.id === objectId) ??
      favoriteObjects.find((object) => object.id === objectId) ??
      pinnedFavoriteObjects.find((object) => object.id === objectId);

    setObjects((current) => current.map(update));
    setRecentObjects((current) => current.map(update));

    if (!state.pinned) {
      setPinnedObjects((current) => current.filter((object) => object.id !== objectId));
      setPinnedFavoriteObjects((current) => current.filter((object) => object.id !== objectId));
      if (source?.favorited) {
        setFavoriteObjects((current) => [
          { ...source, pinned: false, pinned_at: null },
          ...current.filter((object) => object.id !== objectId),
        ]);
      }
      return;
    }

    if (source) {
      const updated = { ...source, pinned: true, pinned_at: state.pinned_at };
      setPinnedObjects((current) => [
        updated,
        ...current.filter((object) => object.id !== objectId),
      ]);

      if (source.favorited) {
        setFavoriteObjects((current) => current.filter((object) => object.id !== objectId));
        setPinnedFavoriteObjects((current) => [
          updated,
          ...current.filter((object) => object.id !== objectId),
        ]);
      }
    }
  }

  async function openObject(objectId: string) {
    if (!workspace) {
      return;
    }

    objectLoadControllerRef.current?.abort();
    objectRefreshControllerRef.current?.abort();
    contextLoadControllerRef.current?.abort();

    const controller = new AbortController();
    objectLoadControllerRef.current = controller;
    const requestId = ++objectRequestIdRef.current;
    objectRefreshRequestIdRef.current += 1;
    contextRequestIdRef.current += 1;
    const currentWorkspaceId = workspace.id;
    const generation = workspaceGenerationRef.current;

    setObjectLoading(true);
    setSelectedObject(null);
    setObjectContext(null);
    setApplicationError(null);

    try {
      const object = await kival.getObject({
        workspaceId: currentWorkspaceId,
        objectId,
        signal: controller.signal,
      });

      if (
        controller.signal.aborted ||
        workspaceGenerationRef.current !== generation ||
        objectRequestIdRef.current !== requestId
      ) {
        return;
      }

      if (object.object.status === "archived") {
        setSelectedObject(object);
        return;
      }

      try {
        const [backlinks, edges, graph] = await Promise.all([
          kival.getObjectBacklinks({
            workspaceId: currentWorkspaceId,
            objectId,
            signal: controller.signal,
          }),
          kival.listObjectEdges({
            workspaceId: currentWorkspaceId,
            objectId,
            signal: controller.signal,
          }),
          kival.getObjectGraph({
            workspaceId: currentWorkspaceId,
            objectId,
            depth: 1,
            direction: "both",
            signal: controller.signal,
          }),
        ]);

        if (
          controller.signal.aborted ||
          workspaceGenerationRef.current !== generation ||
          objectRequestIdRef.current !== requestId ||
          backlinks.object_id !== objectId
        ) {
          return;
        }

        setSelectedObject(object);
        setObjectContext({ backlinks, edges, graph });
      } catch (cause) {
        if (cause instanceof KivalTransportError && cause.kind === "abort") {
          return;
        }

        if (
          workspaceGenerationRef.current !== generation ||
          objectRequestIdRef.current !== requestId
        ) {
          return;
        }

        setSelectedObject(object);
        setObjectContext(null);
        setApplicationError(cause instanceof Error ? cause.message : String(cause));
      }
    } catch (cause) {
      if (
        (cause instanceof KivalTransportError && cause.kind === "abort") ||
        workspaceGenerationRef.current !== generation ||
        objectRequestIdRef.current !== requestId
      ) {
        return;
      }

      setApplicationError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      if (
        workspaceGenerationRef.current === generation &&
        objectRequestIdRef.current === requestId
      ) {
        setObjectLoading(false);
      }
    }
  }

  async function refreshSelectedObjectAccess(objectId: string) {
    if (!workspace) {
      return;
    }

    objectRefreshControllerRef.current?.abort();
    const controller = new AbortController();
    objectRefreshControllerRef.current = controller;
    const requestId = ++objectRefreshRequestIdRef.current;
    const currentWorkspaceId = workspace.id;
    const generation = workspaceGenerationRef.current;

    try {
      const object = await kival.getObject({
        workspaceId: currentWorkspaceId,
        objectId,
        signal: controller.signal,
      });

      if (
        controller.signal.aborted ||
        workspaceGenerationRef.current !== generation ||
        objectRefreshRequestIdRef.current !== requestId ||
        routeObjectIdRef.current !== objectId ||
        selectedObjectIdRef.current !== objectId
      ) {
        return;
      }

      setSelectedObject(object);
    } catch (cause) {
      if (
        (cause instanceof KivalTransportError && cause.kind === "abort") ||
        controller.signal.aborted ||
        workspaceGenerationRef.current !== generation ||
        objectRefreshRequestIdRef.current !== requestId ||
        routeObjectIdRef.current !== objectId ||
        selectedObjectIdRef.current !== objectId
      ) {
        return;
      }

      const message = cause instanceof Error ? cause.message : "Could not refresh object access.";

      if (
        cause instanceof KivalApiError &&
        (cause.kind === "forbidden" || cause.kind === "notFound")
      ) {
        setSelectedObject(null);
        setObjectContext(null);
        setApplicationError(message);
        navigate(`/w/${currentWorkspaceId}`);
        return;
      }

      throw cause instanceof Error ? cause : new Error(message);
    } finally {
      if (objectRefreshControllerRef.current === controller) {
        objectRefreshControllerRef.current = null;
      }
    }
  }

  async function refreshWorkspaceAccess() {
    if (!workspace || workspace.id !== routeWorkspaceIdRef.current) {
      return;
    }

    workspaceAccessRefreshControllerRef.current?.abort();
    const controller = new AbortController();
    workspaceAccessRefreshControllerRef.current = controller;
    const currentWorkspaceId = workspace.id;
    const generation = workspaceGenerationRef.current;

    try {
      const refreshed = await kival.getWorkspace({
        workspaceId: currentWorkspaceId,
        signal: controller.signal,
      });
      if (
        controller.signal.aborted ||
        workspaceGenerationRef.current !== generation ||
        routeWorkspaceIdRef.current !== currentWorkspaceId
      ) {
        return;
      }

      if (refreshed.status !== "active") {
        cancelWorkspaceRequests();
        setWorkspace(null);
        setSelectedObject(null);
        setObjectContext(null);
        removeWorkspace(currentWorkspaceId);
        setApplicationError("Workspace is archived.");
        navigate("/", { replace: true });
        return;
      }

      setWorkspace(refreshed);
      replaceWorkspace(refreshed);
    } catch (cause) {
      if (
        (cause instanceof KivalTransportError && cause.kind === "abort") ||
        controller.signal.aborted ||
        workspaceGenerationRef.current !== generation ||
        routeWorkspaceIdRef.current !== currentWorkspaceId
      ) {
        return;
      }

      if (
        cause instanceof KivalApiError &&
        (cause.kind === "forbidden" || cause.kind === "notFound")
      ) {
        cancelWorkspaceRequests();
        setWorkspace(null);
        setSelectedObject(null);
        setObjectContext(null);
        removeWorkspace(currentWorkspaceId);
        setApplicationError("Workspace not found or you no longer have access to it.");
        navigate("/", { replace: true });
        return;
      }

      throw cause;
    } finally {
      if (workspaceAccessRefreshControllerRef.current === controller) {
        workspaceAccessRefreshControllerRef.current = null;
      }
    }
  }

  return {
    workspace,
    objects,
    pinnedObjects,
    recentObjects,
    favoriteObjects,
    pinnedFavoriteObjects,
    archivedObjects,
    selectedObject,
    objectContext,
    workspaceLoading,
    objectLoading,
    objectsNextCursor,
    archivedObjectsNextCursor,
    objectsLoadingMore,
    archivedObjectsLoadingMore,
    recentLoading,
    recentLoadingMore,
    recentIncomplete: recentNextCursor != null,
    favoritesIncomplete: favoritesNextCursor != null,
    favoritesLoadingMore,
    loadMoreObjects,
    loadRecentObjects,
    loadMoreRecentObjects,
    loadMoreFavoriteObjects,
    refreshObjectContext,
    refreshSelectedObjectAccess,
    refreshWorkspaceAccess,
    setObjectFavorite,
    setObjectPin,
    createObject: handleCreateObject,
    updateObject: handleUpdateObject,
    archiveObject: handleArchiveObject,
    unarchiveObject: handleUnarchiveObject,
    updateWorkspace: handleUpdateWorkspace,
    archiveWorkspace: handleArchiveWorkspace,
  };
}
