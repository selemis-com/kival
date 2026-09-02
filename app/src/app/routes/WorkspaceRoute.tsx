import { useMemo } from "react";
import { matchPath, useLocation, useNavigate, useParams } from "react-router";
import { useWorkspaceController } from "../../features/workspaces/hooks/useWorkspaceController";
import { WorkspaceView } from "../../features/workspaces/WorkspaceView";
import { useKivalApp } from "../context";
import { usePageTitle } from "../documentTitle";

export function WorkspaceRoute() {
  const { workspaceId } = useParams();
  const location = useLocation();
  const navigate = useNavigate();
  const app = useKivalApp();

  const resolvedWorkspaceId = workspaceId ?? "";
  const objectRoute = matchPath("/w/:workspaceId/objects/:objectId/*", location.pathname);
  const routeObjectId = objectRoute?.params.objectId ?? null;
  const workspaceDirectory = useMemo(() => {
    const pinnedWorkspaceIds = new Set(app.pinnedWorkspaces.map((workspace) => workspace.id));
    return [
      ...app.pinnedWorkspaces,
      ...app.workspaces.filter((workspace) => !pinnedWorkspaceIds.has(workspace.id)),
    ];
  }, [app.pinnedWorkspaces, app.workspaces]);
  const controller = useWorkspaceController({
    user: app.user,
    workspaceId: resolvedWorkspaceId,
    objectId: routeObjectId,
    workspaces: workspaceDirectory,
    replaceWorkspace: app.replaceWorkspace,
    removeWorkspace: app.removeWorkspace,
    setApplicationError: app.setApplicationError,
  });
  const workspaceName =
    controller.workspace?.name ??
    workspaceDirectory.find((workspace) => workspace.id === resolvedWorkspaceId)?.name ??
    "Workspace";
  const objectTitle = controller.selectedObject?.current_version?.title ?? "Object";
  const pageTitle = objectRoute
    ? location.pathname.endsWith("/edit")
      ? `Edit ${objectTitle} · ${workspaceName}`
      : `${objectTitle} · ${workspaceName}`
    : location.pathname === `/w/${resolvedWorkspaceId}/new`
      ? `New object · ${workspaceName}`
      : location.pathname === `/w/${resolvedWorkspaceId}/recent`
        ? `Recent · ${workspaceName}`
        : location.pathname === `/w/${resolvedWorkspaceId}/favorites`
          ? `Favorites · ${workspaceName}`
          : location.pathname === `/w/${resolvedWorkspaceId}/graph`
            ? `Graph · ${workspaceName}`
            : location.pathname === `/w/${resolvedWorkspaceId}/archived`
              ? `Archived · ${workspaceName}`
              : location.pathname === `/w/${resolvedWorkspaceId}/members`
                ? `Members · ${workspaceName}`
                : location.pathname === `/w/${resolvedWorkspaceId}/settings`
                  ? `Settings · ${workspaceName}`
                  : workspaceName;
  usePageTitle(pageTitle);

  if (!workspaceId) {
    throw new Error("Workspace route is missing its workspace ID.");
  }

  if (!controller.workspace) {
    return null;
  }

  return (
    <WorkspaceView
      user={app.user}
      isGlobalAdmin={app.isGlobalAdmin}
      workspaces={workspaceDirectory}
      workspacesNextCursor={app.workspacesNextCursor}
      workspacesLoadingMore={app.workspacesLoadingMore}
      workspace={controller.workspace}
      objects={controller.objects}
      pinnedObjects={controller.pinnedObjects}
      recentObjects={controller.recentObjects}
      favoriteObjects={controller.favoriteObjects}
      pinnedFavoriteObjects={controller.pinnedFavoriteObjects}
      archivedObjects={controller.archivedObjects}
      selectedObject={controller.selectedObject}
      objectContext={controller.objectContext}
      workspaceLoading={controller.workspaceLoading}
      objectLoading={controller.objectLoading}
      objectsNextCursor={controller.objectsNextCursor}
      archivedObjectsNextCursor={controller.archivedObjectsNextCursor}
      objectsLoadingMore={controller.objectsLoadingMore}
      archivedObjectsLoadingMore={controller.archivedObjectsLoadingMore}
      recentLoading={controller.recentLoading}
      recentLoadingMore={controller.recentLoadingMore}
      recentIncomplete={controller.recentIncomplete}
      favoritesIncomplete={controller.favoritesIncomplete}
      favoritesLoadingMore={controller.favoritesLoadingMore}
      error={app.error}
      unreadInboxCount={app.unreadInboxCount}
      onLogout={app.logout}
      onInboxClick={() => navigate("/inbox")}
      onSecurityClick={() => navigate("/settings/security")}
      onApiKeysClick={() => navigate("/settings/api-keys")}
      onCloseWorkspace={() => navigate("/")}
      onOpenObject={(id, versionId) => {
        const versionQuery = versionId ? `?version=${encodeURIComponent(versionId)}` : "";
        navigate(`/w/${controller.workspace?.id}/objects/${id}${versionQuery}`, {
          state: { from: `${location.pathname}${location.search}` },
        });
      }}
      onOpenArchivedObject={(id) => {
        navigate(`/w/${controller.workspace?.id}/objects/${id}`, {
          state: { from: `${location.pathname}${location.search}` },
        });
      }}
      onLoadMoreWorkspaces={app.loadMoreWorkspaces}
      onLoadMoreObjects={() => controller.loadMoreObjects("active")}
      onLoadMoreArchivedObjects={() => controller.loadMoreObjects("archived")}
      onLoadRecentObjects={controller.loadRecentObjects}
      onLoadMoreRecentObjects={controller.loadMoreRecentObjects}
      onLoadMoreFavoriteObjects={controller.loadMoreFavoriteObjects}
      onRefreshObjectContext={controller.refreshObjectContext}
      onRefreshObjectAccess={controller.refreshSelectedObjectAccess}
      onRefreshWorkspaceAccess={controller.refreshWorkspaceAccess}
      onSetObjectFavorite={controller.setObjectFavorite}
      onSetObjectPin={controller.setObjectPin}
      onCreateObject={controller.createObject}
      onUpdateObject={controller.updateObject}
      onArchiveObject={controller.archiveObject}
      onUnarchiveObject={controller.unarchiveObject}
      onUpdateWorkspace={controller.updateWorkspace}
      onArchiveWorkspace={controller.archiveWorkspace}
    />
  );
}
