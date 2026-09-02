import { KivalTransportError } from "kival-sdk";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { kival } from "../../shared/api";
import { colors, graphThemes, shadows } from "../../shared/styles/constants";
import { styles } from "../../shared/styles/index";
import { useTheme } from "../../shared/styles/theme";
import type { Workspace, WorkspaceGraphResponse } from "../../shared/types";
import { LoadingIndicator } from "../../shared/ui/LoadingIndicator";
import { GraphRenderer } from "./GraphRenderer";
import { DEFAULT_GRAPH_LAYOUT_OPTIONS, layoutGraph } from "./layout";
import {
  createNodeClusterMap,
  detectGraphClusters,
  type PositionedGraph,
  type PositionedNode,
} from "./model";
import { GraphPhysics } from "./physics";

type Props = {
  workspace: Workspace;
  onOpenObject: (objectId: string) => void;
  focusObjectId?: string | null;
};

type PointerState = {
  pointerId: number;
  x: number;
  y: number;
  cameraX: number;
  cameraY: number;
  nodeId: string | null;
  nodeOffsetX: number;
  nodeOffsetY: number;
  dragging: boolean;
};

function drawGraphOverlay(
  overlay: HTMLCanvasElement,
  renderer: GraphRenderer,
  graph: PositionedGraph,
  hoveredNodeId: string | null,
  hoveredNeighborIds: ReadonlySet<string>,
  showArrows: boolean,
  textFadeThreshold: number,
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

  context.clearRect(0, 0, width, height);
  context.save();
  context.scale(devicePixelRatio, devicePixelRatio);
  context.font = "12px Inter, ui-sans-serif, system-ui, sans-serif";
  context.textBaseline = "middle";

  const camera = renderer.getCamera();
  const nodeById = new Map(graph.nodes.map((node) => [node.id, node]));
  const clusters = detectGraphClusters(graph);
  const clusterByNodeId = createNodeClusterMap(clusters);

  const gridWorldSpacing = camera.zoom < 0.45 ? 180 : camera.zoom < 0.9 ? 90 : 45;
  const topLeft = renderer.screenToWorld(0, 0);
  const bottomRight = renderer.screenToWorld(overlay.clientWidth, overlay.clientHeight);
  const startX = Math.floor(topLeft.x / gridWorldSpacing) * gridWorldSpacing;
  const startY = Math.floor(topLeft.y / gridWorldSpacing) * gridWorldSpacing;

  context.fillStyle = theme.grid;

  for (let worldX = startX; worldX <= bottomRight.x; worldX += gridWorldSpacing) {
    for (let worldY = startY; worldY <= bottomRight.y; worldY += gridWorldSpacing) {
      const point = renderer.worldToScreen({ x: worldX, y: worldY });
      context.beginPath();
      context.arc(point.x, point.y, 0.85, 0, Math.PI * 2);
      context.fill();
    }
  }

  if (showArrows) {
    context.fillStyle = theme.arrow;

    for (const edge of graph.edges) {
      const source = nodeById.get(edge.source_object_id);
      const target = nodeById.get(edge.target_object_id);

      if (!source || !target) {
        continue;
      }

      const sourcePoint = renderer.worldToScreen(source);
      const targetPoint = renderer.worldToScreen(target);
      const dx = targetPoint.x - sourcePoint.x;
      const dy = targetPoint.y - sourcePoint.y;
      const distance = Math.hypot(dx, dy);

      if (distance < 24) {
        continue;
      }

      const ux = dx / distance;
      const uy = dy / distance;
      const tipX = targetPoint.x - ux * 11;
      const tipY = targetPoint.y - uy * 11;
      const size = 5;

      context.globalAlpha = edge.kind === "reference" ? 0.45 : 1;
      context.beginPath();
      context.moveTo(tipX, tipY);
      context.lineTo(tipX - ux * 8 - uy * size, tipY - uy * 8 + ux * size);
      context.lineTo(tipX - ux * 8 + uy * size, tipY - uy * 8 - ux * size);
      context.closePath();
      context.fill();
    }

    context.globalAlpha = 1;
  }

  for (const node of graph.nodes) {
    const point = renderer.worldToScreen(node);

    if (
      point.x < -100 ||
      point.y < -30 ||
      point.x > overlay.clientWidth + 100 ||
      point.y > overlay.clientHeight + 30
    ) {
      continue;
    }

    const isHovered = hoveredNodeId === node.id;
    const isNeighbor = hoveredNeighborIds.has(node.id);
    const cluster = clusterByNodeId.get(node.id);
    const isClusterHub = cluster?.nodeIds[0] === node.id;
    const shouldShow =
      isHovered ||
      isNeighbor ||
      camera.zoom >= textFadeThreshold ||
      isClusterHub ||
      node.in_degree + node.out_degree >= 6;

    if (!shouldShow) {
      continue;
    }

    const label = node.title.length > 34 ? `${node.title.slice(0, 33)}…` : node.title;
    const emphasized = isHovered || isNeighbor;

    context.globalAlpha = hoveredNodeId && !isHovered && !isNeighbor ? 0.16 : emphasized ? 1 : 0.78;
    context.font = "500 11.5px Inter, ui-sans-serif, system-ui, sans-serif";

    context.lineWidth = emphasized ? 4 : 3;
    context.strokeStyle = theme.labelStroke;
    context.strokeText(label, point.x + 15, point.y);

    context.fillStyle = isHovered
      ? theme.labelHovered
      : emphasized
        ? theme.labelEmphasized
        : theme.label;
    context.fillText(label, point.x + 15, point.y);
  }

  context.globalAlpha = 1;
  context.restore();
}

