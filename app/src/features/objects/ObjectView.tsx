import { KivalTransportError } from "kival-sdk";
import { useEffect, useRef, useState } from "react";
import { kival } from "../../shared/api";
import { formatEventKind, formatTimestamp } from "../../shared/format";
import { styles } from "../../shared/styles/index";
import type {
  CurrentObjectResponse,
  Event,
  ObjectContext,
  ObjectSummary,
  ObjectVersion,
  ObjectVersionWikilink,
  User,
} from "../../shared/types";
import { CopyableId } from "../../shared/ui/CopyableId";
import { InfiniteScrollSentinel } from "../../shared/ui/InfiniteScrollSentinel";
import { LoadingIndicator } from "../../shared/ui/LoadingIndicator";
import { ProfileHoverName } from "../../shared/ui/ProfileHoverCard";
import { CommentaryPanel } from "./components/CommentaryPanel";
import { MarkdownBody } from "./components/MarkdownBody";
import { ObjectShareAvatars } from "./components/ObjectShareAvatars";
import { ObjectShareDialog } from "./components/ObjectShareDialog";

type Props = {
  user: User;
  value: CurrentObjectResponse;
  initialVersionId?: string | null;
  initialCommentId?: string | null;
  initialThreadId?: string | null;
  objects: ObjectSummary[];
  context: ObjectContext | null;
  onOpenObject: (objectId: string) => void;
  backLabel: string;
  onBack: () => void;
  onEdit: () => void;
  onRevealInGraph: () => void;
  onArchive: () => Promise<void>;
  onUnarchive: () => Promise<void>;
  onAccessChanged: () => Promise<void>;
};

function formatEventActor(event: Event, user: User) {
  if (!event.actor_user_id) {
    return "System";
  }

  if (event.actor_user_id === user.id) {
    return event.actor_username ?? user.username;
  }

  return event.actor_username ?? "Unknown user";
}

function eventPayload(event: Event) {
  if (!event.payload || typeof event.payload !== "object" || Array.isArray(event.payload)) {
    return {};
  }

  return event.payload as Record<string, unknown>;
}

function findVersionNumber(event: Event, versions: ObjectVersion[]) {
  if (!event.object_version_id) {
    return null;
  }

  return versions.find((version) => version.id === event.object_version_id)?.version_number ?? null;
}

function findObjectTitle(
  objectId: string | null | undefined,
  objects: ObjectSummary[],
  context: ObjectContext | null,
) {
  if (!objectId) {
    return null;
  }

  return (
    context?.graph.nodes.find((node) => node.id === objectId)?.title ??
    objects.find((object) => object.id === objectId)?.title ??
    null
  );
}

function describeEvent(
  event: Event,
  versions: ObjectVersion[],
  objects: ObjectSummary[],
  context: ObjectContext | null,
) {
  const payload = eventPayload(event);
  const versionNumber = findVersionNumber(event, versions);

  switch (event.event_kind) {
    case "object.created":
      return {
        title: "Object created",
        detail: versionNumber ? `Version ${versionNumber}` : null,
      };

    case "object.updated":
      return {
        title: "Object updated",
        detail: versionNumber ? `Version ${versionNumber}` : null,
      };

    case "object.version.appended":
      return {
        title: "New version created",
        detail: versionNumber ? `Version ${versionNumber}` : null,
      };

    case "object.references.updated": {
      const resolved = typeof payload.resolved_count === "number" ? payload.resolved_count : null;
      const ambiguous =
        typeof payload.ambiguous_count === "number" ? payload.ambiguous_count : null;

      const parts: string[] = [];

      if (resolved !== null) {
        parts.push(`${resolved} ${resolved === 1 ? "reference" : "references"} resolved`);
      }

      if (ambiguous !== null && ambiguous > 0) {
        parts.push(`${ambiguous} ambiguous`);
      }

      return {
        title: "References updated",
        detail: parts.length > 0 ? parts.join(" · ") : null,
      };
    }

    case "object.edge.created": {
      const edge =
        context?.edges.items.find((candidate) => candidate.id === event.object_edge_id) ?? null;
      const targetObjectId =
        edge?.target_object_id ??
        (typeof payload.target_object_id === "string" ? payload.target_object_id : null);
      const targetTitle = findObjectTitle(targetObjectId, objects, context);

      return {
        title: "Connection created",
        detail: targetTitle ? `→ ${targetTitle}` : null,
      };
    }

    case "object.edge.revoked":
      return {
        title: "Connection removed",
        detail: null,
      };

    case "object.grant.created": {
      const role =
        typeof payload.object_role === "string"
          ? payload.object_role
          : typeof payload.role === "string"
            ? payload.role
            : typeof payload.access_role === "string"
              ? payload.access_role
              : null;

      if (event.group_id) {
        return {
          title: "Access granted",
          detail: role ? `${role} access granted to a group` : "Access granted to a group",
        };
      }

      if (event.target_user_id) {
        return {
          title: "Access granted",
          detail: role ? `${role} access granted to a user` : "Access granted to a user",
        };
      }

      return {
        title: "Access granted",
        detail: null,
      };
    }

    case "object.grant.revoked":
      return {
        title: "Access revoked",
        detail: null,
      };

    case "object.grant.updated": {
      const role = typeof payload.object_role === "string" ? payload.object_role : null;
      return {
        title: "Access level changed",
        detail: role ? `Access changed to ${role}` : null,
      };
    }

    case "object.archived":
      return {
        title: "Object archived",
        detail: null,
      };

    case "object.unarchived":
      return {
        title: "Object restored",
        detail: null,
      };

    default:
      return {
        title: formatEventKind(event.event_kind),
        detail: null,
      };
  }
}

