<template>
  <div
    ref="containerRef"
    class="unified-viewport"
    @mousedown="onMouseDown"
    @mousemove="onMouseMove"
    @mouseup="onMouseUp"
    @mouseleave="onMouseLeave"
    @contextmenu.prevent="onRightClick"
    @dblclick="onDoubleClick"
  >
    <!-- View toolbar -->
    <div class="vp-toolbar">
      <button @click.stop="setView('front')" title="前视">前</button>
      <button @click.stop="setView('top')" title="上视">上</button>
      <button @click.stop="setView('right')" title="右视">右</button>
      <button @click.stop="setView('iso')" title="等轴测">3D</button>
      <button v-if="isSketchMode" @click.stop="setView('face')" title="正对草图面">↩ 平面</button>
      <span class="vp-sep"></span>
      <button @click.stop="toggleWireframe" :class="{ active: wireframe }" title="线框">线框</button>
      <button @click.stop="toggleEdges" :class="{ active: showEdges }" title="边线">边线</button>
      <button @click.stop="() => fitView()" title="适配">⊞</button>
    </div>

    <!-- Sketch info bar -->
    <div v-if="isDrawing" class="sketch-info">
      <span>工具: {{ currentToolLabel }}</span>
      <span v-if="isDrawingLine">按 Esc 取消 / 连续绘制</span>
      <span v-if="snappedAngle !== null">吸附: {{ snappedAngle }}°</span>
      <span v-if="circleRadiusDisplay !== null" class="radius-display">R = {{ circleRadiusDisplay }}</span>
    </div>

    <!-- Snap indicator -->
    <div
      v-if="activeSnap && isDrawing"
      class="snap-indicator"
      :style="snapStyle"
      :class="'snap-' + activeSnap.type"
    >
      <span class="snap-label">{{ snapLabel }}</span>
    </div>

    <!-- Driving dimension overlay -->
    <DimensionOverlay
      v-if="isSketchMode && store.showDimensions && ctx"
      :entities="store.entities"
      :constraints="store.constraints"
      :visible="true"
      :project="projectFn"
      @refresh="onDimensionRefresh"
    />

    <!-- Dimension visibility toggle -->
    <button
      v-if="isSketchMode"
      class="vp-dim-toggle"
      :class="{ active: store.showDimensions }"
      @click.stop="store.showDimensions = !store.showDimensions"
      title="显示/隐藏尺寸"
    >📐 尺寸</button>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch } from "vue";
import { listen } from "@tauri-apps/api/event";
import * as THREE from "three";
import {
  getAllSolidMeshes,
  getSketchEntities, getSketchConstraints,
  addPoint, addLine, addCircle, addArc, addSpline, addEllipse,
  addConstraint, solveSketch, movePoint, movePointNoSnapshot, deleteEntity,
  type RenderMesh, type EntityId,
} from "@/commands/sketch";
import { useSketchStore } from "@/stores/sketch";
import { useToastStore } from "@/stores/toast";
import DimensionOverlay from "@/components/DimensionOverlay.vue";
import {
  useThreeScene,
  sketchToWorld3D,
  worldToSketch2D,
  planeFrame,
  type ViewName,
} from "@/composables/useThreeScene";
import {
  createSketchState, resetDrawingState,
  findPointAt, findClosestEntity, snapAngle, lineIntersection,
  SNAP_PX, SNAP_LABELS, SNAP_PRIORITY, toolLabel,
  type SnapResult,
} from "@/composables/useSketchInteraction";

const emit = defineEmits<{
  (e: "faceSelected", featureId: number, faceIndex: number): void;
  /** Emitted when the user finishes dragging a point. M9 should listen for
   *  this and call unifiedViewport.value?.refreshViewport() to keep the
   *  solid mesh in sync after sketch edits. */
  (e: "drag-end"): void;
  /** Emitted when the user clicks an edge in edge-pick mode. */
  (e: "edgeSelected", featureId: number, vA: number, vB: number): void;
  /** Emitted when a driving dimension is edited via the overlay. HomeView
   *  should re-run loadState() so feature errors/params stay fresh. */
  (e: "dimension-edited"): void;
}>();

const store = useSketchStore();
const toast = useToastStore();

// ── Three.js scene ──────────────────────────────────────────────
const {
  container: containerRef,
  ctx,
  wireframe,
  showEdges,
  initScene,
  setView: setSceneView,
  toggleWireframe: toggleSceneWireframe,
  toggleEdges: toggleSceneEdges,
  fitView: fitSceneView,
  worldToScreen,
  captureViewport,
  dispose,
  animate,
} = useThreeScene();

let solidMeshes: THREE.Mesh[] = [];
let edgeLines: THREE.LineSegments[] = [];
let sketchPreviewLine: THREE.Line | null = null;
let sketchPreviewCircle: THREE.Line | null = null;

// ── Drawing state ───────────────────────────────────────────────
const state = createSketchState();
const { activeSnap, hoverSketch, pendingPointIds } = state;

const isSketchMode = computed(() => store.isEditingSketch());
const isDrawing = computed(() => isSketchMode.value && store.activeTool !== "select" && store.activeTool !== "plugin");
const isDrawingLine = computed(() => store.activeTool === "line" && (state.lineStartId.value !== null || state.lineStartPos.value !== null));
const currentToolLabel = computed(() => toolLabel(store.activeTool));

const snappedAngle = computed(() => {
  if (!isDrawingLine.value) return null;
  const start = getLineStartCoords();
  if (!start) return null;
  const target = activeSnap.value?.world ?? hoverSketch.value;
  const dx = target.x - start.x;
  const dy = target.y - start.y;
  const dist = Math.sqrt(dx * dx + dy * dy);
  if (dist < 1e-6) return null;
  let angle = Math.atan2(dy, dx) * (180 / Math.PI);
  if (angle < 0) angle += 360;
  for (const std of [0, 45, 90, 135, 180, 225, 270, 315]) {
    let diff = Math.abs(angle - std);
    if (diff > 180) diff = 360 - diff;
    if (diff <= 4) return std;
  }
  return null;
});

const snapStyle = computed(() => {
  if (!activeSnap.value) return { display: "none" };
  return { left: activeSnap.value.screen.x + "px", top: activeSnap.value.screen.y + "px" };
});

const snapLabel = computed(() => activeSnap.value ? SNAP_LABELS[activeSnap.value.type] : "");

/// Live radius readout while drawing a circle (second click pending).
const circleRadiusDisplay = computed(() => {
  if (store.activeTool !== "circle") return null;
  if (state.circleCenterId.value === null && state.circleCenterPos.value === null) return null;
  const center = getCircleCenterCoords();
  if (!center) return null;
  const target = activeSnap.value?.world ?? hoverSketch.value;
  const r = Math.hypot(target.x - center.x, target.y - center.y);
  if (r < 0.001) return null;
  return r.toFixed(3);
});

const SOLID_COLORS = [0x4fc3f7, 0xff8a65, 0x69f0ae, 0xffd54f, 0xce93d8, 0x80cbc4, 0xf48fb1, 0xaed581, 0xfff176, 0x90caf9];

// ── Highlight / edge-pick groups (added to scene after init) ───────
let highlightGroup: THREE.Group | null = null;
let edgeHoverLine: THREE.Line | null = null;    // temporary hover highlight
let selectedEdgeLines: THREE.Line[] = [];        // persistent selected edges
let measureLine: THREE.Line | null = null;
let measureLabelSprite: THREE.Sprite | null = null;

function ensureHighlightGroup(): THREE.Group {
  if (!highlightGroup && ctx.value) {
    highlightGroup = new THREE.Group();
    highlightGroup.name = "edge-measure-highlights";
    ctx.value.scene.add(highlightGroup);
  }
  return highlightGroup!;
}

function destroyHighlightGroup() {
  if (!highlightGroup) return;
  clearEdgeHighlights();
  clearMeasureVisuals();
  if (ctx.value) ctx.value.scene.remove(highlightGroup);
  highlightGroup = null;
}

// ── Edge raycasting ─────────────────────────────────────────────

