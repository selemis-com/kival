import { useCallback, useMemo, useState } from "react";
import { useNavigate } from "react-router";
import { kival } from "../../shared/api";
import { formatTimestampOr } from "../../shared/format";
import { usePaginatedResource } from "../../shared/hooks/usePaginatedResource";
import { KivalSideBar } from "../../shared/navigation/KivalSideBar";
import { TopBar } from "../../shared/navigation/TopBar";
import { styles } from "../../shared/styles/index";
import type { InboxEntry, User, Workspace } from "../../shared/types";
import { AnimatedSelect } from "../../shared/ui/AnimatedSelect";
import { InfiniteScrollSentinel } from "../../shared/ui/InfiniteScrollSentinel";
import { LoadingIndicator } from "../../shared/ui/LoadingIndicator";

type Props = {
  user: User;
  workspaces: Workspace[];
  workspacesNextCursor: string | null;
  workspacesLoadingMore: boolean;
  unreadCount: number;
  inboxRevision: number;
  onLoadMoreWorkspaces: () => void;
  onHome: () => void;
  onWorkspaceSelect: (workspaceId: string) => void;
  onInboxClick: () => void;
  onUsersClick?: () => void;
  onGroupsClick?: () => void;
  onEventsClick?: () => void;
  onSecurityClick: () => void;
  onApiKeysClick: () => void;
  onInboxChanged: () => Promise<void>;
  onLogout: () => Promise<void>;
};

type InboxFilter = "all" | "unread";