function DisclosureIcon({ open }: { open: boolean }) {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
      style={{
        flexShrink: 0,
        transform: open ? "rotate(180deg)" : "rotate(0deg)",
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
  );
}

function BackIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
      style={{ flexShrink: 0 }}
    >
      <path
        d="m15 18-6-6 6-6"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function ObjectView({
  user,
  value,
  initialVersionId,
  initialCommentId,
  initialThreadId,
  objects,
  context,
  onOpenObject,
  backLabel,
  onBack,
  onEdit,
  onRevealInGraph,
  onArchive,
  onUnarchive,
  onAccessChanged,
}: Props) {
  const { object, current_version } = value;
  const [archiveConfirmOpen, setArchiveConfirmOpen] = useState(false);
  const [archiveLoading, setArchiveLoading] = useState(false);
  const [archiveError, setArchiveError] = useState<string | null>(null);
  const [unarchiveLoading, setUnarchiveLoading] = useState(false);
  const [unarchiveError, setUnarchiveError] = useState<string | null>(null);
  const [versions, setVersions] = useState<ObjectVersion[]>([]);
  const [versionsNextCursor, setVersionsNextCursor] = useState<string | null>(null);
  const [selectedVersionId, setSelectedVersionId] = useState<string | null>(null);
  const [versionsLoading, setVersionsLoading] = useState(false);
  const [versionsLoadingMore, setVersionsLoadingMore] = useState(false);
  const [versionsError, setVersionsError] = useState<string | null>(null);
  const [wikilinks, setWikilinks] = useState<ObjectVersionWikilink[]>([]);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [activityOpen, setActivityOpen] = useState(false);
  const [events, setEvents] = useState<Event[]>([]);
  const [eventsLoading, setEventsLoading] = useState(false);
  const [eventsError, setEventsError] = useState<string | null>(null);
  const [actionsOpen, setActionsOpen] = useState(false);
  const [shareOpen, setShareOpen] = useState(false);
  const [shareRevision, setShareRevision] = useState(0);
  const [ordinaryNotificationsEnabled, setOrdinaryNotificationsEnabled] = useState<boolean | null>(
    null,
  );
  const [notificationPreferenceLoading, setNotificationPreferenceLoading] = useState(false);
  const [notificationPreferenceError, setNotificationPreferenceError] = useState<string | null>(
    null,
  );
  const actionsRef = useRef<HTMLDivElement>(null);
  const versionsGenerationRef = useRef(0);
  const versionsScopeRef = useRef<string | null>(null);
  const versionsScope = `${object.workspace_id}:${object.id}`;

  useEffect(() => {
    const controller = new AbortController();
    const generation = ++versionsGenerationRef.current;
    versionsScopeRef.current = null;

    const requestedVersionId =
      initialVersionId && initialVersionId !== current_version.id ? initialVersionId : null;

    setSelectedVersionId(requestedVersionId);
    setVersionsLoading(true);
    setVersionsLoadingMore(false);
    setVersionsError(null);
    setVersionsNextCursor(null);

    void kival
      .listObjectVersions({
        workspaceId: object.workspace_id,
        objectId: object.id,
        signal: controller.signal,
      })
      .then((response) => {
        if (generation !== versionsGenerationRef.current) {
          return;
        }

        versionsScopeRef.current = versionsScope;
        setVersions((versions) => {
          const requestedVersion = requestedVersionId
            ? versions.find((version) => version.id === requestedVersionId)
            : null;

          if (
            requestedVersion &&
            !response.items.some((version) => version.id === requestedVersion.id)
          ) {
            return [requestedVersion, ...response.items];
          }

          return response.items;
        });
        setVersionsNextCursor(response.next_cursor ?? null);
      })
      .catch((error: unknown) => {
        if (
          (error instanceof KivalTransportError && error.kind === "abort") ||
          generation !== versionsGenerationRef.current
        ) {
          return;
        }

        setVersionsError(error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        if (!controller.signal.aborted && generation === versionsGenerationRef.current) {
          setVersionsLoading(false);
        }
      });

    if (requestedVersionId) {
      void kival
        .getObjectVersion({
          workspaceId: object.workspace_id,
          objectId: object.id,
          version: requestedVersionId,
          signal: controller.signal,
        })
        .then((version) => {
          if (generation !== versionsGenerationRef.current) {
            return;
          }

          setVersions((versions) =>
            versions.some((candidate) => candidate.id === version.id)
              ? versions
              : [version, ...versions],
          );
        })
        .catch((error: unknown) => {
          if (
            (error instanceof KivalTransportError && error.kind === "abort") ||
            generation !== versionsGenerationRef.current
          ) {
            return;
          }

          setSelectedVersionId(null);
          setVersionsError(error instanceof Error ? error.message : String(error));
        });
    }

    return () => {
      controller.abort();
    };
  }, [object.id, object.workspace_id, current_version.id, initialVersionId, versionsScope]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: A new current version can emit new object events.
  useEffect(() => {
    const controller = new AbortController();

    setEventsLoading(true);
    setEventsError(null);

    void kival
      .listObjectEvents({
        workspaceId: object.workspace_id,
        objectId: object.id,
        signal: controller.signal,
      })
      .then((response) => {
        setEvents([...response.items].reverse());
      })
      .catch((error: unknown) => {
        if (error instanceof KivalTransportError && error.kind === "abort") {
          return;
        }

        setEventsError(error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setEventsLoading(false);
        }
      });

    return () => {
      controller.abort();
    };
  }, [object.id, object.workspace_id, current_version.id]);

  const selectedVersion =
    versions.find((version) => version.id === selectedVersionId) ?? current_version;
  const isCurrentVersion = selectedVersion.id === current_version.id;
  const selectedMetadata =
    typeof selectedVersion.metadata === "object" &&
    selectedVersion.metadata !== null &&
    !Array.isArray(selectedVersion.metadata)
      ? selectedVersion.metadata
      : {};

  useEffect(() => {
    const controller = new AbortController();
    let active = true;

    setWikilinks([]);
    void kival
      .getObjectVersionWikilinks({
        workspaceId: object.workspace_id,
        objectId: object.id,
        version: selectedVersion.id,
        signal: controller.signal,
      })
      .then((references) => {
        if (active) {
          setWikilinks(references);
        }
      })
      .catch((error: unknown) => {
        if (error instanceof KivalTransportError && error.kind === "abort") {
          return;
        }

        if (active) {
          setWikilinks([]);
        }
      });

    return () => {
      active = false;
      controller.abort();
    };
  }, [object.id, object.workspace_id, selectedVersion.id]);

  useEffect(() => {
    if (object.status !== "active") {
      setOrdinaryNotificationsEnabled(null);
      setNotificationPreferenceError(null);
      return;
    }

    const controller = new AbortController();
    setOrdinaryNotificationsEnabled(null);
    setNotificationPreferenceError(null);

    void kival
      .getObjectNotificationPreference({
        workspaceId: object.workspace_id,
        objectId: object.id,
        signal: controller.signal,
      })
      .then((preference) => {
        setOrdinaryNotificationsEnabled(preference.ordinary_notifications_enabled);
      })
      .catch((cause: unknown) => {
        if (cause instanceof KivalTransportError && cause.kind === "abort") {
          return;
        }
        setNotificationPreferenceError(
          cause instanceof Error ? cause.message : "Could not load notification preference.",
        );
      });

    return () => controller.abort();
  }, [object.id, object.status, object.workspace_id]);

  async function toggleNotificationPreference() {
    if (ordinaryNotificationsEnabled === null || notificationPreferenceLoading) {
      return;
    }

    setNotificationPreferenceLoading(true);
    setNotificationPreferenceError(null);

    try {
      const preference = await kival.updateObjectNotificationPreference({
        workspaceId: object.workspace_id,
        objectId: object.id,
        input: {
          ordinary_notifications_enabled: !ordinaryNotificationsEnabled,
        },
      });
      setOrdinaryNotificationsEnabled(preference.ordinary_notifications_enabled);
    } catch (cause) {
      setNotificationPreferenceError(
        cause instanceof Error ? cause.message : "Could not update notification preference.",
      );
    } finally {
      setNotificationPreferenceLoading(false);
    }
  }

  async function loadMoreVersions() {
    if (
      !versionsNextCursor ||
      versionsLoading ||
      versionsLoadingMore ||
      versionsScopeRef.current !== versionsScope
    ) {
      return;
    }

    const generation = versionsGenerationRef.current;
    const cursor = versionsNextCursor;

    setVersionsLoadingMore(true);
    setVersionsError(null);

    try {
      const response = await kival.listObjectVersions({
        workspaceId: object.workspace_id,
        objectId: object.id,
        cursor,
      });
      if (generation !== versionsGenerationRef.current) {
        return;
      }
      setVersions((items) => [
        ...items,
        ...response.items.filter(
          (version) => !items.some((candidate) => candidate.id === version.id),
        ),
      ]);
      setVersionsNextCursor(response.next_cursor ?? null);
    } catch (error) {
      if (generation === versionsGenerationRef.current) {
        setVersionsError(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (generation === versionsGenerationRef.current) {
        setVersionsLoadingMore(false);
      }
    }
  }

  async function handleArchive() {
    setArchiveLoading(true);
    setArchiveError(null);

    try {
      await onArchive();
    } catch (error) {
      setArchiveError(error instanceof Error ? error.message : String(error));
      setArchiveLoading(false);
    }
  }

  async function handleUnarchive() {
    setUnarchiveLoading(true);
    setUnarchiveError(null);

    try {
      await onUnarchive();
    } catch (error) {
      setUnarchiveError(error instanceof Error ? error.message : String(error));
      setUnarchiveLoading(false);
    }
  }

  const isArchived = object.status === "archived";
  const canEdit = value.effective_role === "editor" || value.effective_role === "admin";
  const canAdmin = value.effective_role === "admin";
  const updatedBy = current_version.created_by_username ? (
    <ProfileHoverName
      displayName={current_version.created_by_display_name ?? current_version.created_by_username}
      username={current_version.created_by_username}
      workspaceRole={current_version.created_by_workspace_role}
      accessRole={current_version.created_by_object_role}
    >
      {current_version.created_by === user.id
        ? `you (@${current_version.created_by_username})`
        : `@${current_version.created_by_username}`}
    </ProfileHoverName>
  ) : current_version.created_by ? (
    "Unknown user"
  ) : (
    "the system"
  );

  useEffect(() => {
    if (!actionsOpen) {
      return;
    }

    function handlePointerDown(event: PointerEvent) {
      if (actionsRef.current && !actionsRef.current.contains(event.target as Node)) {
        setActionsOpen(false);
      }
    }

    function handleActionsKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setActionsOpen(false);
      }
    }

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleActionsKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleActionsKeyDown);
    };
  }, [actionsOpen]);

  useEffect(() => {
    if (!archiveConfirmOpen || archiveLoading) {
      return;
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setArchiveConfirmOpen(false);
        setArchiveError(null);
      }
    }

    document.addEventListener("keydown", handleKeyDown);

    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [archiveConfirmOpen, archiveLoading]);

  return (
    <>
      <button type="button" onClick={onBack} style={styles.backButton}>
        <BackIcon />

        <span>{backLabel}</span>
      </button>

      <div style={styles.objectHeader}>
        <div style={styles.pageHeader}>
          <p style={styles.eyebrow}>Object</p>

          <h1 style={styles.pageTitle}>{selectedVersion.title}</h1>

          <span style={styles.objectStatus}>
            {isCurrentVersion
              ? `${object.status} · version ${selectedVersion.version_number}`
              : `Historical · version ${selectedVersion.version_number}`}
          </span>

          <CopyableId value={object.id} displayValue={`ID: ${object.id}`} label="object ID" />

          {isCurrentVersion && (
            <span style={styles.objectUpdatedBy}>
              Updated {formatTimestamp(object.updated_at)} by {updatedBy}
            </span>
          )}
        </div>

        <div style={styles.sectionActions}>
          {!isCurrentVersion && (
            <button
              type="button"
              style={styles.secondaryButton}
              onClick={() => setSelectedVersionId(null)}
            >
              Current version
            </button>
          )}

          {isCurrentVersion && isArchived && (
            <button
              type="button"
              style={styles.secondaryButton}
              disabled={unarchiveLoading}
              onClick={() => void handleUnarchive()}
            >
              {unarchiveLoading ? "Restoring…" : "Unarchive"}
            </button>
          )}

          {isCurrentVersion && !isArchived && canAdmin && (
            <>
              <ObjectShareAvatars
                key={shareRevision}
                workspaceId={object.workspace_id}
                objectId={object.id}
                currentUserId={user.id}
                onClick={() => setShareOpen(true)}
              />
              <button
                type="button"
                style={styles.secondaryButton}
                onClick={() => setShareOpen(true)}
              >
                Share
              </button>
            </>
          )}

          {isCurrentVersion && !isArchived && canEdit && (
            <button type="button" style={styles.secondaryButton} onClick={onEdit}>
              Edit
            </button>
          )}

          {isCurrentVersion && !isArchived && (
            <div ref={actionsRef} style={styles.objectActionsMenu}>
              <button
                type="button"
                style={styles.objectActionsTrigger}
                aria-label="Object actions"
                aria-haspopup="menu"
                aria-expanded={actionsOpen}
                onClick={() => setActionsOpen((open) => !open)}
              >
                <span aria-hidden="true">•••</span>
              </button>

              {actionsOpen && (
                <div role="menu" style={styles.objectActionsPopover}>
                  <button
                    type="button"
                    role="menuitem"
                    style={styles.objectActionsItem}
                    onClick={() => {
                      setActionsOpen(false);
                      onRevealInGraph();
                    }}
                  >
                    Reveal in graph
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    style={styles.objectActionsItem}
                    onClick={() => {
                      setActionsOpen(false);
                      void navigator.clipboard.writeText(window.location.href);
                    }}
                  >
                    Copy link
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    style={styles.objectActionsItem}
                    disabled={
                      ordinaryNotificationsEnabled === null || notificationPreferenceLoading
                    }
                    title="Direct mentions and replies remain visible."
                    onClick={() => {
                      setActionsOpen(false);
                      void toggleNotificationPreference();
                    }}
                  >
                    {notificationPreferenceLoading
                      ? "Saving notification preference…"
                      : ordinaryNotificationsEnabled === false
                        ? "Resume object activity"
                        : "Mute object activity"}
                  </button>
                  {canAdmin && (
                    <>
                      <div style={styles.objectActionsDivider} />
                      <button
                        type="button"
                        role="menuitem"
                        style={styles.objectActionsDanger}
                        onClick={() => {
                          setActionsOpen(false);
                          setArchiveConfirmOpen(true);
                          setArchiveError(null);
                        }}
                      >
                        Archive
                      </button>
                    </>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {notificationPreferenceError && (
        <div style={styles.errorBox} role="alert">
          <strong>Could not update notifications</strong>
          <span>{notificationPreferenceError}</span>
        </div>
      )}

      {archiveConfirmOpen && !isArchived && (
        <div style={styles.modalBackdrop}>
          <button
            type="button"
            style={styles.modalBackdropDismiss}
            aria-label="Close archive confirmation"
            disabled={archiveLoading}
            onClick={() => {
              setArchiveConfirmOpen(false);
              setArchiveError(null);
            }}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="archive-dialog-title"
            aria-describedby="archive-dialog-description"
            style={styles.modalDialog}
          >
            <div style={styles.modalCopy}>
              <h2 id="archive-dialog-title" style={styles.modalTitle}>
                Archive “{selectedVersion.title}”?
              </h2>

              <p id="archive-dialog-description" style={styles.muted}>
                This object will be removed from the active workspace. You can restore it later from
                Archived.
              </p>
            </div>

            {archiveError && (
              <div style={styles.errorBox}>
                <strong>Could not archive object</strong>
                <span>{archiveError}</span>
              </div>
            )}

            <div style={styles.modalActions}>
              <button
                type="button"
                style={styles.secondaryButton}
                disabled={archiveLoading}
                onClick={() => {
                  setArchiveConfirmOpen(false);
                  setArchiveError(null);
                }}
              >
                Cancel
              </button>

              <button
                type="button"
                style={styles.dangerButton}
                disabled={archiveLoading}
                onClick={() => void handleArchive()}
              >
                {archiveLoading ? "Archiving…" : "Archive"}
              </button>
            </div>
          </div>
        </div>
      )}

      {shareOpen && canAdmin && (
        <ObjectShareDialog
          user={user}
          workspaceId={object.workspace_id}
          objectId={object.id}
          objectTitle={selectedVersion.title}
          onClose={() => setShareOpen(false)}
          onAccessChanged={async () => {
            setShareRevision((revision) => revision + 1);
            await onAccessChanged();
          }}
        />
      )}

      {unarchiveError && (
        <div style={styles.errorBox}>
          <strong>Could not unarchive object</strong>
          <span>{unarchiveError}</span>
        </div>
      )}

      {Object.keys(selectedMetadata).length > 0 && (
        <section style={styles.metadataSection}>
          <h2 style={styles.sectionTitle}>Metadata</h2>

          <div className="kival-row-list" style={styles.metadataGrid}>
            {Object.entries(selectedMetadata).map(([key, metadataValue]) => (
              <div key={key} style={styles.metadataRow}>
                <span style={styles.metadataKey}>{key}</span>

                <span>{String(metadataValue)}</span>
              </div>
            ))}
          </div>
        </section>
      )}

      <article style={styles.objectContent}>
        <MarkdownBody
          body={selectedVersion.body}
          workspaceId={object.workspace_id}
          objectId={object.id}
          wikilinks={wikilinks}
          onOpenObject={onOpenObject}
        />
      </article>

      <CommentaryPanel
        workspaceId={object.workspace_id}
        objectId={object.id}
        currentUserId={user.id}
        effectiveRole={value.effective_role}
        archived={isArchived}
        targetCommentId={initialCommentId}
        targetThreadId={initialThreadId}
      />

      <section style={styles.versionHistory}>
        {versionsLoading && <LoadingIndicator label="Loading history…" compact />}

        {!versionsLoading && versionsError && (
          <span style={styles.error}>Could not load version history.</span>
        )}

        {!versionsLoading && !versionsError && (versions.length > 1 || versionsNextCursor) && (
          <>
            <button
              type="button"
              style={styles.versionHistoryToggle}
              onClick={() => setHistoryOpen((open) => !open)}
            >
              <DisclosureIcon open={historyOpen} />

              <span>
                {historyOpen
                  ? "Hide history"
                  : `View history (${versions.length}${versionsNextCursor ? "+" : ""})`}
              </span>
            </button>

            {historyOpen && (
              <div style={styles.versionHistoryList}>
                {versions.map((version) => {
                  const isCurrent = version.id === current_version.id;
                  const isSelected = version.id === selectedVersion.id;

                  return (
                    <button
                      key={version.id}
                      type="button"
                      style={
                        isSelected ? styles.versionHistoryItemActive : styles.versionHistoryItem
                      }
                      onClick={() => setSelectedVersionId(isCurrent ? null : version.id)}
                    >
                      <span>Version {version.version_number}</span>

                      <span style={styles.versionHistoryMeta}>
                        {isCurrent ? "Current" : formatTimestamp(version.created_at)}
                      </span>
                    </button>
                  );
                })}
                <InfiniteScrollSentinel
                  hasMore={Boolean(versionsNextCursor)}
                  loading={versionsLoadingMore}
                  onLoadMore={loadMoreVersions}
                  label="Loading older history…"
                />
              </div>
            )}
          </>
        )}
      </section>

      <section style={styles.objectActivity}>
        {eventsLoading && <LoadingIndicator label="Loading activity…" compact />}

        {!eventsLoading && eventsError && (
          <span style={styles.muted}>Activity is unavailable for this object.</span>
        )}

        {!eventsLoading && !eventsError && events.length > 0 && (
          <>
            <button
              type="button"
              style={styles.versionHistoryToggle}
              onClick={() => setActivityOpen((open) => !open)}
            >
              <DisclosureIcon open={activityOpen} />

              <span>{activityOpen ? "Hide activity" : `View activity (${events.length})`}</span>
            </button>

            {activityOpen && (
              <div style={styles.objectActivityList}>
                {events.map((event) => {
                  const description = describeEvent(event, versions, objects, context);

                  return (
                    <div key={event.id} style={styles.objectActivityItem}>
                      <div style={styles.objectActivityMain}>
                        <strong>{description.title}</strong>

                        <span style={styles.objectActivityMeta}>
                          {formatEventActor(event, user)} · {formatTimestamp(event.created_at)}
                        </span>

                        {description.detail && (
                          <span style={styles.objectActivityDetails}>{description.detail}</span>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </>
        )}
      </section>
    </>
  );
}