/// Compute the closest distance between a ray and a line segment.
/// Returns the 3D point on the segment nearest to the ray, the distance,
/// and the parametric `t` along the segment (used to order hits).
function rayToSegmentDistance(
  rayOrigin: THREE.Vector3,
  rayDir: THREE.Vector3,
  a: THREE.Vector3,
  b: THREE.Vector3,
): { dist: number; point: THREE.Vector3; t: number } {
  const ab = new THREE.Vector3().subVectors(b, a);
  const len = ab.length();
  if (len < 1e-10) {
    // Degenerate segment → treat as point.
    const v = new THREE.Vector3().subVectors(a, rayOrigin);
    const tProj = Math.max(0, v.dot(rayDir));
    const closest = rayOrigin.clone().addScaledVector(rayDir, tProj);
    return { dist: closest.distanceTo(a), point: a.clone(), t: 0 };
  }
  ab.normalize();

  const ao = new THREE.Vector3().subVectors(rayOrigin, a);
  const cross = new THREE.Vector3().crossVectors(rayDir, ab);
  const crossLenSq = cross.lengthSq();

  let tRay: number;
  let tSeg: number;

  if (crossLenSq < 1e-9) {
    // Parallel → project ray origin onto the infinite line, clamp to segment.
    tRay = 0;
    const proj = ao.dot(ab);
    tSeg = Math.max(0, Math.min(len, proj));
  } else {
    const denom = crossLenSq;
    const cross2 = new THREE.Vector3().crossVectors(ao, ab);
    tRay = cross2.dot(cross) / denom;

    const cross3 = new THREE.Vector3().crossVectors(ao, rayDir);
    tSeg = cross3.dot(cross) / denom;
    tSeg = Math.max(0, Math.min(len, tSeg));
  }

  if (tRay < 0) tRay = 0;

  const rayPoint = rayOrigin.clone().addScaledVector(rayDir, tRay);
  const segPoint = a.clone().addScaledVector(ab, tSeg);
  const dist = rayPoint.distanceTo(segPoint);

  return { dist, point: segPoint, t: tSeg };
}

/// Find the closest mesh edge to the raycaster's ray across all solid meshes.
/// Returns null when no edge is within `threshold` world units.
interface EdgeHit {
  mesh: THREE.Mesh;
  featureId: number;
  vA: number;
  vB: number;
  point: THREE.Vector3;
}
function findClosestEdge(raycaster: THREE.Raycaster, threshold: number): EdgeHit | null {
  let best: EdgeHit | null = null;
  let bestDist = threshold;

  for (const mesh of solidMeshes) {
    const geo = mesh.geometry;
    const posAttr = geo.getAttribute("position") as THREE.BufferAttribute;
    const idxAttr = geo.getIndex();
    if (!idxAttr) continue;

    // Transform ray into mesh-local space.
    const invMatrix = new THREE.Matrix4().copy(mesh.matrixWorld).invert();
    const localOrigin = raycaster.ray.origin.clone().applyMatrix4(invMatrix);
    const localDir = raycaster.ray.direction.clone().transformDirection(invMatrix).normalize();

    const seenEdges = new Set<string>(); // dedupe shared edges

    for (let i = 0; i < idxAttr.count; i += 3) {
      const i0 = idxAttr.getX(i);
      const i1 = idxAttr.getX(i + 1);
      const i2 = idxAttr.getX(i + 2);

      for (const [ia, ib] of [[i0, i1], [i1, i2], [i2, i0]] as [number, number][]) {
        const key = ia < ib ? `${ia}-${ib}` : `${ib}-${ia}`;
        if (seenEdges.has(key)) continue;
        seenEdges.add(key);

        const va = new THREE.Vector3(posAttr.getX(ia), posAttr.getY(ia), posAttr.getZ(ia));
        const vb = new THREE.Vector3(posAttr.getX(ib), posAttr.getY(ib), posAttr.getZ(ib));

        const result = rayToSegmentDistance(localOrigin, localDir, va, vb);
        if (result.dist < bestDist) {
          bestDist = result.dist;
          const worldPoint = result.point.clone().applyMatrix4(mesh.matrixWorld);
          best = {
            mesh,
            featureId: (mesh.userData as any).featureId ?? 0,
            vA: ia,
            vB: ib,
            point: worldPoint,
          };
        }
      }
    }
  }
  return best;
}

// ── Edge highlight helpers ───────────────────────────────────────

/// Show a temporary hover highlight on an edge.
function showEdgeHover(mesh: THREE.Mesh, vA: number, vB: number) {
  clearEdgeHover();
  const pos = mesh.geometry.getAttribute("position") as THREE.BufferAttribute;
  const a = new THREE.Vector3(pos.getX(vA), pos.getY(vA), pos.getZ(vA)).applyMatrix4(mesh.matrixWorld);
  const b = new THREE.Vector3(pos.getX(vB), pos.getY(vB), pos.getZ(vB)).applyMatrix4(mesh.matrixWorld);
  const geo = new THREE.BufferGeometry().setFromPoints([a, b]);
  const mat = new THREE.LineBasicMaterial({ color: 0xffeb3b, linewidth: 1, depthTest: true, transparent: true, opacity: 0.9 });
  edgeHoverLine = new THREE.Line(geo, mat);
  ensureHighlightGroup().add(edgeHoverLine);
}

function clearEdgeHover() {
  if (edgeHoverLine) {
    edgeHoverLine.geometry.dispose();
    (edgeHoverLine.material as THREE.Material).dispose();
    highlightGroup?.remove(edgeHoverLine);
    edgeHoverLine = null;
  }
}

/// Refresh the persistent highlight for all currently-selected edges.
function refreshSelectedEdgeHighlights() {
  clearEdgeHighlights();
  const edges = store.pendingEdges;
  if (edges.length === 0) return;
  for (const [vA, vB] of edges) {
    // Find the mesh that owns these vertex indices.
    for (const mesh of solidMeshes) {
      const pos = mesh.geometry.getAttribute("position") as THREE.BufferAttribute;
      if (vA >= pos.count || vB >= pos.count) continue;
      const a = new THREE.Vector3(pos.getX(vA), pos.getY(vA), pos.getZ(vA)).applyMatrix4(mesh.matrixWorld);
      const b = new THREE.Vector3(pos.getX(vB), pos.getY(vB), pos.getZ(vB)).applyMatrix4(mesh.matrixWorld);
      const geo = new THREE.BufferGeometry().setFromPoints([a, b]);
      const mat = new THREE.LineBasicMaterial({ color: 0x4fc3f7, linewidth: 1, depthTest: true, transparent: true, opacity: 0.85 });
      const line = new THREE.Line(geo, mat);
      ensureHighlightGroup().add(line);
      selectedEdgeLines.push(line);
      break; // assume unique match per edge
    }
  }
}

function clearEdgeHighlights() {
  for (const line of selectedEdgeLines) {
    line.geometry.dispose();
    (line.material as THREE.Material).dispose();
    highlightGroup?.remove(line);
  }
  selectedEdgeLines = [];
}

// ── Measurement visualization ────────────────────────────────────

/// Build a text sprite (canvas-based) for measurement labels.
function createTextSprite(text: string, position: THREE.Vector3, color = "#ffd54f"): THREE.Sprite {
  const canvas = document.createElement("canvas");
  canvas.width = 256;
  canvas.height = 64;
  const ctx2d = canvas.getContext("2d")!;
  ctx2d.fillStyle = "rgba(30,30,30,0.85)";
  ctx2d.fillRect(0, 0, canvas.width, canvas.height);
  ctx2d.font = "bold 28px monospace";
  ctx2d.fillStyle = color;
  ctx2d.textAlign = "center";
  ctx2d.textBaseline = "middle";
  ctx2d.fillText(text, canvas.width / 2, canvas.height / 2);

  const texture = new THREE.CanvasTexture(canvas);
  texture.minFilter = THREE.LinearFilter;
  const spriteMat = new THREE.SpriteMaterial({ map: texture, depthTest: false, transparent: true });
  const sprite = new THREE.Sprite(spriteMat);
  sprite.position.copy(position);
  sprite.scale.set(2, 0.5, 1);
  return sprite;
}

