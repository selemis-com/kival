import { useEffect, useRef, useState } from "react";
import { matchPath, useBlocker, useLocation, useNavigate, useSearchParams } from "react-router";
import { SideBar } from "../../shared/navigation/SideBar";
import { TopBar } from "../../shared/navigation/TopBar";
import { comparePinOrder } from "../../shared/pins";
import { styles } from "../../shared/styles/index";
import type {
  CreateObjectRequest,
  CurrentObjectResponse,
  ObjectContext,
  ObjectResponse,
  ObjectSummary,
  RecentObject,
  UpdateObjectRequest,
  UpdateWorkspaceRequest,
  User,
  Workspace,
} from "../../shared/types";
import { CopyableId } from "../../shared/ui/CopyableId";
import { InfiniteScrollSentinel } from "../../shared/ui/InfiniteScrollSentinel";
import { LoadingIndicator } from "../../shared/ui/LoadingIndicator";
import { PinIcon } from "../../shared/ui/PinIcon";
import { ProfileHoverName } from "../../shared/ui/ProfileHoverCard";
import { Toast } from "../../shared/ui/Toast";
import { GraphView } from "../graph/GraphView";
import { ContextPanel } from "../objects/components/ContextPanel";
import { ObjectEditor } from "../objects/ObjectEditor";
import { ObjectView } from "../objects/ObjectView";
import { CommandPalette } from "./components/CommandPalette";
import { WorkspaceMembersPanel } from "./components/WorkspaceMembersPanel";
import { useWorkspaceSearch } from "./hooks/useWorkspaceSearch";
import {
  type GroupedSearchResult,
  groupWorkspaceSearchResults,
  HighlightedSearchText,
} from "./search";
import { WorkspaceSettings } from "./WorkspaceSettings";

type Props = {
  user: User;
  isGlobalAdmin: boolean;
  workspaces: Workspace[];
  workspacesNextCursor: string | null;
  workspacesLoadingMore: boolean;
  workspace: Workspace;
  objects: ObjectSummary[];
  pinnedObjects: ObjectSummary[];
  recentObjects: RecentObject[];
  favoriteObjects: ObjectSummary[];
  pinnedFavoriteObjects: ObjectSummary[];
  archivedObjects: ObjectSummary[];
  selectedObject: ObjectResponse | null;
  objectContext: ObjectContext | null;
  workspaceLoading: boolean;
  objectLoading: boolean;
  objectsNextCursor: string | null;
  archivedObjectsNextCursor: string | null;
  objectsLoadingMore: boolean;
  archivedObjectsLoadingMore: boolean;
  recentLoading: boolean;
  recentLoadingMore: boolean;
  recentIncomplete: boolean;
  favoritesIncomplete: boolean;
  favoritesLoadingMore: boolean;
  error: string | null;
  unreadInboxCount: number;
  onLogout: () => Promise<void>;
  onInboxClick: () => void;
  onSecurityClick: () => void;
  onApiKeysClick: () => void;
  onCloseWorkspace: () => void;
  onOpenObject: (id: string, versionId?: string) => void;
  onOpenArchivedObject: (id: string) => void;
  onLoadMoreWorkspaces: () => void;
  onLoadMoreObjects: () => Promise<void>;
  onLoadMoreArchivedObjects: () => Promise<void>;
  onLoadRecentObjects: () => Promise<void>;
  onLoadMoreRecentObjects: () => Promise<void>;
  onLoadMoreFavoriteObjects: () => Promise<void>;
  onRefreshObjectContext: (id: string) => Promise<void>;
  onRefreshObjectAccess: (id: string) => Promise<void>;
  onRefreshWorkspaceAccess: () => Promise<void>;
  onSetObjectFavorite: (id: string, favorited: boolean) => Promise<void>;
  onSetObjectPin: (id: string, pinned: boolean) => Promise<void>;
  onCreateObject: (input: CreateObjectRequest) => Promise<boolean>;
  onUpdateObject: (id: string, input: UpdateObjectRequest) => Promise<boolean>;
  onArchiveObject: (id: string) => Promise<boolean>;
  onUnarchiveObject: (id: string) => Promise<boolean>;
  onUpdateWorkspace: (input: UpdateWorkspaceRequest) => Promise<boolean>;
  onArchiveWorkspace: () => Promise<boolean>;
};