function referenceCountForNode(graph: PositionedGraph, nodeId: string) {
  return graph.edges.filter(
    (edge) =>
      (edge.source_object_id === nodeId || edge.target_object_id === nodeId) &&
      (edge.kind === "reference" || edge.kind === "relationship_and_reference"),
  ).length;
}

const toolbarStyle = {
  position: "absolute",
  top: 14,
  left: 14,
  zIndex: 20,
  pointerEvents: "auto",
  display: "flex",
  alignItems: "center",
  gap: 8,
} as const;

export function GraphView({ workspace, onOpenObject, focusObjectId = null }: Props) {
  const { resolvedTheme } = useTheme();
  const graphTheme = graphThemes[resolvedTheme];
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const overlayRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<GraphRenderer | null>(null);
  const graphRef = useRef<PositionedGraph>({ nodes: [], edges: [] });
  const physicsRef = useRef<GraphPhysics | null>(null);
  const physicsFrameRef = useRef<number | null>(null);
  const pointerRef = useRef<PointerState | null>(null);
  const hoveredNodeRef = useRef<PositionedNode | null>(null);
  const focusedNodeIdRef = useRef<string | null>(null);
  const focusedNeighborIdsRef = useRef(new Set<string>());

  const [response, setResponse] = useState<WorkspaceGraphResponse | null>(null);
  const [hoveredNode, setHoveredNode] = useState<PositionedNode | null>(null);
  const [tooltipPosition, setTooltipPosition] = useState({ x: 0, y: 0 });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [graphQuery, setGraphQuery] = useState("");
  const layoutOptions = DEFAULT_GRAPH_LAYOUT_OPTIONS;
  const showArrowsRef = useRef(false);
  const textFadeThresholdRef = useRef(0.62);

  const filteredGraphData = useMemo(() => {
    if (!response) {
      return null;
    }

    const normalizedQuery = graphQuery.trim().toLowerCase();
    const visibleNodeIds = new Set(response.nodes.map((node) => node.id));

    if (!normalizedQuery) {
      return {
        nodes: response.nodes.filter((node) => visibleNodeIds.has(node.id)),
        edges: response.edges.filter(
          (edge) =>
            visibleNodeIds.has(edge.source_object_id) && visibleNodeIds.has(edge.target_object_id),
        ),
      };
    }

    const matchedNodeIds = new Set(
      response.nodes
        .filter(
          (node) =>
            visibleNodeIds.has(node.id) && node.title.toLowerCase().includes(normalizedQuery),
        )
        .map((node) => node.id),
    );
    const includedNodeIds = new Set(matchedNodeIds);

    for (const edge of response.edges) {
      if (matchedNodeIds.has(edge.source_object_id)) {
        includedNodeIds.add(edge.target_object_id);
      }

      if (matchedNodeIds.has(edge.target_object_id)) {
        includedNodeIds.add(edge.source_object_id);
      }
    }

    return {
      nodes: response.nodes.filter(
        (node) => visibleNodeIds.has(node.id) && includedNodeIds.has(node.id),
      ),
      edges: response.edges.filter(
        (edge) =>
          includedNodeIds.has(edge.source_object_id) && includedNodeIds.has(edge.target_object_id),
      ),
    };
  }, [graphQuery, response]);

  const getNeighborIds = useCallback((nodeId: string) => {
    const neighbors = new Set<string>();

    for (const edge of graphRef.current.edges) {
      if (edge.source_object_id === nodeId) {
        neighbors.add(edge.target_object_id);
      } else if (edge.target_object_id === nodeId) {
        neighbors.add(edge.source_object_id);
      }
    }

    return neighbors;
  }, []);

  const drawLabels = useCallback(() => {
    const overlay = overlayRef.current;
    const renderer = rendererRef.current;

    if (!overlay || !renderer) {
      return;
    }

    drawGraphOverlay(
      overlay,
      renderer,
      graphRef.current,
      focusedNodeIdRef.current,
      focusedNeighborIdsRef.current,
      showArrowsRef.current,
      textFadeThresholdRef.current,
      graphTheme,
    );
  }, [graphTheme]);

  const startPhysicsLoop = useCallback(() => {
    if (physicsFrameRef.current !== null) {
      return;
    }

    const tick = () => {
      const physics = physicsRef.current;
      const renderer = rendererRef.current;

      if (!physics || !renderer || !physics.step()) {
        physicsFrameRef.current = null;
        return;
      }

      renderer.updateGraphPositions();
      drawLabels();
      physicsFrameRef.current = requestAnimationFrame(tick);
    };

    physicsFrameRef.current = requestAnimationFrame(tick);
  }, [drawLabels]);

  function clearGraphFocus() {
    focusedNodeIdRef.current = null;
    focusedNeighborIdsRef.current = new Set();
    rendererRef.current?.setFocusedNode(null);
    rendererRef.current?.setSelectedNode(null);
    drawLabels();
  }

  const focusGraphNode = useCallback(
    (node: PositionedNode) => {
      const renderer = rendererRef.current;

      if (!renderer) {
        return;
      }

      const neighborIds = getNeighborIds(node.id);
      focusedNodeIdRef.current = node.id;
      focusedNeighborIdsRef.current = neighborIds;
      renderer.setFocusedNode(node.id, neighborIds);
      renderer.setSelectedNode(node.id);
      drawLabels();
    },
    [drawLabels, getNeighborIds],
  );

  useEffect(() => {
    const controller = new AbortController();

    setLoading(true);
    setError(null);

    void kival
      .getWorkspaceGraph({ workspaceId: workspace.id, signal: controller.signal })
      .then((graphResponse) => {
        setResponse(graphResponse);
      })
      .catch((error: unknown) => {
        if (error instanceof KivalTransportError && error.kind === "abort") {
          return;
        }

        setError(error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setLoading(false);
        }
      });

    return () => controller.abort();
  }, [workspace.id]);

  useEffect(() => {
    if (!response) {
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
      const message = error instanceof Error ? error.message : String(error);
      setError(message);
      return;
    }

    rendererRef.current = renderer;

    return () => {
      renderer.destroy();
      rendererRef.current = null;
    };
  }, [response, resolvedTheme]);

  useEffect(() => {
    rendererRef.current?.setTheme(resolvedTheme);
    drawLabels();
  }, [drawLabels, resolvedTheme]);

  useEffect(() => {
    const renderer = rendererRef.current;

    if (!renderer || !filteredGraphData) {
      return;
    }

    const graph = layoutGraph(filteredGraphData.nodes, filteredGraphData.edges, layoutOptions);
    hoveredNodeRef.current = null;
    focusedNodeIdRef.current = null;
    focusedNeighborIdsRef.current = new Set();
    setHoveredNode(null);

    graphRef.current = graph;
    physicsRef.current = new GraphPhysics(graph, layoutOptions);
    renderer.setGraph(graph);
    renderer.setNodeScale(1);

    if (focusObjectId) {
      const focusedNode = graph.nodes.find((node) => node.id === focusObjectId);

      if (focusedNode) {
        focusGraphNode(focusedNode);
        const camera = renderer.getCamera();
        renderer.setCamera({
          x: focusedNode.x,
          y: focusedNode.y,
          zoom: Math.max(camera.zoom, 1.15),
        });
      }
    }

    startPhysicsLoop();
    drawLabels();
  }, [drawLabels, filteredGraphData, focusGraphNode, focusObjectId, startPhysicsLoop]);

  useEffect(() => {
    return () => {
      if (physicsFrameRef.current !== null) {
        cancelAnimationFrame(physicsFrameRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!response) {
      return;
    }

    const container = containerRef.current;

    if (!container) {
      return;
    }

    const observer = new ResizeObserver(() => {
      const renderer = rendererRef.current;
      const overlay = overlayRef.current;

      if (!renderer || !overlay) {
        return;
      }

      renderer.render();
      drawLabels();
    });

    observer.observe(container);

    return () => observer.disconnect();
  }, [drawLabels, response]);

  function handlePointerMove(event: React.PointerEvent<HTMLDivElement>) {
    const renderer = rendererRef.current;
    const container = containerRef.current;

    if (!renderer || !container) {
      return;
    }

    const rect = container.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    const pointer = pointerRef.current;

    if (pointer && pointer.pointerId === event.pointerId) {
      const dx = x - pointer.x;
      const dy = y - pointer.y;

      if (!pointer.dragging && Math.abs(dx) + Math.abs(dy) > 3) {
        pointer.dragging = true;

        if (pointer.nodeId) {
          physicsRef.current?.pinNode(pointer.nodeId);
          startPhysicsLoop();
        }
      }

      if (pointer.dragging) {
        if (pointer.nodeId) {
          const world = renderer.screenToWorld(x, y);
          physicsRef.current?.movePinnedNode(
            world.x - pointer.nodeOffsetX,
            world.y - pointer.nodeOffsetY,
          );
          renderer.updateGraphPositions();
          startPhysicsLoop();
        } else {
          const camera = renderer.getCamera();
          renderer.setCamera({
            ...camera,
            x: pointer.cameraX - dx / camera.zoom,
            y: pointer.cameraY - dy / camera.zoom,
          });
        }
        hoveredNodeRef.current = null;
        setHoveredNode(null);
        drawLabels();
        return;
      }
    }

    const node = renderer.pickNode(x, y);

    hoveredNodeRef.current = node;
    setHoveredNode(node);
    setTooltipPosition({ x: x + 14, y: y + 14 });
    drawLabels();
  }

  function handlePointerDown(event: React.PointerEvent<HTMLDivElement>) {
    const renderer = rendererRef.current;

    if (!renderer) {
      return;
    }

    const camera = renderer.getCamera();
    const container = containerRef.current;

    if (!container) {
      return;
    }

    const rect = container.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    const node = renderer.pickNode(x, y);
    const world = renderer.screenToWorld(x, y);

    pointerRef.current = {
      pointerId: event.pointerId,
      x,
      y,
      cameraX: camera.x,
      cameraY: camera.y,
      nodeId: node?.id ?? null,
      nodeOffsetX: node ? world.x - node.x : 0,
      nodeOffsetY: node ? world.y - node.y : 0,
      dragging: false,
    };

    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function handlePointerUp(event: React.PointerEvent<HTMLDivElement>) {
    const renderer = rendererRef.current;
    const container = containerRef.current;
    const pointer = pointerRef.current;

    if (!renderer || !container || !pointer || pointer.pointerId !== event.pointerId) {
      return;
    }

    if (pointer.nodeId && pointer.dragging) {
      physicsRef.current?.releaseNode();
      startPhysicsLoop();
    }

    if (!pointer.dragging) {
      const rect = container.getBoundingClientRect();
      const node = renderer.pickNode(event.clientX - rect.left, event.clientY - rect.top);

      if (node) {
        focusGraphNode(node);
      } else {
        clearGraphFocus();
      }
    }

    pointerRef.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
  }

  function handleDoubleClick(event: React.MouseEvent<HTMLDivElement>) {
    const renderer = rendererRef.current;
    const container = containerRef.current;

    if (!renderer || !container) {
      return;
    }

    const rect = container.getBoundingClientRect();
    const node = renderer.pickNode(event.clientX - rect.left, event.clientY - rect.top);

    if (node) {
      onOpenObject(node.id);
    }
  }

  useEffect(() => {
    if (!response) {
      return;
    }

    const graphElement = containerRef.current;

    if (graphElement === null) {
      return;
    }

    const element: HTMLDivElement = graphElement;

    function handleWheel(event: WheelEvent) {
      const renderer = rendererRef.current;
      const overlay = overlayRef.current;

      if (!renderer || !overlay) {
        return;
      }

      event.preventDefault();

      const rect = element.getBoundingClientRect();
      const x = event.clientX - rect.left;
      const y = event.clientY - rect.top;
      const before = renderer.screenToWorld(x, y);
      const camera = renderer.getCamera();
      const zoomFactor = Math.exp(-event.deltaY * 0.0012);
      const zoom = Math.min(4, Math.max(0.18, camera.zoom * zoomFactor));

      renderer.setCamera({
        x: before.x - (x - element.clientWidth / 2) / zoom,
        y: before.y - (y - element.clientHeight / 2) / zoom,
        zoom,
      });

      drawGraphOverlay(
        overlay,
        renderer,
        graphRef.current,
        focusedNodeIdRef.current,
        focusedNeighborIdsRef.current,
        showArrowsRef.current,
        textFadeThresholdRef.current,
        graphTheme,
      );
    }

    element.addEventListener("wheel", handleWheel, { passive: false });

    return () => {
      element.removeEventListener("wheel", handleWheel);
    };
  }, [response, graphTheme]);

  function stopGraphPointerEvent(event: React.PointerEvent<HTMLElement>) {
    event.stopPropagation();
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    if (
      event.target instanceof HTMLInputElement ||
      event.target instanceof HTMLTextAreaElement ||
      event.target instanceof HTMLSelectElement
    ) {
      return;
    }

    const renderer = rendererRef.current;

    if (!renderer) {
      return;
    }

    if (event.key === "Escape") {
      clearGraphFocus();
      return;
    }

    if (event.key === "+" || event.key === "=") {
      renderer.zoomBy(1.18);
      drawLabels();
      return;
    }

    if (event.key === "-") {
      renderer.zoomBy(1 / 1.18);
      drawLabels();
      return;
    }

    const panAmount = event.shiftKey ? 120 : 48;

    if (event.key === "ArrowLeft") {
      renderer.panByScreenPixels(-panAmount, 0);
    } else if (event.key === "ArrowRight") {
      renderer.panByScreenPixels(panAmount, 0);
    } else if (event.key === "ArrowUp") {
      renderer.panByScreenPixels(0, -panAmount);
    } else if (event.key === "ArrowDown") {
      renderer.panByScreenPixels(0, panAmount);
    } else {
      return;
    }

    event.preventDefault();
    drawLabels();
  }

  const hoveredReferenceCount = hoveredNode
    ? referenceCountForNode(graphRef.current, hoveredNode.id)
    : 0;

  return (
    <section style={styles.graphPage}>
      {loading && <LoadingIndicator label="Loading graph…" />}

      {!loading && error && (
        <div style={styles.errorBox}>
          <strong>Could not load graph</strong>
          <span>{error}</span>
        </div>
      )}

      {!loading && !error && response && (
        <div
          ref={containerRef}
          style={{ ...styles.graphViewport, outline: "none" }}
          role="application"
          aria-label={`${workspace.name} graph`}
          // biome-ignore lint/a11y/noNoninteractiveTabindex: The graph viewport is an application-style canvas with keyboard pan and zoom controls.
          tabIndex={0}
          onKeyDown={handleKeyDown}
          onPointerMove={handlePointerMove}
          onPointerDown={handlePointerDown}
          onPointerUp={handlePointerUp}
          onDoubleClick={handleDoubleClick}
          onPointerCancel={() => {
            const pointer = pointerRef.current;

            if (pointer?.nodeId && pointer.dragging) {
              physicsRef.current?.releaseNode();
              startPhysicsLoop();
            }

            pointerRef.current = null;
          }}
          onPointerLeave={() => {
            if (!pointerRef.current) {
              hoveredNodeRef.current = null;
              setHoveredNode(null);
              drawLabels();
            }
          }}
        >
          <div
            style={toolbarStyle}
            role="toolbar"
            aria-label="Graph controls"
            onPointerDown={stopGraphPointerEvent}
            onPointerMove={stopGraphPointerEvent}
            onPointerUp={stopGraphPointerEvent}
          >
            <input
              type="search"
              data-1p-ignore="true"
              autoComplete="off"
              value={graphQuery}
              placeholder="Filter graph…"
              onChange={(event) => setGraphQuery(event.target.value)}
              style={{
                width: 220,
                minHeight: 36,
                padding: "0 11px",
                border: `1px solid ${colors.borderStrong}`,
                borderRadius: 10,
                background: colors.glass,
                boxShadow: shadows.small,
                color: colors.text,
                font: "inherit",
                fontSize: 13,
                outline: "none",
              }}
            />
          </div>

          <canvas
            ref={canvasRef}
            style={{ ...styles.graphCanvas, zIndex: 0, pointerEvents: "none" }}
          />
          <canvas
            ref={overlayRef}
            style={{ ...styles.graphOverlay, zIndex: 1, pointerEvents: "none" }}
          />

          {hoveredNode && (
            <div
              style={{
                ...styles.graphTooltip,
                left: tooltipPosition.x,
                top: tooltipPosition.y,
              }}
            >
              <strong>{hoveredNode.title}</strong>
              <span style={styles.graphTooltipMeta}>
                {hoveredNode.in_degree + hoveredNode.out_degree} connections
                {hoveredReferenceCount > 0 &&
                  ` · ${hoveredReferenceCount} ${
                    hoveredReferenceCount === 1 ? "reference" : "references"
                  }`}
              </span>
            </div>
          )}

          <div style={styles.graphHint}>
            Drag nodes to rearrange · Drag empty space to pan · Scroll to zoom · Double-click to
            open
          </div>
        </div>
      )}
    </section>
  );
}