function refreshMeasureVisuals() {
  clearMeasureVisuals();
  const pts = store.measurePoints;
  if (pts.length < 2) return;

  const group = ensureHighlightGroup();
  const p0 = new THREE.Vector3(pts[0].x, pts[0].y, pts[0].z);
  const p1 = new THREE.Vector3(pts[1].x, pts[1].y, pts[1].z);

  // Dashed line between the two points.
  const dashGeo = new THREE.BufferGeometry().setFromPoints([p0, p1]);
  const dashMat = new THREE.LineDashedMaterial({ color: 0xffd54f, dashSize: 0.3, gapSize: 0.15, depthTest: true });
  measureLine = new THREE.Line(dashGeo, dashMat);
  measureLine.computeLineDistances();
  group.add(measureLine);

  // Distance label at the midpoint.
  const dist = p0.distanceTo(p1);
  const mid = new THREE.Vector3().addVectors(p0, p1).multiplyScalar(0.5);
  measureLabelSprite = createTextSprite(`${dist.toFixed(3)}`, mid, "#ffd54f");
  group.add(measureLabelSprite);
}

function clearMeasureVisuals() {
  if (measureLine) {
    measureLine.geometry.dispose();
    (measureLine.material as THREE.Material).dispose();
    highlightGroup?.remove(measureLine);
    measureLine = null;
  }
  if (measureLabelSprite) {
    // Material.dispose() does NOT dispose textures — the per-label
    // CanvasTexture would leak one GPU texture per measurement.
    const mat = measureLabelSprite.material as THREE.SpriteMaterial;
    mat.map?.dispose();
    mat.dispose();
    highlightGroup?.remove(measureLabelSprite);
    measureLabelSprite = null;
  }
}

// ── Sketch plane helpers ────────────────────────────────────────

function getActivePlane() {
  // While editing a sketch, always use the sketch's stored plane — the
  // toolbar's activePlane dropdown is for the NEXT sketch only.
  if (isSketchMode.value) {
    const sketchId = store.editingSketchId ?? store.activeFeatureId;
    const f = sketchId ? store.features.find(x => x.id === sketchId) : null;
    if (f?.plane && f.plane !== "offset") {
      return planeFrame(f.plane);
    }
  }
  return planeFrame(getActivePlaneName());
}

function getActivePlaneName(): string {
  if (isSketchMode.value) {
    // Use the editing sketch ID to get the correct plane — activeFeatureId
    // may point to a different feature selected in the tree.
    const sketchId = store.editingSketchId ?? store.activeFeatureId;
    const f = sketchId ? store.features.find(x => x.id === sketchId) : null;
    if (f?.plane && f.plane !== "offset") {
      return f.plane;
    }
  }
  return store.activePlane || "xy";
}

function getSketchPoint(event: MouseEvent): { x: number; y: number } | null {
  if (!containerRef.value || !ctx.value) return null;
  const rect = containerRef.value.getBoundingClientRect();
  const sx = event.clientX - rect.left;
  const sy = event.clientY - rect.top;
  const el = ctx.value.renderer.domElement;
  const mouse = new THREE.Vector2(
    (sx / el.clientWidth) * 2 - 1,
    -(sy / el.clientHeight) * 2 + 1,
  );
  const rc = new THREE.Raycaster();
  rc.setFromCamera(mouse, ctx.value.camera);
  const plane = getActivePlane();
  const threePlane = new THREE.Plane(plane.normal, -plane.origin.dot(plane.normal));
  const hit = new THREE.Vector3();
  const intersected = rc.ray.intersectPlane(threePlane, hit);
  if (!intersected) return null;
  return worldToSketch2D(hit, getActivePlaneName());
}

// ── Sketch entity rendering ────────────────────────────────────

function refreshSketchEntities() {
  if (!ctx.value) return;
  const group = ctx.value.sketchGroup;

  // Dispose all previous children
  while (group.children.length > 0) {
    const child = group.children[0];
    if (child instanceof THREE.Line || child instanceof THREE.Points || child instanceof THREE.Mesh) {
      child.geometry?.dispose();
      (child.material as THREE.Material)?.dispose();
    }
    group.remove(child);
  }

  const planeName = getActivePlaneName();
  for (const entity of store.entities) {
    const selected = store.selectedId === entity.id;
    const color = selected ? 0x4fc3f7 : 0xc0c0c0;

    switch (entity.type) {
      case "Point": {
        const p = sketchToWorld3D(entity.x, entity.y, planeName);
        const geo = new THREE.SphereGeometry(0.05, 8, 8);
        const dot = new THREE.Mesh(geo, new THREE.MeshBasicMaterial({ color }));
        dot.position.copy(p);
        group.add(dot);
        break;
      }
      case "Line": {
        const constr = (entity as any).construction === true;
        const pts = [
          sketchToWorld3D(entity.x1, entity.y1, planeName),
          sketchToWorld3D(entity.x2, entity.y2, planeName),
        ];
        const geo = new THREE.BufferGeometry().setFromPoints(pts);
        const lColor = constr ? (selected ? 0x4fc3f7 : 0x666666) : color;
        group.add(new THREE.Line(geo, new THREE.LineBasicMaterial({ color: lColor, depthTest: false })));
        break;
      }
      case "Circle": {
        const constr = (entity as any).construction === true;
        renderCircle(group, planeName, entity.cx, entity.cy, entity.radius, constr, color, selected);
        break;
      }
      case "Arc": {
        renderArc(group, planeName, entity.cx, entity.cy, entity.radius, entity.start_angle, entity.end_angle, color);
        break;
      }
      case "Spline": {
        if (entity.points.length >= 2) {
          const pts = entity.points.map(([x, y]) => sketchToWorld3D(x, y, planeName));
          const geo = new THREE.BufferGeometry().setFromPoints(pts);
          group.add(new THREE.Line(geo, new THREE.LineBasicMaterial({ color, depthTest: false })));
        }
        break;
      }
      case "Ellipse": {
        renderEllipse(group, planeName, entity.cx, entity.cy, entity.major_rx, entity.major_ry, entity.ratio, color);
        break;
      }
    }
  }
}

function renderCircle(group: THREE.Group, planeName: string, cx: number, cy: number, r: number, constr: boolean, color: number, selected: boolean) {
  const center = sketchToWorld3D(cx, cy, planeName);
  const { uDir, vDir } = planeFrame(planeName);
  const pts: THREE.Vector3[] = [];
  for (let i = 0; i <= 64; i++) {
    const a = (i / 64) * Math.PI * 2;
    pts.push(center.clone().add(uDir.clone().multiplyScalar(Math.cos(a) * r)).add(vDir.clone().multiplyScalar(Math.sin(a) * r)));
  }
  const geo = new THREE.BufferGeometry().setFromPoints(pts);
  const c = constr ? (selected ? 0x4fc3f7 : 0x666666) : color;
  group.add(new THREE.Line(geo, new THREE.LineBasicMaterial({ color: c, depthTest: false })));
}

function renderArc(group: THREE.Group, planeName: string, cx: number, cy: number, r: number, sa: number, ea: number, color: number) {
  const center = sketchToWorld3D(cx, cy, planeName);
  const { uDir, vDir } = planeFrame(planeName);
  const n = 32;
  const pts: THREE.Vector3[] = [];
  for (let i = 0; i <= n; i++) {
    const a = sa + (ea - sa) * (i / n);
    pts.push(center.clone().add(uDir.clone().multiplyScalar(Math.cos(a) * r)).add(vDir.clone().multiplyScalar(Math.sin(a) * r)));
  }
  const geo = new THREE.BufferGeometry().setFromPoints(pts);
  group.add(new THREE.Line(geo, new THREE.LineBasicMaterial({ color, depthTest: false })));
}

function renderEllipse(group: THREE.Group, planeName: string, cx: number, cy: number, majorRx: number, majorRy: number, ratio: number, color: number) {
  const center = sketchToWorld3D(cx, cy, planeName);
  const majorLen = Math.sqrt(majorRx ** 2 + majorRy ** 2);
  const a = majorLen;
  const b = a * ratio;
  const angle = Math.atan2(majorRy, majorRx);
  const { uDir, vDir } = planeFrame(planeName);
  const pts: THREE.Vector3[] = [];
  for (let i = 0; i <= 64; i++) {
    const t = (i / 64) * Math.PI * 2;
    const lx = a * Math.cos(t);
    const ly = b * Math.sin(t);
    const wx = lx * Math.cos(angle) - ly * Math.sin(angle);
    const wy = lx * Math.sin(angle) + ly * Math.cos(angle);
    pts.push(center.clone().add(uDir.clone().multiplyScalar(wx)).add(vDir.clone().multiplyScalar(wy)));
  }
  const geo = new THREE.BufferGeometry().setFromPoints(pts);
  group.add(new THREE.Line(geo, new THREE.LineBasicMaterial({ color, depthTest: false })));
}

