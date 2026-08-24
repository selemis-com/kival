import type { WorkspaceGraphEdge, WorkspaceGraphNode } from "../../shared/types";
import {
  createNodeClusterMap,
  detectGraphClusters,
  type PositionedGraph,
  type PositionedNode,
} from "./model";

function hashString(value: string) {
  let hash = 2166136261;

  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }

  return hash >>> 0;
}

export type GraphLayoutOptions = {
  centerForce: number;
  repelForce: number;
  linkForce: number;
  linkDistance: number;
  clusterForce: number;
};

export const DEFAULT_GRAPH_LAYOUT_OPTIONS: GraphLayoutOptions = {
  centerForce: 1,
  repelForce: 1,
  linkForce: 1,
  linkDistance: 120,
  clusterForce: 0.8,
};

export function layoutGraph(
  nodes: WorkspaceGraphNode[],
  edges: WorkspaceGraphEdge[],
  options: GraphLayoutOptions = DEFAULT_GRAPH_LAYOUT_OPTIONS,
): PositionedGraph {
  if (nodes.length === 0) {
    return { nodes: [], edges };
  }

  const radius = Math.max(180, Math.sqrt(nodes.length) * 72);
  const positioned: PositionedNode[] = nodes.map((node, index) => {
    const angle = (index / nodes.length) * Math.PI * 2;
    const jitter = ((hashString(node.id) % 1000) / 1000 - 0.5) * 48;

    return {
      ...node,
      x: Math.cos(angle) * (radius + jitter),
      y: Math.sin(angle) * (radius + jitter),
    };
  });

  const indexById = new Map(positioned.map((node, index) => [node.id, index]));
  const clusters = detectGraphClusters({ nodes: positioned, edges });
  const clusterByNodeId = createNodeClusterMap(clusters);
  const velocities = positioned.map(() => ({ x: 0, y: 0 }));
  const iterations = Math.min(140, 70 + Math.floor(Math.sqrt(nodes.length) * 5));

  for (let iteration = 0; iteration < iterations; iteration += 1) {
    const cooling = 1 - iteration / iterations;
    const repulsion = 12000 * options.repelForce * cooling;
    const attraction = (0.018 + (1 - cooling) * 0.012) * options.linkForce;

    for (let left = 0; left < positioned.length; left += 1) {
      for (let right = left + 1; right < positioned.length; right += 1) {
        const a = positioned[left];
        const b = positioned[right];

        let dx = b.x - a.x;
        let dy = b.y - a.y;
        const distanceSquared = Math.max(dx * dx + dy * dy, 36);
        const distance = Math.sqrt(distanceSquared);
        dx /= distance;
        dy /= distance;

        const force = repulsion / distanceSquared;
        velocities[left].x -= dx * force;
        velocities[left].y -= dy * force;
        velocities[right].x += dx * force;
        velocities[right].y += dy * force;
      }
    }

    for (const edge of edges) {
      const sourceIndex = indexById.get(edge.source_object_id);
      const targetIndex = indexById.get(edge.target_object_id);

      if (sourceIndex === undefined || targetIndex === undefined) {
        continue;
      }

      const source = positioned[sourceIndex];
      const target = positioned[targetIndex];
      const dx = target.x - source.x;
      const dy = target.y - source.y;
      const distance = Math.max(Math.sqrt(dx * dx + dy * dy), 1);
      const desiredDistance = options.linkDistance;
      const force = (distance - desiredDistance) * attraction;

      velocities[sourceIndex].x += (dx / distance) * force;
      velocities[sourceIndex].y += (dy / distance) * force;
      velocities[targetIndex].x -= (dx / distance) * force;
      velocities[targetIndex].y -= (dy / distance) * force;
    }

    const clusterCenters = new Map<number, { x: number; y: number; count: number }>();

    for (const node of positioned) {
      const cluster = clusterByNodeId.get(node.id);

      if (!cluster) {
        continue;
      }

      const center = clusterCenters.get(cluster.id) ?? { x: 0, y: 0, count: 0 };
      center.x += node.x;
      center.y += node.y;
      center.count += 1;
      clusterCenters.set(cluster.id, center);
    }

    for (const center of clusterCenters.values()) {
      center.x /= Math.max(center.count, 1);
      center.y /= Math.max(center.count, 1);
    }

    for (let index = 0; index < positioned.length; index += 1) {
      const node = positioned[index];
      const velocity = velocities[index];
      const cluster = clusterByNodeId.get(node.id);
      const clusterCenter = cluster ? clusterCenters.get(cluster.id) : undefined;

      if (clusterCenter) {
        velocity.x += (clusterCenter.x - node.x) * 0.0045 * options.clusterForce;
        velocity.y += (clusterCenter.y - node.y) * 0.0045 * options.clusterForce;
      }

      velocity.x += -node.x * 0.0018 * options.centerForce;
      velocity.y += -node.y * 0.0018 * options.centerForce;
      velocity.x *= 0.82;
      velocity.y *= 0.82;

      const speed = Math.sqrt(velocity.x * velocity.x + velocity.y * velocity.y);
      const maxSpeed = 18 * cooling + 2;

      if (speed > maxSpeed) {
        velocity.x = (velocity.x / speed) * maxSpeed;
        velocity.y = (velocity.y / speed) * maxSpeed;
      }

      node.x += velocity.x;
      node.y += velocity.y;
    }
  }

  return {
    nodes: positioned,
    edges,
  };
}
