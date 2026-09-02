import { graphThemes, type ResolvedTheme } from "../../shared/styles/constants";
import {
  createNodeClusterMap,
  detectGraphClusters,
  type PositionedGraph,
  type PositionedNode,
} from "./model";

type Camera = {
  x: number;
  y: number;
  zoom: number;
};

const FIELD_VERTEX_SHADER = `#version 300 es
in vec2 a_corner;
in vec2 a_source;
in vec2 a_target;
in vec3 a_color;
in float a_radius;
in float a_strength;

uniform vec2 u_center;
uniform vec2 u_resolution;
uniform float u_zoom;
uniform float u_pixel_ratio;

out vec2 v_local;
flat out float v_length;
flat out vec3 v_color;
flat out float v_radius;
flat out float v_strength;

void main() {
  vec2 sourcePixel = (a_source - u_center) * u_zoom;
  vec2 targetPixel = (a_target - u_center) * u_zoom;
  vec2 delta = targetPixel - sourcePixel;
  float lengthPixels = length(delta);
  vec2 direction = lengthPixels > 0.0001 ? delta / lengthPixels : vec2(1.0, 0.0);
  vec2 normal = vec2(-direction.y, direction.x);
  float radius = a_radius * u_pixel_ratio;

  float along = (a_corner.x + 1.0) * 0.5;
  float localX = mix(-radius, lengthPixels + radius, along);
  float localY = a_corner.y * radius;
  vec2 pixelPosition = sourcePixel + direction * localX + normal * localY;
  vec2 clip = pixelPosition * 2.0 / u_resolution;

  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
  v_local = vec2(localX, localY);
  v_length = lengthPixels;
  v_color = a_color;
  v_radius = radius;
  v_strength = a_strength;
}
`;

const FIELD_FRAGMENT_SHADER = `#version 300 es
precision highp float;

in vec2 v_local;
flat in float v_length;
flat in vec3 v_color;
flat in float v_radius;
flat in float v_strength;
layout(location = 0) out vec4 outDensity;

void main() {
  float nearestX = clamp(v_local.x, 0.0, v_length);
  float distanceToField = length(v_local - vec2(nearestX, 0.0));
  float normalizedDistance = distanceToField / max(v_radius, 0.0001);

  if (normalizedDistance >= 1.0) {
    discard;
  }

  float density = exp(-4.2 * normalizedDistance * normalizedDistance) * v_strength;
  outDensity = vec4(v_color * density, density);
}
`;

const FIELD_COMPOSITE_VERTEX_SHADER = `#version 300 es
in vec2 a_position;
out vec2 v_uv;

void main() {
  v_uv = a_position * 0.5 + 0.5;
  gl_Position = vec4(a_position, 0.0, 1.0);
}
`;

const FIELD_COMPOSITE_FRAGMENT_SHADER = `#version 300 es
precision highp float;

in vec2 v_uv;
uniform sampler2D u_field;
uniform vec2 u_texel_size;
uniform vec3 u_field_light_color;
out vec4 outColor;

float densityAt(vec2 uv) {
  return texture(u_field, uv).a;
}

void main() {
  vec4 field = texture(u_field, v_uv);
  float density = field.a;

  if (density <= 0.002) {
    discard;
  }

  vec3 color = field.rgb / max(density, 0.0001);

  float left = densityAt(v_uv - vec2(u_texel_size.x, 0.0));
  float right = densityAt(v_uv + vec2(u_texel_size.x, 0.0));
  float up = densityAt(v_uv - vec2(0.0, u_texel_size.y));
  float down = densityAt(v_uv + vec2(0.0, u_texel_size.y));

  vec2 gradient = vec2(right - left, down - up);
  float slope = clamp(length(gradient) * 7.0, 0.0, 1.0);

  float body = smoothstep(0.055, 0.22, density);
  float outer = smoothstep(0.018, 0.085, density);
  float contourA = 1.0 - smoothstep(0.0, 0.010, abs(density - 0.070));
  float contourB = 1.0 - smoothstep(0.0, 0.012, abs(density - 0.145));
  float contour = max(contourA * 0.34, contourB * 0.22);

  vec3 saturated = mix(vec3(dot(color, vec3(0.2126, 0.7152, 0.0722))), color, 1.32);
  vec3 lit = mix(saturated, u_field_light_color, body * 0.08);
  lit = mix(lit, saturated * 0.68, slope * 0.16);

  float alpha = outer * 0.11 + body * 0.16 + contour * 0.055;

  outColor = vec4(lit, alpha);
}
`;

const EDGE_VERTEX_SHADER = `#version 300 es
in vec2 a_corner;
in vec2 a_source;
in vec2 a_target;
in float a_state;
in float a_kind;

uniform vec2 u_center;
uniform vec2 u_resolution;
uniform float u_zoom;
uniform float u_pixel_ratio;

out float v_side;
out float v_along;
flat out float v_length;
flat out float v_state;
flat out float v_kind;

void main() {
  vec2 sourcePixel = (a_source - u_center) * u_zoom;
  vec2 targetPixel = (a_target - u_center) * u_zoom;
  vec2 delta = targetPixel - sourcePixel;
  float lengthPixels = max(length(delta), 0.0001);
  vec2 direction = delta / lengthPixels;
  vec2 normal = vec2(-direction.y, direction.x);

  float along = (a_corner.x + 1.0) * 0.5;
  float halfWidth = 1.15 * u_pixel_ratio;
  vec2 pixelPosition = mix(sourcePixel, targetPixel, along) + normal * a_corner.y * halfWidth;
  vec2 clip = pixelPosition * 2.0 / u_resolution;

  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
  v_side = a_corner.y;
  v_along = along;
  v_length = lengthPixels;
  v_state = a_state;
  v_kind = a_kind;
}
`;