// ── Preview drawing ────────────────────────────────────────────

function clearSketchPreviews() {
  if (!ctx.value) return;
  if (sketchPreviewLine) {
    ctx.value.sketchGroup.remove(sketchPreviewLine);
    sketchPreviewLine.geometry.dispose();
    (sketchPreviewLine.material as THREE.Material).dispose();
    sketchPreviewLine = null;
  }
  if (sketchPreviewCircle) {
    ctx.value.sketchGroup.remove(sketchPreviewCircle);
    sketchPreviewCircle.geometry.dispose();
    (sketchPreviewCircle.material as THREE.Material).dispose();
    sketchPreviewCircle = null;
  }
}

function drawLinePreview(start: { x: number; y: number }, end: { x: number; y: number }) {
  if (!ctx.value) return;
  clearSketchPreviews();
  const planeName = getActivePlaneName();
  const pts = [sketchToWorld3D(start.x, start.y, planeName), sketchToWorld3D(end.x, end.y, planeName)];
  const geo = new THREE.BufferGeometry().setFromPoints(pts);
  sketchPreviewLine = new THREE.Line(geo, new THREE.LineBasicMaterial({
    color: 0x4fc3f7, depthTest: false, transparent: true, opacity: 0.6,
  }));
  ctx.value.sketchGroup.add(sketchPreviewLine);
}

function drawCirclePreview(center: { x: number; y: number }, radius: number) {
  if (!ctx.value) return;
  clearSketchPreviews();
  const planeName = getActivePlaneName();
  const c = sketchToWorld3D(center.x, center.y, planeName);
  const { uDir, vDir } = planeFrame(planeName);
  const pts: THREE.Vector3[] = [];
  for (let i = 0; i <= 64; i++) {
    const a = (i / 64) * Math.PI * 2;
    pts.push(c.clone().add(uDir.clone().multiplyScalar(Math.cos(a) * radius)).add(vDir.clone().multiplyScalar(Math.sin(a) * radius)));
  }
  const geo = new THREE.BufferGeometry().setFromPoints(pts);
  sketchPreviewCircle = new THREE.Line(geo, new THREE.LineBasicMaterial({
    color: 0x4fc3f7, depthTest: false, transparent: true, opacity: 0.6,
  }));
  ctx.value.sketchGroup.add(sketchPreviewCircle);
}

/// Preview an arc sweep from `startAngle` to `endAngle` (radians).
/// Draws the arc plus two radii so the sweep is visually obvious.
function drawArcPreview(center: { x: number; y: number }, radius: number, startAngle: number, endAngle: number) {
  if (!ctx.value) return;
  clearSketchPreviews();
  const planeName = getActivePlaneName();
  const c = sketchToWorld3D(center.x, center.y, planeName);
  const { uDir, vDir } = planeFrame(planeName);
  // Normalize span so endAngle > startAngle
  let span = endAngle - startAngle;
  while (span < 0) span += Math.PI * 2;
  while (span > Math.PI * 2) span -= Math.PI * 2;
  const n = Math.max(8, Math.ceil(span / (Math.PI / 32)));
  const pts: THREE.Vector3[] = [];
  // Start radius
  const startPt = c.clone()
    .add(uDir.clone().multiplyScalar(Math.cos(startAngle) * radius))
    .add(vDir.clone().multiplyScalar(Math.sin(startAngle) * radius));
  pts.push(c.clone());
  pts.push(startPt.clone());
  // Arc
  for (let i = 0; i <= n; i++) {
    const a = startAngle + (span * i) / n;
    pts.push(c.clone()
      .add(uDir.clone().multiplyScalar(Math.cos(a) * radius))
      .add(vDir.clone().multiplyScalar(Math.sin(a) * radius)));
  }
  // End radius back to center
  pts.push(c.clone());
  const geo = new THREE.BufferGeometry().setFromPoints(pts);
  sketchPreviewCircle = new THREE.Line(geo, new THREE.LineBasicMaterial({
    color: 0x4fc3f7, depthTest: false, transparent: true, opacity: 0.6,
  }));
  ctx.value.sketchGroup.add(sketchPreviewCircle);
}

// ── Solid rendering ────────────────────────────────────────────

function buildGeometry(rm: RenderMesh): THREE.BufferGeometry {
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.BufferAttribute(new Float32Array(rm.positions), 3));
  g.setAttribute("normal", new THREE.BufferAttribute(new Float32Array(rm.normals), 3));
  g.setIndex(new THREE.BufferAttribute(new Uint32Array(rm.indices), 1));
  g.computeBoundingSphere();
  return g;
}

function clearSolids() {
  if (!ctx.value) return;
  for (const m of solidMeshes) {
    ctx.value.scene.remove(m);
    m.geometry.dispose();
    (m.material as THREE.Material).dispose();
  }
  for (const l of edgeLines) {
    ctx.value.scene.remove(l);
    l.geometry.dispose();
    (l.material as THREE.Material).dispose();
  }
  solidMeshes = [];
  edgeLines = [];
  // Edge-pick highlights and measurement visuals are tied to specific
  // meshes — they become invalid when solids are replaced.
  clearEdgeHover();
  clearEdgeHighlights();
  clearMeasureVisuals();
}

async function refreshViewport() {
  try {
    const entries = await getAllSolidMeshes();
    if (!ctx.value) return;
    clearSolids();

    // Build a feature-id -> hex-color map from the store's featureColors
    // and from FeatureNode.color (for future backend persistence).
    const colorMap: Record<number, string> = { ...store.featureColors };
    for (const f of store.features) {
      if (f.color && !colorMap[f.id]) {
        colorMap[f.id] = f.color;
      }
    }

    for (let i = 0; i < entries.length; i++) {
      const e = entries[i];
      const geo = buildGeometry(e.mesh);
      const defaultColor = SOLID_COLORS[i % SOLID_COLORS.length];
      const hexColor = colorMap[e.feature_id];
      const color = hexColor ? parseInt(hexColor.replace("#", ""), 16) : defaultColor;
      const mat = new THREE.MeshStandardMaterial({
        color,
        roughness: 0.35,
        metalness: 0.05,
        side: THREE.DoubleSide,
        wireframe: wireframe.value,
      });

      // Per-feature transparency (M19). If opacity < 1.0, make the material
      // transparent and adjust depthWrite for correct blending. Edge overlays
      // always stay fully opaque.
      const opacity = store.featureOpacities[e.feature_id] ?? 1.0;
      if (opacity < 1.0) {
        mat.transparent = true;
        mat.opacity = opacity;
        mat.depthWrite = opacity >= 0.5;
      }

      const mesh = new THREE.Mesh(geo, mat);
      mesh.name = e.name;
      mesh.userData = { featureId: e.feature_id };
      // Render transparent meshes after opaque ones so they blend correctly.
      if (opacity < 1.0) {
        mesh.renderOrder = 1;
      }
      ctx.value.scene.add(mesh);
      solidMeshes.push(mesh);

      const edgeGeo = new THREE.EdgesGeometry(geo, 15);
      const edgeMat = new THREE.LineBasicMaterial({ color: 0x111111, transparent: true, opacity: 0.4 });
      const line = new THREE.LineSegments(edgeGeo, edgeMat);
      line.visible = showEdges.value;
      ctx.value.scene.add(line);
      edgeLines.push(line);
    }

    if (solidMeshes.length > 0) fitSceneView(solidMeshes);

    // Restore edge-pick highlights after mesh rebuild (best-effort).
    if (store.edgePickMode) refreshSelectedEdgeHighlights();
  } catch (err) {
    toast.error("刷新视口失败: " + String(err));
  }
}

// ── View commands exposed to template ──────────────────────────

function setView(name: ViewName) {
  if (name === "face" && isSketchMode.value) {
    if (!ctx.value) return;
    const plane = getActivePlane();
    const dist = 8;
    ctx.value.camera.position.copy(plane.origin.clone().add(plane.normal.clone().multiplyScalar(dist)));
    ctx.value.controls.target.copy(plane.origin);
    ctx.value.controls.update();
    return;
  }
  setSceneView(name);
}