function formatRecentDate(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function hasCurrentVersion(value: ObjectResponse | null): value is CurrentObjectResponse {
  const metadata = value?.current_version?.metadata;
  return typeof metadata === "object" && metadata !== null && !Array.isArray(metadata);
}

export function WorkspaceView({
  user,
  isGlobalAdmin,
  workspaces,
  workspacesNextCursor,
  workspacesLoadingMore,
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
  recentIncomplete,
  favoritesIncomplete,
  favoritesLoadingMore,
  error,
  unreadInboxCount,
  onLogout,
  onInboxClick,
  onSecurityClick,
  onApiKeysClick,
  onCloseWorkspace,
  onOpenObject,
  onOpenArchivedObject,
  onLoadMoreWorkspaces,
  onLoadMoreObjects,
  onLoadMoreArchivedObjects,
  onLoadRecentObjects,
  onLoadMoreRecentObjects,
  onLoadMoreFavoriteObjects,
  onRefreshObjectContext,
  onRefreshObjectAccess,
  onRefreshWorkspaceAccess,
  onSetObjectFavorite,
  onSetObjectPin,
  onCreateObject,
  onUpdateObject,
  onArchiveObject,
  onUnarchiveObject,
  onUpdateWorkspace,
  onArchiveWorkspace,
}: Props) {
  const location = useLocation();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const [searchSelectionIndex, setSearchSelectionIndex] = useState(0);
  const [saveLoading, setSaveLoading] = useState(false);
  const [workspaceSaveLoading, setWorkspaceSaveLoading] = useState(false);
  const [unarchiveTargetId, setUnarchiveTargetId] = useState<string | null>(null);
  const [unarchivingObjectId, setUnarchivingObjectId] = useState<string | null>(null);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [editorDirty, setEditorDirty] = useState(false);
  const [discardNavigationOpen, setDiscardNavigationOpen] = useState(false);
  const discardDialogRef = useRef<HTMLDivElement>(null);
  const pendingNavigationRef = useRef<(() => void) | null>(null);
  const bypassNavigationBlockRef = useRef(false);
  const [toastMessage, setToastMessage] = useState<string | null>(null);
  const navigationBlocker = useBlocker(
    () => editorDirty && !saveLoading && !bypassNavigationBlockRef.current,
  );

  const workspaceBasePath = `/w/${workspace.id}`;
  const view =
    location.pathname === `${workspaceBasePath}/favorites`
      ? "favorites"
      : location.pathname === `${workspaceBasePath}/recent`
        ? "recent"
        : location.pathname === `${workspaceBasePath}/graph`
          ? "graph"
          : location.pathname === `${workspaceBasePath}/archived`
            ? "archived"
            : location.pathname === `${workspaceBasePath}/members`
              ? "members"
              : location.pathname === `${workspaceBasePath}/settings`
                ? "settings"
                : "home";
  const editorMode =
    location.pathname === `${workspaceBasePath}/new`
      ? "create"
      : matchPath(`${workspaceBasePath}/objects/:objectId/edit`, location.pathname)
        ? "edit"
        : null;
  const searchQuery = searchParams.get("q") ?? "";
  const includeSearchHistory = searchParams.get("history") === "1";

  function objectBackLabel(from: string | undefined) {
    if (!from || from === workspaceBasePath) {
      return "Back to objects";
    }

    const [pathname, query = ""] = from.split("?", 2);

    if (pathname === workspaceBasePath && new URLSearchParams(query).get("q")) {
      return "Back to search";
    }

    if (pathname === `${workspaceBasePath}/graph`) {
      return "Back to graph";
    }

    if (pathname === `${workspaceBasePath}/recent`) {
      return "Back to recent";
    }

    if (pathname === `${workspaceBasePath}/favorites`) {
      return "Back to favorites";
    }

    if (pathname === `${workspaceBasePath}/archived`) {
      return "Back to archived";
    }

    if (matchPath(`${workspaceBasePath}/objects/:objectId`, pathname)) {
      return "Back";
    }

    return "Back";
  }
  const search = useWorkspaceSearch(workspace.id, searchQuery, includeSearchHistory);
  const normalizedSearchQuery = search.normalizedQuery;
  const isSearching = search.active;
  const searchResults = search.results;
  const searchNextCursor = search.nextCursor;
  const searchLoading = search.loading;
  const searchLoadingMore = search.loadingMore;
  const searchError = search.error;
  const groupedSearchResults = groupWorkspaceSearchResults(searchResults);
  const visiblePinnedObjects = [...pinnedObjects].sort(comparePinOrder);
  const pinnedFavoriteIds = new Set(pinnedFavoriteObjects.map((object) => object.id));
  const visibleFavoriteObjects = [
    ...[...pinnedFavoriteObjects].sort(comparePinOrder),
    ...favoriteObjects.filter((object) => !pinnedFavoriteIds.has(object.id)),
  ];
  const canManageWorkspace = workspace.effective_role === "admin";
  const unarchiveTarget = archivedObjects.find((object) => object.id === unarchiveTargetId) ?? null;
  const currentObject = hasCurrentVersion(selectedObject) ? selectedObject : null;
  const showContextPanel =
    !objectLoading &&
    !editorMode &&
    currentObject?.object.status === "active" &&
    objectContext?.backlinks.object_id === currentObject.object.id;
  const graphMode = view === "graph" && !objectLoading && !editorMode && !selectedObject;

  useEffect(() => {
    if (view === "settings" && !canManageWorkspace) {
      navigate(workspaceBasePath, { replace: true });
    }
  }, [canManageWorkspace, navigate, view, workspaceBasePath]);

  useEffect(() => {
    function handleCommandPaletteShortcut(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        if (
          event.target instanceof HTMLTextAreaElement ||
          (event.target instanceof HTMLElement && event.target.isContentEditable)
        ) {
          return;
        }

        event.preventDefault();
        setCommandPaletteOpen((open) => !open);
      }
    }

    document.addEventListener("keydown", handleCommandPaletteShortcut);
    return () => document.removeEventListener("keydown", handleCommandPaletteShortcut);
  }, []);

  useEffect(() => {
    if (navigationBlocker.state === "blocked") {
      setDiscardNavigationOpen(true);
    }
  }, [navigationBlocker.state]);

  useEffect(() => {
    if (!discardNavigationOpen) {
      return;
    }

    discardDialogRef.current?.focus();

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        pendingNavigationRef.current = null;
        if (navigationBlocker.state === "blocked") {
          navigationBlocker.reset();
        }
        setDiscardNavigationOpen(false);
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [discardNavigationOpen, navigationBlocker]);

  useEffect(() => {
    if (!unarchiveTarget || unarchivingObjectId === unarchiveTarget.id) {
      return;
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setUnarchiveTargetId(null);
      }
    }

    document.addEventListener("keydown", handleKeyDown);

    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [unarchiveTarget, unarchivingObjectId]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: Reset selection whenever the effective query changes.
  useEffect(() => {
    setSearchSelectionIndex(0);
  }, [includeSearchHistory, normalizedSearchQuery]);

  const handleOpenSearchResultRef = useRef<(result: GroupedSearchResult) => void>(() => {});

  useEffect(() => {
    if (!isSearching || groupedSearchResults.length === 0) {
      return;
    }

    function handleSearchNavigation(event: KeyboardEvent) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setSearchSelectionIndex((index) => Math.min(index + 1, groupedSearchResults.length - 1));
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        setSearchSelectionIndex((index) => Math.max(index - 1, 0));
      } else if (
        event.key === "Enter" &&
        event.target instanceof HTMLInputElement &&
        event.target.type === "search"
      ) {
        event.preventDefault();
        handleOpenSearchResultRef.current(groupedSearchResults[searchSelectionIndex]);
      }
    }

    document.addEventListener("keydown", handleSearchNavigation);
    return () => document.removeEventListener("keydown", handleSearchNavigation);
  }, [groupedSearchResults, isSearching, searchSelectionIndex]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: Recent loading is route-keyed.
  useEffect(() => {
    if (view === "recent") {
      void onLoadRecentObjects();
    }
  }, [view, workspace.id]);

  const loadMoreSearchResults = search.loadMore;

  function requestNavigation(action: () => void) {
    if (editorDirty && !saveLoading) {
      pendingNavigationRef.current = action;
      setDiscardNavigationOpen(true);
      return;
    }

    action();
  }

  function guardedNavigate(path: string) {
    requestNavigation(() => navigate(path));
  }

  function guardedOpenObject(objectId: string, versionId?: string) {
    requestNavigation(() => onOpenObject(objectId, versionId));
  }

  function guardedOpenArchivedObject(objectId: string) {
    requestNavigation(() => onOpenArchivedObject(objectId));
  }

  function handleOpenSearchResult(result: GroupedSearchResult) {
    guardedOpenObject(result.objectId, result.versionId);
  }

  handleOpenSearchResultRef.current = handleOpenSearchResult;

  return (
    <div style={styles.app}>
      <Toast message={toastMessage} onDismiss={() => setToastMessage(null)} />

      <CommandPalette
        open={commandPaletteOpen}
        workspace={workspace}
        workspaces={workspaces}
        objects={objects}
        canManageWorkspace={canManageWorkspace}
        onClose={() => setCommandPaletteOpen(false)}
        onNavigate={guardedNavigate}
        onOpenObject={guardedOpenObject}
        onSwitchWorkspace={(workspaceId) => guardedNavigate(`/w/${workspaceId}`)}
      />

      <TopBar
        user={user}
        workspaces={workspaces}
        workspacesNextCursor={workspacesNextCursor}
        workspacesLoadingMore={workspacesLoadingMore}
        workspace={workspace}
        onCreateObject={(workspaceId) => guardedNavigate(`/w/${workspaceId}/new`)}
        onCreateWorkspaceClick={() => requestNavigation(() => navigate("/?create=workspace"))}
        onInboxClick={() => requestNavigation(onInboxClick)}
        unreadInboxCount={unreadInboxCount}
        onSecurityClick={() => requestNavigation(onSecurityClick)}
        onApiKeysClick={() => requestNavigation(onApiKeysClick)}
        onLogout={() => {
          requestNavigation(() => {
            void onLogout();
          });
          return Promise.resolve();
        }}
        searchQuery={searchQuery}
        onSearchQueryChange={(query) => {
          const next = new URLSearchParams(searchParams);

          if (query.trim()) {
            next.set("q", query);
          } else {
            next.delete("q");
            next.delete("history");
          }

          requestNavigation(() => {
            const search = next.toString();
            setSearchParams(next);

            if (location.pathname !== workspaceBasePath) {
              navigate({
                pathname: workspaceBasePath,
                search: search ? `?${search}` : "",
              });
            }
          });
        }}
        onHomeClick={() => requestNavigation(onCloseWorkspace)}
        onWorkspaceSelect={(workspaceId) => guardedNavigate(`/w/${workspaceId}`)}
        onLoadMoreWorkspaces={onLoadMoreWorkspaces}
      />

      <div style={showContextPanel ? styles.shellWithContext : styles.shell}>
        <SideBar
          view={view}
          canManageWorkspace={canManageWorkspace}
          onViewChange={(nextView) => {
            const path =
              nextView === "home" ? workspaceBasePath : `${workspaceBasePath}/${nextView}`;

            guardedNavigate(path);
          }}
        />

        <main style={graphMode ? styles.mainContentGraph : styles.mainContent}>
          <div style={graphMode ? styles.contentPaneInnerGraph : styles.contentPaneInner}>
            {objectLoading && <LoadingIndicator label="Loading object…" />}

            {!objectLoading && editorMode === "create" && (
              <ObjectEditor
                mode="create"
                loading={saveLoading}
                workspaceId={workspace.id}
                onOpenObject={guardedOpenObject}
                onDirtyChange={setEditorDirty}
                onCancel={() => guardedNavigate(workspaceBasePath)}
                onSubmit={async (input) => {
                  setSaveLoading(true);

                  try {
                    if (await onCreateObject(input)) {
                      setToastMessage("Object created");
                    }
                  } finally {
                    setSaveLoading(false);
                  }
                }}
              />
            )}

            {!objectLoading && editorMode === "edit" && currentObject && (
              <ObjectEditor
                mode="edit"
                value={currentObject}
                loading={saveLoading}
                workspaceId={workspace.id}
                onOpenObject={guardedOpenObject}
                onDirtyChange={setEditorDirty}
                onCancel={() =>
                  guardedNavigate(`${workspaceBasePath}/objects/${currentObject.object.id}`)
                }
                onSubmit={async (input) => {
                  setSaveLoading(true);

                  try {
                    if (await onUpdateObject(currentObject.object.id, input)) {
                      setToastMessage("New version saved");
                    }
                  } finally {
                    setSaveLoading(false);
                  }
                }}
              />
            )}

            {!objectLoading && !editorMode && currentObject && (
              <ObjectView
                user={user}
                value={currentObject}
                initialVersionId={searchParams.get("version")}
                initialCommentId={searchParams.get("comment")}
                initialThreadId={searchParams.get("thread")}
                objects={objects}
                context={objectContext}
                onOpenObject={guardedOpenObject}
                backLabel={objectBackLabel((location.state as { from?: string } | null)?.from)}
                onBack={() => {
                  const from = (location.state as { from?: string } | null)?.from;
                  guardedNavigate(from ?? workspaceBasePath);
                }}
                onEdit={() =>
                  guardedNavigate(`${workspaceBasePath}/objects/${currentObject.object.id}/edit`)
                }
                onRevealInGraph={() =>
                  navigate(`${workspaceBasePath}/graph?focus=${currentObject.object.id}`)
                }
                onArchive={async () => {
                  if (await onArchiveObject(currentObject.object.id)) {
                    setToastMessage("Object archived");
                  }
                }}
                onUnarchive={async () => {
                  if (await onUnarchiveObject(currentObject.object.id)) {
                    setToastMessage("Object restored");
                  }
                }}
                onAccessChanged={() => onRefreshObjectAccess(currentObject.object.id)}
              />
            )}

            {!objectLoading && !editorMode && !selectedObject && view === "home" && isSearching && (
              <section>
                <div style={styles.pageHeader}>
                  <p style={styles.eyebrow}>Search</p>

                  <h1 style={styles.pageTitle}>Results for “{normalizedSearchQuery}”</h1>
                </div>

                {searchLoading && <LoadingIndicator label="Searching workspace…" />}

                {!searchLoading && searchError && (
                  <div style={styles.errorBox}>
                    <strong>Could not search workspace</strong>
                    <span>{searchError}</span>
                  </div>
                )}

                {!searchLoading && !searchError && (
                  <>
                    <div style={styles.sectionHeader}>
                      <h2 style={styles.sectionTitle}>Matches</h2>

                      <div style={styles.sectionActions}>
                        <span style={styles.muted}>
                          {groupedSearchResults.length}{" "}
                          {groupedSearchResults.length === 1 ? "item" : "items"}
                          {searchResults.length !== groupedSearchResults.length &&
                            ` · ${searchResults.length} matches`}
                        </span>

                        <button
                          type="button"
                          style={styles.secondaryButtonCompact}
                          onClick={() => {
                            const next = new URLSearchParams(searchParams);
                            if (includeSearchHistory) {
                              next.delete("history");
                            } else {
                              next.set("history", "1");
                            }
                            setSearchParams(next);
                          }}
                        >
                          {includeSearchHistory ? "Current only" : "Search history"}
                        </button>
                      </div>
                    </div>

                    <div className="kival-row-list kival-object-list" style={styles.objectList}>
                      {groupedSearchResults.map((result, index) => (
                        <button
                          key={`${result.objectId}:${result.versionId}`}
                          type="button"
                          style={
                            index === searchSelectionIndex
                              ? styles.objectRowSelected
                              : styles.objectRow
                          }
                          onPointerMove={() => setSearchSelectionIndex(index)}
                          onClick={() => handleOpenSearchResult(result)}
                        >
                          <div style={styles.objectMain}>
                            <strong>
                              <HighlightedSearchText
                                value={result.title}
                                query={normalizedSearchQuery}
                                matchedTerms={result.termCoverage?.matched_terms}
                              />
                            </strong>

                            <span style={styles.objectMeta}>
                              {[
                                includeSearchHistory ? `v${result.versionNumber}` : null,
                                result.matchCount > 1 ? `${result.matchCount} matches` : null,
                                result.termCoverage &&
                                result.termCoverage.matched_terms.length <
                                  result.termCoverage.query_term_count
                                  ? `${result.termCoverage.matched_terms.length}/${result.termCoverage.query_term_count} terms`
                                  : null,
                                ...result.categories,
                              ]
                                .filter(Boolean)
                                .join(" · ")}
                            </span>

                            {result.snippets[0] && (
                              <span style={styles.searchSnippet}>
                                <HighlightedSearchText
                                  value={result.snippets[0]}
                                  query={normalizedSearchQuery}
                                  matchedTerms={result.termCoverage?.matched_terms}
                                />
                              </span>
                            )}
                          </div>
                        </button>
                      ))}

                      {groupedSearchResults.length === 0 && (
                        <div style={styles.emptyState}>
                          <strong>No matches found</strong>

                          <span>Try a different search term.</span>
                        </div>
                      )}
                    </div>
                    <InfiniteScrollSentinel
                      hasMore={Boolean(searchNextCursor)}
                      loading={searchLoadingMore}
                      onLoadMore={loadMoreSearchResults}
                      label="Loading more results…"
                    />
                  </>
                )}
              </section>
            )}

            {!objectLoading &&
              !editorMode &&
              !selectedObject &&
              view === "home" &&
              !isSearching && (
                <>
                  <div style={styles.pageHeader}>
                    <p style={styles.eyebrow}>Workspace</p>

                    <h1 style={styles.pageTitle}>{workspace.name}</h1>

                    {workspace.description && <p style={styles.muted}>{workspace.description}</p>}
                  </div>

                  {visiblePinnedObjects.length > 0 && (
                    <section style={styles.pinnedCardSection}>
                      <div style={styles.pinnedCardHeader}>
                        <h2 style={styles.sectionTitle}>Pinned</h2>
                      </div>

                      <div style={styles.pinnedCardGrid}>
                        {visiblePinnedObjects.map((object) => (
                          <article key={object.id} style={styles.pinnedCard}>
                            <button
                              type="button"
                              style={styles.pinnedCardAction}
                              aria-label={`Open ${object.title}`}
                              onClick={() => guardedOpenObject(object.id)}
                            />
                            <span style={styles.pinnedCardPinAction}>
                              <button
                                type="button"
                                style={styles.pinButtonActive}
                                aria-label={`Unpin ${object.title}`}
                                aria-pressed="true"
                                title="Unpin object"
                                onClick={() => void onSetObjectPin(object.id, false)}
                              >
                                <PinIcon active />
                              </button>
                            </span>
                            <div style={styles.pinnedCardMain}>
                              <strong style={styles.workspaceName}>{object.title}</strong>
                              <span style={styles.objectMeta}>
                                Updated {formatRecentDate(object.updated_at)}
                              </span>
                            </div>
                            <div style={styles.pinnedCardFooter}>
                              <CopyableId
                                value={object.id}
                                displayValue={`ID: ${object.id}`}
                                label="object ID"
                              />
                              <div style={styles.objectOverviewActions}>
                                <button
                                  type="button"
                                  style={styles.favoriteButton}
                                  aria-label={
                                    object.favorited
                                      ? `Remove ${object.title} from favorites`
                                      : `Add ${object.title} to favorites`
                                  }
                                  aria-pressed={Boolean(object.favorited)}
                                  title={
                                    object.favorited ? "Remove from favorites" : "Add to favorites"
                                  }
                                  onClick={() =>
                                    void onSetObjectFavorite(object.id, !object.favorited)
                                  }
                                >
                                  {object.favorited ? "★" : "☆"}
                                </button>
                              </div>
                            </div>
                          </article>
                        ))}
                      </div>
                    </section>
                  )}

                  {workspaceLoading && <LoadingIndicator label="Loading objects…" />}

                  {!workspaceLoading && error && (
                    <div style={styles.errorBox}>
                      <strong>Could not load objects</strong>
                      <span>{error}</span>
                    </div>
                  )}

                  {!workspaceLoading && !error && (
                    <section>
                      <div style={styles.sectionHeader}>
                        <h2 style={styles.sectionTitle}>All objects</h2>

                        <div style={styles.sectionActions}>
                          <span style={styles.muted}>
                            {objects.length}
                            {objectsNextCursor ? "+" : ""} items
                          </span>

                          <button
                            type="button"
                            style={styles.primaryButtonCompact}
                            onClick={() => guardedNavigate(`${workspaceBasePath}/new`)}
                          >
                            New object
                          </button>
                        </div>
                      </div>

                      <div className="kival-row-list kival-object-list" style={styles.objectList}>
                        {objects.map((object) => (
                          <div
                            key={object.id}
                            style={{ ...styles.objectRow, position: "relative" }}
                          >
                            <button
                              type="button"
                              style={styles.objectOverviewRowAction}
                              aria-label={`Open ${object.title}`}
                              onClick={() => guardedOpenObject(object.id)}
                            />
                            <div style={styles.objectOverviewMain}>
                              <span style={styles.objectTitleLine}>
                                <strong>{object.title}</strong>
                              </span>
                              <span style={styles.objectMeta}>
                                Updated {formatRecentDate(object.updated_at)}
                                {object.updated_by_username && (
                                  <>
                                    {" by "}
                                    <ProfileHoverName
                                      displayName={
                                        object.updated_by_display_name || object.updated_by_username
                                      }
                                      username={object.updated_by_username}
                                      workspaceRole={object.updated_by_workspace_role}
                                      accessRole={object.updated_by_object_role}
                                    >
                                      @{object.updated_by_username}
                                    </ProfileHoverName>
                                  </>
                                )}
                                {object.connection_count
                                  ? ` · ${object.connection_count} ${
                                      object.connection_count === 1 ? "connection" : "connections"
                                    }`
                                  : ""}
                                {object.unresolved_thread_count
                                  ? ` · ${object.unresolved_thread_count} open ${
                                      object.unresolved_thread_count === 1 ? "thread" : "threads"
                                    }`
                                  : ""}
                              </span>
                            </div>

                            <div style={styles.objectOverviewActions}>
                              <button
                                type="button"
                                style={styles.favoriteButton}
                                aria-label={
                                  object.favorited
                                    ? `Remove ${object.title} from favorites`
                                    : `Add ${object.title} to favorites`
                                }
                                aria-pressed={Boolean(object.favorited)}
                                title={
                                  object.favorited ? "Remove from favorites" : "Add to favorites"
                                }
                                onClick={() =>
                                  void onSetObjectFavorite(object.id, !object.favorited)
                                }
                              >
                                {object.favorited ? "★" : "☆"}
                              </button>
                              <button
                                type="button"
                                style={object.pinned ? styles.pinButtonActive : styles.pinButton}
                                aria-label={
                                  object.pinned ? `Unpin ${object.title}` : `Pin ${object.title}`
                                }
                                aria-pressed={Boolean(object.pinned)}
                                title={object.pinned ? "Unpin object" : "Pin object"}
                                onClick={() => void onSetObjectPin(object.id, !object.pinned)}
                              >
                                <PinIcon active={Boolean(object.pinned)} />
                              </button>
                              <CopyableId
                                value={object.id}
                                displayValue={`ID: ${object.id}`}
                                label="object ID"
                                style={styles.objectOverviewId}
                              />
                            </div>
                          </div>
                        ))}

                        {objects.length === 0 && (
                          <div style={styles.emptyState}>
                            <strong>No objects found</strong>

                            <span>This workspace does not contain any visible objects yet.</span>
                          </div>
                        )}
                      </div>
                      <InfiniteScrollSentinel
                        hasMore={Boolean(objectsNextCursor)}
                        loading={objectsLoadingMore}
                        onLoadMore={onLoadMoreObjects}
                        label="Loading more objects…"
                      />
                    </section>
                  )}
                </>
              )}

            {!objectLoading && !editorMode && !selectedObject && view === "favorites" && (
              <>
                <div style={styles.pageHeader}>
                  <p style={styles.eyebrow}>Workspace</p>
                  <h1 style={styles.pageTitle}>Favorites</h1>
                  <p style={styles.muted}>Objects you have starred in this workspace.</p>
                </div>

                <section>
                  <div className="kival-row-list kival-object-list" style={styles.objectList}>
                    {visibleFavoriteObjects.map((object) => (
                      <div key={object.id} style={{ ...styles.objectRow, position: "relative" }}>
                        <button
                          type="button"
                          style={styles.objectOverviewRowAction}
                          aria-label={`Open ${object.title}`}
                          onClick={() => guardedOpenObject(object.id)}
                        />
                        <div style={styles.objectOverviewMain}>
                          <span style={styles.objectTitleLine}>
                            <strong>{object.title}</strong>
                          </span>
                          <span style={styles.objectMeta}>
                            Updated {formatRecentDate(object.updated_at)}
                          </span>
                        </div>
                        <div style={styles.objectOverviewActions}>
                          <button
                            type="button"
                            style={styles.favoriteButton}
                            aria-label={`Remove ${object.title} from favorites`}
                            aria-pressed="true"
                            title="Remove from favorites"
                            onClick={() => void onSetObjectFavorite(object.id, false)}
                          >
                            ★
                          </button>
                          <button
                            type="button"
                            style={object.pinned ? styles.pinButtonActive : styles.pinButton}
                            aria-label={
                              object.pinned ? `Unpin ${object.title}` : `Pin ${object.title}`
                            }
                            aria-pressed={Boolean(object.pinned)}
                            title={object.pinned ? "Unpin object" : "Pin object"}
                            onClick={() => void onSetObjectPin(object.id, !object.pinned)}
                          >
                            <PinIcon active={Boolean(object.pinned)} />
                          </button>
                          <CopyableId
                            value={object.id}
                            displayValue={`ID: ${object.id}`}
                            label="object ID"
                            style={styles.objectOverviewId}
                          />
                        </div>
                      </div>
                    ))}

                    {visibleFavoriteObjects.length === 0 && (
                      <div style={styles.emptyState}>
                        <strong>No favorite objects</strong>
                        <span>Star an object to keep it in this list.</span>
                      </div>
                    )}
                  </div>
                  <InfiniteScrollSentinel
                    hasMore={favoritesIncomplete}
                    loading={favoritesLoadingMore}
                    onLoadMore={onLoadMoreFavoriteObjects}
                    label="Loading more favorites…"
                  />
                </section>
              </>
            )}

            {!objectLoading && !editorMode && !selectedObject && view === "recent" && (
              <>
                <div style={styles.pageHeader}>
                  <p style={styles.eyebrow}>Workspace</p>

                  <h1 style={styles.pageTitle}>Recent</h1>

                  <p style={styles.muted}>Recently updated objects in this workspace.</p>
                </div>

                {recentLoading && <LoadingIndicator label="Loading recent objects…" />}

                {!recentLoading && (
                  <section>
                    <div style={styles.sectionHeader}>
                      <h2 style={styles.sectionTitle}>Recently updated</h2>

                      <span style={styles.muted}>
                        {recentObjects.length}
                        {recentIncomplete ? "+" : ""}{" "}
                        {recentObjects.length === 1 ? "item" : "items"}
                      </span>
                    </div>

                    <div className="kival-row-list kival-object-list" style={styles.objectList}>
                      {recentObjects.map((object) => (
                        <button
                          key={object.id}
                          type="button"
                          style={styles.objectRow}
                          onClick={() => guardedOpenObject(object.id)}
                        >
                          <div style={styles.objectMain}>
                            <strong>{object.title}</strong>

                            <span style={styles.objectMeta}>
                              {formatRecentDate(object.updated_at)}
                            </span>
                          </div>
                        </button>
                      ))}

                      {recentObjects.length === 0 && (
                        <div style={styles.emptyState}>
                          <strong>No recent objects</strong>

                          <span>Objects will appear here after they are created or updated.</span>
                        </div>
                      )}
                    </div>
                    <InfiniteScrollSentinel
                      hasMore={recentIncomplete}
                      loading={recentLoadingMore}
                      onLoadMore={onLoadMoreRecentObjects}
                      label="Loading more recent objects…"
                    />
                  </section>
                )}
              </>
            )}

            {!objectLoading && !editorMode && !selectedObject && view === "graph" && (
              <GraphView
                workspace={workspace}
                onOpenObject={guardedOpenObject}
                focusObjectId={searchParams.get("focus")}
              />
            )}

            {!objectLoading && !editorMode && !selectedObject && view === "members" && (
              <WorkspaceMembersPanel
                user={user}
                isGlobalAdmin={isGlobalAdmin}
                workspace={workspace}
                canManageWorkspace={canManageWorkspace}
                onCurrentUserRemoved={onCloseWorkspace}
                onCurrentUserRoleChanged={onRefreshWorkspaceAccess}
                onToast={setToastMessage}
              />
            )}

            {!objectLoading &&
              !editorMode &&
              !selectedObject &&
              view === "settings" &&
              canManageWorkspace && (
                <WorkspaceSettings
                  workspace={workspace}
                  loading={workspaceSaveLoading}
                  onSave={async (input) => {
                    setWorkspaceSaveLoading(true);

                    try {
                      if (await onUpdateWorkspace(input)) {
                        setToastMessage("Workspace updated");
                        guardedNavigate(workspaceBasePath);
                      }
                    } finally {
                      setWorkspaceSaveLoading(false);
                    }
                  }}
                  onArchive={async () => {
                    if (await onArchiveWorkspace()) {
                      setToastMessage("Workspace archived");
                    }
                  }}
                />
              )}

            {!objectLoading && !editorMode && !selectedObject && view === "archived" && (
              <>
                <div style={styles.pageHeader}>
                  <p style={styles.eyebrow}>Workspace</p>

                  <h1 style={styles.pageTitle}>Archived</h1>

                  <p style={styles.muted}>Objects removed from the active workspace.</p>
                </div>

                <section>
                  <div style={styles.sectionHeader}>
                    <h2 style={styles.sectionTitle}>Archived objects</h2>

                    <span style={styles.muted}>
                      {archivedObjects.length}
                      {archivedObjectsNextCursor ? "+" : ""}{" "}
                      {archivedObjects.length === 1 ? "item" : "items"}
                    </span>
                  </div>

                  <div className="kival-row-list kival-object-list" style={styles.objectList}>
                    {archivedObjects.map((object) => (
                      <div key={object.id} style={styles.objectRow}>
                        <button
                          type="button"
                          style={styles.objectRowMainAction}
                          onClick={() => guardedOpenArchivedObject(object.id)}
                        >
                          <div style={styles.objectMain}>
                            <strong>{object.title}</strong>
                          </div>
                        </button>

                        <div style={styles.objectRowActions}>
                          <button
                            type="button"
                            style={styles.secondaryButtonCompact}
                            disabled={unarchivingObjectId === object.id}
                            onClick={() => setUnarchiveTargetId(object.id)}
                          >
                            Unarchive
                          </button>
                        </div>
                      </div>
                    ))}

                    {archivedObjects.length === 0 && (
                      <div style={styles.emptyState}>
                        <strong>No archived objects</strong>

                        <span>Archived objects will appear here.</span>
                      </div>
                    )}
                  </div>
                  <InfiniteScrollSentinel
                    hasMore={Boolean(archivedObjectsNextCursor)}
                    loading={archivedObjectsLoadingMore}
                    onLoadMore={onLoadMoreArchivedObjects}
                    label="Loading more archived objects…"
                  />
                </section>
              </>
            )}
          </div>
        </main>

        {discardNavigationOpen && (
          <div style={styles.modalBackdrop}>
            <button
              type="button"
              aria-label="Close unsaved changes confirmation"
              style={styles.modalBackdropDismiss}
              onClick={() => {
                pendingNavigationRef.current = null;
                if (navigationBlocker.state === "blocked") {
                  navigationBlocker.reset();
                }
                setDiscardNavigationOpen(false);
              }}
            />
            <div
              ref={discardDialogRef}
              role="alertdialog"
              aria-modal="true"
              aria-labelledby="discard-dialog-title"
              aria-describedby="discard-dialog-description"
              style={styles.modalDialog}
              tabIndex={-1}
            >
              <div style={styles.modalCopy}>
                <h2 id="discard-dialog-title" style={styles.modalTitle}>
                  Discard unsaved changes?
                </h2>

                <p id="discard-dialog-description" style={styles.muted}>
                  Your edits have not been saved. Leaving now will permanently discard them.
                </p>
              </div>

              <div style={styles.modalActions}>
                <button
                  type="button"
                  style={styles.secondaryButton}
                  onClick={() => {
                    pendingNavigationRef.current = null;
                    if (navigationBlocker.state === "blocked") {
                      navigationBlocker.reset();
                    }
                    setDiscardNavigationOpen(false);
                  }}
                >
                  Keep editing
                </button>

                <button
                  type="button"
                  style={styles.dangerButton}
                  onClick={() => {
                    const action = pendingNavigationRef.current;
                    pendingNavigationRef.current = null;
                    setDiscardNavigationOpen(false);
                    setEditorDirty(false);

                    if (navigationBlocker.state === "blocked") {
                      navigationBlocker.proceed();
                      return;
                    }

                    if (action) {
                      bypassNavigationBlockRef.current = true;
                      action();
                      queueMicrotask(() => {
                        bypassNavigationBlockRef.current = false;
                      });
                    }
                  }}
                >
                  Discard changes
                </button>
              </div>
            </div>
          </div>
        )}

        {unarchiveTarget && (
          <div style={styles.modalBackdrop}>
            <button
              type="button"
              style={styles.modalBackdropDismiss}
              aria-label="Close unarchive confirmation"
              disabled={unarchivingObjectId === unarchiveTarget.id}
              onClick={() => setUnarchiveTargetId(null)}
            />
            <div
              role="dialog"
              aria-modal="true"
              aria-labelledby="unarchive-dialog-title"
              aria-describedby="unarchive-dialog-description"
              style={styles.modalDialog}
            >
              <div style={styles.modalCopy}>
                <h2 id="unarchive-dialog-title" style={styles.modalTitle}>
                  Unarchive “{unarchiveTarget.title}”?
                </h2>

                <p id="unarchive-dialog-description" style={styles.muted}>
                  This object will be restored to the active workspace.
                </p>
              </div>

              <div style={styles.modalActions}>
                <button
                  type="button"
                  style={styles.secondaryButton}
                  disabled={unarchivingObjectId === unarchiveTarget.id}
                  onClick={() => setUnarchiveTargetId(null)}
                >
                  Cancel
                </button>

                <button
                  type="button"
                  style={styles.primaryButtonCompact}
                  disabled={unarchivingObjectId === unarchiveTarget.id}
                  onClick={async () => {
                    setUnarchivingObjectId(unarchiveTarget.id);

                    try {
                      await onUnarchiveObject(unarchiveTarget.id);
                      setUnarchiveTargetId(null);
                    } finally {
                      setUnarchivingObjectId(null);
                    }
                  }}
                >
                  {unarchivingObjectId === unarchiveTarget.id ? "Restoring…" : "Unarchive"}
                </button>
              </div>
            </div>
          </div>
        )}

        {showContextPanel && (
          <ContextPanel
            workspaceId={workspace.id}
            context={objectContext}
            value={currentObject}
            objects={objects}
            onOpenObject={guardedOpenObject}
            onRevealInGraph={(objectId) =>
              guardedNavigate(`${workspaceBasePath}/graph?focus=${encodeURIComponent(objectId)}`)
            }
            onContextChanged={onRefreshObjectContext}
          />
        )}
      </div>
    </div>
  );
}
