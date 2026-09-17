import { useMemo, useState } from "react";
import { kival } from "../../../shared/api";
import { selectSoleResultOnEnter } from "../../../shared/forms";
import { styles } from "../../../shared/styles/index";
import type { CurrentObjectResponse, ObjectContext, ObjectSummary } from "../../../shared/types";
import { ConfirmationDialog } from "../../../shared/ui/ConfirmationDialog";
import { LocalGraph } from "../../graph/LocalGraph";

type Props = {
  workspaceId: string;
  context: ObjectContext | null;
  value: CurrentObjectResponse | null;
  objects: ObjectSummary[];
  onOpenObject: (objectId: string) => void;
  onRevealInGraph: (objectId: string) => void;
  onCreateConnectedObject: (objectId: string) => void;
  onContextChanged: (objectId: string) => Promise<void>;
};

type CreationContextPanelProps = {
  workspaceId: string;
  draftTitle: string;
  connectedFrom: Pick<ObjectSummary, "id" | "title"> | null;
  onOpenObject: (objectId: string) => void;
};

export function CreationContextPanel({
  workspaceId,
  draftTitle,
  connectedFrom,
  onOpenObject,
}: CreationContextPanelProps) {
  const graphContext = useMemo<ObjectContext>(() => {
    const timestamp = new Date().toISOString();
    const draftObjectId = "draft-object";
    const sourceNodes = connectedFrom
      ? [
          {
            id: connectedFrom.id,
            workspace_id: workspaceId,
            current_version_id: null,
            title: connectedFrom.title,
            status: "active" as const,
            created_by: null,
            created_at: timestamp,
            updated_at: timestamp,
            distance: 1,
            incoming_count: 0,
            outgoing_count: 1,
          },
        ]
      : [];
    const sourceEdges = connectedFrom
      ? [
          {
            id: "draft-edge",
            workspace_id: workspaceId,
            source_object_id: connectedFrom.id,
            target_object_id: draftObjectId,
            kind: "relationship" as const,
            created_by: null,
            created_at: timestamp,
            updated_at: timestamp,
          },
        ]
      : [];

    return {
      backlinks: {
        object_id: draftObjectId,
        incoming_edges: [],
        incoming_references: [],
      },
      edges: { items: [] },
      graph: {
        workspace_id: workspaceId,
        root_object_id: draftObjectId,
        depth: 1,
        direction: "both",
        max_nodes: connectedFrom ? 2 : 1,
        max_edges: connectedFrom ? 1 : 0,
        truncated: false,
        truncation: { nodes: false, edges: false },
        nodes: [
          {
            id: draftObjectId,
            workspace_id: workspaceId,
            current_version_id: null,
            title: draftTitle,
            status: "active",
            created_by: null,
            created_at: timestamp,
            updated_at: timestamp,
            distance: 0,
            incoming_count: connectedFrom ? 1 : 0,
            outgoing_count: 0,
          },
          ...sourceNodes,
        ],
        edges: sourceEdges,
      },
    };
  }, [connectedFrom, draftTitle, workspaceId]);

  return (
    <aside style={styles.contextPanel}>
      <span style={styles.sidebarLabel}>Context</span>

      <LocalGraph context={graphContext} onOpenObject={onOpenObject} showIsolated />

      <div style={styles.contextBlock}>
        <div style={styles.contextHeading}>
          <strong>Connections</strong>

          <span style={styles.contextCount}>{connectedFrom ? 1 : 0}</span>
        </div>

        {!connectedFrom && <span style={styles.muted}>No connected objects yet.</span>}
      </div>

      {connectedFrom && (
        <div style={styles.contextBlock}>
          <div style={styles.contextHeading}>
            <strong>Incoming</strong>

            <span style={styles.contextCount}>1</span>
          </div>

          <div style={styles.connectionList}>
            <button
              type="button"
              style={styles.connectionItem}
              onClick={() => onOpenObject(connectedFrom.id)}
            >
              <strong style={styles.connectionTitle}>{connectedFrom.title}</strong>
            </button>
          </div>
        </div>
      )}
    </aside>
  );
}

