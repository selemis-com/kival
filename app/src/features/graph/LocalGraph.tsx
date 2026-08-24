import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { graphThemes } from "../../shared/styles/constants";
import { styles } from "../../shared/styles/index";
import { useTheme } from "../../shared/styles/theme";
import type {
  CurrentObjectResponse,
  ObjectContext,
  ObjectSummary,
  WorkspaceGraphEdge,
  WorkspaceGraphNode,
} from "../../shared/types";
import { GraphRenderer } from "./GraphRenderer";
import { DEFAULT_GRAPH_LAYOUT_OPTIONS } from "./layout";
import type { PositionedGraph, PositionedNode } from "./model";
import { GraphPhysics } from "./physics";

type Props = {
  value: CurrentObjectResponse;
  context: ObjectContext;
  objects: ObjectSummary[];
  onOpenObject: (objectId: string) => void;
  onRevealInGraph: () => void;
};

type PointerState = {
  pointerId: number;
  x: number;
  y: number;
  nodeId: string | null;
  nodeOffsetX: number;
  nodeOffsetY: number;
  dragging: boolean;
};

const LOCAL_GRAPH_LAYOUT = {
  ...DEFAULT_GRAPH_LAYOUT_OPTIONS,
  centerForce: 1.2,
  repelForce: 0.75,
  linkForce: 1.15,
  linkDistance: 84,
  clusterForce: 0,
};

const LOCAL_GRAPH_VIEWPORT_PADDING = 18;

function clampLocalGraphPosition(renderer: GraphRenderer, x: number, y: number) {
  const { canvasCssWidth, canvasCssHeight } = renderer.getDebugInfo();
  const min = renderer.screenToWorld(LOCAL_GRAPH_VIEWPORT_PADDING, LOCAL_GRAPH_VIEWPORT_PADDING);
  const max = renderer.screenToWorld(
    Math.max(LOCAL_GRAPH_VIEWPORT_PADDING, canvasCssWidth - LOCAL_GRAPH_VIEWPORT_PADDING),
    Math.max(LOCAL_GRAPH_VIEWPORT_PADDING, canvasCssHeight - LOCAL_GRAPH_VIEWPORT_PADDING),
  );

  return {
    x: Math.min(Math.max(x, Math.min(min.x, max.x)), Math.max(min.x, max.x)),
    y: Math.min(Math.max(y, Math.min(min.y, max.y)), Math.max(min.y, max.y)),
  };
}

function constrainLocalGraphToViewport(renderer: GraphRenderer, graph: PositionedGraph) {
  let changed = false;

  for (const node of graph.nodes) {
    const clamped = clampLocalGraphPosition(renderer, node.x, node.y);

    if (clamped.x !== node.x || clamped.y !== node.y) {
      node.x = clamped.x;
      node.y = clamped.y;
      changed = true;
    }
  }

  return changed;
}

function softlyCenterLocalGraphCurrentNode(
  graph: PositionedGraph,
  currentObjectId: string,
  draggingNodeId: string | null,
) {
  if (draggingNodeId === currentObjectId) {
    return false;
  }

  const current = graph.nodes.find((node) => node.id === currentObjectId);

  if (!current) {
    return false;
  }

  const distance = Math.hypot(current.x, current.y);

  if (distance < 0.25) {
    current.x = 0;
    current.y = 0;
    return false;
  }

  current.x *= 0.94;
  current.y *= 0.94;
  return true;
}

type LabelPlacement = {
  text: string;
  x: number;
  y: number;
  align: CanvasTextAlign;
  rect: { left: number; top: number; right: number; bottom: number };
};

function fitLabelText(context: CanvasRenderingContext2D, text: string, maxWidth: number) {
  if (maxWidth <= 8) {
    return "";
  }

  if (context.measureText(text).width <= maxWidth) {
    return text;
  }

  const ellipsis = "…";
  let end = text.length;

  while (end > 0) {
    const candidate = `${text.slice(0, end).trimEnd()}${ellipsis}`;

    if (context.measureText(candidate).width <= maxWidth) {
      return candidate;
    }

    end -= 1;
  }

  return ellipsis;
}

function labelOverlapArea(a: LabelPlacement["rect"], b: LabelPlacement["rect"]) {
  const width = Math.max(0, Math.min(a.right, b.right) - Math.max(a.left, b.left));
  const height = Math.max(0, Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top));
  return width * height;
}