function toggleWireframe() {
  toggleSceneWireframe();
  for (const m of solidMeshes) {
    (m.material as THREE.MeshStandardMaterial).wireframe = wireframe.value;
  }
}

function toggleEdges() {
  toggleSceneEdges();
  for (const l of edgeLines) l.visible = showEdges.value;
}

function fitView(meshes?: THREE.Object3D[]) {
  fitSceneView(meshes ?? solidMeshes);
}

// ── Mouse / sketch interaction ─────────────────────────────────

function computeSnap(sketch: { x: number; y: number }, sx: number, sy: number): SnapResult | null {
  if (!ctx.value) return null;
  const planeName = getActivePlaneName();
  const candidates: SnapResult[] = [];

  const toScreen = (x: number, y: number) => {
    const p = sketchToWorld3D(x, y, planeName);
    return worldToScreen(p.x, p.y, p.z);
  };

  for (const entity of store.entities) {
    if (entity.type === "Point") {
      const s = toScreen(entity.x, entity.y);
      const dist = Math.hypot(s.x - sx, s.y - sy);
      if (dist < SNAP_PX) candidates.push({ type: "endpoint", world: { x: entity.x, y: entity.y }, screen: s });
    }
  }
  for (const entity of store.entities) {
    if (entity.type === "Line") {
      const mx = (entity.x1 + entity.x2) / 2;
      const my = (entity.y1 + entity.y2) / 2;
      const s = toScreen(mx, my);
      if (Math.hypot(s.x - sx, s.y - sy) < SNAP_PX) candidates.push({ type: "midpoint", world: { x: mx, y: my }, screen: s });
    }
  }
  for (const entity of store.entities) {
    if (entity.type === "Circle" || entity.type === "Arc") {
      const s = toScreen(entity.cx, entity.cy);
      if (Math.hypot(s.x - sx, s.y - sy) < SNAP_PX) candidates.push({ type: "center", world: { x: entity.cx, y: entity.cy }, screen: s });
    }
  }
  // Line intersections
  const lines = store.entities.filter(e => e.type === "Line");
  for (let i = 0; i < lines.length; i++) {
    for (let j = i + 1; j < lines.length; j++) {
      const inter = lineIntersection(
        lines[i].x1, lines[i].y1, lines[i].x2, lines[i].y2,
        lines[j].x1, lines[j].y1, lines[j].x2, lines[j].y2,
      );
      if (inter) {
        const s = toScreen(inter.x, inter.y);
        if (Math.hypot(s.x - sx, s.y - sy) < SNAP_PX) candidates.push({ type: "intersection", world: inter, screen: s });
      }
    }
  }
  // Grid snap
  const gx = Math.round(sketch.x);
  const gy = Math.round(sketch.y);
  const gs = toScreen(gx, gy);
  if (Math.hypot(gs.x - sx, gs.y - sy) < SNAP_PX * 0.6) candidates.push({ type: "grid", world: { x: gx, y: gy }, screen: gs });

  candidates.sort((a, b) => {
    const pd = SNAP_PRIORITY[a.type] - SNAP_PRIORITY[b.type];
    if (pd !== 0) return pd;
    return Math.hypot(a.screen.x - sx, a.screen.y - sy) - Math.hypot(b.screen.x - sx, b.screen.y - sy);
  });
  return candidates[0] ?? null;
}

async function onMouseDown(e: MouseEvent) {
  if (e.button === 1 || e.button === 2) return;

  // ── Edge-pick mode (only outside sketch editing) ──────────────────
  if (store.edgePickMode && !isSketchMode.value && ctx.value) {
    const rect = ctx.value.renderer.domElement.getBoundingClientRect();
    ctx.value.raycaster.setFromCamera(
      new THREE.Vector2(
        ((e.clientX - rect.left) / rect.width) * 2 - 1,
        -((e.clientY - rect.top) / rect.height) * 2 + 1,
      ),
      ctx.value.camera,
    );
    const edgeHit = findClosestEdge(ctx.value.raycaster, 0.08);
    if (edgeHit) {
      store.addPickedEdge(edgeHit.vA, edgeHit.vB);
      refreshSelectedEdgeHighlights();
      emit("edgeSelected", edgeHit.featureId, edgeHit.vA, edgeHit.vB);
    }
    return;
  }

  // ── Measurement mode ──────────────────────────────────────────────
  if (store.measureMode && !isSketchMode.value && ctx.value) {
    const rect = ctx.value.renderer.domElement.getBoundingClientRect();
    ctx.value.raycaster.setFromCamera(
      new THREE.Vector2(
        ((e.clientX - rect.left) / rect.width) * 2 - 1,
        -((e.clientY - rect.top) / rect.height) * 2 + 1,
      ),
      ctx.value.camera,
    );
    const hits = ctx.value.raycaster.intersectObjects(solidMeshes);
    if (hits.length > 0) {
      // If we already have 2 points, clear and start fresh.
      if (store.measurePoints.length >= 2) {
        store.clearMeasurePoints();
      }
      store.measurePoints.push({
        x: hits[0].point.x,
        y: hits[0].point.y,
        z: hits[0].point.z,
      });
      if (store.measurePoints.length >= 2) {
        refreshMeasureVisuals();
      }
    }
    return;
  }

  if (isDrawing.value) {
    const sketch = getSketchPoint(e);
    if (!sketch) return;
    await handleDrawingMouseDown(sketch);
    return;
  }

  if (isSketchMode.value && store.activeTool === "select") {
    const sketch = getSketchPoint(e);
    if (!sketch) return;
    const closest = findClosestEntity(store.entities, sketch.x, sketch.y);
    if (closest !== null) {
      store.select(closest);
      const isPoint = store.entities.find(en => en.id === closest)?.type === "Point";
      if (isPoint) {
        state.isDragging.value = true;
        state.dragId.value = closest;
      }
    } else {
      store.select(null);
    }
    return;
  }

  if (!isSketchMode.value && ctx.value) {
    const rect = ctx.value.renderer.domElement.getBoundingClientRect();
    ctx.value.raycaster.setFromCamera(
      new THREE.Vector2(
        ((e.clientX - rect.left) / rect.width) * 2 - 1,
        -((e.clientY - rect.top) / rect.height) * 2 + 1,
      ),
      ctx.value.camera,
    );
    const hits = ctx.value.raycaster.intersectObjects(solidMeshes);
    if (hits.length > 0) {
      const m = hits[0].object as THREE.Mesh;
      emit("faceSelected", (m.userData as any).featureId ?? 0, Math.floor(hits[0].faceIndex ?? 0));
    }
  }
}

