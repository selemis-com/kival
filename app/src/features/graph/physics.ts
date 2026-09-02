import type { GraphLayoutOptions } from "./layout";
import type { PositionedGraph } from "./model";

type Velocity = { x: number; y: number };

export class GraphPhysics {
  private readonly velocities = new Map<string, Velocity>();
  private pinnedNodeId: string | null = null;
  private alpha = 0;

  constructor(
    private readonly graph: PositionedGraph,
    private options: GraphLayoutOptions,
  ) {
    for (const node of graph.nodes) {
      this.velocities.set(node.id, { x: 0, y: 0 });
    }

    this.reheat(0.35);
  }

  setOptions(options: GraphLayoutOptions) {
    this.options = options;
    this.reheat(0.45);
  }

  pinNode(nodeId: string) {
    this.pinnedNodeId = nodeId;
    this.velocities.set(nodeId, { x: 0, y: 0 });
    this.reheat(0.7);
  }

  movePinnedNode(x: number, y: number) {
    if (!this.pinnedNodeId) {
      return;
    }

    const node = this.graph.nodes.find((candidate) => candidate.id === this.pinnedNodeId);

    if (!node) {
      return;
    }

    node.x = x;
    node.y = y;
    this.velocities.set(node.id, { x: 0, y: 0 });
    this.reheat(0.7);
  }

  releaseNode() {
    this.pinnedNodeId = null;
    this.reheat(0.45);
  }

  reheat(alpha = 1) {
    this.alpha = Math.max(this.alpha, alpha);
  }

  step() {
    if (this.alpha < 0.003 || this.graph.nodes.length === 0) {
      return false;
    }

    const { nodes, edges } = this.graph;
    const indexById = new Map(nodes.map((node, index) => [node.id, index]));
    const forces = nodes.map(() => ({ x: 0, y: 0 }));
    const repulsion = 9000 * this.options.repelForce * this.alpha;

    for (let left = 0; left < nodes.length; left += 1) {
      for (let right = left + 1; right < nodes.length; right += 1) {
        const a = nodes[left];
        const b = nodes[right];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        const distanceSquared = Math.max(dx * dx + dy * dy, 64);
        const distance = Math.sqrt(distanceSquared);
        dx /= distance;
        dy /= distance;

        const force = repulsion / distanceSquared;
        forces[left].x -= dx * force;
        forces[left].y -= dy * force;
        forces[right].x += dx * force;
        forces[right].y += dy * force;
      }
    }

    const linkStrength = 0.018 * this.options.linkForce * this.alpha;

    for (const edge of edges) {
      const sourceIndex = indexById.get(edge.source_object_id);
      const targetIndex = indexById.get(edge.target_object_id);

      if (sourceIndex === undefined || targetIndex === undefined) {
        continue;
      }

      const source = nodes[sourceIndex];
      const target = nodes[targetIndex];
      const dx = target.x - source.x;
      const dy = target.y - source.y;
      const distance = Math.max(Math.hypot(dx, dy), 1);
      const force = (distance - this.options.linkDistance) * linkStrength;
      const forceX = (dx / distance) * force;
      const forceY = (dy / distance) * force;

      forces[sourceIndex].x += forceX;
      forces[sourceIndex].y += forceY;
      forces[targetIndex].x -= forceX;
      forces[targetIndex].y -= forceY;
    }

    for (let index = 0; index < nodes.length; index += 1) {
      const node = nodes[index];

      if (node.id === this.pinnedNodeId) {
        continue;
      }

      const velocity = this.velocities.get(node.id) ?? { x: 0, y: 0 };
      velocity.x += forces[index].x;
      velocity.y += forces[index].y;
      velocity.x += -node.x * 0.0012 * this.options.centerForce * this.alpha;
      velocity.y += -node.y * 0.0012 * this.options.centerForce * this.alpha;
      velocity.x *= 0.82;
      velocity.y *= 0.82;

      const speed = Math.hypot(velocity.x, velocity.y);
      const maxSpeed = 16;

      if (speed > maxSpeed) {
        velocity.x = (velocity.x / speed) * maxSpeed;
        velocity.y = (velocity.y / speed) * maxSpeed;
      }

      node.x += velocity.x;
      node.y += velocity.y;
      this.velocities.set(node.id, velocity);
    }

    this.alpha *= this.pinnedNodeId ? 0.992 : 0.965;
    return true;
  }
}
