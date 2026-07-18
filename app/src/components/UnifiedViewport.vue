<template>
  <div
    ref="containerRef"
    class="unified-viewport"
    @mousedown="onMouseDown"
    @mousemove="onMouseMove"
    @mouseup="onMouseUp"
    @mouseleave="onMouseLeave"
    @wheel="onWheel"
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
import { computed, onMounted, onUnmounted, watch, ref } from "vue";
import * as THREE from "three";
import {
  getAllSolidMeshes,
  getSketchEntities, getSketchConstraints,
  addPoint, addLine, addCircle, addArc, addSpline, addEllipse,
  solveSketch, movePoint,
  type RenderMesh, type EntityId,
} from "@/commands/sketch";
import { useSketchStore } from "@/stores/sketch";
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
}>();

const store = useSketchStore();

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
  dispose,
  animate,
} = useThreeScene();

let solidMeshes: THREE.Mesh[] = [];
let edgeLines: THREE.LineSegments[] = [];
let sketchPreviewLine: THREE.Line | null = null;
let sketchPreviewCircle: THREE.Line | null = null;

// ── Drawing state ───────────────────────────────────────────────
const state = createSketchState();
const { activeSnap, hoverSketch } = state;

const isSketchMode = computed(() => store.isEditingSketch());
const isDrawing = computed(() => isSketchMode.value && store.activeTool !== "select" && store.activeTool !== "plugin");
const isDrawingLine = computed(() => store.activeTool === "line" && state.lineStartId.value !== null);
const currentToolLabel = computed(() => toolLabel(store.activeTool));

const snappedAngle = computed(() => {
  if (!isDrawingLine.value) return null;
  const start = getPointCoords(state.lineStartId.value!);
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
  if (state.circleCenterId.value === null) return null;
  const center = getPointCoords(state.circleCenterId.value);
  if (!center) return null;
  const target = activeSnap.value?.world ?? hoverSketch.value;
  const r = Math.hypot(target.x - center.x, target.y - center.y);
  if (r < 0.001) return null;
  return r.toFixed(3);
});

const SOLID_COLORS = [0x4fc3f7, 0xff8a65, 0x69f0ae, 0xffd54f, 0xce93d8, 0x80cbc4, 0xf48fb1, 0xaed581, 0xfff176, 0x90caf9];

// ── Sketch plane helpers ────────────────────────────────────────

function getActivePlane() {
  // While editing a sketch, always use the sketch's stored plane — the
  // toolbar's activePlane dropdown is for the NEXT sketch only.
  if (isSketchMode.value) {
    const f = store.features.find(x => x.id === store.activeFeatureId);
    if (f?.plane && f.plane !== "offset") {
      return planeFrame(f.plane);
    }
  }
  return planeFrame(getActivePlaneName());
}