const EDGE_FRAGMENT_SHADER = `#version 300 es
precision mediump float;

in float v_side;
in float v_along;
flat in float v_length;
flat in float v_state;
flat in float v_kind;
uniform float u_focus_progress;
uniform vec3 u_edge_base_color;
uniform vec3 u_edge_focused_color;
out vec4 outColor;

void main() {
  vec3 baseColor = u_edge_base_color;
  vec3 focusedColor = u_edge_focused_color;
  float focusedAlpha = v_state < -0.5 ? 0.055 : v_state > 0.5 ? 0.72 : 0.34;
  float alpha = mix(0.30, focusedAlpha, u_focus_progress);
  vec3 color = mix(baseColor, focusedColor, max(v_state, 0.0) * u_focus_progress);

  float referenceOnly = step(0.5, v_kind) * (1.0 - step(1.5, v_kind));
  float relationshipCoverage = 1.0 - smoothstep(0.58, 1.0, abs(v_side));
  float referenceCoverage = 1.0 - smoothstep(0.42, 0.86, abs(v_side));
  float dashCount = max(v_length / 11.0, 1.0);
  float dashPhase = fract(v_along * dashCount);
  float dash = 1.0 - smoothstep(0.58, 0.72, dashPhase);
  float coverage = mix(relationshipCoverage, referenceCoverage * dash, referenceOnly);
  float kindAlpha = mix(1.0, 0.48, referenceOnly);

  outColor = vec4(color, alpha * coverage * kindAlpha);
}
`;

const NODE_VERTEX_SHADER = `#version 300 es
in vec2 a_corner;
in vec2 a_position;
in float a_radius;
in float a_state;
in vec3 a_color;

uniform vec2 u_center;
uniform vec2 u_resolution;
uniform float u_zoom;
uniform float u_pixel_ratio;

out vec2 v_corner;
flat out float v_state;
flat out vec3 v_color;

void main() {
  vec2 pixelCenter = (a_position - u_center) * u_zoom;
  vec2 pixelPosition = pixelCenter + a_corner * a_radius * 1.42 * u_pixel_ratio;
  vec2 clip = pixelPosition * 2.0 / u_resolution;

  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
  v_corner = a_corner * 1.42;
  v_state = a_state;
  v_color = a_color;
}
`;

const NODE_FRAGMENT_SHADER = `#version 300 es
precision highp float;

in vec2 v_corner;
flat in float v_state;
flat in vec3 v_color;
uniform float u_focus_progress;
uniform vec3 u_node_neutral_color;
uniform vec3 u_node_connected_color;
uniform vec3 u_node_selected_color;
uniform vec3 u_node_rim_color;
uniform vec3 u_node_rim_connected_color;
uniform vec3 u_node_rim_selected_color;
out vec4 outColor;

void main() {
  float distanceFromCenter = length(v_corner);
  float edgeWidth = max(fwidth(distanceFromCenter), 0.0015);

  float unrelated = v_state < -0.5 ? u_focus_progress : 0.0;
  float connected = v_state > 0.5 && v_state < 1.5 ? u_focus_progress : 0.0;
  float selected = v_state > 1.5 ? 1.0 : 0.0;

  vec3 neutralFill = mix(u_node_neutral_color, v_color, 0.56);
  vec3 connectedFill = mix(u_node_connected_color, v_color, 0.40);
  vec3 selectedFill = mix(u_node_selected_color, v_color, 0.28);
  vec3 fill = mix(neutralFill, connectedFill, connected);
  fill = mix(fill, selectedFill, selected);

  float coreCoverage = 1.0 - smoothstep(1.0 - edgeWidth, 1.0 + edgeWidth, distanceFromCenter);
  float rimInner = smoothstep(0.72 - edgeWidth, 0.72 + edgeWidth, distanceFromCenter);
  float rimOuter = 1.0 - smoothstep(1.0 - edgeWidth, 1.0 + edgeWidth, distanceFromCenter);
  float rim = rimInner * rimOuter;

  float haloInner = smoothstep(1.0 - edgeWidth, 1.0 + edgeWidth, distanceFromCenter);
  float haloOuter =
    1.0 - smoothstep(1.42 - edgeWidth * 1.5, 1.42 + edgeWidth * 1.5, distanceFromCenter);
  float halo = haloInner * haloOuter;

  vec3 rimColor = mix(u_node_rim_color, u_node_rim_selected_color, selected);
  rimColor = mix(rimColor, u_node_rim_connected_color, connected);

  float dimAlpha = mix(1.0, 0.10, unrelated);
  float haloStrength = mix(0.08, 0.20, max(connected, selected));
  vec3 color = mix(fill, rimColor, rim * 0.72);

  float alpha = coreCoverage * dimAlpha + halo * haloStrength * dimAlpha;

  if (alpha <= 0.001) {
    discard;
  }

  vec3 finalColor = mix(rimColor, color, coreCoverage);
  outColor = vec4(finalColor, alpha);
}
`;

function compileShader(gl: WebGL2RenderingContext, type: number, source: string) {
  const shader = gl.createShader(type);

  if (!shader) {
    throw new Error("Could not create WebGL shader.");
  }

  gl.shaderSource(shader, source);
  gl.compileShader(shader);

  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const message = gl.getShaderInfoLog(shader) ?? "Unknown shader compile error.";
    gl.deleteShader(shader);
    throw new Error(message);
  }

  return shader;
}