function createLocalGraph(
  value: CurrentObjectResponse,
  context: ObjectContext,
  objects: ObjectSummary[],
): PositionedGraph {
  const current = value.object;
  const objectById = new Map(objects.map((object) => [object.id, object]));
  const nodeById = new Map<string, WorkspaceGraphNode>();
  const edges: WorkspaceGraphEdge[] = [];
  const edgeIds = new Set<string>();

  function ensureNode(node: WorkspaceGraphNode) {
    if (!nodeById.has(node.id)) {
      nodeById.set(node.id, node);
    }
  }

  ensureNode({
    id: current.id,
    workspace_id: current.workspace_id,
    current_version_id: current.current_version_id,
    title: value.current_version.title,
    status: current.status,
    created_by: current.created_by,
    created_at: current.created_at,
    updated_at: current.updated_at,
    in_degree: 0,
    out_degree: 0,
  });

  for (const incoming of context.backlinks.incoming_edges) {
    ensureNode({
      id: incoming.source_object.id,
      workspace_id: current.workspace_id,
      current_version_id: null,
      title: incoming.source_object.title,
      status: incoming.source_object.status,
      created_by: null,
      created_at: incoming.created_at,
      updated_at: incoming.created_at,
      in_degree: 0,
      out_degree: 0,
    });

    if (!edgeIds.has(incoming.edge_id)) {
      edgeIds.add(incoming.edge_id);
      edges.push({
        id: incoming.edge_id,
        workspace_id: current.workspace_id,
        source_object_id: incoming.source_object.id,
        target_object_id: current.id,
        created_by: incoming.created_by,
        created_at: incoming.created_at,
        updated_at: incoming.created_at,
      });
    }
  }

  for (const edge of context.edges.items) {
    if (edge.source_object_id !== current.id) {
      continue;
    }

    const target = objectById.get(edge.target_object_id);

    ensureNode({
      id: edge.target_object_id,
      workspace_id: current.workspace_id,
      current_version_id: null,
      title: target?.title ?? edge.target_object_id,
      status: target?.status ?? "active",
      created_by: null,
      created_at: edge.created_at,
      updated_at: edge.updated_at,
      in_degree: 0,
      out_degree: 0,
    });

    if (!edgeIds.has(edge.id)) {
      edgeIds.add(edge.id);
      edges.push({
        id: edge.id,
        workspace_id: edge.workspace_id,
        source_object_id: edge.source_object_id,
        target_object_id: edge.target_object_id,
        created_by: edge.created_by,
        created_at: edge.created_at,
        updated_at: edge.updated_at,
      });
    }
  }

  const nodes = [...nodeById.values()];
  const degreeById = new Map(nodes.map((node) => [node.id, { incoming: 0, outgoing: 0 }]));

  for (const edge of edges) {
    const source = degreeById.get(edge.source_object_id);
    const target = degreeById.get(edge.target_object_id);

    if (source) {
      source.outgoing += 1;
    }

    if (target) {
      target.incoming += 1;
    }
  }

  const neighbors = nodes.filter((node) => node.id !== current.id);
  const radius = Math.max(86, Math.min(132, 72 + neighbors.length * 4));
  const positioned: PositionedNode[] = nodes.map((node) => {
    const degree = degreeById.get(node.id) ?? { incoming: 0, outgoing: 0 };

    if (node.id === current.id) {
      return {
        ...node,
        in_degree: degree.incoming,
        out_degree: degree.outgoing,
        x: 0,
        y: 0,
      };
    }

    const index = neighbors.findIndex((neighbor) => neighbor.id === node.id);
    const angle = (index / Math.max(1, neighbors.length)) * Math.PI * 2 - Math.PI / 2;

    return {
      ...node,
      in_degree: degree.incoming,
      out_degree: degree.outgoing,
      x: Math.cos(angle) * radius,
      y: Math.sin(angle) * radius,
    };
  });

  return { nodes: positioned, edges };
}

