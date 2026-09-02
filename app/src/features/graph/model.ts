import type { WorkspaceGraphEdge, WorkspaceGraphNode } from "../../shared/types";

export type PositionedNode = WorkspaceGraphNode & {
  x: number;
  y: number;
};

export type PositionedGraph = {
  nodes: PositionedNode[];
  edges: WorkspaceGraphEdge[];
};

export type GraphCluster = {
  id: number;
  nodeIds: string[];
  label: string;
  color: [number, number, number];
};

const GRAPH_CLUSTER_COLORS: Array<[number, number, number]> = [
  [0.32, 0.47, 0.74],
  [0.46, 0.61, 0.43],
  [0.68, 0.48, 0.66],
  [0.76, 0.55, 0.34],
  [0.36, 0.62, 0.65],
  [0.7, 0.42, 0.46],
  [0.53, 0.5, 0.72],
  [0.58, 0.58, 0.36],
];

export function detectGraphClusters(graph: PositionedGraph): GraphCluster[] {
  const adjacency = new Map<string, Set<string>>();
  const nodeById = new Map(graph.nodes.map((node) => [node.id, node]));

  for (const node of graph.nodes) {
    adjacency.set(node.id, new Set());
  }

  for (const edge of graph.edges) {
    adjacency.get(edge.source_object_id)?.add(edge.target_object_id);
    adjacency.get(edge.target_object_id)?.add(edge.source_object_id);
  }

  const labels = new Map(graph.nodes.map((node) => [node.id, node.id]));

  for (let iteration = 0; iteration < 8; iteration += 1) {
    let changed = false;
    const orderedNodes = [...graph.nodes].sort(
      (left, right) =>
        (adjacency.get(right.id)?.size ?? 0) - (adjacency.get(left.id)?.size ?? 0) ||
        left.id.localeCompare(right.id),
    );

    for (const node of orderedNodes) {
      const neighbors = adjacency.get(node.id);

      if (!neighbors || neighbors.size === 0) {
        continue;
      }

      const counts = new Map<string, number>();

      for (const neighborId of neighbors) {
        const label = labels.get(neighborId);

        if (label) {
          counts.set(label, (counts.get(label) ?? 0) + 1);
        }
      }

      const nextLabel = [...counts.entries()].sort(
        (left, right) => right[1] - left[1] || left[0].localeCompare(right[0]),
      )[0]?.[0];

      if (nextLabel && nextLabel !== labels.get(node.id)) {
        labels.set(node.id, nextLabel);
        changed = true;
      }
    }

    if (!changed) {
      break;
    }
  }

  const grouped = new Map<string, string[]>();

  for (const node of graph.nodes) {
    const label = labels.get(node.id) ?? node.id;
    const nodeIds = grouped.get(label) ?? [];
    nodeIds.push(node.id);
    grouped.set(label, nodeIds);
  }

  return [...grouped.values()]
    .sort((left, right) => right.length - left.length || left[0].localeCompare(right[0]))
    .map((nodeIds, index) => {
      const representative = [...nodeIds]
        .map((nodeId) => nodeById.get(nodeId))
        .filter((node): node is PositionedNode => Boolean(node))
        .sort(
          (left, right) =>
            right.in_degree + right.out_degree - (left.in_degree + left.out_degree) ||
            left.title.localeCompare(right.title),
        )[0];

      return {
        id: index,
        nodeIds,
        label: representative?.title ?? `Cluster ${index + 1}`,
        color: GRAPH_CLUSTER_COLORS[index % GRAPH_CLUSTER_COLORS.length],
      };
    });
}

export function createNodeClusterMap(clusters: GraphCluster[]) {
  const clusterByNodeId = new Map<string, GraphCluster>();

  for (const cluster of clusters) {
    for (const nodeId of cluster.nodeIds) {
      clusterByNodeId.set(nodeId, cluster);
    }
  }

  return clusterByNodeId;
}
