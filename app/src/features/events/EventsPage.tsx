import { useCallback, useMemo, useState } from "react";
import { kival } from "../../shared/api";
import { formatEventKind, formatTimestampOr } from "../../shared/format";
import { usePaginatedResource } from "../../shared/hooks/usePaginatedResource";
import { KivalSideBar } from "../../shared/navigation/KivalSideBar";
import { TopBar } from "../../shared/navigation/TopBar";
import { styles } from "../../shared/styles/index";
import type { Event, User, Workspace } from "../../shared/types";
import { CopyableId } from "../../shared/ui/CopyableId";
import { InfiniteScrollSentinel } from "../../shared/ui/InfiniteScrollSentinel";
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
  onUsersClick: () => void;
  onGroupsClick?: () => void;
  onEventsClick: () => void;
  onSecurityClick: () => void;
  onApiKeysClick: () => void;
  onLogout: () => Promise<void>;
};

type GlobalEventFilters = {
  eventKind?: string;
  actorUserId?: string;
  targetUserId?: string;
  objectId?: string;
  groupId?: string;
};

const emptyFilters: GlobalEventFilters = {};

function shortId(value: string) {
  return `${value.slice(0, 8)}…`;
}

function normalizedFilters(filters: GlobalEventFilters): GlobalEventFilters {
  return Object.fromEntries(
    Object.entries(filters)
      .map(([key, value]) => [key, value?.trim()])
      .filter((entry): entry is [string, string] => Boolean(entry[1])),
  );
}