function createProgram(gl: WebGL2RenderingContext, vertexSource: string, fragmentSource: string) {
  const program = gl.createProgram();

  if (!program) {
    throw new Error("Could not create WebGL program.");
  }

  const vertexShader = compileShader(gl, gl.VERTEX_SHADER, vertexSource);
  const fragmentShader = compileShader(gl, gl.FRAGMENT_SHADER, fragmentSource);

  gl.attachShader(program, vertexShader);
  gl.attachShader(program, fragmentShader);
  gl.linkProgram(program);
  gl.deleteShader(vertexShader);
  gl.deleteShader(fragmentShader);

  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const message = gl.getProgramInfoLog(program) ?? "Unknown WebGL link error.";
    gl.deleteProgram(program);
    throw new Error(message);
  }

  return program;
}

function requireLocation(location: number, name: string) {
  if (location < 0) {
    throw new Error(`Missing WebGL attribute: ${name}`);
  }

  return location;
}

function requireUniform(location: WebGLUniformLocation | null, name: string) {
  if (!location) {
    throw new Error(`Missing WebGL uniform: ${name}`);
  }

  return location;
}

export class GraphRenderer {
  private readonly gl: WebGL2RenderingContext;
  private readonly fieldProgram: WebGLProgram;
  private readonly fieldCompositeProgram: WebGLProgram;
  private readonly edgeProgram: WebGLProgram;
  private readonly nodeProgram: WebGLProgram;
  private readonly fieldSourceBuffer: WebGLBuffer;
  private readonly fieldTargetBuffer: WebGLBuffer;
  private readonly fieldColorBuffer: WebGLBuffer;
  private readonly fieldRadiusBuffer: WebGLBuffer;
  private readonly fieldStrengthBuffer: WebGLBuffer;
  private readonly edgeSourceBuffer: WebGLBuffer;
  private readonly edgeTargetBuffer: WebGLBuffer;
  private readonly edgeStateBuffer: WebGLBuffer;
  private readonly edgeKindBuffer: WebGLBuffer;
  private readonly nodePositionBuffer: WebGLBuffer;
  private readonly nodeRadiusBuffer: WebGLBuffer;
  private readonly nodeStateBuffer: WebGLBuffer;
  private readonly nodeColorBuffer: WebGLBuffer;
  private readonly fieldVao: WebGLVertexArrayObject;
  private readonly fieldCompositeVao: WebGLVertexArrayObject;
  private readonly fieldFramebuffer: WebGLFramebuffer;
  private readonly fieldTexture: WebGLTexture;
  private readonly nodeVao: WebGLVertexArrayObject;
  private readonly edgeVao: WebGLVertexArrayObject;

  private graph: PositionedGraph = { nodes: [], edges: [] };
  private camera: Camera = { x: 0, y: 0, zoom: 1 };
  private focusedNodeId: string | null = null;
  private focusedNeighborIds = new Set<string>();
  private selectedNodeId: string | null = null;
  private fieldInstanceCount = 0;
  private fieldTextureWidth = 0;
  private fieldTextureHeight = 0;
  private edgeInstanceCount = 0;
  private nodeScale = 1;
  private focusProgress = 0;
  private focusTarget = 0;
  private focusAnimationFrame: number | null = null;
  private focusAnimationStartedAt = 0;
  private focusAnimationStartValue = 0;
  private theme: ResolvedTheme = "light";