function drawLocalLabels(
  overlay: HTMLCanvasElement,
  renderer: GraphRenderer,
  graph: PositionedGraph,
  currentObjectId: string,
  hoveredNodeId: string | null,
  theme: (typeof graphThemes)[keyof typeof graphThemes],
) {
  const devicePixelRatio = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.floor(overlay.clientWidth * devicePixelRatio));
  const height = Math.max(1, Math.floor(overlay.clientHeight * devicePixelRatio));

  if (overlay.width !== width || overlay.height !== height) {
    overlay.width = width;
    overlay.height = height;
  }

  const context = overlay.getContext("2d");

  if (!context) {
    return;
  }

  const viewportWidth = overlay.clientWidth;
  const viewportHeight = overlay.clientHeight;
  const centerX = viewportWidth / 2;
  const centerY = viewportHeight / 2;
  const placedLabels: LabelPlacement["rect"][] = [];

  context.clearRect(0, 0, width, height);
  context.save();
  context.scale(devicePixelRatio, devicePixelRatio);
  context.textBaseline = "middle";
  context.font = "500 10.5px Inter, ui-sans-serif, system-ui, sans-serif";

  for (const node of graph.nodes) {
    const point = renderer.worldToScreen(node);
    const isCurrent = node.id === currentObjectId;
    const isHovered = node.id === hoveredNodeId;
    const rawLabel = node.title;
    const horizontalDirection = point.x < centerX ? -1 : 1;
    const verticalDirection = point.y < centerY ? -1 : 1;
    const labelHeight = 13;
    const horizontalGap = 12;
    const verticalGap = 15;
    const maxVerticalWidth = Math.max(
      32,
      Math.min(118, viewportWidth - 2 * LOCAL_GRAPH_VIEWPORT_PADDING),
    );

    const makeHorizontal = (direction: -1 | 1, priority: number): [LabelPlacement, number] => {
      const availableWidth =
        direction === 1
          ? viewportWidth - LOCAL_GRAPH_VIEWPORT_PADDING - (point.x + horizontalGap)
          : point.x - horizontalGap - LOCAL_GRAPH_VIEWPORT_PADDING;
      const text = fitLabelText(context, rawLabel, Math.min(118, Math.max(0, availableWidth)));
      const textWidth = context.measureText(text).width;
      const x = point.x + direction * horizontalGap;
      const left = direction === 1 ? x : x - textWidth;
      const right = direction === 1 ? x + textWidth : x;
      const rect = {
        left,
        top: point.y - labelHeight / 2,
        right,
        bottom: point.y + labelHeight / 2,
      };
      const overlap = placedLabels.reduce(
        (sum, existing) => sum + labelOverlapArea(rect, existing),
        0,
      );
      const cramped = text.length < Math.min(rawLabel.length, 4) ? 500 : 0;

      return [
        { text, x, y: point.y, align: direction === 1 ? "left" : "right", rect },
        priority + overlap * 20 + cramped,
      ];
    };

    const makeVertical = (direction: -1 | 1, priority: number): [LabelPlacement, number] => {
      const text = fitLabelText(context, rawLabel, maxVerticalWidth);
      const textWidth = context.measureText(text).width;
      const halfWidth = textWidth / 2;
      const x = Math.min(
        Math.max(point.x, LOCAL_GRAPH_VIEWPORT_PADDING + halfWidth),
        viewportWidth - LOCAL_GRAPH_VIEWPORT_PADDING - halfWidth,
      );
      const y = Math.min(
        Math.max(point.y + direction * verticalGap, LOCAL_GRAPH_VIEWPORT_PADDING + labelHeight / 2),
        viewportHeight - LOCAL_GRAPH_VIEWPORT_PADDING - labelHeight / 2,
      );
      const rect = {
        left: x - halfWidth,
        top: y - labelHeight / 2,
        right: x + halfWidth,
        bottom: y + labelHeight / 2,
      };
      const overlap = placedLabels.reduce(
        (sum, existing) => sum + labelOverlapArea(rect, existing),
        0,
      );

      return [{ text, x, y, align: "center", rect }, priority + overlap * 20];
    };

    const candidates: Array<[LabelPlacement, number]> = isCurrent
      ? [makeHorizontal(1, 0), makeVertical(-1, 8), makeVertical(1, 12), makeHorizontal(-1, 16)]
      : [
          makeHorizontal(horizontalDirection as -1 | 1, 0),
          makeVertical(verticalDirection as -1 | 1, 4),
          makeVertical(-verticalDirection as -1 | 1, 8),
          makeHorizontal(-horizontalDirection as -1 | 1, 12),
        ];

    const [placement] = candidates.reduce((best, candidate) =>
      candidate[1] < best[1] ? candidate : best,
    );

    if (!placement.text) {
      continue;
    }

    placedLabels.push(placement.rect);
    context.globalAlpha = hoveredNodeId && !isHovered && !isCurrent ? 0.35 : 1;
    context.textAlign = placement.align;
    context.lineWidth = 3;
    context.strokeStyle = theme.labelStroke;
    context.strokeText(placement.text, placement.x, placement.y);
    context.fillStyle = isCurrent
      ? theme.localLabelCurrent
      : isHovered
        ? theme.localLabelHovered
        : theme.localLabel;
    context.fillText(placement.text, placement.x, placement.y);
  }

  context.restore();
}