export function EventsPage({
  user,
  workspaces,
  workspacesNextCursor,
  workspacesLoadingMore,
  onLoadMoreWorkspaces,
  onHome,
  onInboxClick,
  unreadInboxCount,
  onUsersClick,
  onGroupsClick,
  onEventsClick,
  onSecurityClick,
  onApiKeysClick,
  onLogout,
}: Props) {
  const [filters, setFilters] = useState<GlobalEventFilters>(emptyFilters);
  const [draftFilters, setDraftFilters] = useState<GlobalEventFilters>(emptyFilters);
  const loadEventPage = useCallback(
    async (beforeSequence: number | null, signal: AbortSignal) => {
      const response = await kival.listEvents({
        limit: 25,
        order: "desc",
        before_sequence: beforeSequence,
        event_kind: filters.eventKind?.trim() || undefined,
        actor_user_id: filters.actorUserId?.trim() || undefined,
        target_user_id: filters.targetUserId?.trim() || undefined,
        object_id: filters.objectId?.trim() || undefined,
        group_id: filters.groupId?.trim() || undefined,
        signal,
      });
      const nextCursor =
        response.items.length === 25 ? (response.items.at(-1)?.sequence_number ?? null) : null;
      return { items: response.items, nextCursor };
    },
    [filters],
  );
  const {
    items: events,
    nextCursor,
    loading,
    loadingMore,
    error,
    loadMore: loadOlderEvents,
  } = usePaginatedResource<Event, number>({
    queryKey: JSON.stringify(filters),
    loadPage: loadEventPage,
    errorMessage: "Could not load global events.",
    itemKey: (event) => event.id,
    clearItemsOnError: true,
  });
  const hasMore = nextCursor !== null;
  const workspaceNames = useMemo(
    () => new Map(workspaces.map((workspace) => [workspace.id, workspace.name])),
    [workspaces],
  );

  function updateDraftFilter(key: keyof GlobalEventFilters, value: string) {
    setDraftFilters((current) => ({ ...current, [key]: value }));
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
          active="events"
          onWorkspacesClick={onHome}
          onUsersClick={onUsersClick}
          onGroupsClick={onGroupsClick}
          onEventsClick={onEventsClick}
          onSecurityClick={onSecurityClick}
          onApiKeysClick={onApiKeysClick}
        />

        <main style={styles.eventsPage}>
          <div style={styles.eventsContent}>
            <header style={styles.pageHeader}>
              <p style={styles.eyebrow}>Global administration</p>
              <h1 style={styles.pageTitle}>Events</h1>
              <p style={styles.muted}>
                Security, administration, and content events across Kival, newest first.
              </p>
            </header>

            <form
              style={styles.eventsFilterForm}
              onSubmit={(event) => {
                event.preventDefault();
                setFilters(normalizedFilters(draftFilters));
              }}
            >
              <div style={styles.settingsSectionHeader}>
                <strong>Filters</strong>
                <span style={styles.muted}>Values are matched exactly.</span>
              </div>
              <div style={styles.eventsFilterGrid}>
                <label style={styles.field}>
                  <span>Event kind</span>
                  <input
                    data-1p-ignore="true"
                    autoComplete="off"
                    value={draftFilters.eventKind ?? ""}
                    onChange={(event) => updateDraftFilter("eventKind", event.target.value)}
                    placeholder="auth.api_key_updated"
                    style={styles.input}
                  />
                </label>
                <label style={styles.field}>
                  <span>Actor user ID</span>
                  <input
                    data-1p-ignore="true"
                    autoComplete="off"
                    value={draftFilters.actorUserId ?? ""}
                    onChange={(event) => updateDraftFilter("actorUserId", event.target.value)}
                    placeholder="UUID"
                    style={styles.input}
                  />
                </label>
                <label style={styles.field}>
                  <span>Target user ID</span>
                  <input
                    data-1p-ignore="true"
                    autoComplete="off"
                    value={draftFilters.targetUserId ?? ""}
                    onChange={(event) => updateDraftFilter("targetUserId", event.target.value)}
                    placeholder="UUID"
                    style={styles.input}
                  />
                </label>
                <label style={styles.field}>
                  <span>Object ID</span>
                  <input
                    data-1p-ignore="true"
                    autoComplete="off"
                    value={draftFilters.objectId ?? ""}
                    onChange={(event) => updateDraftFilter("objectId", event.target.value)}
                    placeholder="UUID"
                    style={styles.input}
                  />
                </label>
                <label style={styles.field}>
                  <span>Group ID</span>
                  <input
                    data-1p-ignore="true"
                    autoComplete="off"
                    value={draftFilters.groupId ?? ""}
                    onChange={(event) => updateDraftFilter("groupId", event.target.value)}
                    placeholder="UUID"
                    style={styles.input}
                  />
                </label>
              </div>
              <div style={styles.modalActions}>
                <button
                  type="button"
                  style={styles.secondaryButton}
                  onClick={() => {
                    setDraftFilters(emptyFilters);
                    setFilters(emptyFilters);
                  }}
                >
                  Clear
                </button>
                <button type="submit" style={styles.primaryButtonCompact}>
                  Apply filters
                </button>
              </div>
            </form>

            {error && (
              <div style={{ ...styles.errorBox, marginTop: 20 }} role="alert">
                <strong>Could not load events</strong>
                <span>{error}</span>
              </div>
            )}

            {loading ? (
              <LoadingIndicator label="Loading events…" />
            ) : (
              <section style={styles.eventsList} aria-label="Global events">
                {events.map((event) => {
                  const actor = event.actor_username
                    ? event.actor_username
                    : event.actor_user_id
                      ? "Unknown user"
                      : "System";
                  const actorLabel = event.api_key_label
                    ? `${actor} via ${event.api_key_label}`
                    : actor;
                  const context = [
                    event.workspace_id
                      ? {
                          key: "workspace",
                          value: event.workspace_id,
                          label: "workspace ID",
                          displayValue: `Workspace: ${workspaceNames.get(event.workspace_id) ?? shortId(event.workspace_id)}`,
                        }
                      : null,
                    event.object_id
                      ? {
                          key: "object",
                          value: event.object_id,
                          label: "object ID",
                          displayValue: `Object: ${shortId(event.object_id)}`,
                        }
                      : null,
                    event.group_id
                      ? {
                          key: "group",
                          value: event.group_id,
                          label: "group ID",
                          displayValue: `Group: ${shortId(event.group_id)}`,
                        }
                      : null,
                    event.target_user_id
                      ? {
                          key: "target-user",
                          value: event.target_user_id,
                          label: "user ID",
                          displayValue: `Target user: ${shortId(event.target_user_id)}`,
                        }
                      : null,
                    event.object_edge_id
                      ? {
                          key: "edge",
                          value: event.object_edge_id,
                          label: "edge ID",
                          displayValue: `Edge: ${shortId(event.object_edge_id)}`,
                        }
                      : null,
                    event.object_grant_id
                      ? {
                          key: "grant",
                          value: event.object_grant_id,
                          label: "grant ID",
                          displayValue: `Grant: ${shortId(event.object_grant_id)}`,
                        }
                      : null,
                  ].filter((value): value is NonNullable<typeof value> => Boolean(value));

                  return (
                    <article key={event.id} style={styles.eventCard}>
                      <div style={styles.eventCardHeader}>
                        <div style={styles.eventIdentity}>
                          <strong>{formatEventKind(event.event_kind)}</strong>
                          <span style={styles.eventMeta}>
                            {actorLabel} · {formatTimestampOr(event.created_at, "Unknown time")}
                          </span>
                        </div>
                        <span style={styles.eventSequence}>#{event.sequence_number}</span>
                      </div>

                      {context.length > 0 && (
                        <div style={styles.eventContext}>
                          {context.map((item) => (
                            <CopyableId
                              key={item.key}
                              value={item.value}
                              displayValue={item.displayValue}
                              label={item.label}
                              style={styles.eventContextPill}
                            />
                          ))}
                        </div>
                      )}

                      <details>
                        <summary>Event details</summary>
                        <pre style={styles.eventPayload}>
                          {JSON.stringify(event.payload, null, 2)}
                        </pre>
                      </details>
                    </article>
                  );
                })}

                {events.length === 0 && (
                  <div style={styles.emptyState}>
                    <strong>No events found</strong>
                    <span>Try clearing or changing the current filters.</span>
                  </div>
                )}
              </section>
            )}

            {!loading && (
              <InfiniteScrollSentinel
                hasMore={hasMore}
                loading={loadingMore}
                onLoadMore={() => void loadOlderEvents()}
                label="Loading older events…"
              />
            )}
          </div>
        </main>
      </div>
    </div>
  );
}
