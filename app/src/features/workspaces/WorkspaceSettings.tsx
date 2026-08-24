import { KivalTransportError } from "kival-sdk";
import { useEffect, useRef, useState } from "react";
import { kival } from "../../shared/api";
import { submitFormOnEnter } from "../../shared/forms";
import { styles } from "../../shared/styles/index";
import type { Event, UpdateWorkspaceRequest, Workspace } from "../../shared/types";
import { ConfirmationDialog } from "../../shared/ui/ConfirmationDialog";
import { LoadingIndicator } from "../../shared/ui/LoadingIndicator";

type Props = {
  workspace: Workspace;
  loading: boolean;
  onSave: (input: UpdateWorkspaceRequest) => Promise<void>;
  onArchive: () => Promise<void>;
};

function formatTimestamp(value: string) {
  const date = new Date(value);

  if (Number.isNaN(date.getTime())) {
    return "Unknown time";
  }

  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

function formatEventKind(kind: string) {
  return kind
    .replace(/^workspace\./, "")
    .replaceAll("_", " ")
    .replace(/\b\w/g, (character) => character.toUpperCase());
}

export function WorkspaceSettings({ workspace, loading, onSave, onArchive }: Props) {
  const [name, setName] = useState(workspace.name);
  const [description, setDescription] = useState(workspace.description ?? "");
  const [error, setError] = useState<string | null>(null);
  const [events, setEvents] = useState<Event[]>([]);
  const [eventsLoading, setEventsLoading] = useState(true);
  const [eventsLoadingMore, setEventsLoadingMore] = useState(false);
  const [eventsError, setEventsError] = useState<string | null>(null);
  const [hasMoreEvents, setHasMoreEvents] = useState(false);
  const [archiveConfirmOpen, setArchiveConfirmOpen] = useState(false);
  const [archiving, setArchiving] = useState(false);
  const eventsGenerationRef = useRef(0);

  useEffect(() => {
    setName(workspace.name);
    setDescription(workspace.description ?? "");
    setError(null);
  }, [workspace]);

  useEffect(() => {
    const controller = new AbortController();
    const generation = ++eventsGenerationRef.current;
    setEventsLoading(true);
    setEventsLoadingMore(false);
    setEventsError(null);

    void kival
      .listWorkspaceEvents({
        workspaceId: workspace.id,
        limit: 10,
        order: "desc",
        signal: controller.signal,
      })
      .then((response) => {
        if (generation !== eventsGenerationRef.current) {
          return;
        }
        setEvents(response.items);
        setHasMoreEvents(response.items.length === 10);
      })
      .catch((cause: unknown) => {
        if (
          (cause instanceof KivalTransportError && cause.kind === "abort") ||
          generation !== eventsGenerationRef.current
        ) {
          return;
        }
        setEventsError(cause instanceof Error ? cause.message : "Could not load events.");
      })
      .finally(() => {
        if (!controller.signal.aborted && generation === eventsGenerationRef.current) {
          setEventsLoading(false);
        }
      });

    return () => controller.abort();
  }, [workspace.id]);

  const normalizedName = name.trim();
  const normalizedDescription = description.trim() || null;
  const changed =
    normalizedName !== workspace.name || normalizedDescription !== workspace.description;

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();

    if (!normalizedName) {
      setError("Workspace name is required.");
      return;
    }

    const input: UpdateWorkspaceRequest = {};

    if (normalizedName !== workspace.name) {
      input.name = normalizedName;
    }

    if (normalizedDescription !== workspace.description) {
      input.description = normalizedDescription;
    }

    if (Object.keys(input).length === 0) {
      return;
    }

    setError(null);

    try {
      await onSave(input);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  async function loadOlderEvents() {
    const beforeSequence = events.at(-1)?.sequence_number;

    if (!beforeSequence || eventsLoadingMore) {
      return;
    }

    const generation = eventsGenerationRef.current;

    setEventsLoadingMore(true);
    setEventsError(null);

    try {
      const response = await kival.listWorkspaceEvents({
        workspaceId: workspace.id,
        limit: 10,
        order: "desc",
        before_sequence: beforeSequence,
      });
      if (generation !== eventsGenerationRef.current) {
        return;
      }
      setEvents((current) => [
        ...current,
        ...response.items.filter(
          (event) => !current.some((candidate) => candidate.id === event.id),
        ),
      ]);
      setHasMoreEvents(response.items.length === 10);
    } catch (cause) {
      if (generation === eventsGenerationRef.current) {
        setEventsError(cause instanceof Error ? cause.message : "Could not load older events.");
      }
    } finally {
      if (generation === eventsGenerationRef.current) {
        setEventsLoadingMore(false);
      }
    }
  }

  async function handleArchive() {
    setArchiving(true);
    setError(null);

    try {
      await onArchive();
      setArchiveConfirmOpen(false);
      setArchiving(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not archive this workspace.");
      setArchiving(false);
    }
  }

  return (
    <>
      <div style={styles.pageHeader}>
        <p style={styles.eyebrow}>Workspace</p>
        <h1 style={styles.pageTitle}>Settings</h1>
        <p style={styles.muted}>Manage this workspace and its events.</p>
      </div>

      <div style={styles.settingsForm}>
        <form style={styles.settingsSection} onSubmit={(event) => void handleSubmit(event)}>
          <div style={styles.settingsSectionHeader}>
            <h2 style={styles.sectionTitle}>General</h2>
            <p style={styles.muted}>The name and description shown throughout Kival.</p>
          </div>

          <label style={styles.field}>
            <span>Name</span>
            <input
              data-1p-ignore="true"
              value={name}
              onChange={(event) => setName(event.target.value)}
              style={styles.input}
              autoComplete="off"
            />
          </label>

          <label style={styles.field}>
            <span>Description</span>
            <textarea
              data-1p-ignore="true"
              autoComplete="off"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              onKeyDown={submitFormOnEnter}
              style={styles.settingsTextarea}
              rows={4}
              placeholder="Optional workspace description"
            />
          </label>

          {error && (
            <div style={styles.errorBox}>
              <strong>Workspace action failed</strong>
              <span>{error}</span>
            </div>
          )}

          <div style={styles.settingsActions}>
            <button
              type="submit"
              style={styles.primaryButtonCompact}
              disabled={loading || !changed || !normalizedName}
            >
              {loading ? "Saving…" : "Save changes"}
            </button>
          </div>
        </form>

        <section style={styles.settingsSection}>
          <div style={styles.settingsSectionHeader}>
            <h2 style={styles.sectionTitle}>Events</h2>
            <p style={styles.muted}>
              Administrative and content events recorded in this workspace.
            </p>
          </div>

          {eventsLoading && <LoadingIndicator label="Loading events…" compact />}
          {!eventsLoading && eventsError && (
            <div style={styles.errorBox}>
              <strong>Could not load workspace events</strong>
              <span>{eventsError}</span>
            </div>
          )}
          {!eventsLoading && (
            <div style={styles.objectActivityList}>
              {events.map((event) => (
                <div key={event.id} style={styles.objectActivityItem}>
                  <div style={styles.objectActivityMain}>
                    <strong>{formatEventKind(event.event_kind)}</strong>
                    <span style={styles.objectActivityMeta}>
                      {event.actor_username ?? (event.actor_user_id ? "Unknown user" : "System")}
                      {" · "}
                      {formatTimestamp(event.created_at)}
                    </span>
                  </div>
                </div>
              ))}
              {events.length === 0 && (
                <div style={styles.emptyState}>
                  <strong>No events found</strong>
                  <span>Workspace events will appear here.</span>
                </div>
              )}
            </div>
          )}

          {hasMoreEvents && (
            <div style={styles.settingsActions}>
              <button
                type="button"
                style={styles.secondaryButtonCompact}
                disabled={eventsLoadingMore}
                onClick={() => void loadOlderEvents()}
              >
                {eventsLoadingMore ? "Loading…" : "Load older events"}
              </button>
            </div>
          )}
        </section>

        <section style={styles.settingsSection}>
          <div style={styles.settingsSectionHeader}>
            <h2 style={styles.sectionTitle}>Archive workspace</h2>
            <p style={styles.muted}>
              Archive this workspace and temporarily make its objects unavailable.
            </p>
          </div>
          <div style={styles.settingsActions}>
            <button
              type="button"
              style={styles.apiKeyDangerButtonSolid}
              disabled={archiving}
              onClick={() => {
                setArchiveConfirmOpen(true);
                setError(null);
              }}
            >
              Archive workspace
            </button>
          </div>
        </section>
      </div>

      {archiveConfirmOpen ? (
        <ConfirmationDialog
          title={`Archive “${workspace.name}”?`}
          description="Its objects will be unavailable until an administrator restores the workspace."
          confirmLabel="Archive workspace"
          pendingLabel="Archiving…"
          pending={archiving}
          error={error}
          errorTitle="Could not archive workspace"
          closeLabel="Cancel workspace archival"
          onCancel={() => {
            setArchiveConfirmOpen(false);
            setError(null);
          }}
          onConfirm={() => void handleArchive()}
        />
      ) : null}
    </>
  );
}
