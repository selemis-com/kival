import { KivalTransportError } from "kival-sdk";
import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { kival } from "../api";
import { styles } from "../styles/index";
import type { User, Workspace } from "../types";
import { InfiniteScrollSentinel } from "../ui/InfiniteScrollSentinel";
import { KivalLogo } from "../ui/KivalLogo";
import { LoadingIndicator } from "../ui/LoadingIndicator";
import { ProfileMenu } from "./ProfileMenu";

type Props = {
  workspaces?: Workspace[];
  workspacesNextCursor?: string | null;
  workspacesLoadingMore?: boolean;
  workspace?: Workspace | null;
  searchQuery?: string;
  user?: User;
  onSearchQueryChange?: (query: string) => void;
  onHomeClick?: () => void;
  onWorkspaceSelect?: (workspaceId: string) => void;
  onLoadMoreWorkspaces?: () => void;
  onCreateObject?: (workspaceId: string) => void;
  onCreateWorkspaceClick?: () => void;
  onInboxClick?: () => void;
  unreadInboxCount?: number;
  onSecurityClick?: () => void;
  onApiKeysClick?: () => void;
  onLogout?: () => Promise<void>;
};

export function TopBar({
  workspaces = [],
  workspacesNextCursor = null,
  workspacesLoadingMore = false,
  workspace,
  searchQuery = "",
  user,
  onSearchQueryChange,
  onHomeClick,
  onWorkspaceSelect,
  onLoadMoreWorkspaces,
  onCreateObject,
  onCreateWorkspaceClick,
  onInboxClick,
  unreadInboxCount = 0,
  onSecurityClick,
  onApiKeysClick,
  onLogout,
}: Props) {
  const navigate = useNavigate();
  const [workspaceMenuOpen, setWorkspaceMenuOpen] = useState(false);
  const [createMenuOpen, setCreateMenuOpen] = useState(false);
  const [createMenuView, setCreateMenuView] = useState<"root" | "workspace">("root");
  const [workspaceQuery, setWorkspaceQuery] = useState("");
  const [workspaceSearchResults, setWorkspaceSearchResults] = useState<Workspace[]>([]);
  const [workspaceSearchNextCursor, setWorkspaceSearchNextCursor] = useState<string | null>(null);
  const [workspaceSearchLoading, setWorkspaceSearchLoading] = useState(false);
  const [workspaceSearchLoadingMore, setWorkspaceSearchLoadingMore] = useState(false);
  const [workspaceSearchError, setWorkspaceSearchError] = useState<string | null>(null);
  const [settledWorkspaceQuery, setSettledWorkspaceQuery] = useState<string | null>(null);
  const [highlightedWorkspaceId, setHighlightedWorkspaceId] = useState<string | null>(null);
  const workspaceMenuRef = useRef<HTMLDivElement>(null);
  const createMenuRef = useRef<HTMLDivElement>(null);
  const workspaceSearchGenerationRef = useRef(0);
  const mainSearchRef = useRef<HTMLInputElement>(null);
  const normalizedWorkspaceQuery = workspaceQuery.trim();
  const workspaceSearchActive = normalizedWorkspaceQuery.length > 0;
  const workspaceSearching =
    workspaceSearchActive &&
    (workspaceSearchLoading || settledWorkspaceQuery !== normalizedWorkspaceQuery);
  const visibleWorkspaces = workspaceSearchActive ? workspaceSearchResults : workspaces;
  const createWorkspaceOptions = visibleWorkspaces.filter(
    (candidate) => candidate.id !== workspace?.id,
  );
  const keyboardWorkspaces =
    workspaceSearching || workspaceSearchError
      ? []
      : createMenuOpen && createMenuView === "workspace"
        ? createWorkspaceOptions
        : visibleWorkspaces;
  const workspaceResultsHaveMore = Boolean(
    workspaceSearchActive ? workspaceSearchNextCursor : workspacesNextCursor,
  );
  const workspacePickerOpen =
    workspaceMenuOpen || (createMenuOpen && createMenuView === "workspace");

  useEffect(() => {
    if (!createMenuOpen) {
      return;
    }

    function handlePointerDown(event: PointerEvent) {
      if (createMenuRef.current && !createMenuRef.current.contains(event.target as Node)) {
        setCreateMenuOpen(false);
        setCreateMenuView("root");
        setWorkspaceQuery("");
        setHighlightedWorkspaceId(null);
      }
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setCreateMenuOpen(false);
        setCreateMenuView("root");
        setWorkspaceQuery("");
        setHighlightedWorkspaceId(null);
      }
    }

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [createMenuOpen]);

  useEffect(() => {
    if (!workspace) {
      return;
    }

    function handleSearchShortcut(event: KeyboardEvent) {
      if (event.key !== "/" || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
        return;
      }

      const target = event.target;
      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        (target instanceof HTMLElement && target.isContentEditable)
      ) {
        return;
      }

      event.preventDefault();
      mainSearchRef.current?.focus();
    }

    document.addEventListener("keydown", handleSearchShortcut);
    return () => document.removeEventListener("keydown", handleSearchShortcut);
  }, [workspace]);

  useEffect(() => {
    if (!workspaceMenuOpen) {
      return;
    }

    function handlePointerDown(event: PointerEvent) {
      const target = event.target as Node;

      if (workspaceMenuRef.current && !workspaceMenuRef.current.contains(target)) {
        setWorkspaceMenuOpen(false);
        setWorkspaceQuery("");
        setHighlightedWorkspaceId(null);
      }
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setWorkspaceMenuOpen(false);
        setWorkspaceQuery("");
        setHighlightedWorkspaceId(null);
      }
    }

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);

    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [workspaceMenuOpen]);

  useEffect(() => {
    const generation = ++workspaceSearchGenerationRef.current;
    setWorkspaceSearchLoadingMore(false);

    if (!workspacePickerOpen || !workspaceSearchActive) {
      setWorkspaceSearchResults([]);
      setWorkspaceSearchNextCursor(null);
      setWorkspaceSearchLoading(false);
      setWorkspaceSearchError(null);
      setSettledWorkspaceQuery(null);
      return;
    }

    const controller = new AbortController();
    const timeout = window.setTimeout(() => {
      setWorkspaceSearchLoading(true);
      setWorkspaceSearchError(null);
      setWorkspaceSearchNextCursor(null);

      void kival
        .listWorkspaces({ q: normalizedWorkspaceQuery, signal: controller.signal })
        .then((response) => {
          if (generation !== workspaceSearchGenerationRef.current) {
            return;
          }
          setWorkspaceSearchResults(response.items);
          setWorkspaceSearchNextCursor(response.next_cursor ?? null);
        })
        .catch((cause: unknown) => {
          if (
            (cause instanceof KivalTransportError && cause.kind === "abort") ||
            generation !== workspaceSearchGenerationRef.current
          ) {
            return;
          }
          setWorkspaceSearchError(
            cause instanceof Error ? cause.message : "Could not search workspaces.",
          );
        })
        .finally(() => {
          if (!controller.signal.aborted && generation === workspaceSearchGenerationRef.current) {
            setWorkspaceSearchLoading(false);
            setSettledWorkspaceQuery(normalizedWorkspaceQuery);
          }
        });
    }, 150);

    return () => {
      window.clearTimeout(timeout);
      controller.abort();
    };
  }, [normalizedWorkspaceQuery, workspacePickerOpen, workspaceSearchActive]);

  useEffect(() => {
    if (
      highlightedWorkspaceId &&
      !keyboardWorkspaces.some((candidate) => candidate.id === highlightedWorkspaceId)
    ) {
      setHighlightedWorkspaceId(null);
    }
  }, [highlightedWorkspaceId, keyboardWorkspaces]);

  useEffect(() => {
    if (!highlightedWorkspaceId) {
      return;
    }

    document
      .getElementById(
        `${workspaceMenuOpen ? "workspace-switcher" : "create-object-workspace"}-option-${highlightedWorkspaceId}`,
      )
      ?.scrollIntoView({ block: "nearest" });
  }, [highlightedWorkspaceId, workspaceMenuOpen]);

  function closeWorkspaceMenu() {
    setWorkspaceMenuOpen(false);
    setWorkspaceQuery("");
    setHighlightedWorkspaceId(null);
  }

  function selectWorkspace(workspaceId: string) {
    closeWorkspaceMenu();
    onWorkspaceSelect?.(workspaceId);
  }

  function closeCreateMenu() {
    setCreateMenuOpen(false);
    setCreateMenuView("root");
    setWorkspaceQuery("");
    setHighlightedWorkspaceId(null);
  }

  function createObject(workspaceId: string) {
    closeCreateMenu();
    if (onCreateObject) {
      onCreateObject(workspaceId);
    } else {
      navigate(`/w/${workspaceId}/new`);
    }
  }

  function createWorkspace() {
    closeCreateMenu();
    if (onCreateWorkspaceClick) {
      onCreateWorkspaceClick();
    } else {
      navigate("/?create=workspace");
    }
  }

  function handleWorkspaceSearchKeyDown(
    event: React.KeyboardEvent<HTMLInputElement>,
    onSelect: (workspaceId: string) => void,
  ) {
    if (event.nativeEvent.isComposing) {
      return;
    }

    const currentIndex = keyboardWorkspaces.findIndex(
      (candidate) => candidate.id === highlightedWorkspaceId,
    );
    let nextIndex: number | null = null;

    switch (event.key) {
      case "ArrowDown":
        nextIndex =
          currentIndex < 0 ? 0 : Math.min(currentIndex + 1, keyboardWorkspaces.length - 1);
        break;
      case "ArrowUp":
        nextIndex =
          currentIndex < 0 ? keyboardWorkspaces.length - 1 : Math.max(currentIndex - 1, 0);
        break;
      case "PageDown":
        nextIndex =
          currentIndex < 0 ? 0 : Math.min(currentIndex + 5, keyboardWorkspaces.length - 1);
        break;
      case "PageUp":
        nextIndex =
          currentIndex < 0 ? keyboardWorkspaces.length - 1 : Math.max(currentIndex - 5, 0);
        break;
      case "Home":
        if (currentIndex >= 0) {
          nextIndex = 0;
        }
        break;
      case "End":
        if (currentIndex >= 0) {
          nextIndex = keyboardWorkspaces.length - 1;
        }
        break;
      case "Enter": {
        const selected =
          keyboardWorkspaces.find((candidate) => candidate.id === highlightedWorkspaceId) ??
          (keyboardWorkspaces.length === 1 && !workspaceResultsHaveMore
            ? keyboardWorkspaces[0]
            : undefined);

        if (selected) {
          event.preventDefault();
          onSelect(selected.id);
        }
        return;
      }
      default:
        return;
    }

    if (nextIndex !== null && nextIndex >= 0 && keyboardWorkspaces[nextIndex]) {
      event.preventDefault();
      setHighlightedWorkspaceId(keyboardWorkspaces[nextIndex].id);
    }
  }

  async function loadMoreWorkspaceSearchResults() {
    if (
      !workspaceSearchNextCursor ||
      workspaceSearchLoading ||
      workspaceSearchLoadingMore ||
      !workspaceSearchActive ||
      settledWorkspaceQuery !== normalizedWorkspaceQuery
    ) {
      return;
    }

    const generation = workspaceSearchGenerationRef.current;
    const cursor = workspaceSearchNextCursor;

    setWorkspaceSearchLoadingMore(true);
    setWorkspaceSearchError(null);
    try {
      const response = await kival.listWorkspaces({
        cursor,
        q: normalizedWorkspaceQuery,
      });
      if (generation !== workspaceSearchGenerationRef.current) {
        return;
      }
      setWorkspaceSearchResults((current) => [
        ...current,
        ...response.items.filter(
          (candidate) => !current.some((existing) => existing.id === candidate.id),
        ),
      ]);
      setWorkspaceSearchNextCursor(response.next_cursor ?? null);
    } catch (cause) {
      if (generation === workspaceSearchGenerationRef.current) {
        setWorkspaceSearchError(
          cause instanceof Error ? cause.message : "Could not load more workspaces.",
        );
      }
    } finally {
      if (generation === workspaceSearchGenerationRef.current) {
        setWorkspaceSearchLoadingMore(false);
      }
    }
  }

  return (
    <header style={styles.topBar}>
      <div style={styles.topBarLeft}>
        <button type="button" onClick={onHomeClick} style={styles.wordmarkButton}>
          <KivalLogo style={{ width: 68 }} />
        </button>

        {workspace && (
          <div ref={workspaceMenuRef} style={styles.workspaceMenu}>
            <button
              type="button"
              onClick={() => {
                closeCreateMenu();
                if (workspaceMenuOpen) {
                  setWorkspaceQuery("");
                  setHighlightedWorkspaceId(null);
                }
                setWorkspaceMenuOpen(!workspaceMenuOpen);
              }}
              style={styles.workspaceSwitcher}
              aria-haspopup="dialog"
              aria-expanded={workspaceMenuOpen}
            >
              <span>{workspace.name}</span>

              <svg
                width="12"
                height="12"
                viewBox="0 0 24 24"
                fill="none"
                aria-hidden="true"
                style={{
                  flexShrink: 0,
                  transform: workspaceMenuOpen ? "rotate(180deg)" : "rotate(0deg)",
                  transition: "transform 120ms ease",
                }}
              >
                <path
                  d="m6 9 6 6 6-6"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            </button>

            {workspaceMenuOpen && (
              <div role="dialog" aria-label="Switch workspace" style={styles.workspaceMenuPopover}>
                <input
                  data-1p-ignore="true"
                  autoComplete="off"
                  autoFocus
                  type="search"
                  value={workspaceQuery}
                  placeholder="Search workspaces…"
                  aria-label="Search workspaces"
                  role="combobox"
                  aria-autocomplete="list"
                  aria-controls="workspace-switcher-options"
                  aria-expanded="true"
                  aria-activedescendant={
                    highlightedWorkspaceId
                      ? `workspace-switcher-option-${highlightedWorkspaceId}`
                      : undefined
                  }
                  style={styles.workspaceMenuSearch}
                  onChange={(event) => {
                    setWorkspaceQuery(event.target.value);
                    setHighlightedWorkspaceId(null);
                  }}
                  onKeyDown={(event) => handleWorkspaceSearchKeyDown(event, selectWorkspace)}
                />

                <div
                  id="workspace-switcher-options"
                  role="listbox"
                  aria-label="Workspaces"
                  style={styles.workspaceMenuList}
                >
                  {workspaceSearching && <LoadingIndicator label="Searching workspaces…" compact />}

                  {!workspaceSearching &&
                    !workspaceSearchError &&
                    visibleWorkspaces.map((candidate) => {
                      const isCurrent = candidate.id === workspace.id;
                      const isHighlighted = candidate.id === highlightedWorkspaceId;

                      return (
                        <button
                          key={candidate.id}
                          id={`workspace-switcher-option-${candidate.id}`}
                          type="button"
                          role="option"
                          tabIndex={-1}
                          aria-selected={isCurrent}
                          style={
                            isHighlighted
                              ? styles.workspaceMenuItemHighlighted
                              : isCurrent
                                ? styles.workspaceMenuItemActive
                                : styles.workspaceMenuItem
                          }
                          onPointerMove={() => setHighlightedWorkspaceId(candidate.id)}
                          onClick={() => selectWorkspace(candidate.id)}
                        >
                          <span style={styles.workspaceMenuItemText}>
                            <span>{candidate.name}</span>

                            {candidate.description && (
                              <span style={styles.workspaceMenuItemDescription}>
                                {candidate.description}
                              </span>
                            )}
                          </span>

                          {isCurrent && (
                            <svg
                              width="14"
                              height="14"
                              viewBox="0 0 24 24"
                              fill="none"
                              aria-hidden="true"
                              style={{ flexShrink: 0 }}
                            >
                              <path
                                d="m5 12 4 4L19 6"
                                stroke="currentColor"
                                strokeWidth="2"
                                strokeLinecap="round"
                                strokeLinejoin="round"
                              />
                            </svg>
                          )}
                        </button>
                      );
                    })}

                  {!workspaceSearching &&
                    !workspaceSearchError &&
                    workspaceSearchActive &&
                    visibleWorkspaces.length === 0 && (
                      <p style={styles.workspaceMenuMessage}>No matching workspaces</p>
                    )}

                  {workspaceSearchError && (
                    <p style={styles.workspaceMenuError}>{workspaceSearchError}</p>
                  )}

                  <InfiniteScrollSentinel
                    hasMore={workspaceResultsHaveMore}
                    loading={
                      workspaceSearchActive ? workspaceSearchLoadingMore : workspacesLoadingMore
                    }
                    onLoadMore={() => {
                      if (workspaceSearchActive) {
                        void loadMoreWorkspaceSearchResults();
                      } else {
                        onLoadMoreWorkspaces?.();
                      }
                    }}
                    label="Loading more workspaces…"
                  />
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {workspace ? (
        <div style={styles.searchInputWrapper}>
          <input
            data-1p-ignore="true"
            autoComplete="off"
            ref={mainSearchRef}
            type="search"
            value={searchQuery}
            onChange={(event) => onSearchQueryChange?.(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.currentTarget.blur();
              }
            }}
            placeholder="Search Kival"
            aria-label="Search workspace"
            style={styles.searchInput}
          />
          {!searchQuery && (
            <span aria-hidden="true">
              <kbd style={styles.searchShortcutKey}>/</kbd>
            </span>
          )}
        </div>
      ) : (
        <div />
      )}

      <div style={styles.topBarRight}>
        <div ref={createMenuRef} style={styles.createMenu}>
          <button
            type="button"
            style={styles.topBarIconButton}
            aria-label="Create new"
            aria-haspopup="dialog"
            aria-expanded={createMenuOpen}
            onClick={() => {
              if (createMenuOpen) {
                closeCreateMenu();
              } else {
                closeWorkspaceMenu();
                setCreateMenuOpen(true);
              }
            }}
          >
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path
                d="M12 5v14M5 12h14"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
              />
            </svg>
            <svg width="9" height="9" viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path
                d="m6 9 6 6 6-6"
                stroke="currentColor"
                strokeWidth="2.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </button>

          {createMenuOpen && (
            <div
              role="dialog"
              aria-label={createMenuView === "root" ? "Create new" : "Choose a workspace"}
              style={styles.createMenuPopover}
            >
              {createMenuView === "root" ? (
                <>
                  {workspace && (
                    <button
                      type="button"
                      style={styles.createMenuItem}
                      onClick={() => createObject(workspace.id)}
                    >
                      <span style={styles.createMenuItemText}>
                        <strong>New object</strong>
                        <span style={styles.createMenuItemDescription}>{workspace.name}</span>
                      </span>
                    </button>
                  )}

                  <button
                    type="button"
                    style={styles.createMenuItem}
                    onClick={() => {
                      setCreateMenuView("workspace");
                      setWorkspaceQuery("");
                      setHighlightedWorkspaceId(null);
                    }}
                  >
                    <span>New object in…</span>
                    <span aria-hidden="true">›</span>
                  </button>

                  <div style={styles.createMenuDivider} />

                  <button type="button" style={styles.createMenuItem} onClick={createWorkspace}>
                    New workspace
                  </button>
                </>
              ) : (
                <>
                  <div style={styles.createMenuHeader}>
                    <button
                      type="button"
                      aria-label="Back to create menu"
                      style={styles.createMenuBackButton}
                      onClick={() => {
                        setCreateMenuView("root");
                        setWorkspaceQuery("");
                        setHighlightedWorkspaceId(null);
                      }}
                    >
                      ←
                    </button>
                    <span style={styles.createMenuLabel}>New object in…</span>
                  </div>

                  <input
                    data-1p-ignore="true"
                    autoComplete="off"
                    autoFocus
                    type="search"
                    value={workspaceQuery}
                    placeholder="Search workspaces…"
                    aria-label="Search workspaces"
                    role="combobox"
                    aria-autocomplete="list"
                    aria-controls="create-object-workspace-options"
                    aria-expanded="true"
                    aria-activedescendant={
                      highlightedWorkspaceId
                        ? `create-object-workspace-option-${highlightedWorkspaceId}`
                        : undefined
                    }
                    style={styles.workspaceMenuSearch}
                    onChange={(event) => {
                      setWorkspaceQuery(event.target.value);
                      setHighlightedWorkspaceId(null);
                    }}
                    onKeyDown={(event) => handleWorkspaceSearchKeyDown(event, createObject)}
                  />

                  <div
                    id="create-object-workspace-options"
                    role="listbox"
                    aria-label="Workspaces"
                    style={styles.createMenuList}
                  >
                    {workspaceSearching && (
                      <LoadingIndicator label="Searching workspaces…" compact />
                    )}

                    {!workspaceSearching &&
                      !workspaceSearchError &&
                      createWorkspaceOptions.map((candidate) => (
                        <button
                          key={candidate.id}
                          id={`create-object-workspace-option-${candidate.id}`}
                          type="button"
                          role="option"
                          tabIndex={-1}
                          aria-selected="false"
                          style={
                            candidate.id === highlightedWorkspaceId
                              ? styles.workspaceMenuItemHighlighted
                              : styles.workspaceMenuItem
                          }
                          onPointerMove={() => setHighlightedWorkspaceId(candidate.id)}
                          onClick={() => createObject(candidate.id)}
                        >
                          <span style={styles.workspaceMenuItemText}>
                            <span>{candidate.name}</span>
                            {candidate.description && (
                              <span style={styles.workspaceMenuItemDescription}>
                                {candidate.description}
                              </span>
                            )}
                          </span>
                        </button>
                      ))}

                    {!workspaceSearching &&
                      !workspaceSearchError &&
                      createWorkspaceOptions.length === 0 && (
                        <p style={styles.workspaceMenuMessage}>No matching workspaces</p>
                      )}

                    {workspaceSearchError && (
                      <p style={styles.workspaceMenuError}>{workspaceSearchError}</p>
                    )}

                    <InfiniteScrollSentinel
                      hasMore={workspaceResultsHaveMore}
                      loading={
                        workspaceSearchActive ? workspaceSearchLoadingMore : workspacesLoadingMore
                      }
                      onLoadMore={() => {
                        if (workspaceSearchActive) {
                          void loadMoreWorkspaceSearchResults();
                        } else {
                          onLoadMoreWorkspaces?.();
                        }
                      }}
                      label="Loading more workspaces…"
                    />
                  </div>
                </>
              )}
            </div>
          )}
        </div>

        <button
          type="button"
          style={styles.topBarIconButton}
          aria-label={
            unreadInboxCount > 0
              ? `Inbox, ${unreadInboxCount} unread notification${unreadInboxCount === 1 ? "" : "s"}`
              : "Inbox"
          }
          title="Inbox"
          onClick={() => {
            if (onInboxClick) {
              onInboxClick();
            } else {
              navigate("/inbox");
            }
          }}
        >
          <svg width="17" height="17" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <path
              d="M3.35 2.25h9.3l2.6 6.35v4.15a1 1 0 0 1-1 1H1.75a1 1 0 0 1-1-1V8.6l2.6-6.35Z"
              stroke="currentColor"
              strokeWidth="1.25"
              strokeLinejoin="round"
            />
            <path
              d="M.9 8.6h3.55l1.4 1.8h4.3l1.4-1.8h3.55"
              stroke="currentColor"
              strokeWidth="1.25"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
          {unreadInboxCount > 0 && (
            <span style={styles.topBarUnreadBadge} aria-hidden="true">
              {unreadInboxCount > 99 ? "99+" : unreadInboxCount}
            </span>
          )}
        </button>

        {user && (
          <ProfileMenu
            user={user}
            onSecurityClick={onSecurityClick}
            onApiKeysClick={onApiKeysClick}
            onLogout={onLogout}
          />
        )}
      </div>
    </header>
  );
}