function formatCompactTimestamp(value: string) {
  const timestamp = new Date(value);
  const elapsed = Date.now() - timestamp.getTime();

  if (Number.isNaN(timestamp.getTime()) || elapsed < 0) {
    return formatTimestampOr(value, "Unknown time");
  }

  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) {
    return "now";
  }
  if (minutes < 60) {
    return `${minutes}m`;
  }

  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours}h`;
  }

  const days = Math.floor(hours / 24);
  if (days < 7) {
    return `${days}d`;
  }

  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(timestamp);
}

function compactCommentExcerpt(value: string) {
  return value.replaceAll(/\s+/g, " ").trim();
}

function NotificationReasonIcon({ reason }: { reason: InboxEntry["reason"] }) {
  if (reason === "mention") {
    return <span style={styles.inboxReasonAt}>@</span>;
  }

  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      {reason === "reply" ? (
        <path
          d="m9 17-5-5 5-5m-5 5h9a7 7 0 0 1 7 7"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      ) : reason === "review_requested" ? (
        <>
          <path
            d="M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6-9.5-6-9.5-6Z"
            stroke="currentColor"
            strokeWidth="2"
          />
          <circle cx="12" cy="12" r="2.5" fill="currentColor" />
        </>
      ) : (
        <path
          d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9ZM10 21h4"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      )}
    </svg>
  );
}

function ReadStateIcon({ unread }: { unread: boolean }) {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      {unread ? (
        <path
          d="m5 12 4 4L19 6"
          stroke="currentColor"
          strokeWidth="2.25"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      ) : (
        <>
          <rect x="3" y="5" width="18" height="14" rx="2" stroke="currentColor" strokeWidth="2" />
          <path d="m4 7 8 6 8-6" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
        </>
      )}
    </svg>
  );
}

function notificationCopy(entry: InboxEntry) {
  const actor = entry.actor_username ?? "Someone";
  const count = entry.event_count;
  const objectTitle = entry.object_title ?? "an object";

  switch (entry.reason) {
    case "workspace_access_granted":
      return {
        title: `${actor} granted you workspace access`,
        detail: entry.workspace_name,
      };
    case "object_access_granted":
      return {
        title: `${actor} granted you access to ${objectTitle}`,
        detail: "Object access",
      };
    case "mention":
      return {
        title: `${actor} mentioned you`,
        detail: `In ${objectTitle}`,
      };
    case "reply":
      return {
        title: `${actor} replied to you`,
        detail: `In ${objectTitle}`,
      };
    case "review_requested":
      return {
        title: `${actor} requested your review`,
        detail: objectTitle,
      };
    case "watcher_added":
      return {
        title: `${actor} added you as a watcher`,
        detail: objectTitle,
      };
    default:
      if (entry.notification_type === "comment.created") {
        return {
          title: `${actor} commented on ${objectTitle}`,
          detail: "New commentary",
        };
      }
      if (entry.notification_type === "comment.replied") {
        return {
          title: `${actor} replied on ${objectTitle}`,
          detail: "New commentary reply",
        };
      }
      if (entry.notification_type === "comment_thread.resolved") {
        return {
          title: `${actor} resolved a thread`,
          detail: `On ${objectTitle}`,
        };
      }
      if (entry.notification_type === "comment_thread.reopened") {
        return {
          title: `${actor} reopened a thread`,
          detail: `On ${objectTitle}`,
        };
      }
      return {
        title: count > 1 ? `${count} updates on ${objectTitle}` : `${actor} updated ${objectTitle}`,
        detail: count > 1 ? `Latest activity from ${actor}` : "Object activity",
      };
  }
}

function objectPath(entry: InboxEntry) {
  if (!entry.object_id) {
    return `/w/${entry.workspace_id}`;
  }

  const parameters = new URLSearchParams();

  if (entry.comment_id) {
    parameters.set("comment", entry.comment_id);
  }
  if (entry.thread_id) {
    parameters.set("thread", entry.thread_id);
  }

  const query = parameters.toString();
  return `/w/${entry.workspace_id}/objects/${entry.object_id}${query ? `?${query}` : ""}`;
}

export function InboxPage({
  user,
  workspaces,
  workspacesNextCursor,
  workspacesLoadingMore,
  unreadCount,
  inboxRevision,
  onLoadMoreWorkspaces,
  onHome,
  onWorkspaceSelect,
  onInboxClick,
  onUsersClick,
  onGroupsClick,
  onEventsClick,
  onSecurityClick,
  onApiKeysClick,
  onInboxChanged,
  onLogout,
}: Props) {
  const navigate = useNavigate();
  const [filter, setFilter] = useState<InboxFilter>("unread");
  const [workspaceId, setWorkspaceId] = useState<string>("");
  const [updatingId, setUpdatingId] = useState<string | null>(null);
  const [markingAllRead, setMarkingAllRead] = useState(false);
  const queryKey = `${filter}:${workspaceId}:${inboxRevision}`;
  const workspaceNames = useMemo(
    () => new Map(workspaces.map((workspace) => [workspace.id, workspace.name])),
    [workspaces],
  );

  const loadInboxPage = useCallback(
    async (cursor: string | null, signal: AbortSignal) => {
      const response = await kival.listInbox({
        cursor,
        unread_only: filter === "unread",
        workspace_id: workspaceId || undefined,
        signal,
      });
      return { items: response.items, nextCursor: response.next_cursor ?? null };
    },
    [filter, workspaceId],
  );
  const { items, setItems, nextCursor, loading, loadingMore, error, setError, loadMore } =
    usePaginatedResource({
      queryKey,
      loadPage: loadInboxPage,
      errorMessage: "Could not load your inbox.",
      itemKey: (entry: InboxEntry) => entry.id,
      clearItemsOnError: true,
    });

  async function updateReadState(entry: InboxEntry, read: boolean) {
    setUpdatingId(entry.id);
    setError(null);

    try {
      const updated = await kival.updateInboxEntry({
        inboxEntryId: entry.id,
        input: { read },
      });
      setItems((current) => {
        if (filter === "unread" && read) {
          return current.filter((candidate) => candidate.id !== entry.id);
        }
        return current.map((candidate) => (candidate.id === entry.id ? updated : candidate));
      });
      await onInboxChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not update the inbox entry.");
    } finally {
      setUpdatingId(null);
    }
  }

  async function openEntry(entry: InboxEntry) {
    if (!entry.read_at) {
      try {
        await kival.updateInboxEntry({
          inboxEntryId: entry.id,
          input: { read: true },
        });
        await onInboxChanged();
      } catch {
        // Navigation remains useful even when read-state persistence fails.
      }
    }

    navigate(objectPath(entry));
  }

  async function markAllRead() {
    setMarkingAllRead(true);
    setError(null);

    try {
      await kival.markInboxRead({
        input: {
          workspace_id: workspaceId || null,
          through_sequence: items[0]?.sequence_number ?? null,
        },
      });
      setItems((current) =>
        filter === "unread"
          ? []
          : current.map((entry) => ({
              ...entry,
              read_at: entry.read_at ?? new Date().toISOString(),
            })),
      );
      await onInboxChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not mark inbox entries read.");
    } finally {
      setMarkingAllRead(false);
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
        unreadInboxCount={unreadCount}
        onSecurityClick={onSecurityClick}
        onApiKeysClick={onApiKeysClick}
        onLogout={onLogout}
      />

      <div style={styles.kivalShell}>
        <KivalSideBar
          active="inbox"
          onWorkspacesClick={onHome}
          onUsersClick={onUsersClick}
          onGroupsClick={onGroupsClick}
          onEventsClick={onEventsClick}
          onSecurityClick={onSecurityClick}
          onApiKeysClick={onApiKeysClick}
        />

        <main style={styles.inboxPage}>
          <div style={styles.inboxContent}>
            <div style={styles.pageHeader}>
              <p style={styles.eyebrow}>Personal</p>
              <h1 style={styles.pageTitle}>Inbox</h1>
              <p style={styles.muted}>Attention-worthy activity and new access granted to you.</p>
            </div>

            <div style={styles.inboxToolbar}>
              <div style={styles.inboxFilters}>
                <label htmlFor="inbox-filter" style={{ ...styles.field, minWidth: 170 }}>
                  <span style={styles.fieldLabel}>Show</span>
                  <AnimatedSelect
                    id="inbox-filter"
                    value={filter}
                    style={styles.input}
                    onChange={(event) => setFilter(event.target.value as InboxFilter)}
                  >
                    <option value="all">All activity</option>
                    <option value="unread">Unread only</option>
                  </AnimatedSelect>
                </label>

                <label htmlFor="inbox-workspace" style={{ ...styles.field, minWidth: 230 }}>
                  <span style={styles.fieldLabel}>Workspace</span>
                  <AnimatedSelect
                    id="inbox-workspace"
                    value={workspaceId}
                    style={styles.input}
                    onChange={(event) => setWorkspaceId(event.target.value)}
                  >
                    <option value="">All workspaces</option>
                    {workspaces.map((workspace) => (
                      <option key={workspace.id} value={workspace.id}>
                        {workspace.name}
                      </option>
                    ))}
                  </AnimatedSelect>
                </label>
              </div>

              <button
                type="button"
                style={styles.secondaryButton}
                disabled={markingAllRead || unreadCount === 0}
                onClick={() => void markAllRead()}
              >
                {markingAllRead ? "Marking read…" : "Mark all read"}
              </button>
            </div>

            {loading && items.length === 0 && <LoadingIndicator label="Loading inbox…" />}

            {!loading && error && (
              <div style={styles.errorBox} role="alert">
                <strong>Could not load inbox</strong>
                <span>{error}</span>
              </div>
            )}

            {!loading && !error && items.length === 0 && (
              <div style={styles.inboxEmpty}>
                <strong>{filter === "unread" ? "Nothing unread" : "Your inbox is clear"}</strong>
                <span>
                  Relevant mentions, replies, access grants, and object activity will appear here.
                </span>
              </div>
            )}

            {!error && items.length > 0 && (
              <div className="kival-row-list" style={styles.inboxList}>
                {items.map((entry) => {
                  const copy = notificationCopy(entry);
                  const workspaceName =
                    workspaceNames.get(entry.workspace_id) ?? entry.workspace_name;
                  const unread = entry.read_at === null;
                  const detail = entry.comment_excerpt
                    ? `${copy.detail} · ${compactCommentExcerpt(entry.comment_excerpt)}`
                    : copy.detail;

                  return (
                    <article
                      key={entry.id}
                      style={unread ? styles.inboxItemUnread : styles.inboxItem}
                    >
                      <button
                        type="button"
                        style={unread ? styles.inboxItemMain : styles.inboxItemMainRead}
                        onClick={() => void openEntry(entry)}
                      >
                        <span
                          style={unread ? styles.inboxReasonIconUnread : styles.inboxReasonIcon}
                          aria-hidden="true"
                        >
                          <NotificationReasonIcon reason={entry.reason} />
                        </span>
                        <span style={styles.inboxItemCopy}>
                          <span style={styles.inboxItemHeading}>
                            {unread && <span style={styles.inboxUnreadDot} aria-hidden="true" />}
                            <strong style={styles.inboxItemTitle}>{copy.title}</strong>
                          </span>
                          <span style={styles.inboxItemDetail}>{detail}</span>
                          <span style={styles.inboxItemMeta}>{workspaceName}</span>
                        </span>
                      </button>

                      <div style={styles.inboxItemActions}>
                        <time
                          dateTime={entry.updated_at}
                          title={formatTimestampOr(entry.updated_at, "Unknown time")}
                          style={styles.inboxItemTime}
                        >
                          {formatCompactTimestamp(entry.updated_at)}
                        </time>
                        <button
                          type="button"
                          style={styles.inboxReadButton}
                          disabled={updatingId === entry.id}
                          aria-label={
                            unread ? `Mark ${copy.title} read` : `Mark ${copy.title} unread`
                          }
                          title={unread ? "Mark read" : "Mark unread"}
                          onClick={() => void updateReadState(entry, unread)}
                        >
                          {updatingId === entry.id ? (
                            <span style={styles.inboxSavingDots}>•••</span>
                          ) : (
                            <ReadStateIcon unread={unread} />
                          )}
                        </button>
                      </div>
                    </article>
                  );
                })}

                <InfiniteScrollSentinel
                  hasMore={Boolean(nextCursor)}
                  loading={loadingMore}
                  onLoadMore={() => void loadMore()}
                />
              </div>
            )}
          </div>
        </main>
      </div>
    </div>
  );
}