async function handleDrawingMouseDown(sketch: { x: number; y: number }) {
  const world = activeSnap.value?.world ?? sketch;

  switch (store.activeTool) {
    case "line": {
      const hit = findPointAt(store.entities, world.x, world.y);
      if (state.lineStartId.value === null && state.lineStartPos.value === null) {
        // First click: defer point creation. Snap to existing point or store
        // position to be committed on the second click.
        if (hit) {
          state.lineStartId.value = hit;
        } else {
          state.lineStartPos.value = { x: world.x, y: world.y };
        }
      } else {
        // Second click: commit the start point (if deferred), then the end
        // point, then the line.
        try {
          let startId = state.lineStartId.value;
          if (startId === null && state.lineStartPos.value) {
            const id = await addTrackedPoint(state.lineStartPos.value.x, state.lineStartPos.value.y);
            if (id === null) { resetDrawingState(state); return; }
            startId = id;
            state.lineStartId.value = id;
          }
          const start = getLineStartCoords();
          if (!start || startId === null) { resetDrawingState(state); return; }
          const endWorld = snapAngle(start, world, true);
          let endId = hit;
          if (!endId) {
            const newId = await addTrackedPoint(endWorld.x, endWorld.y);
            if (newId === null) { resetDrawingState(state); return; }
            endId = newId;
          }
          await addLine(startId, endId);
          // Points are now referenced by the line — clear tracking.
          pendingPointIds.value = [];
          state.lineStartId.value = endId;
          state.lineStartPos.value = null;
          await refreshSketch();
        } catch (err) {
          toast.error("绘制直线失败: " + String(err));
          await cleanupPendingPoints();
          resetDrawingState(state);
          clearSketchPreviews();
        }
      }
      break;
    }
    case "circle": {
      if (state.circleCenterId.value === null && state.circleCenterPos.value === null) {
        // First click: defer center point creation.
        const centerHit = findPointAt(store.entities, world.x, world.y);
        if (centerHit) {
          state.circleCenterId.value = centerHit;
        } else {
          state.circleCenterPos.value = { x: world.x, y: world.y };
        }
        drawCirclePreview(world, 0.1);
      } else {
        // Second click: commit center point (if deferred), then create circle.
        try {
          if (state.circleCenterId.value === null && state.circleCenterPos.value) {
            const id = await addTrackedPoint(state.circleCenterPos.value.x, state.circleCenterPos.value.y);
            if (id === null) { resetDrawingState(state); toast.error("无法创建圆心点"); return; }
            state.circleCenterId.value = id;
          }
          const center = getCircleCenterCoords();
          if (center && state.circleCenterId.value !== null) {
            const r = Math.hypot(world.x - center.x, world.y - center.y);
            if (r > 0.01) {
              const circleId = await addCircle(state.circleCenterId.value, r);
              if (circleId === null) {
                toast.error("无法创建圆 — 请确认当前处于草图编辑模式");
                resetDrawingState(state);
                clearSketchPreviews();
                return;
              }
            }
          } else {
            toast.error("无法获取圆心坐标");
          }
          // Center point is now referenced by the circle — clear tracking.
          pendingPointIds.value = [];
          state.circleCenterId.value = null;
          state.circleCenterPos.value = null;
          clearSketchPreviews();
          await refreshSketch();
        } catch (err) {
          toast.error("绘制圆失败: " + String(err));
          await cleanupPendingPoints();
          resetDrawingState(state);
          clearSketchPreviews();
        }
      }
      break;
    }
    case "arc": {
      if (state.arcCenterId.value === null && state.arcCenterPos.value === null) {
        // First click: defer center point.
        const centerHit = findPointAt(store.entities, world.x, world.y);
        if (centerHit) {
          state.arcCenterId.value = centerHit;
        } else {
          state.arcCenterPos.value = { x: world.x, y: world.y };
        }
      } else if (!state.arcRadiusSet.value) {
        state.arcRadiusPoint.value = { x: world.x, y: world.y };
        state.arcRadiusSet.value = true;
      } else {
        // Third click: commit center point (if deferred), then create arc.
        try {
          if (state.arcCenterId.value === null && state.arcCenterPos.value) {
            const id = await addTrackedPoint(state.arcCenterPos.value.x, state.arcCenterPos.value.y);
            if (id === null) { resetDrawingState(state); return; }
            state.arcCenterId.value = id;
          }
          const center = getArcCenterCoords();
          if (!center || state.arcCenterId.value === null) {
            state.arcCenterId.value = null;
            state.arcCenterPos.value = null;
            state.arcRadiusSet.value = false;
            state.arcRadiusPoint.value = null;
            return;
          }
          const dx1 = state.arcRadiusPoint.value!.x - center.x;
          const dy1 = state.arcRadiusPoint.value!.y - center.y;
          const r = Math.sqrt(dx1 * dx1 + dy1 * dy1);
          const startAngle = Math.atan2(dy1, dx1);
          const endAngle = Math.atan2(world.y - center.y, world.x - center.x);
          if (r > 0.01 && Math.abs(endAngle - startAngle) > 0.01) {
            await addArc(state.arcCenterId.value, r, startAngle, endAngle);
          }
          // Center point is now referenced by the arc — clear tracking.
          pendingPointIds.value = [];
          state.arcCenterId.value = null;
          state.arcCenterPos.value = null;
          state.arcRadiusPoint.value = null;
          state.arcRadiusSet.value = false;
          clearSketchPreviews();
          await refreshSketch();
        } catch (err) {
          toast.error("绘制弧线失败: " + String(err));
          await cleanupPendingPoints();
          resetDrawingState(state);
          clearSketchPreviews();
        }
      }
      break;
    }
    case "spline": {
      try {
        const hit = findPointAt(store.entities, world.x, world.y);
        if (hit) {
          state.splinePoints.value.push(hit);
        } else {
          const pid = await addTrackedPoint(world.x, world.y);
          if (pid !== null) state.splinePoints.value.push(pid);
          await refreshSketch();
        }
      } catch (err) {
        toast.error("添加样条点失败: " + String(err));
      }
      break;
    }
    case "ellipse": {
      if (state.ellipseCenterId.value === null && state.ellipseCenterPos.value === null) {
        // First click: defer center point.
        const hit = findPointAt(store.entities, world.x, world.y);
        if (hit) {
          state.ellipseCenterId.value = hit;
        } else {
          state.ellipseCenterPos.value = { x: world.x, y: world.y };
        }
      } else {
        // Second click: commit center point (if deferred), create end point, then ellipse.
        try {
          if (state.ellipseCenterId.value === null && state.ellipseCenterPos.value) {
            const pid = await addTrackedPoint(state.ellipseCenterPos.value.x, state.ellipseCenterPos.value.y);
            if (pid !== null) state.ellipseCenterId.value = pid;
          }
          const hit = findPointAt(store.entities, world.x, world.y);
          let endId = hit;
          if (!endId) {
            const pid = await addTrackedPoint(world.x, world.y);
            if (pid === null) return;
            endId = pid;
          }
          if (state.ellipseCenterId.value !== null) {
            await addEllipse(state.ellipseCenterId.value, endId, 0.5);
          }
          // Points are now referenced by the ellipse — clear tracking.
          pendingPointIds.value = [];
          state.ellipseCenterId.value = null;
          state.ellipseCenterPos.value = null;
          await refreshSketch();
        } catch (err) {
          toast.error("绘制椭圆失败: " + String(err));
          await cleanupPendingPoints();
          resetDrawingState(state);
          clearSketchPreviews();
        }
      }
      break;
    }
    case "rectangle": {
      if (state.rectStart.value === null) {
        state.rectStart.value = { ...world };
      } else {
        const x1 = state.rectStart.value.x, y1 = state.rectStart.value.y;
        const x2 = world.x, y2 = world.y;
        try {
          const p1 = await addTrackedPoint(x1, y1), p2 = await addTrackedPoint(x2, y1);
          const p3 = await addTrackedPoint(x2, y2), p4 = await addTrackedPoint(x1, y2);
          if (p1 && p2 && p3 && p4) {
            const l1 = await addLine(p1, p2);
            const l2 = await addLine(p2, p3);
            const l3 = await addLine(p3, p4);
            const l4 = await addLine(p4, p1);
            // Points are now referenced — clear tracking before auto-constraining.
            pendingPointIds.value = [];
            if (l1 && l2 && l3 && l4) {
              await addConstraint({ Horizontal: { line: l1 } });
              await addConstraint({ Horizontal: { line: l3 } });
              await addConstraint({ Vertical: { line: l2 } });
              await addConstraint({ Vertical: { line: l4 } });
              await addConstraint({ Parallel: { line_a: l1, line_b: l3 } });
              await addConstraint({ Parallel: { line_a: l2, line_b: l4 } });
              await addConstraint({ Equal: { a: l1, b: l3 } });
              await addConstraint({ Equal: { a: l2, b: l4 } });
              await solveSketch();
            }
          }
          state.rectStart.value = null;
          await refreshSketch();
        } catch (err) {
          toast.error("绘制矩形失败: " + String(err));
          await cleanupPendingPoints();
          resetDrawingState(state);
          clearSketchPreviews();
        }
      }
      break;
    }
  }
}