  constructor(private readonly canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", {
      alpha: false,
      antialias: true,
      preserveDrawingBuffer: false,
    });

    if (!gl) {
      throw new Error("WebGL 2 is required to render the graph.");
    }

    this.gl = gl;
    this.fieldProgram = createProgram(gl, FIELD_VERTEX_SHADER, FIELD_FRAGMENT_SHADER);
    this.fieldCompositeProgram = createProgram(
      gl,
      FIELD_COMPOSITE_VERTEX_SHADER,
      FIELD_COMPOSITE_FRAGMENT_SHADER,
    );
    this.edgeProgram = createProgram(gl, EDGE_VERTEX_SHADER, EDGE_FRAGMENT_SHADER);
    this.nodeProgram = createProgram(gl, NODE_VERTEX_SHADER, NODE_FRAGMENT_SHADER);

    const fieldSourceBuffer = gl.createBuffer();
    const fieldTargetBuffer = gl.createBuffer();
    const fieldColorBuffer = gl.createBuffer();
    const fieldRadiusBuffer = gl.createBuffer();
    const fieldStrengthBuffer = gl.createBuffer();
    const edgeSourceBuffer = gl.createBuffer();
    const edgeTargetBuffer = gl.createBuffer();
    const edgeStateBuffer = gl.createBuffer();
    const edgeKindBuffer = gl.createBuffer();
    const nodePositionBuffer = gl.createBuffer();
    const nodeRadiusBuffer = gl.createBuffer();
    const nodeStateBuffer = gl.createBuffer();
    const nodeColorBuffer = gl.createBuffer();
    const fieldVao = gl.createVertexArray();
    const fieldCompositeVao = gl.createVertexArray();
    const fieldFramebuffer = gl.createFramebuffer();
    const fieldTexture = gl.createTexture();
    const nodeVao = gl.createVertexArray();
    const edgeVao = gl.createVertexArray();

    if (
      !fieldSourceBuffer ||
      !fieldTargetBuffer ||
      !fieldColorBuffer ||
      !fieldRadiusBuffer ||
      !fieldStrengthBuffer ||
      !edgeSourceBuffer ||
      !edgeTargetBuffer ||
      !edgeStateBuffer ||
      !edgeKindBuffer ||
      !nodePositionBuffer ||
      !nodeRadiusBuffer ||
      !nodeStateBuffer ||
      !nodeColorBuffer ||
      !fieldVao ||
      !fieldCompositeVao ||
      !fieldFramebuffer ||
      !fieldTexture ||
      !nodeVao ||
      !edgeVao
    ) {
      throw new Error("Could not allocate WebGL graph buffers.");
    }

    this.fieldSourceBuffer = fieldSourceBuffer;
    this.fieldTargetBuffer = fieldTargetBuffer;
    this.fieldColorBuffer = fieldColorBuffer;
    this.fieldRadiusBuffer = fieldRadiusBuffer;
    this.fieldStrengthBuffer = fieldStrengthBuffer;
    this.edgeSourceBuffer = edgeSourceBuffer;
    this.edgeTargetBuffer = edgeTargetBuffer;
    this.edgeStateBuffer = edgeStateBuffer;
    this.edgeKindBuffer = edgeKindBuffer;
    this.nodePositionBuffer = nodePositionBuffer;
    this.nodeRadiusBuffer = nodeRadiusBuffer;
    this.nodeStateBuffer = nodeStateBuffer;
    this.nodeColorBuffer = nodeColorBuffer;
    this.fieldVao = fieldVao;
    this.fieldCompositeVao = fieldCompositeVao;
    this.fieldFramebuffer = fieldFramebuffer;
    this.fieldTexture = fieldTexture;
    this.nodeVao = nodeVao;
    this.edgeVao = edgeVao;

    this.configureFieldAttributes();
    this.configureFieldComposite();
    this.configureEdgeAttributes();
    this.configureNodeAttributes();

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
  }

  setGraph(graph: PositionedGraph) {
    this.graph = graph;
    this.uploadFields();
    this.uploadEdges();
    this.uploadNodes();
    this.fitToGraph();
    this.render();
  }

  updateGraphPositions() {
    this.uploadFields();
    this.uploadEdges();
    this.uploadNodes();
    this.render();
  }

  setFocusedNode(nodeId: string | null, neighborIds: Iterable<string> = []) {
    if (nodeId) {
      this.focusedNodeId = nodeId;
      this.focusedNeighborIds = new Set(neighborIds);
      this.uploadNodeStates();
      this.uploadEdgeStates();
      this.animateFocusTo(1);
      return;
    }

    this.animateFocusTo(0);
  }

  setNodeScale(scale: number) {
    this.nodeScale = scale;
    this.uploadNodes();
    this.render();
  }

  setTheme(theme: ResolvedTheme) {
    if (this.theme === theme) {
      return;
    }

    this.theme = theme;
    this.render();
  }

  zoomBy(factor: number) {
    this.camera.zoom = Math.min(4, Math.max(0.18, this.camera.zoom * factor));
    this.render();
  }

  panByScreenPixels(x: number, y: number) {
    this.camera.x += x / this.camera.zoom;
    this.camera.y += y / this.camera.zoom;
    this.render();
  }

  setSelectedNode(nodeId: string | null) {
    this.selectedNodeId = nodeId;
    this.uploadNodeStates();
    this.render();
  }

  getCamera() {
    return { ...this.camera };
  }

  getDebugInfo() {
    return {
      canvasCssWidth: this.canvas.clientWidth,
      canvasCssHeight: this.canvas.clientHeight,
      canvasWidth: this.canvas.width,
      canvasHeight: this.canvas.height,
      nodeCount: this.graph.nodes.length,
      edgeCount: this.graph.edges.length,
      edgeInstanceCount: this.edgeInstanceCount,
      camera: { ...this.camera },
    };
  }

  setCamera(camera: Camera) {
    this.camera = camera;
    this.render();
  }

  screenToWorld(x: number, y: number) {
    return {
      x: this.camera.x + (x - this.canvas.clientWidth / 2) / this.camera.zoom,
      y: this.camera.y + (y - this.canvas.clientHeight / 2) / this.camera.zoom,
    };
  }

  worldToScreen(node: Pick<PositionedNode, "x" | "y">) {
    return {
      x: (node.x - this.camera.x) * this.camera.zoom + this.canvas.clientWidth / 2,
      y: (node.y - this.camera.y) * this.camera.zoom + this.canvas.clientHeight / 2,
    };
  }

  pickNode(x: number, y: number) {
    let best: PositionedNode | null = null;
    let bestDistance = Number.POSITIVE_INFINITY;

    for (const node of this.graph.nodes) {
      const point = this.worldToScreen(node);
      const dx = point.x - x;
      const dy = point.y - y;
      const distance = Math.sqrt(dx * dx + dy * dy);

      if (distance <= 14 && distance < bestDistance) {
        best = node;
        bestDistance = distance;
      }
    }

    return best;
  }

  render() {
    this.resize();

    const { gl } = this;
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    const theme = graphThemes[this.theme];
    gl.clearColor(theme.background[0], theme.background[1], theme.background[2], 1);
    gl.clear(gl.COLOR_BUFFER_BIT);

    if (this.fieldInstanceCount > 0) {
      this.renderClusterField();
    }

    if (this.edgeInstanceCount > 0) {
      // biome-ignore lint/correctness/useHookAtTopLevel: WebGL useProgram is not a React hook.
      gl.useProgram(this.edgeProgram);
      this.setCameraUniforms(this.edgeProgram);
      this.setFocusUniform(this.edgeProgram);
      this.setVec3Uniform(this.edgeProgram, "u_edge_base_color", theme.edgeBase);
      this.setVec3Uniform(this.edgeProgram, "u_edge_focused_color", theme.edgeFocused);
      gl.bindVertexArray(this.edgeVao);
      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.edgeInstanceCount);
    }

    if (this.graph.nodes.length > 0) {
      // biome-ignore lint/correctness/useHookAtTopLevel: WebGL useProgram is not a React hook.
      gl.useProgram(this.nodeProgram);
      this.setCameraUniforms(this.nodeProgram);
      this.setFocusUniform(this.nodeProgram);
      this.setVec3Uniform(this.nodeProgram, "u_node_neutral_color", theme.nodeNeutral);
      this.setVec3Uniform(this.nodeProgram, "u_node_connected_color", theme.nodeConnected);
      this.setVec3Uniform(this.nodeProgram, "u_node_selected_color", theme.nodeSelected);
      this.setVec3Uniform(this.nodeProgram, "u_node_rim_color", theme.nodeRim);
      this.setVec3Uniform(this.nodeProgram, "u_node_rim_connected_color", theme.nodeRimConnected);
      this.setVec3Uniform(this.nodeProgram, "u_node_rim_selected_color", theme.nodeRimSelected);
      gl.bindVertexArray(this.nodeVao);
      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.graph.nodes.length);
    }

    gl.bindVertexArray(null);
  }

  destroy() {
    if (this.focusAnimationFrame !== null) {
      cancelAnimationFrame(this.focusAnimationFrame);
      this.focusAnimationFrame = null;
    }

    const { gl } = this;
    gl.deleteBuffer(this.fieldSourceBuffer);
    gl.deleteBuffer(this.fieldTargetBuffer);
    gl.deleteBuffer(this.fieldColorBuffer);
    gl.deleteBuffer(this.fieldRadiusBuffer);
    gl.deleteBuffer(this.fieldStrengthBuffer);
    gl.deleteBuffer(this.edgeSourceBuffer);
    gl.deleteBuffer(this.edgeTargetBuffer);
    gl.deleteBuffer(this.edgeStateBuffer);
    gl.deleteBuffer(this.edgeKindBuffer);
    gl.deleteBuffer(this.nodePositionBuffer);
    gl.deleteBuffer(this.nodeRadiusBuffer);
    gl.deleteBuffer(this.nodeStateBuffer);
    gl.deleteBuffer(this.nodeColorBuffer);
    gl.deleteVertexArray(this.fieldVao);
    gl.deleteVertexArray(this.fieldCompositeVao);
    gl.deleteFramebuffer(this.fieldFramebuffer);
    gl.deleteTexture(this.fieldTexture);
    gl.deleteVertexArray(this.nodeVao);
    gl.deleteVertexArray(this.edgeVao);
    gl.deleteProgram(this.fieldProgram);
    gl.deleteProgram(this.fieldCompositeProgram);
    gl.deleteProgram(this.edgeProgram);
    gl.deleteProgram(this.nodeProgram);
  }

  private resize() {
    const devicePixelRatio = window.devicePixelRatio || 1;
    const width = Math.max(1, Math.floor(this.canvas.clientWidth * devicePixelRatio));
    const height = Math.max(1, Math.floor(this.canvas.clientHeight * devicePixelRatio));

    if (this.canvas.width !== width || this.canvas.height !== height) {
      this.canvas.width = width;
      this.canvas.height = height;
    }
  }

  private setCameraUniforms(program: WebGLProgram) {
    this.setCameraUniformsForResolution(program, this.canvas.width, this.canvas.height);
  }

  private setCameraUniformsForResolution(program: WebGLProgram, width: number, height: number) {
    const { gl } = this;
    const devicePixelRatio = window.devicePixelRatio || 1;
    const resolutionScale = width / this.canvas.width;

    gl.uniform2f(
      requireUniform(gl.getUniformLocation(program, "u_center"), "u_center"),
      this.camera.x,
      this.camera.y,
    );
    gl.uniform2f(
      requireUniform(gl.getUniformLocation(program, "u_resolution"), "u_resolution"),
      width,
      height,
    );
    gl.uniform1f(
      requireUniform(gl.getUniformLocation(program, "u_zoom"), "u_zoom"),
      this.camera.zoom * devicePixelRatio * resolutionScale,
    );

    const pixelRatioLocation = gl.getUniformLocation(program, "u_pixel_ratio");

    if (pixelRatioLocation) {
      gl.uniform1f(pixelRatioLocation, devicePixelRatio * resolutionScale);
    }
  }

  private setFocusUniform(program: WebGLProgram) {
    const location = this.gl.getUniformLocation(program, "u_focus_progress");

    if (location) {
      this.gl.uniform1f(location, this.focusProgress);
    }
  }

  private setVec3Uniform(
    program: WebGLProgram,
    name: string,
    value: readonly [number, number, number],
  ) {
    const location = this.gl.getUniformLocation(program, name);

    if (location) {
      this.gl.uniform3f(location, value[0], value[1], value[2]);
    }
  }

  private animateFocusTo(target: number) {
    if (this.focusTarget === target && this.focusAnimationFrame !== null) {
      return;
    }

    if (this.focusAnimationFrame !== null) {
      cancelAnimationFrame(this.focusAnimationFrame);
    }

    this.focusTarget = target;
    this.focusAnimationStartValue = this.focusProgress;
    this.focusAnimationStartedAt = performance.now();

    const duration = target > this.focusProgress ? 180 : 220;

    const tick = (now: number) => {
      const elapsed = Math.min(1, (now - this.focusAnimationStartedAt) / duration);
      const eased = 1 - (1 - elapsed) * (1 - elapsed);

      this.focusProgress =
        this.focusAnimationStartValue + (this.focusTarget - this.focusAnimationStartValue) * eased;

      this.render();

      if (elapsed < 1) {
        this.focusAnimationFrame = requestAnimationFrame(tick);
        return;
      }

      this.focusProgress = this.focusTarget;
      this.focusAnimationFrame = null;

      if (this.focusTarget === 0) {
        this.focusedNodeId = null;
        this.focusedNeighborIds = new Set();
        this.uploadNodeStates();
        this.uploadEdgeStates();
        this.render();
      }
    };

    this.focusAnimationFrame = requestAnimationFrame(tick);
  }

  private ensureFieldTarget() {
    const { gl } = this;
    const width = Math.max(1, Math.floor(this.canvas.width * 0.5));
    const height = Math.max(1, Math.floor(this.canvas.height * 0.5));

    gl.bindTexture(gl.TEXTURE_2D, this.fieldTexture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

    if (this.fieldTextureWidth !== width || this.fieldTextureHeight !== height) {
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, width, height, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);

      this.fieldTextureWidth = width;
      this.fieldTextureHeight = height;
    }

    gl.bindFramebuffer(gl.FRAMEBUFFER, this.fieldFramebuffer);
    gl.framebufferTexture2D(
      gl.FRAMEBUFFER,
      gl.COLOR_ATTACHMENT0,
      gl.TEXTURE_2D,
      this.fieldTexture,
      0,
    );

    const status = gl.checkFramebufferStatus(gl.FRAMEBUFFER);

    if (status !== gl.FRAMEBUFFER_COMPLETE) {
      throw new Error(`Could not create graph field framebuffer: ${status}`);
    }

    return { width, height };
  }

  private renderClusterField() {
    const { gl } = this;
    const target = this.ensureFieldTarget();

    gl.bindFramebuffer(gl.FRAMEBUFFER, this.fieldFramebuffer);
    gl.viewport(0, 0, target.width, target.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE);

    // biome-ignore lint/correctness/useHookAtTopLevel: WebGL useProgram is not a React hook.
    gl.useProgram(this.fieldProgram);
    this.setCameraUniformsForResolution(this.fieldProgram, target.width, target.height);
    gl.bindVertexArray(this.fieldVao);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.fieldInstanceCount);

    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);

    // biome-ignore lint/correctness/useHookAtTopLevel: WebGL useProgram is not a React hook.
    gl.useProgram(this.fieldCompositeProgram);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.fieldTexture);
    gl.uniform1i(
      requireUniform(gl.getUniformLocation(this.fieldCompositeProgram, "u_field"), "u_field"),
      0,
    );
    gl.uniform2f(
      requireUniform(
        gl.getUniformLocation(this.fieldCompositeProgram, "u_texel_size"),
        "u_texel_size",
      ),
      1 / target.width,
      1 / target.height,
    );
    this.setVec3Uniform(
      this.fieldCompositeProgram,
      "u_field_light_color",
      graphThemes[this.theme].fieldLight,
    );
    gl.bindVertexArray(this.fieldCompositeVao);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  private configureFieldAttributes() {
    const { gl } = this;
    gl.bindVertexArray(this.fieldVao);

    const cornerBuffer = gl.createBuffer();

    if (!cornerBuffer) {
      throw new Error("Could not allocate field corner buffer.");
    }

    gl.bindBuffer(gl.ARRAY_BUFFER, cornerBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);

    const cornerLocation = requireLocation(
      gl.getAttribLocation(this.fieldProgram, "a_corner"),
      "a_corner",
    );
    gl.enableVertexAttribArray(cornerLocation);
    gl.vertexAttribPointer(cornerLocation, 2, gl.FLOAT, false, 0, 0);

    const sourceLocation = requireLocation(
      gl.getAttribLocation(this.fieldProgram, "a_source"),
      "a_source",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.fieldSourceBuffer);
    gl.enableVertexAttribArray(sourceLocation);
    gl.vertexAttribPointer(sourceLocation, 2, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(sourceLocation, 1);

    const targetLocation = requireLocation(
      gl.getAttribLocation(this.fieldProgram, "a_target"),
      "a_target",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.fieldTargetBuffer);
    gl.enableVertexAttribArray(targetLocation);
    gl.vertexAttribPointer(targetLocation, 2, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(targetLocation, 1);

    const colorLocation = requireLocation(
      gl.getAttribLocation(this.fieldProgram, "a_color"),
      "a_color",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.fieldColorBuffer);
    gl.enableVertexAttribArray(colorLocation);
    gl.vertexAttribPointer(colorLocation, 3, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(colorLocation, 1);

    const radiusLocation = requireLocation(
      gl.getAttribLocation(this.fieldProgram, "a_radius"),
      "a_radius",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.fieldRadiusBuffer);
    gl.enableVertexAttribArray(radiusLocation);
    gl.vertexAttribPointer(radiusLocation, 1, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(radiusLocation, 1);

    const strengthLocation = requireLocation(
      gl.getAttribLocation(this.fieldProgram, "a_strength"),
      "a_strength",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.fieldStrengthBuffer);
    gl.enableVertexAttribArray(strengthLocation);
    gl.vertexAttribPointer(strengthLocation, 1, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(strengthLocation, 1);

    gl.bindVertexArray(null);
  }

  private configureFieldComposite() {
    const { gl } = this;
    gl.bindVertexArray(this.fieldCompositeVao);

    const buffer = gl.createBuffer();

    if (!buffer) {
      throw new Error("Could not allocate field composite buffer.");
    }

    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);

    const positionLocation = requireLocation(
      gl.getAttribLocation(this.fieldCompositeProgram, "a_position"),
      "a_position",
    );
    gl.enableVertexAttribArray(positionLocation);
    gl.vertexAttribPointer(positionLocation, 2, gl.FLOAT, false, 0, 0);

    gl.bindVertexArray(null);
  }

  private configureEdgeAttributes() {
    const { gl } = this;
    gl.bindVertexArray(this.edgeVao);

    const cornerBuffer = gl.createBuffer();

    if (!cornerBuffer) {
      throw new Error("Could not allocate edge corner buffer.");
    }

    gl.bindBuffer(gl.ARRAY_BUFFER, cornerBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);

    const cornerLocation = requireLocation(
      gl.getAttribLocation(this.edgeProgram, "a_corner"),
      "a_corner",
    );
    gl.enableVertexAttribArray(cornerLocation);
    gl.vertexAttribPointer(cornerLocation, 2, gl.FLOAT, false, 0, 0);

    const sourceLocation = requireLocation(
      gl.getAttribLocation(this.edgeProgram, "a_source"),
      "a_source",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.edgeSourceBuffer);
    gl.enableVertexAttribArray(sourceLocation);
    gl.vertexAttribPointer(sourceLocation, 2, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(sourceLocation, 1);

    const targetLocation = requireLocation(
      gl.getAttribLocation(this.edgeProgram, "a_target"),
      "a_target",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.edgeTargetBuffer);
    gl.enableVertexAttribArray(targetLocation);
    gl.vertexAttribPointer(targetLocation, 2, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(targetLocation, 1);

    const stateLocation = requireLocation(
      gl.getAttribLocation(this.edgeProgram, "a_state"),
      "a_state",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.edgeStateBuffer);
    gl.enableVertexAttribArray(stateLocation);
    gl.vertexAttribPointer(stateLocation, 1, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(stateLocation, 1);

    const kindLocation = requireLocation(
      gl.getAttribLocation(this.edgeProgram, "a_kind"),
      "a_kind",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.edgeKindBuffer);
    gl.enableVertexAttribArray(kindLocation);
    gl.vertexAttribPointer(kindLocation, 1, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(kindLocation, 1);

    gl.bindVertexArray(null);
  }

  private configureNodeAttributes() {
    const { gl } = this;
    gl.bindVertexArray(this.nodeVao);

    const cornerBuffer = gl.createBuffer();

    if (!cornerBuffer) {
      throw new Error("Could not allocate node corner buffer.");
    }

    gl.bindBuffer(gl.ARRAY_BUFFER, cornerBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);

    const cornerLocation = requireLocation(
      gl.getAttribLocation(this.nodeProgram, "a_corner"),
      "a_corner",
    );
    gl.enableVertexAttribArray(cornerLocation);
    gl.vertexAttribPointer(cornerLocation, 2, gl.FLOAT, false, 0, 0);

    const positionLocation = requireLocation(
      gl.getAttribLocation(this.nodeProgram, "a_position"),
      "a_position",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.nodePositionBuffer);
    gl.enableVertexAttribArray(positionLocation);
    gl.vertexAttribPointer(positionLocation, 2, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(positionLocation, 1);

    const radiusLocation = requireLocation(
      gl.getAttribLocation(this.nodeProgram, "a_radius"),
      "a_radius",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.nodeRadiusBuffer);
    gl.enableVertexAttribArray(radiusLocation);
    gl.vertexAttribPointer(radiusLocation, 1, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(radiusLocation, 1);

    const stateLocation = requireLocation(
      gl.getAttribLocation(this.nodeProgram, "a_state"),
      "a_state",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.nodeStateBuffer);
    gl.enableVertexAttribArray(stateLocation);
    gl.vertexAttribPointer(stateLocation, 1, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(stateLocation, 1);

    const colorLocation = requireLocation(
      gl.getAttribLocation(this.nodeProgram, "a_color"),
      "a_color",
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, this.nodeColorBuffer);
    gl.enableVertexAttribArray(colorLocation);
    gl.vertexAttribPointer(colorLocation, 3, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(colorLocation, 1);

    gl.bindVertexArray(null);
  }

  private uploadFields() {
    const clusters = detectGraphClusters(this.graph);
    const clusterByNodeId = createNodeClusterMap(clusters);
    const nodeById = new Map(this.graph.nodes.map((node) => [node.id, node]));
    const sources: number[] = [];
    const targets: number[] = [];
    const colors: number[] = [];
    const radii: number[] = [];
    const strengths: number[] = [];
    const clusterMetrics = new Map<number, { radius: number; strength: number }>();

    for (const cluster of clusters) {
      const nodes = cluster.nodeIds
        .map((nodeId) => nodeById.get(nodeId))
        .filter((node): node is PositionedNode => Boolean(node));

      if (nodes.length < 3) {
        continue;
      }

      const center = nodes.reduce((sum, node) => ({ x: sum.x + node.x, y: sum.y + node.y }), {
        x: 0,
        y: 0,
      });
      center.x /= nodes.length;
      center.y /= nodes.length;

      const spread =
        nodes.reduce((sum, node) => sum + Math.hypot(node.x - center.x, node.y - center.y), 0) /
        nodes.length;

      const density = nodes.length / Math.max(spread, 36);
      const compactness = Math.min(1, Math.max(0, density * 9));

      clusterMetrics.set(cluster.id, {
        radius: 76 - compactness * 26,
        strength: 0.028 + compactness * 0.026,
      });
    }

    for (const node of this.graph.nodes) {
      const cluster = clusterByNodeId.get(node.id);
      const metrics = cluster ? clusterMetrics.get(cluster.id) : undefined;

      if (!cluster || !metrics) {
        continue;
      }

      sources.push(node.x, node.y);
      targets.push(node.x, node.y);
      colors.push(...cluster.color);
      radii.push(metrics.radius);
      strengths.push(metrics.strength);
    }

    for (const edge of this.graph.edges) {
      const source = nodeById.get(edge.source_object_id);
      const target = nodeById.get(edge.target_object_id);
      const sourceCluster = clusterByNodeId.get(edge.source_object_id);
      const targetCluster = clusterByNodeId.get(edge.target_object_id);
      const metrics = sourceCluster ? clusterMetrics.get(sourceCluster.id) : undefined;

      if (
        !source ||
        !target ||
        !sourceCluster ||
        sourceCluster.id !== targetCluster?.id ||
        !metrics
      ) {
        continue;
      }

      sources.push(source.x, source.y);
      targets.push(target.x, target.y);
      colors.push(...sourceCluster.color);
      radii.push(metrics.radius * 0.68);
      strengths.push(metrics.strength * 0.56);
    }

    this.fieldInstanceCount = radii.length;

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.fieldSourceBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, new Float32Array(sources), this.gl.STATIC_DRAW);

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.fieldTargetBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, new Float32Array(targets), this.gl.STATIC_DRAW);

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.fieldColorBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, new Float32Array(colors), this.gl.STATIC_DRAW);

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.fieldRadiusBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, new Float32Array(radii), this.gl.STATIC_DRAW);

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.fieldStrengthBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, new Float32Array(strengths), this.gl.STATIC_DRAW);
  }

  private uploadEdges() {
    const nodeById = new Map(this.graph.nodes.map((node) => [node.id, node]));
    const sources: number[] = [];
    const targets: number[] = [];
    const kinds: number[] = [];

    for (const edge of this.graph.edges) {
      const source = nodeById.get(edge.source_object_id);
      const target = nodeById.get(edge.target_object_id);

      if (!source || !target) {
        continue;
      }

      sources.push(source.x, source.y);
      targets.push(target.x, target.y);
      kinds.push(
        edge.kind === "reference" ? 1 : edge.kind === "relationship_and_reference" ? 2 : 0,
      );
    }

    this.edgeInstanceCount = sources.length / 2;

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.edgeSourceBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, new Float32Array(sources), this.gl.STATIC_DRAW);

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.edgeTargetBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, new Float32Array(targets), this.gl.STATIC_DRAW);

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.edgeKindBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, new Float32Array(kinds), this.gl.STATIC_DRAW);

    this.uploadEdgeStates();
  }

  private uploadNodes() {
    const positions = new Float32Array(this.graph.nodes.length * 2);
    const radii = new Float32Array(this.graph.nodes.length);
    const colors = new Float32Array(this.graph.nodes.length * 3);
    const clusterByNodeId = createNodeClusterMap(detectGraphClusters(this.graph));

    this.graph.nodes.forEach((node, index) => {
      positions[index * 2] = node.x;
      positions[index * 2 + 1] = node.y;
      const degree = node.in_degree + node.out_degree;
      radii[index] = Math.min(15, 6.5 + Math.sqrt(degree + 1) * 1.45) * this.nodeScale;

      const color = clusterByNodeId.get(node.id)?.color ?? [0.5, 0.5, 0.5];
      colors[index * 3] = color[0];
      colors[index * 3 + 1] = color[1];
      colors[index * 3 + 2] = color[2];
    });

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.nodePositionBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, positions, this.gl.STATIC_DRAW);

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.nodeRadiusBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, radii, this.gl.STATIC_DRAW);

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.nodeColorBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, colors, this.gl.STATIC_DRAW);

    this.uploadNodeStates();
  }

  private uploadNodeStates() {
    const states = new Float32Array(
      this.graph.nodes.map((node) => {
        if (node.id === this.selectedNodeId) {
          return 2;
        }

        if (node.id === this.focusedNodeId) {
          return 2;
        }

        if (this.focusedNodeId && this.focusedNeighborIds.has(node.id)) {
          return 1;
        }

        if (this.focusedNodeId) {
          return -1;
        }

        return 0;
      }),
    );

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.nodeStateBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, states, this.gl.DYNAMIC_DRAW);
  }

  private uploadEdgeStates() {
    const states: number[] = [];
    const nodeIds = new Set(this.graph.nodes.map((node) => node.id));

    for (const edge of this.graph.edges) {
      if (!nodeIds.has(edge.source_object_id) || !nodeIds.has(edge.target_object_id)) {
        continue;
      }

      let state = 0;

      if (this.focusedNodeId) {
        const isConnected =
          edge.source_object_id === this.focusedNodeId ||
          edge.target_object_id === this.focusedNodeId;

        state = isConnected ? 1 : -1;
      }

      states.push(state);
    }

    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.edgeStateBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, new Float32Array(states), this.gl.DYNAMIC_DRAW);
  }

  fitToGraph() {
    if (this.graph.nodes.length === 0) {
      this.camera = { x: 0, y: 0, zoom: 1 };
      this.render();
      return;
    }

    let minX = Number.POSITIVE_INFINITY;
    let minY = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;
    let maxY = Number.NEGATIVE_INFINITY;

    for (const node of this.graph.nodes) {
      minX = Math.min(minX, node.x);
      minY = Math.min(minY, node.y);
      maxX = Math.max(maxX, node.x);
      maxY = Math.max(maxY, node.y);
    }

    const width = Math.max(maxX - minX, 120);
    const height = Math.max(maxY - minY, 120);
    const horizontalPadding = Math.min(220, this.canvas.clientWidth * 0.2);
    const verticalPadding = Math.min(160, this.canvas.clientHeight * 0.2);
    const availableWidth = Math.max(this.canvas.clientWidth - horizontalPadding, 120);
    const availableHeight = Math.max(this.canvas.clientHeight - verticalPadding, 120);

    this.camera = {
      x: (minX + maxX) / 2,
      y: (minY + maxY) / 2,
      zoom: Math.min(availableWidth / width, availableHeight / height, 1.7),
    };
    this.render();
  }
}
