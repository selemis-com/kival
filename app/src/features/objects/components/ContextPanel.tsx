import { useState } from "react";
import { kival } from "../../../shared/api";
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
  onContextChanged: (objectId: string) => Promise<void>;
};

export function ContextPanel({
  workspaceId,
  context,
  value,
  objects,
  onOpenObject,
  onRevealInGraph,
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
          <button
            type="button"
            style={styles.contextAction}
            onClick={() => setCreatingConnection(true)}
          >
            Add connection
          </button>
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
                    value={targetQuery}
                    onChange={(event) => setTargetQuery(event.target.value)}
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
              const target = objectsById.get(edge.target_object_id);

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