export function ContextPanel({
  workspaceId,
  context,
  value,
  objects,
  onOpenObject,
  onRevealInGraph,
  onCreateConnectedObject,
  onContextChanged,
}: Props) {
  const [creatingConnection, setCreatingConnection] = useState(false);
  const [targetObjectId, setTargetObjectId] = useState("");
  const [targetQuery, setTargetQuery] = useState("");
  const [connectionLoading, setConnectionLoading] = useState(false);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [removalTarget, setRemovalTarget] = useState<{
    edgeId: string;
    targetTitle: string;
  } | null>(null);
  const [removalError, setRemovalError] = useState<string | null>(null);

  if (!context || !value || context.backlinks.object_id !== value.object.id) {
    return (
      <aside style={styles.contextPanel}>
        <span style={styles.sidebarLabel}>Context</span>

        <div style={styles.contextBlock}>
          <strong>Connections</strong>

          <span style={styles.muted}>Select an object to explore its relationships.</span>
        </div>
      </aside>
    );
  }

  const objectId = value.object.id;
  const incoming = context.backlinks.incoming_edges;
  const outgoing = context.edges.items.filter((edge) => edge.source_object_id === objectId);
  const objectsById = new Map(objects.map((object) => [object.id, object]));
  const graphNodesById = new Map(context.graph.nodes.map((node) => [node.id, node]));
  const availableTargets = objects.filter((object) => object.id !== objectId);
  const normalizedTargetQuery = targetQuery.trim().toLowerCase();
  const filteredTargets = availableTargets
    .filter(
      (object) =>
        !normalizedTargetQuery || object.title.toLowerCase().includes(normalizedTargetQuery),
    )
    .slice(0, 8);
  const selectedTarget = availableTargets.find((object) => object.id === targetObjectId) ?? null;
  const connectionCount = incoming.length + outgoing.length;

  async function handleCreateConnection() {
    if (!targetObjectId) {
      return;
    }

    setConnectionLoading(true);
    setConnectionError(null);

    try {
      await kival.createObjectEdge({
        workspaceId,
        input: {
          source_object_id: objectId,
          target_object_id: targetObjectId,
        },
      });
      setCreatingConnection(false);
      setTargetObjectId("");
      setTargetQuery("");
      await onContextChanged(objectId);
    } catch (error) {
      setConnectionError(error instanceof Error ? error.message : String(error));
    } finally {
      setConnectionLoading(false);
    }
  }

  async function handleRevokeConnection() {
    if (!removalTarget || connectionLoading) {
      return;
    }

    setConnectionLoading(true);
    setRemovalError(null);

    try {
      await kival.revokeObjectEdge({ workspaceId, edgeId: removalTarget.edgeId });
      await onContextChanged(objectId);
      setRemovalTarget(null);
    } catch (error) {
      setRemovalError(error instanceof Error ? error.message : String(error));
    } finally {
      setConnectionLoading(false);
    }
  }

  return (
    <aside style={styles.contextPanel}>
      <span style={styles.sidebarLabel}>Context</span>

      <LocalGraph
        context={context}
        onOpenObject={onOpenObject}
        onRevealInGraph={() => onRevealInGraph(objectId)}
      />

      <div style={styles.contextBlock}>
        <div style={styles.contextHeading}>
          <strong>Connections</strong>

          <span style={styles.contextCount}>{connectionCount}</span>
        </div>

        {connectionCount === 0 && <span style={styles.muted}>No connected objects yet.</span>}

        {!creatingConnection && (
          <div
            style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", gap: 8 }}
          >
            <button
              type="button"
              style={styles.contextAction}
              onClick={() => onCreateConnectedObject(objectId)}
            >
              Create connected object
            </button>
            <button
              type="button"
              style={styles.contextAction}
              onClick={() => setCreatingConnection(true)}
            >
              Add existing connection
            </button>
          </div>
        )}

        {creatingConnection && (
          <div style={styles.connectionEditor}>
            <div style={styles.field}>
              <span>Target</span>

              {selectedTarget ? (
                <div style={styles.connectionTargetSelected}>
                  <span style={styles.connectionTargetText}>
                    <strong>{selectedTarget.title}</strong>
                  </span>
                  <button
                    type="button"
                    style={styles.connectionTargetClear}
                    aria-label="Choose a different target"
                    onClick={() => {
                      setTargetObjectId("");
                      setTargetQuery("");
                    }}
                  >
                    ×
                  </button>
                </div>
              ) : (
                <>
                  <input
                    data-1p-ignore="true"
                    autoFocus
                    value={targetQuery}
                    onChange={(event) => setTargetQuery(event.target.value)}
                    onKeyDown={(event) =>
                      selectSoleResultOnEnter(event, filteredTargets, (target) => {
                        setTargetObjectId(target.id);
                        setTargetQuery("");
                      })
                    }
                    style={styles.input}
                    placeholder="Search objects…"
                    autoComplete="off"
                    disabled={connectionLoading}
                  />

                  <div style={styles.connectionTargetResults}>
                    {filteredTargets.map((object) => (
                      <button
                        key={object.id}
                        type="button"
                        style={styles.connectionTargetResult}
                        onClick={() => {
                          setTargetObjectId(object.id);
                          setTargetQuery("");
                        }}
                      >
                        <strong>{object.title}</strong>
                      </button>
                    ))}

                    {filteredTargets.length === 0 && (
                      <span style={styles.muted}>No matching objects.</span>
                    )}
                  </div>
                </>
              )}
            </div>

            {connectionError && <span style={styles.error}>{connectionError}</span>}

            <div style={styles.connectionEditorActions}>
              <button
                type="button"
                style={styles.secondaryButton}
                disabled={connectionLoading}
                onClick={() => {
                  setCreatingConnection(false);
                  setTargetObjectId("");
                  setTargetQuery("");
                  setConnectionError(null);
                }}
              >
                Cancel
              </button>

              <button
                type="button"
                style={styles.primaryButtonCompact}
                disabled={connectionLoading || !targetObjectId}
                onClick={() => void handleCreateConnection()}
              >
                {connectionLoading ? "Adding…" : "Add"}
              </button>
            </div>
          </div>
        )}
      </div>

      {outgoing.length > 0 && (
        <div style={styles.contextBlock}>
          <div style={styles.contextHeading}>
            <strong>Outgoing</strong>

            <span style={styles.contextCount}>{outgoing.length}</span>
          </div>

          <div style={styles.connectionList}>
            {outgoing.map((edge) => {
              const target =
                graphNodesById.get(edge.target_object_id) ?? objectsById.get(edge.target_object_id);

              return (
                <div key={edge.id} style={styles.connectionRow}>
                  <button
                    type="button"
                    style={styles.connectionItem}
                    onClick={() => onOpenObject(edge.target_object_id)}
                  >
                    <strong style={styles.connectionTitle}>
                      {target?.title ?? edge.target_object_id}
                    </strong>
                  </button>

                  <button
                    type="button"
                    style={styles.connectionRemove}
                    disabled={connectionLoading}
                    aria-label={`Remove connection to ${target?.title ?? edge.target_object_id}`}
                    onClick={() => {
                      setRemovalTarget({
                        edgeId: edge.id,
                        targetTitle: target?.title ?? edge.target_object_id,
                      });
                      setRemovalError(null);
                    }}
                  >
                    ×
                  </button>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {incoming.length > 0 && (
        <div style={styles.contextBlock}>
          <div style={styles.contextHeading}>
            <strong>Incoming</strong>

            <span style={styles.contextCount}>{incoming.length}</span>
          </div>

          <div style={styles.connectionList}>
            {incoming.map((edge) => (
              <button
                key={edge.edge_id}
                type="button"
                style={styles.connectionItem}
                onClick={() => onOpenObject(edge.source_object.id)}
              >
                <strong style={styles.connectionTitle}>{edge.source_object.title}</strong>
              </button>
            ))}
          </div>
        </div>
      )}

      {context.backlinks.incoming_references.length > 0 && (
        <div style={styles.contextBlock}>
          <strong>Backlinks</strong>

          <span style={styles.muted}>
            {context.backlinks.incoming_references.length} references
          </span>
        </div>
      )}

      {removalTarget ? (
        <ConfirmationDialog
          title={`Remove connection to “${removalTarget.targetTitle}”?`}
          description="This relationship will be removed from the object graph. You can add it again later."
          confirmLabel="Remove connection"
          pendingLabel="Removing…"
          pending={connectionLoading}
          error={removalError}
          errorTitle="Could not remove connection"
          closeLabel="Cancel connection removal"
          onCancel={() => {
            setRemovalTarget(null);
            setRemovalError(null);
          }}
          onConfirm={() => void handleRevokeConnection()}
        />
      ) : null}
    </aside>
  );
}