async function onMouseMove(e: MouseEvent) {
  if (!containerRef.value) return;
  const rect = containerRef.value.getBoundingClientRect();
  const sx = e.clientX - rect.left;
  const sy = e.clientY - rect.top;

  // ── Edge hover highlight (when in edge-pick mode, outside sketch) ─
  if (store.edgePickMode && !isSketchMode.value && ctx.value) {
    clearEdgeHover();
    ctx.value.raycaster.setFromCamera(
      new THREE.Vector2(
        ((e.clientX - rect.left) / rect.width) * 2 - 1,
        -((e.clientY - rect.top) / rect.height) * 2 + 1,
      ),
      ctx.value.camera,
    );
    const edgeHit = findClosestEdge(ctx.value.raycaster, 0.05);
    if (edgeHit) {
      showEdgeHover(edgeHit.mesh, edgeHit.vA, edgeHit.vB);
    }
  }

  if (isSketchMode.value && store.activeTool === "select" && state.isDragging.value && state.dragId.value) {
    const sketch = getSketchPoint(e);
    if (sketch) {
      try {
        const target = activeSnap.value?.world ?? sketch;
        // First move takes a snapshot; subsequent moves skip it to avoid flooding undo stack
        if (!state.dragSnapshotted) {
          await movePoint(state.dragId.value, target.x, target.y);
          state.dragSnapshotted = true;
        } else {
          await movePointNoSnapshot(state.dragId.value, target.x, target.y);
        }
        await solveSketch();
        await refreshSketch();
      } catch (err) {
        toast.error("拖动失败: " + String(err));
      }
    }
    return;
  }

  if (!isDrawing.value) return;

  const sketch = getSketchPoint(e);
  if (!sketch) return;
  hoverSketch.value = sketch;
  state.isHovering.value = true;
  activeSnap.value = computeSnap(sketch, sx, sy);

  if (state.isDragging.value && state.dragId.value) {
    try {
      const target = activeSnap.value?.world ?? sketch;
      await movePoint(state.dragId.value, target.x, target.y);
      await solveSketch();
      await refreshSketch();
    } catch (err) {
      toast.error("拖动失败: " + String(err));
    }
    return;
  }

  clearSketchPreviews();
  const previewWorld = activeSnap.value?.world ?? sketch;
  const planeName = getActivePlaneName();
  if (store.activeTool === "line" && (state.lineStartId.value !== null || state.lineStartPos.value !== null)) {
    const start = getLineStartCoords();
    if (start) {
      const snapped = snapAngle(start, previewWorld, true);
      drawLinePreview(start, snapped);
    }
  }
  if (store.activeTool === "circle" && (state.circleCenterId.value !== null || state.circleCenterPos.value !== null)) {
    const center = getCircleCenterCoords();
    if (center) {
      const r = Math.hypot(previewWorld.x - center.x, previewWorld.y - center.y);
      drawCirclePreview(center, r);
    }
  }
  if (store.activeTool === "arc" && (state.arcCenterId.value !== null || state.arcCenterPos.value !== null)) {
    const center = getArcCenterCoords();
    if (center) {
      if (state.arcRadiusSet.value && state.arcRadiusPoint.value) {
        // Third step: show arc sweep from start angle to current cursor angle.
        const dx1 = state.arcRadiusPoint.value.x - center.x;
        const dy1 = state.arcRadiusPoint.value.y - center.y;
        const r = Math.sqrt(dx1 * dx1 + dy1 * dy1);
        const startAngle = Math.atan2(dy1, dx1);
        const endAngle = Math.atan2(previewWorld.y - center.y, previewWorld.x - center.x);
        drawArcPreview(center, r, startAngle, endAngle);
      } else {
        // Second step: show full circle preview for radius selection.
        const r = Math.hypot(previewWorld.x - center.x, previewWorld.y - center.y);
        drawCirclePreview(center, r);
      }
    }
  }
  if (store.activeTool === "spline" && state.splinePoints.value.length > 0) {
    const pts = state.splinePoints.value
      .map(id => getPointCoords(id))
      .filter((p): p is { x: number; y: number } => p !== null);
    pts.push(previewWorld);
    if (pts.length >= 2) {
      const points3d = pts.map(p => sketchToWorld3D(p.x, p.y, planeName));
      const geo = new THREE.BufferGeometry().setFromPoints(points3d);
      sketchPreviewLine = new THREE.Line(geo, new THREE.LineBasicMaterial({
        color: 0x4fc3f7, depthTest: false, transparent: true, opacity: 0.6,
      }));
      ctx.value?.sketchGroup.add(sketchPreviewLine);
    }
  }
}

function onMouseUp() {
  const wasDragging = state.isDragging.value;
  state.isDragging.value = false;
  state.dragSnapshotted = false;
  state.dragId.value = null;
  // M9 should listen for this event and call unifiedViewport.value?.refreshViewport()
  // to keep the solid mesh in sync after the drag moves sketch points.
  if (wasDragging) {
    emit("drag-end");
  }
}

function onMouseLeave() {
  state.isHovering.value = false;
  activeSnap.value = null;
  clearSketchPreviews();
  clearEdgeHover();
}

async function onRightClick() {
  if (state.splinePoints.value.length >= 2) {
    try {
      await addSpline([...state.splinePoints.value]);
      // Spline points are now referenced — clear tracking.
      pendingPointIds.value = [];
      state.splinePoints.value = [];
      await refreshSketch();
      clearSketchPreviews();
    } catch (err) {
      toast.error("确认样条失败: " + String(err));
    }
    return;
  }
  // Cancel current drawing gesture: delete any pending orphan entities,
  // then reset the tool-specific state.
  await cleanupPendingPoints();
  if (state.lineStartId.value !== null || state.lineStartPos.value !== null) {
    state.lineStartId.value = null;
    state.lineStartPos.value = null;
    clearSketchPreviews();
  } else if (state.circleCenterId.value !== null || state.circleCenterPos.value !== null) {
    state.circleCenterId.value = null;
    state.circleCenterPos.value = null;
    clearSketchPreviews();
  } else if (state.rectStart.value !== null) {
    state.rectStart.value = null;
    clearSketchPreviews();
  } else if (state.arcCenterId.value !== null || state.arcCenterPos.value !== null) {
    state.arcCenterId.value = null;
    state.arcCenterPos.value = null;
    state.arcRadiusPoint.value = null;
    state.arcRadiusSet.value = false;
    clearSketchPreviews();
  } else if (state.ellipseCenterId.value !== null || state.ellipseCenterPos.value !== null) {
    state.ellipseCenterId.value = null;
    state.ellipseCenterPos.value = null;
    clearSketchPreviews();
  }
}

async function onDoubleClick() {
  if (state.splinePoints.value.length >= 2) {
    try {
      await addSpline([...state.splinePoints.value]);
      // Spline points are now referenced — clear tracking.
      pendingPointIds.value = [];
      state.splinePoints.value = [];
      await refreshSketch();
      clearSketchPreviews();
    } catch (err) {
      toast.error("确认样条失败: " + String(err));
    }
    return;
  }
  if (store.activeTool === "line" && (state.lineStartId.value !== null || state.lineStartPos.value !== null)) {
    state.lineStartId.value = null;
    state.lineStartPos.value = null;
    clearSketchPreviews();
  }
}

// ── Helpers ────────────────────────────────────────────────────

function getPointCoords(id: EntityId): { x: number; y: number } | null {
  const entity = store.entities.find(e => e.id === id);
  if (entity?.type === "Point") return { x: entity.x, y: entity.y };
  return null;
}

/// Return the current line-start coordinates, whether the start came from
/// an existing snap (`lineStartId`) or a deferred position (`lineStartPos`).
/// Falls back to `lineStartPos` when the point was just created and the store
/// hasn't been refreshed yet (getPointCoords returns null).
function getLineStartCoords(): { x: number; y: number } | null {
  if (state.lineStartId.value !== null) {
    return getPointCoords(state.lineStartId.value) ?? state.lineStartPos.value;
  }
  return state.lineStartPos.value;
}

/// Return the current circle-center coordinates.
/// Falls back to `circleCenterPos` when the center point was just created
/// and the store hasn't been refreshed yet.
function getCircleCenterCoords(): { x: number; y: number } | null {
  if (state.circleCenterId.value !== null) {
    return getPointCoords(state.circleCenterId.value) ?? state.circleCenterPos.value;
  }
  return state.circleCenterPos.value;
}

/// Return the current arc-center coordinates.
/// Falls back to `arcCenterPos` when the center point was just created
/// and the store hasn't been refreshed yet.
function getArcCenterCoords(): { x: number; y: number } | null {
  if (state.arcCenterId.value !== null) {
    return getPointCoords(state.arcCenterId.value) ?? state.arcCenterPos.value;
  }
  return state.arcCenterPos.value;
}