function getActivePlaneName(): string {
  if (isSketchMode.value) {
    const f = store.features.find(x => x.id === store.activeFeatureId);
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
}

async function refreshViewport() {
  const entries = await getAllSolidMeshes();
  if (!ctx.value) return;
  clearSolids();

  for (let i = 0; i < entries.length; i++) {
    const e = entries[i];
    const geo = buildGeometry(e.mesh);
    const color = SOLID_COLORS[i % SOLID_COLORS.length];
    const mat = new THREE.MeshStandardMaterial({
      color,
      roughness: 0.35,
      metalness: 0.05,
      side: THREE.DoubleSide,
      wireframe: wireframe.value,
    });
    const mesh = new THREE.Mesh(geo, mat);
    mesh.name = e.name;
    mesh.userData = { featureId: e.feature_id };
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
      if (state.lineStartId.value === null) {
        if (hit) state.lineStartId.value = hit;
        else {
          const id = await addPoint(world.x, world.y);
          if (id !== null) {
            state.lineStartId.value = id;
            await refreshSketch();
          }
        }
      } else {
        const start = getPointCoords(state.lineStartId.value);
        const endWorld = start ? snapAngle(start, world, true) : world;
        let endId = hit;
        if (!endId) {
          const newId = await addPoint(endWorld.x, endWorld.y);
          if (newId === null) return;
          endId = newId;
        }
        await addLine(state.lineStartId.value, endId);
        state.lineStartId.value = endId;
        await refreshSketch();
      }
      break;
    }
    case "circle": {
      if (state.circleCenterId.value === null) {
        const centerHit = findPointAt(store.entities, world.x, world.y);
        if (centerHit) {
          state.circleCenterId.value = centerHit;
        } else {
          const newId = await addPoint(world.x, world.y);
          if (newId === null) return;
          state.circleCenterId.value = newId;
          // Refresh so store.entities contains the new center — otherwise
          // getPointCoords() returns null on the next click and the circle
          // is silently discarded.
          await refreshSketch();
        }
        drawCirclePreview(world, 0.1);
      } else {
        const center = getPointCoords(state.circleCenterId.value);
        if (center) {
          const r = Math.hypot(world.x - center.x, world.y - center.y);
          // Reject degenerate circles (e.g., accidental double-click at the center).
          if (r > 0.01) {
            await addCircle(state.circleCenterId.value, r);
          }
        }
        state.circleCenterId.value = null;
        clearSketchPreviews();
        await refreshSketch();
      }
      break;
    }
    case "arc": {
      if (state.arcCenterId.value === null) {
        const centerHit = findPointAt(store.entities, world.x, world.y);
        let centerId = centerHit;
        if (!centerId) {
          const newId = await addPoint(world.x, world.y);
          if (newId === null) return;
          centerId = newId;
          // Refresh so store.entities contains the new center point.
          await refreshSketch();
        }
        state.arcCenterId.value = centerId;
      } else if (!state.arcRadiusSet.value) {
        state.arcRadiusPoint.value = { x: world.x, y: world.y };
        state.arcRadiusSet.value = true;
      } else {
        const center = getPointCoords(state.arcCenterId.value);
        if (!center) {
          state.arcCenterId.value = null;
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
        state.arcCenterId.value = null;
        state.arcRadiusPoint.value = null;
        state.arcRadiusSet.value = false;
        clearSketchPreviews();
        await refreshSketch();
      }
      break;
    }
    case "spline": {
      const hit = findPointAt(store.entities, world.x, world.y);
      if (hit) {
        state.splinePoints.value.push(hit);
      } else {
        const pid = await addPoint(world.x, world.y);
        if (pid !== null) state.splinePoints.value.push(pid);
        await refreshSketch();
      }
      break;
    }
    case "ellipse": {
      if (state.ellipseCenterId.value === null) {
        const hit = findPointAt(store.entities, world.x, world.y);
        if (hit) state.ellipseCenterId.value = hit;
        else {
          const pid = await addPoint(world.x, world.y);
          if (pid !== null) {
            state.ellipseCenterId.value = pid;
            await refreshSketch();
          }
        }
      } else {
        const hit = findPointAt(store.entities, world.x, world.y);
        let endId = hit;
        if (!endId) {
          const pid = await addPoint(world.x, world.y);
          if (pid === null) return;
          endId = pid;
        }
        await addEllipse(state.ellipseCenterId.value, endId, 0.5);
        state.ellipseCenterId.value = null;
        await refreshSketch();
      }
      break;
    }
    case "rectangle": {
      if (state.rectStart.value === null) {
        state.rectStart.value = { ...world };
      } else {
        const x1 = state.rectStart.value.x, y1 = state.rectStart.value.y;
        const x2 = world.x, y2 = world.y;
        const p1 = await addPoint(x1, y1), p2 = await addPoint(x2, y1);
        const p3 = await addPoint(x2, y2), p4 = await addPoint(x1, y2);
        if (p1 && p2 && p3 && p4) {
          await addLine(p1, p2);
          await addLine(p2, p3);
          await addLine(p3, p4);
          await addLine(p4, p1);
        }
        state.rectStart.value = null;
        await refreshSketch();
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

  if (isSketchMode.value && store.activeTool === "select" && state.isDragging.value && state.dragId.value) {
    const sketch = getSketchPoint(e);
    if (sketch) {
      const target = activeSnap.value?.world ?? sketch;
      await movePoint(state.dragId.value, target.x, target.y);
      await solveSketch();
      await refreshSketch();
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
    const target = activeSnap.value?.world ?? sketch;
    await movePoint(state.dragId.value, target.x, target.y);
    await solveSketch();
    await refreshSketch();
    return;
  }

  clearSketchPreviews();
  const previewWorld = activeSnap.value?.world ?? sketch;
  const planeName = getActivePlaneName();
  if (store.activeTool === "line" && state.lineStartId.value !== null) {
    const start = getPointCoords(state.lineStartId.value);
    if (start) {
      const snapped = snapAngle(start, previewWorld, true);
      drawLinePreview(start, snapped);
    }
  }
  if (store.activeTool === "circle" && state.circleCenterId.value !== null) {
    const center = getPointCoords(state.circleCenterId.value);
    if (center) {
      const r = Math.hypot(previewWorld.x - center.x, previewWorld.y - center.y);
      drawCirclePreview(center, r);
    }
  }
  if (store.activeTool === "arc" && state.arcCenterId.value !== null) {
    const center = getPointCoords(state.arcCenterId.value);
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
  state.isDragging.value = false;
  state.dragId.value = null;
}

function onMouseLeave() {
  state.isHovering.value = false;
  activeSnap.value = null;
  clearSketchPreviews();
}

async function onRightClick() {
  if (state.splinePoints.value.length >= 2) {
    await addSpline([...state.splinePoints.value]);
    state.splinePoints.value = [];
    await refreshSketch();
    clearSketchPreviews();
    return;
  }
  if (state.lineStartId.value !== null) {
    state.lineStartId.value = null;
    clearSketchPreviews();
  } else if (state.circleCenterId.value !== null) {
    state.circleCenterId.value = null;
    clearSketchPreviews();
  } else if (state.rectStart.value !== null) {
    state.rectStart.value = null;
    clearSketchPreviews();
  } else if (state.arcCenterId.value !== null) {
    state.arcCenterId.value = null;
    state.arcRadiusPoint.value = null;
    state.arcRadiusSet.value = false;
    clearSketchPreviews();
  }
}

async function onDoubleClick() {
  if (state.splinePoints.value.length >= 2) {
    await addSpline([...state.splinePoints.value]);
    state.splinePoints.value = [];
    await refreshSketch();
    clearSketchPreviews();
    return;
  }
  if (store.activeTool === "line" && state.lineStartId.value !== null) {
    state.lineStartId.value = null;
    clearSketchPreviews();
  }
}

function onWheel(_e: WheelEvent) {
  // OrbitControls handle zoom.
}

// ── Helpers ────────────────────────────────────────────────────

function getPointCoords(id: EntityId): { x: number; y: number } | null {
  const entity = store.entities.find(e => e.id === id);
  if (entity?.type === "Point") return { x: entity.x, y: entity.y };
  return null;
}

// ── Public API ─────────────────────────────────────────────────

const containerWidth = ref(0);
const containerHeight = ref(0);

async function refreshSketch() {
  store.setEntities(await getSketchEntities());
  store.setConstraints(await getSketchConstraints());
  refreshSketchEntities();
}

async function onDimensionRefresh() {
  await refreshSketch();
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

defineExpose({ refreshViewport, refreshSketch, showPreviewMesh, clearPreviewMesh, setView });

// ── Watchers ───────────────────────────────────────────────────

watch(isSketchMode, (editing) => {
  if (!ctx.value) return;
  // Only show sketch entities while editing; hide them in 3D view like SolidWorks.
  ctx.value.sketchGroup.visible = editing;
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
  resetDrawingState(state);
  clearSketchPreviews();
});

watch(() => store.entities, () => {
  refreshSketchEntities();
}, { deep: true });

onMounted(() => {
  initScene();
  refreshViewport();
  refreshSketchEntities();
  animate();

  // Track container size so the dimension overlay can render at the right resolution.
  if (containerRef.value) {
    const updateSize = () => {
      if (!containerRef.value) return;
      containerWidth.value = containerRef.value.clientWidth;
      containerHeight.value = containerRef.value.clientHeight;
    };
    updateSize();
    const ro = new ResizeObserver(updateSize);
    ro.observe(containerRef.value);
    onUnmounted(() => ro.disconnect());
  }
});

onUnmounted(() => {
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