export function LocalGraph({ value, context, objects, onOpenObject, onRevealInGraph }: Props) {
  const { resolvedTheme } = useTheme();
  const graphTheme = graphThemes[resolvedTheme];
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const overlayRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<GraphRenderer | null>(null);
  const physicsRef = useRef<GraphPhysics | null>(null);
  const physicsFrameRef = useRef<number | null>(null);
  const pointerRef = useRef<PointerState | null>(null);
  const graphRef = useRef<PositionedGraph>({ nodes: [], edges: [] });
  const hoveredNodeIdRef = useRef<string | null>(null);
  const [renderError, setRenderError] = useState<string | null>(null);

  const graph = useMemo(() => createLocalGraph(value, context, objects), [context, objects, value]);
  const currentObjectId = value.object.id;
  const hasConnections = graph.nodes.length > 1;

  const drawLabels = useCallback(() => {
    const overlay = overlayRef.current;
    const renderer = rendererRef.current;

    if (!overlay || !renderer) {
      return;
    }

    drawLocalLabels(
      overlay,
      renderer,
      graphRef.current,
      currentObjectId,
      hoveredNodeIdRef.current,
      graphTheme,
    );
  }, [currentObjectId, graphTheme]);

  const startPhysicsLoop = useCallback(() => {
    if (physicsFrameRef.current !== null) {
      return;
    }

    const tick = () => {
      const physics = physicsRef.current;
      const renderer = rendererRef.current;

      if (!physics || !renderer) {
        physicsFrameRef.current = null;
        return;
      }

      const active = physics.step();
      const draggingNodeId = pointerRef.current?.dragging ? pointerRef.current.nodeId : null;
      const centered = softlyCenterLocalGraphCurrentNode(
        graphRef.current,
        currentObjectId,
        draggingNodeId,
      );
      const constrained = constrainLocalGraphToViewport(renderer, graphRef.current);

      if (centered) {
        physics.reheat(0.08);
      }

      if (!active && !centered && !constrained) {
        physicsFrameRef.current = null;
        return;
      }

      renderer.updateGraphPositions();
      drawLabels();
      physicsFrameRef.current = requestAnimationFrame(tick);
    };

    physicsFrameRef.current = requestAnimationFrame(tick);
  }, [currentObjectId, drawLabels]);

  useEffect(() => {
    if (!hasConnections) {
      return;
    }

    const canvas = canvasRef.current;

    if (!canvas) {
      return;
    }

    let renderer: GraphRenderer;

    try {
      renderer = new GraphRenderer(canvas);
      renderer.setTheme(resolvedTheme);
    } catch (error) {
      setRenderError(error instanceof Error ? error.message : String(error));
      return;
    }

    rendererRef.current = renderer;

    return () => {
      renderer.destroy();
      rendererRef.current = null;
    };
  }, [hasConnections, resolvedTheme]);

  useEffect(() => {
    rendererRef.current?.setTheme(resolvedTheme);
    drawLabels();
  }, [drawLabels, resolvedTheme]);

  useEffect(() => {
    const renderer = rendererRef.current;

    if (!renderer) {
      return;
    }

    graphRef.current = graph;
    physicsRef.current = new GraphPhysics(graph, LOCAL_GRAPH_LAYOUT);
    renderer.setGraph(graph);
    renderer.setNodeScale(0.86);
    renderer.setSelectedNode(currentObjectId);
    constrainLocalGraphToViewport(renderer, graph);
    renderer.updateGraphPositions();
    startPhysicsLoop();
    drawLabels();
  }, [currentObjectId, drawLabels, graph, startPhysicsLoop]);

  useEffect(() => {
    const container = containerRef.current;

    if (!container) {
      return;
    }

    const observer = new ResizeObserver(() => {
      const renderer = rendererRef.current;

      if (!renderer) {
        return;
      }

      renderer.render();
      constrainLocalGraphToViewport(renderer, graphRef.current);
      renderer.updateGraphPositions();
      drawLabels();
    });

    observer.observe(container);
    return () => observer.disconnect();
  }, [drawLabels]);

  useEffect(() => {
    return () => {
      if (physicsFrameRef.current !== null) {
        cancelAnimationFrame(physicsFrameRef.current);
      }
    };
  }, []);

  function pointerPosition(event: React.PointerEvent<HTMLDivElement>) {
    const container = containerRef.current;

    if (!container) {
      return null;
    }

    const rect = container.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  function handlePointerDown(event: React.PointerEvent<HTMLDivElement>) {
    const renderer = rendererRef.current;
    const point = pointerPosition(event);

    if (!renderer || !point) {
      return;
    }

    const node = renderer.pickNode(point.x, point.y);
    const world = renderer.screenToWorld(point.x, point.y);

    pointerRef.current = {
      pointerId: event.pointerId,
      x: point.x,
      y: point.y,
      nodeId: node?.id ?? null,
      nodeOffsetX: node ? world.x - node.x : 0,
      nodeOffsetY: node ? world.y - node.y : 0,
      dragging: false,
    };

    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function handlePointerMove(event: React.PointerEvent<HTMLDivElement>) {
    const renderer = rendererRef.current;
    const point = pointerPosition(event);

    if (!renderer || !point) {
      return;
    }

    const pointer = pointerRef.current;

    if (pointer?.pointerId === event.pointerId) {
      const distance = Math.abs(point.x - pointer.x) + Math.abs(point.y - pointer.y);

      if (!pointer.dragging && pointer.nodeId && distance > 3) {
        pointer.dragging = true;
        physicsRef.current?.pinNode(pointer.nodeId);
        startPhysicsLoop();
      }

      if (pointer.dragging && pointer.nodeId) {
        const world = renderer.screenToWorld(point.x, point.y);
        const position = clampLocalGraphPosition(
          renderer,
          world.x - pointer.nodeOffsetX,
          world.y - pointer.nodeOffsetY,
        );
        physicsRef.current?.movePinnedNode(position.x, position.y);
        renderer.updateGraphPositions();
        drawLabels();
        startPhysicsLoop();
        return;
      }
    }

    const node = renderer.pickNode(point.x, point.y);
    hoveredNodeIdRef.current = node?.id ?? null;
    drawLabels();
  }

  function handlePointerUp(event: React.PointerEvent<HTMLDivElement>) {
    const renderer = rendererRef.current;
    const point = pointerPosition(event);
    const pointer = pointerRef.current;

    if (!renderer || !point || !pointer || pointer.pointerId !== event.pointerId) {
      return;
    }

    if (pointer.dragging) {
      physicsRef.current?.releaseNode();
      startPhysicsLoop();
    } else {
      const node = renderer.pickNode(point.x, point.y);

      if (node && node.id !== currentObjectId) {
        onOpenObject(node.id);
      }
    }

    pointerRef.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
  }

  function handlePointerCancel(event: React.PointerEvent<HTMLDivElement>) {
    const pointer = pointerRef.current;

    if (!pointer || pointer.pointerId !== event.pointerId) {
      return;
    }

    if (pointer.dragging) {
      physicsRef.current?.releaseNode();
      startPhysicsLoop();
    }

    pointerRef.current = null;
  }

  if (!hasConnections) {
    return null;
  }

  return (
    <div style={styles.localGraphBlock}>
      <div style={styles.contextHeading}>
        <strong>Local graph</strong>
        <span style={styles.contextCount}>{graph.nodes.length - 1}</span>
      </div>

      {renderError ? (
        <span style={styles.muted}>Local graph unavailable.</span>
      ) : (
        <div
          ref={containerRef}
          style={styles.localGraphViewport}
          role="application"
          aria-label="Local graph of direct object connections"
          onPointerDown={handlePointerDown}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerUp}
          onPointerCancel={handlePointerCancel}
          onPointerLeave={() => {
            hoveredNodeIdRef.current = null;
            drawLabels();
          }}
        >
          <canvas ref={canvasRef} style={styles.localGraphCanvas} />
          <canvas ref={overlayRef} style={styles.localGraphOverlay} />
          <button
            type="button"
            style={styles.localGraphRevealButton}
            onPointerDown={(event) => {
              event.stopPropagation();
            }}
            onClick={onRevealInGraph}
            aria-label="Reveal in graph"
            title="Open the workspace graph centered on this object"
          >
            <svg
              aria-hidden="true"
              width="16"
              height="16"
              viewBox="0 0 16 16"
              fill="none"
              xmlns="http://www.w3.org/2000/svg"
            >
              <path
                d="M6 3.5H3.75A1.75 1.75 0 0 0 2 5.25v7A1.75 1.75 0 0 0 3.75 14h7a1.75 1.75 0 0 0 1.75-1.75V10"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
              <path
                d="M8.5 2H14v5.5M13.75 2.25 7.5 8.5"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </button>
        </div>
      )}

      {!renderError && (
        <span style={styles.localGraphHint}>Drag to rearrange · click a neighbour to open</span>
      )}
    </div>
  );
}