/// Delete every pending entity, then clear the tracking array. Used when the
/// user cancels mid-draw so that no orphan entities linger in the sketch.
/// Always re-syncs the store afterwards — without this the deleted points
/// linger as ghosts (rendered, pickable, constrainable) until the next
/// unrelated refresh (原则2).
async function cleanupPendingPoints() {
  const ids = [...pendingPointIds.value];
  pendingPointIds.value = [];
  if (ids.length === 0) return;
  for (const id of ids) {
    try { await deleteEntity(id); } catch { /* best-effort */ }
  }
  await refreshSketch();
}

/// Register a freshly created point id for cleanup on cancel. When the shape
/// is confirmed, the caller clears pendingPointIds after the shape entity
/// (line/circle/etc.) is created — the point is now referenced and must stay.
function trackPendingPoint(id: number) {
  pendingPointIds.value.push(id);
}

/// Call addPoint and track the returned id in pendingPointIds.
async function addTrackedPoint(x: number, y: number): Promise<number | null> {
  const id = await addPoint(x, y);
  if (id !== null) trackPendingPoint(id);
  return id;
}

// ── Public API ─────────────────────────────────────────────────

async function refreshSketch() {
  try {
    store.setEntities(await getSketchEntities());
    store.setConstraints(await getSketchConstraints());
    refreshSketchEntities();
  } catch (err) {
    toast.error("刷新草图失败: " + String(err));
  }
}

async function onDimensionRefresh() {
  await refreshSketch();
  // A dimension edit changes sketch geometry, which any dependent solid is
  // built from — refresh the viewport too (原则2), not just the 2D layer.
  await refreshViewport();
  emit("dimension-edited");
}

/**
 * Project a sketch-space (sx, sy) point to viewport CSS pixels. Passed to
 * the DimensionOverlay as a function prop to avoid exposing THREE objects
 * to Vue's reactivity system (which proxies them and breaks WebGL).
 */
function projectFn(sx: number, sy: number): { x: number; y: number } | null {
  if (!ctx.value) return null;
  const planeName = getActivePlaneName();
  const w = sketchToWorld3D(sx, sy, planeName);
  return worldToScreen(w.x, w.y, w.z);
}

function showPreviewMesh(rm: RenderMesh) {
  if (!ctx.value) return;
  clearPreviewMesh();
  const geo = buildGeometry(rm);
  const mat = new THREE.MeshStandardMaterial({
    color: 0xff9800,
    roughness: 0.3,
    metalness: 0.1,
    side: THREE.DoubleSide,
    transparent: true,
    opacity: 0.55,
  });
  const mesh = new THREE.Mesh(geo, mat);
  ctx.value.previewGroup.add(mesh);

  const edgeGeo = new THREE.EdgesGeometry(geo, 15);
  const edgeMat = new THREE.LineBasicMaterial({ color: 0x664400, transparent: true, opacity: 0.5 });
  ctx.value.previewGroup.add(new THREE.LineSegments(edgeGeo, edgeMat));
}

function clearPreviewMesh() {
  if (!ctx.value) return;
  while (ctx.value.previewGroup.children.length > 0) {
    const child = ctx.value.previewGroup.children[0];
    if (child instanceof THREE.Mesh || child instanceof THREE.LineSegments) {
      child.geometry?.dispose();
      (child.material as THREE.Material)?.dispose();
    }
    ctx.value.previewGroup.remove(child);
  }
}

defineExpose({ refreshViewport, refreshSketch, showPreviewMesh, clearPreviewMesh, setView, fitView });

// ── Watchers ───────────────────────────────────────────────────

watch(isSketchMode, (editing) => {
  if (!ctx.value) return;
  // Always show sketch entities so they are visible in 3D view.
  ctx.value.sketchGroup.visible = true;
  if (editing) {
    setView("face");
    ctx.value.controls.enableRotate = true;
  } else {
    clearSketchPreviews();
  }
}, { immediate: false });

watch(isDrawing, (drawing) => {
  if (!ctx.value) return;
  if (drawing) {
    ctx.value.controls.enabled = false;
  } else {
    ctx.value.controls.enabled = true;
    ctx.value.controls.enableRotate = true;
    ctx.value.controls.mouseButtons = {
      LEFT: THREE.MOUSE.ROTATE,
      MIDDLE: THREE.MOUSE.PAN,
      RIGHT: THREE.MOUSE.PAN,
    };
  }
});

watch(() => store.activeTool, () => {
  // Delete any pending orphan entities before resetting tool state.
  // Fire-and-forget is fine: cleanupPendingPoints re-syncs the store at
  // the end and races with the next draw are prevented because the next
  // gesture can't start until the user clicks again.
  cleanupPendingPoints();
  resetDrawingState(state);
  clearSketchPreviews();
});

watch(() => store.entities, () => {
  refreshSketchEntities();
}, { deep: true });

// ── Edge-pick / measurement watchers ─────────────────────────────
watch(() => store.measureMode, (active) => {
  if (!active) clearMeasureVisuals();
});

watch(() => store.pendingEdges, () => {
  refreshSelectedEdgeHighlights();
}, { deep: true });

watch(() => store.edgePickMode, (active) => {
  if (!active) {
    clearEdgeHover();
    clearEdgeHighlights();
  }
});

watch(() => store.measurePoints, () => {
  // Keep visuals in sync when points are cleared externally.
  if (store.measurePoints.length < 2 && measureLine) {
    clearMeasureVisuals();
  }
}, { deep: true });

// Color / opacity changes should trigger an immediate viewport refresh
watch(() => store.featureColors, () => {
  refreshViewport();
}, { deep: true });

watch(() => store.featureOpacities, () => {
  refreshViewport();
}, { deep: true });

let unlistenCapture: (() => void) | null = null;

onMounted(async () => {
  initScene();
  refreshViewport();
  refreshSketchEntities();
  animate();

  // Listen for viewport capture requests from the backend
  unlistenCapture = await listen("capture-viewport", () => {
    const dataUrl = captureViewport();
    if (dataUrl) {
      const { emit } = (window as any).__TAURI__?.event ?? {};
      emit?.("viewport-image", { data: dataUrl });
    }
  });
});

onUnmounted(() => {
  if (unlistenCapture) unlistenCapture();
  destroyHighlightGroup();
  dispose();
});
</script>

<style scoped>
.unified-viewport { width: 100%; height: 100%; position: relative; overflow: hidden; }
.unified-viewport :deep(canvas) { display: block; }
.vp-toolbar {
  position: absolute; top: 8px; right: 8px; z-index: 20;
  display: flex; gap: 2px; background: rgba(40,40,40,0.85);
  padding: 3px 6px; border-radius: 4px; border: 1px solid #444;
}
.vp-toolbar button {
  background: transparent; border: 1px solid transparent; color: #aaa;
  padding: 3px 7px; font-size: 11px; cursor: pointer; border-radius: 2px;
}
.vp-toolbar button:hover { background: #555; color: #fff; }
.vp-toolbar button.active { background: #007acc; color: #fff; border-color: #007acc; }
.vp-sep { width: 1px; background: #555; margin: 0 3px; }
.sketch-info {
  position: absolute; bottom: 8px; left: 8px;
  display: flex; gap: 16px; font-size: 12px; color: #bbb;
  pointer-events: none; background: rgba(30,30,30,0.85);
  padding: 4px 10px; border-radius: 3px;
}
.snap-indicator {
  position: absolute; pointer-events: none; z-index: 10;
  transform: translate(-50%, -50%);
}
.snap-label {
  position: absolute; top: 12px; left: 8px;
  font-size: 11px; white-space: nowrap;
  background: rgba(0,0,0,0.8); padding: 1px 5px; border-radius: 2px;
}
.snap-endpoint .snap-label { color: #ffeb3b; }
.snap-midpoint .snap-label { color: #69f0ae; }
.snap-center .snap-label { color: #ff5252; }
.snap-intersection .snap-label { color: #ff4081; }
.snap-grid .snap-label { color: #888; }
.snap-tangent .snap-label { color: #e040fb; }
.radius-display { color: #ffd54f; font-weight: 600; }
.vp-dim-toggle {
  position: absolute; top: 8px; left: 8px; z-index: 20;
  background: rgba(40,40,40,0.85); border: 1px solid #444;
  color: #aaa; padding: 3px 10px; font-size: 11px;
  cursor: pointer; border-radius: 3px;
}
.vp-dim-toggle:hover { background: #555; color: #fff; }
.vp-dim-toggle.active { background: #007acc; color: #fff; border-color: #007acc; }
</style>
