<template>
  <div class="main-layout" @keydown="onKeyDown" tabindex="0" ref="layoutRoot">
    <Toolbar
      ref="toolbarRef"
      :recent-files="recentFiles"
      @file-action="onFileAction"
      @open-recent="openRecent"
      @clear-recent="clearRecent"
      @undo="undoAction"
      @redo="redoAction"
      @new-sketch="newSketch"
      @clear-doc="clearDoc"
      @feature="onFeatureAction"
      @pattern="onPatternAction"
      @constraint="onConstraintAction"
      @exit-sketch="exitSketchEdit"
      @open-plugin="openPluginDialog"
      @offset-plane="openOffsetDialog"
      @clear-sketch="clearSketchAction"
    />

    <div class="workspace">
      <!-- Left sidebar: Feature tree + plugins -->
      <aside class="sidebar-left">
        <FeatureTree
          @select="selectFeature"
          @edit="editFeature"
          @contextmenu="onFeatureContextMenu"
          @toggle-suppress="toggleSuppress"
          @delete="deleteFeature"
        />

        <!-- Plugin panel -->
        <div class="panel" v-if="sketchStore.plugins.length > 0">
          <h3>插件</h3>
          <div v-for="plugin in sketchStore.plugins" :key="plugin.id" class="plugin-item">
            <div class="plugin-header">
              <span class="plugin-name">{{ plugin.name }}</span>
              <span class="plugin-version">v{{ plugin.version }}</span>
            </div>
            <div class="plugin-desc">{{ plugin.description }}</div>
            <div v-for="gen in plugin.generators" :key="gen.id" class="plugin-generator" @click="openPluginDialog(gen)">
              + {{ gen.name }}
            </div>
          </div>
        </div>
      </aside>

      <!-- Main viewport -->
      <main class="viewport">
        <UnifiedViewport ref="unifiedViewport" @faceSelected="onFaceSelected" @drag-end="onDragEnd" />
      </main>

      <!-- Right sidebar: Properties -->
      <aside class="sidebar-right">
        <PropertiesPanel
          @rename="renameSelectedFeature"
          @update-param="updateParam"
          @update-entity-prop="updateEntityProp"
          @toggle-construction="toggleConstruction"
          @delete-entity="deleteSelectedEntity"
          @delete-constraint="deleteConstraint"
          @clear-constraints="clearAllConstraints"
          @extrude-change="onExtrudeConfigChange"
          @extrude-confirm="doExtrude"
          @extrude-cancel="cancelExtrude"
        />
      </aside>
    </div>

    <footer class="statusbar">
      <span class="status-text">{{ statusText }}</span>
    </footer>

    <ToastContainer />

    <PluginDialog
      :generator="activeDialog"
      :loading="dialogLoading"
      @run="runGenerator"
      @cancel="closeDialog"
    />

    <CascadeDeleteDialog
      :target="cascadeDialog"
      @confirm="confirmCascadeDelete"
      @cancel="cancelCascade"
    />

    <BooleanDialog
      :targets="booleanDialog"
      @confirm="confirmBoolean"
      @cancel="cancelBoolean"
    />

    <OffsetPlaneDialog
      :open="offsetDialogOpen"
      @confirm="confirmOffsetPlane"
      @cancel="cancelOffset"
    />

    <!-- Feature-tree right-click menu (position: fixed, so DOM location is irrelevant) -->
    <ContextMenu
      :target="ctxMenu"
      :suppressed="ctxFeatureSuppressed"
      @edit-sketch="ctxEditSketch"
      @face-normal="ctxFaceNormal"
      @rename="ctxRename"
      @toggle-suppress="ctxToggleSuppress"
      @delete="ctxDelete"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, ref, nextTick, onMounted, onUnmounted } from "vue";
import UnifiedViewport from "@/components/UnifiedViewport.vue";
import Toolbar, {
  type FileAction,
  type FeatureKind,
  type PatternKind,
  type ConstraintKind,
} from "@/components/Toolbar.vue";
import FeatureTree from "@/components/FeatureTree.vue";
import ContextMenu, { type CtxTarget } from "@/components/ContextMenu.vue";
import PropertiesPanel from "@/components/PropertiesPanel.vue";
import ToastContainer from "@/components/ToastContainer.vue";
import PluginDialog from "@/components/PluginDialog.vue";
import CascadeDeleteDialog, { type CascadeTarget } from "@/components/CascadeDeleteDialog.vue";
import BooleanDialog, { type BooleanTargets, type BooleanResult } from "@/components/BooleanDialog.vue";
import OffsetPlaneDialog, { type OffsetPlaneResult } from "@/components/OffsetPlaneDialog.vue";
import { useSketchStore } from "@/stores/sketch";
import { useToastStore } from "@/stores/toast";

import {
  addConstraint, solveSketch,
  updateEntityProp as updateEntityPropCmd,
  deleteEntity as deleteEntityCmd,
  removeConstraint,
  getSketchEntities, clearDocument, clearSketch,
  getFeatures, setActiveSketch,
  addSketchFeature, deleteFeature as deleteFeatureCmd,
  renameFeature as renameFeatureCmd,
  setFeatureSuppressed as setFeatureSuppressedCmd,
  updateParameter, addExtrudeFeature,
  addRevolveFeature,
  addFilletEdgesFeature, addChamferEdgesFeature,
  createOffsetPlane,
  addLinearPattern as addLinearPatternCmd,
  addCircularPattern as addCircularPatternCmd,
  addMirrorFeature, addSweepFeature, addShellFeature,
  addBooleanFeature as addBooleanFeatureCmd,
  previewExtrude,
  saveProject, loadProject, loadProjectFrom, exportStl, exportObj, exportGltf,
  checkRecovery,
  getRecentFiles, clearRecentFiles,
  undo, redo, listPlugins, listGenerators,
  generatePluginFeature,
  type EntityId, type FeatureId, type FeatureNode,
  type ParameterId, type GeneratorInfo,
} from "@/commands/sketch";

const sketchStore = useSketchStore();
const toastStore = useToastStore();
const unifiedViewport = ref<InstanceType<typeof UnifiedViewport> | null>(null);
const toolbarRef = ref<InstanceType<typeof Toolbar> | null>(null);
const layoutRoot = ref<HTMLDivElement | null>(null);

const isEditingSketch = computed(() => sketchStore.isEditingSketch());

const statusText = computed(() => {
  if (sketchStore.showExtrudePanel) return "拉伸预览 — 在右侧面板调整参数";
  if (isEditingSketch.value) {
    const toolLabels: Record<string, string> = {
      select: "就绪",
      line: "点击放置直线端点",
      circle: "点击放置圆心",
      arc: "点击放置弧线中心",
      rectangle: "点击放置矩形一角",
      spline: "点击放置控制点",
      ellipse: "点击放置椭圆中心",
      plugin: "插件工具",
    };
    return `${toolLabels[sketchStore.activeTool] || sketchStore.activeTool} · ${sketchStore.entities.length} 个实体`;
  }
  return "3D 视口 — 选择特征进行编辑";
});

function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

const recentFiles = ref<string[]>([]);

async function refreshRecent() {
  try {
    recentFiles.value = await getRecentFiles();
  } catch {
    recentFiles.value = [];
  }
}

async function openRecent(path: string) {
  try {
    await loadProjectFrom(path);
    sketchStore.selectedFeatureId = null;
    const features = await getFeatures();
    const firstSketch = features.find(f => f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"));
    if (firstSketch) sketchStore.setActiveFeature(firstSketch.id);
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    toastStore.success(`已加载: ${basename(path)}`);
  } catch (err) {
    toastStore.error(`加载失败: ${err}`);
  }
}

async function clearRecent() {
  await clearRecentFiles();
  recentFiles.value = [];
}

// ── Data loading ─────────────────────────────────────────────────

async function refreshFeatures() {
  sketchStore.setFeatures(await getFeatures());
}

async function refreshSketch() {
  sketchStore.setEntities(await getSketchEntities());
  unifiedViewport.value?.refreshSketch();
}

async function refreshPlugins() {
  const [plugins, generators] = await Promise.all([listPlugins(), listGenerators()]);
  sketchStore.setPlugins(plugins);
  sketchStore.setGenerators(generators);
}

async function loadState() {
  // `isLoading` is wired here so a future spinner can react to document loads.
  // It was previously declared but never set.
  sketchStore.isLoading = true;
  try {
    await refreshFeatures();
    await refreshSketch();
  } finally {
    sketchStore.isLoading = false;
  }
}

// ── Feature tree ─────────────────────────────────────────────────

/**
 * Left-click selection: highlight in the tree and show properties.
 * Does NOT enter sketch edit mode — that's only via double-click or the
 * right-click "编辑草图" menu (SolidWorks behavior).
 */
async function selectFeature(id: FeatureId) {
  sketchStore.selectedFeatureId = id;
  sketchStore.setActiveFeature(id);
  // If we're editing a different sketch, switching selection exits edit mode.
  if (sketchStore.editingSketchId !== null && sketchStore.editingSketchId !== id) {
    await exitSketchEdit();
  }
  const f = sketchStore.features.find(x => x.id === id);
  // Sync the working plane for display (face view) even when not editing.
  if (f?.plane && f.plane !== "offset") {
    sketchStore.activePlane = f.plane;
  }
}

/**
 * Enter sketch edit mode for the given sketch feature. Shows sketch
 * entities in the viewport, enables drawing tools, and faces the plane.
 */
async function enterSketchEdit(id: FeatureId) {
  const f = sketchStore.features.find(x => x.id === id);
  if (!f || !(f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"))) return;

  sketchStore.selectedFeatureId = id;
  sketchStore.setActiveFeature(id);
  sketchStore.setEditingSketch(id);
  sketchStore.setTool("select");

  if (f.plane && f.plane !== "offset") {
    sketchStore.activePlane = f.plane;
  }
  if (sketchStore.showExtrudePanel) {
    sketchStore.showExtrudePanel = false;
    unifiedViewport.value?.clearPreviewMesh();
  }
  await setActiveSketch(id);
  await refreshSketch();
}

/** Double-click on a sketch enters edit mode; on other features, just selects. */
function editFeature(feature: FeatureNode) {
  if (feature.feature_type === "Sketch" || feature.feature_type.startsWith("Custom:")) {
    enterSketchEdit(feature.id);
  } else {
    selectFeature(feature.id);
  }
}

function onFaceSelected(featureId: number, _faceIndex: number) {
  sketchStore.selectedFeatureId = featureId;
}

function onDragEnd() {
  unifiedViewport.value?.refreshViewport();
}

async function toggleSuppress(feature: FeatureNode) {
  await setFeatureSuppressedCmd(feature.id, !feature.suppressed);
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

async function renameSelectedFeature(name: string) {
  const sel = sketchStore.selectedFeature;
  if (!sel) return;
  if (!name || name === sel.name) return;
  await renameFeatureCmd(sel.id, name);
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

// ── Feature operations ───────────────────────────────────────────

async function updateParam(id: ParameterId, event: Event) {
  // Skip virtual parameters (id=0)
  if (id === 0) return;
  const value = parseFloat((event.target as HTMLInputElement).value);
  if (Number.isNaN(value)) return;
  await updateParameter(id, value);
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

async function newSketch() {
  const id = await addSketchFeature(sketchStore.activePlane);
  await refreshFeatures();
  // Creating a new sketch enters edit mode immediately (SolidWorks behavior).
  await enterSketchEdit(id);
}

async function deleteFeature(id: FeatureId) {
  const feature = sketchStore.features.find(f => f.id === id);
  if (!feature) return;

  // Try non-cascade first; the backend returns Err("dependents:name1,name2")
  // when there are dependents.
  try {
    await deleteFeatureCmd(id, false);
    if (sketchStore.selectedFeature?.id === id) sketchStore.selectedFeatureId = null;
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    toastStore.success(`已删除 "${feature.name}"`);
  } catch (err: any) {
    // Parse dependent names from the error message
    const msg = String(err);
    const dependentNames = msg.startsWith("dependents:")
      ? msg.slice("dependents:".length).split(",").filter(Boolean)
      : [];
    cascadeDialog.value = { id, name: feature.name, dependents: dependentNames };
  }
}

const cascadeDialog = ref<CascadeTarget | null>(null);

async function confirmCascadeDelete() {
  if (!cascadeDialog.value) return;
  const { id, name } = cascadeDialog.value;
  cascadeDialog.value = null;
  try {
    await deleteFeatureCmd(id, true);
    if (sketchStore.selectedFeature?.id === id) sketchStore.selectedFeatureId = null;
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    toastStore.success(`已级联删除 "${name}" 及其依赖项`);
  } catch (err: any) {
    toastStore.error(`删除失败: ${err}`);
  }
}

function cancelCascade() {
  cascadeDialog.value = null;
}

async function clearDoc() {
  await clearDocument();
  sketchStore.selectedFeatureId = null;
  const features = await getFeatures();
  const firstSketch = features.find(f => f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"));
  if (firstSketch) sketchStore.setActiveFeature(firstSketch.id);
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

async function clearSketchAction() {
  if (sketchStore.editingSketchId === null) return;
  await clearSketch();
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

// ── Extrude ──────────────────────────────────────────────────────

let extrudePreviewTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * The sketch to use for feature operations (extrude / revolve / sweep source).
 * Prefers the sketch being edited; falls back to the selected feature if
 * it's a sketch. Returns null when nothing usable is selected.
 */
function activeSketchForFeature(): FeatureId | null {
  if (sketchStore.editingSketchId !== null) return sketchStore.editingSketchId;
  const f = sketchStore.features.find(x => x.id === sketchStore.activeFeatureId);
  if (f && (f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"))) {
    return f.id;
  }
  return null;
}

async function showExtrudePanel() {
  if (activeSketchForFeature() === null) {
    toastStore.info("请先选择一个草图");
    return;
  }
  sketchStore.showExtrudePanel = true;
  updateExtrudePreview(sketchStore.extrudeConfig);
}

async function onExtrudeConfigChange(config: { direction: string; depth: number; dist2: number; draft: number }) {
  sketchStore.extrudeConfig = { ...config };
  if (extrudePreviewTimer) clearTimeout(extrudePreviewTimer);
  extrudePreviewTimer = setTimeout(() => updateExtrudePreview(config), 120);
}

async function updateExtrudePreview(cfg: { direction: string; depth: number; dist2: number; draft: number }) {
  const activeId = activeSketchForFeature();
  if (activeId === null) return;
  const mesh = await previewExtrude(activeId, cfg.direction, cfg.dist2, cfg.draft, cfg.depth);
  if (mesh) {
    unifiedViewport.value?.showPreviewMesh(mesh);
  }
}

function cancelExtrude() {
  if (extrudePreviewTimer) clearTimeout(extrudePreviewTimer);
  sketchStore.showExtrudePanel = false;
  unifiedViewport.value?.clearPreviewMesh();
}

async function doExtrude() {
  const activeId = activeSketchForFeature();
  if (activeId === null) return;
  const cfg = sketchStore.extrudeConfig;
  if (extrudePreviewTimer) clearTimeout(extrudePreviewTimer);
  sketchStore.showExtrudePanel = false;
  unifiedViewport.value?.clearPreviewMesh();
  try {
    await addExtrudeFeature(activeId, cfg.direction, cfg.dist2, cfg.draft, cfg.depth);
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    toastStore.success("拉伸特征已创建");
  } catch (err) {
    toastStore.error(`拉伸失败: ${err}`);
  }
}

// ── Other features ───────────────────────────────────────────────

/// Wrap a feature-creation call: shows toast on error, refreshes state on success.
async function withFeatureAction<T>(label: string, fn: () => Promise<T>): Promise<T | null> {
  try {
    const result = await fn();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    toastStore.success(`${label}已创建`);
    return result;
  } catch (err) {
    toastStore.error(`${label}失败: ${err}`);
    return null;
  }
}

async function revolve() {
  const activeId = activeSketchForFeature();
  if (activeId === null) {
    toastStore.info("请先选择一个草图");
    return;
  }
  const axisId = sketchStore.selectedId ?? null;
  await withFeatureAction("旋转特征", () => addRevolveFeature(activeId, axisId));
}

async function addFillet() {
  const tid = selectedSolidFeatureId();
  if (tid === null) {
    toastStore.info("请先点击一个实体面（或选中实体特征）");
    return;
  }
  // Create with all sharp edges (edges=[]); the radius is tunable live from
  // the PropertiesPanel param scrubber after creation.
  const id = await withFeatureAction("圆角特征", () => addFilletEdgesFeature(tid, 0.5, []));
  if (id !== null) sketchStore.selectedFeatureId = id;
}

async function addChamfer() {
  const tid = selectedSolidFeatureId();
  if (tid === null) {
    toastStore.info("请先点击一个实体面（或选中实体特征）");
    return;
  }
  const id = await withFeatureAction("倒角特征", () => addChamferEdgesFeature(tid, 0.5, []));
  if (id !== null) sketchStore.selectedFeatureId = id;
}

/// True for any feature that produces a solid body and therefore makes a
/// valid fillet/chamfer/shell/boolean target. Includes pattern/mirror/boolean
/// results and plugin-generated solids.
function isSolidFeature(f: FeatureNode): boolean {
  return !f.suppressed && (
    ["Extrude", "Revolve", "Fillet", "Chamfer", "LinearPattern", "CircularPattern",
      "Mirror", "Sweep", "Shell", "Boolean"].includes(f.feature_type) ||
    f.feature_type.startsWith("CustomSolid")
  );
}

/// The currently-selected solid, if any. Face picks in the viewport set
/// `selectedFeatureId` to the clicked body, so this naturally reflects a
/// face click as well as a feature-tree click.
function selectedSolidFeatureId(): FeatureId | null {
  const sel = sketchStore.selectedFeatureId;
  if (sel === null) return null;
  const f = sketchStore.features.find(x => x.id === sel);
  return f && isSolidFeature(f) ? f.id : null;
}

function lastSolidFeatureId(): FeatureId | null {
  const solids = sketchStore.features.filter(isSolidFeature);
  return solids.length > 0 ? solids[solids.length - 1].id : null;
}

async function addLinearPattern() {
  const tid = lastSolidFeatureId();
  if (tid === null) {
    toastStore.info("请先创建一个实体");
    return;
  }
  await withFeatureAction("线性阵列", () => addLinearPatternCmd(tid, 1.0, 0.0, 0.0, 3, 2.0));
}

async function addCircularPattern() {
  const tid = lastSolidFeatureId();
  if (tid === null) {
    toastStore.info("请先创建一个实体");
    return;
  }
  await withFeatureAction("圆周阵列", () => addCircularPatternCmd(tid, 0, 0, 0, 0, 0, 1, 4, 360));
}

async function addMirror() {
  const tid = lastSolidFeatureId();
  if (tid === null) {
    toastStore.info("请先创建一个实体");
    return;
  }
  await withFeatureAction("镜像特征", () => addMirrorFeature(tid, 1, 0, 0, 0, 0, 0));
}

async function addSweep() {
  const activeId = activeSketchForFeature();
  if (activeId === null) {
    toastStore.info("请先选择一个草图");
    return;
  }
  const otherSketches = sketchStore.features.filter(f => f.feature_type === "Sketch" && f.id !== activeId);
  if (otherSketches.length === 0) {
    toastStore.info("需要两个草图：轮廓 + 路径");
    return;
  }
  await withFeatureAction("扫描特征", () => addSweepFeature(activeId, otherSketches[0].id));
}

async function addShell() {
  const tid = selectedSolidFeatureId();
  if (tid === null) {
    toastStore.info("请先点击一个实体面（或选中实体特征）");
    return;
  }
  const id = await withFeatureAction("抽壳特征", () => addShellFeature(tid, 0.5));
  if (id !== null) sketchStore.selectedFeatureId = id;
}

const booleanDialog = ref<BooleanTargets | null>(null);

/// Open the boolean-op dialog. Target A defaults to the selected solid
/// (so a face pick drives subtract/intersect order predictably); target B
/// is the most recent *other* solid. If nothing is selected, A/B fall back
/// to the two most recent solids in creation order. The dialog lets the
/// user pick union/subtract/intersect and swap A↔B before confirming.
async function addBoolean() {
  const solids = sketchStore.features.filter(isSolidFeature);
  if (solids.length < 2) {
    toastStore.info("布尔运算需要两个实体");
    return;
  }
  const sel = sketchStore.selectedFeatureId;
  let a: FeatureNode;
  let b: FeatureNode;
  const selFeature = sel !== null ? solids.find(f => f.id === sel) : undefined;
  if (selFeature) {
    a = selFeature;
    const others = solids.filter(f => f.id !== sel);
    b = others[others.length - 1] ?? solids[solids.length - 1];
  } else {
    a = solids[solids.length - 1];
    b = solids[solids.length - 2];
  }
  booleanDialog.value = { idA: a.id, nameA: a.name, idB: b.id, nameB: b.name };
}

async function confirmBoolean(result: BooleanResult) {
  const { op, idA, idB } = result;
  booleanDialog.value = null;
  const id = await withFeatureAction("布尔特征", () => addBooleanFeatureCmd(idA, idB, op));
  if (id !== null) sketchStore.selectedFeatureId = id;
}

function cancelBoolean() {
  booleanDialog.value = null;
}

// ── Offset plane ─────────────────────────────────────────────────

const offsetDialogOpen = ref(false);
function openOffsetDialog() {
  offsetDialogOpen.value = true;
}
function cancelOffset() {
  offsetDialogOpen.value = false;
}

async function confirmOffsetPlane(result: OffsetPlaneResult) {
  offsetDialogOpen.value = false;
  try {
    const id = await createOffsetPlane(result.basePlane, result.distance);
    await refreshFeatures();
    // Select + enter edit on the new sketch via the standard sketch-creation
    // flow (also sets selectedFeatureId, activeFeatureId, editingSketchId).
    await enterSketchEdit(id);
    await unifiedViewport.value?.refreshViewport();
    toastStore.success("偏移平面已创建");
  } catch (err) {
    toastStore.error(`偏移平面创建失败: ${err}`);
  }
}

// ── Toolbar action dispatchers ───────────────────────────────────

function onFileAction(action: FileAction) {
  switch (action) {
    case "new": clearDoc(); break;
    case "open": loadProjectFile(); break;
    case "save":
    case "save-as": saveProjectFile(); break;
    case "export-stl": exportStlFile(); break;
    case "export-obj": exportObjFile(); break;
    case "export-gltf": exportGltfFile(); break;
  }
}

function onFeatureAction(kind: FeatureKind) {
  switch (kind) {
    case "extrude": showExtrudePanel(); break;
    case "revolve": revolve(); break;
    case "sweep": addSweep(); break;
    case "fillet": addFillet(); break;
    case "chamfer": addChamfer(); break;
    case "shell": addShell(); break;
    case "boolean": addBoolean(); break;
  }
}

function onPatternAction(kind: PatternKind) {
  switch (kind) {
    case "linear": addLinearPattern(); break;
    case "circular": addCircularPattern(); break;
    case "mirror": addMirror(); break;
  }
}

function onConstraintAction(kind: ConstraintKind) {
  switch (kind) {
    case "horizontal": addHorizontal(); break;
    case "vertical": addVertical(); break;
    case "parallel": addParallel(); break;
    case "perpendicular": addPerpendicular(); break;
    case "tangent": addTangent(); break;
    case "concentric": addConcentric(); break;
    case "equal": addEqual(); break;
    case "midpoint": addMidpoint(); break;
    case "fix": addFixPoint(); break;
    case "angle": addAngle(); break;
    case "diameter": addDiameter(); break;
    case "distance": addDistance(); break;
    case "solve": solve(); break;
  }
}

// ── File operations ──────────────────────────────────────────────

async function saveProjectFile() {
  try {
    const path = await saveProject();
    toastStore.success(`已保存: ${basename(path)}`);
    await refreshRecent();
  } catch (err) {
    if (String(err).includes("No file selected")) return;
    toastStore.error(`保存失败: ${err}`);
  }
}

async function loadProjectFile() {
  try {
    await loadProject();
    sketchStore.selectedFeatureId = null;
    const features = await getFeatures();
    const firstSketch = features.find(f => f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"));
    if (firstSketch) sketchStore.setActiveFeature(firstSketch.id);
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    await refreshRecent();
    toastStore.success("项目已加载");
  } catch (err) {
    // Don't error if user just cancelled the file picker
    if (String(err).includes("No file selected")) return;
    toastStore.error(`加载失败: ${err}`);
  }
}

async function exportStlFile() {
  try {
    const path = await exportStl();
    toastStore.success(`已导出 STL: ${path}`);
  } catch (err) {
    if (String(err).includes("No file selected")) return;
    toastStore.error(`STL 导出失败: ${err}`);
  }
}

async function exportObjFile() {
  try {
    const path = await exportObj();
    toastStore.success(`已导出 OBJ: ${path}`);
  } catch (err) {
    if (String(err).includes("No file selected")) return;
    toastStore.error(`OBJ 导出失败: ${err}`);
  }
}

async function exportGltfFile() {
  try {
    const path = await exportGltf();
    toastStore.success(`已导出 glTF: ${path}`);
  } catch (err) {
    if (String(err).includes("No file selected")) return;
    toastStore.error(`glTF 导出失败: ${err}`);
  }
}

async function undoAction() {
  if (await undo()) {
    sketchStore.selectedFeatureId = null;
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  }
}

async function redoAction() {
  if (await redo()) {
    sketchStore.selectedFeatureId = null;
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  }
}

// ── Constraint helpers ───────────────────────────────────────────

function pickTwoPoints(): [EntityId, EntityId] | null {
  const points = sketchStore.entities.filter(e => e.type === "Point");
  if (points.length < 2) return null;
  const sel = sketchStore.selectedId;
  if (sel !== null && points.some(p => p.id === sel)) {
    const other = points.find(p => p.id !== sel);
    if (other) return [sel, other.id];
  }
  return [points[0].id, points[1].id];
}

function pickTwoLines(): [EntityId, EntityId] | null {
  const lines = sketchStore.entities.filter(e => e.type === "Line");
  if (lines.length < 2) return null;
  const sel = sketchStore.selectedId;
  if (sel !== null && lines.some(l => l.id === sel)) {
    const other = lines.find(l => l.id !== sel);
    if (other) return [sel, other.id];
  }
  return [lines[0].id, lines[1].id];
}

function pickLineAndCircle(): [EntityId, EntityId] | null {
  const lines = sketchStore.entities.filter(e => e.type === "Line");
  const circles = sketchStore.entities.filter(e => e.type === "Circle" || e.type === "Arc");
  if (lines.length === 0 || circles.length === 0) return null;
  const sel = sketchStore.selectedId;
  if (sel !== null && lines.some(l => l.id === sel)) return [sel, circles[0].id];
  if (sel !== null && circles.some(c => c.id === sel)) return [lines[0].id, sel];
  return [lines[0].id, circles[0].id];
}

function pickTwoCircles(): [EntityId, EntityId] | null {
  const circles = sketchStore.entities.filter(e => e.type === "Circle" || e.type === "Arc");
  if (circles.length < 2) return null;
  const sel = sketchStore.selectedId;
  if (sel !== null && circles.some(c => c.id === sel)) {
    const other = circles.find(c => c.id !== sel);
    if (other) return [sel, other.id];
  }
  return [circles[0].id, circles[1].id];
}

function pickPointAndLine(): [EntityId, EntityId] | null {
  const points = sketchStore.entities.filter(e => e.type === "Point");
  const lines = sketchStore.entities.filter(e => e.type === "Line");
  if (points.length === 0 || lines.length === 0) return null;
  const sel = sketchStore.selectedId;
  if (sel !== null && points.some(p => p.id === sel)) return [sel, lines[0].id];
  if (sel !== null && lines.some(l => l.id === sel)) return [points[0].id, sel];
  return [points[0].id, lines[0].id];
}

async function addHorizontal() {
  const id = sketchStore.selectedId;
  if (id === null) {
    toastStore.info("请先选择一条线");
    return;
  }
  try {
    await addConstraint({ Horizontal: { line: id } });
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`水平约束添加失败: ${err}`);
  }
}

async function addVertical() {
  const id = sketchStore.selectedId;
  if (id === null) {
    toastStore.info("请先选择一条线");
    return;
  }
  try {
    await addConstraint({ Vertical: { line: id } });
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`垂直约束添加失败: ${err}`);
  }
}

async function addParallel() {
  const ids = pickTwoLines();
  if (!ids) { toastStore.info("需要两条线"); return; }
  try {
    await addConstraint({ Parallel: { line_a: ids[0], line_b: ids[1] } });
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`平行约束添加失败: ${err}`);
  }
}

async function addPerpendicular() {
  const ids = pickTwoLines();
  if (!ids) { toastStore.info("需要两条线"); return; }
  try {
    await addConstraint({ Perpendicular: { line_a: ids[0], line_b: ids[1] } });
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`正交约束添加失败: ${err}`);
  }
}

async function addTangent() {
  const ids = pickLineAndCircle();
  if (!ids) { toastStore.info("需要一条线和一个圆/弧"); return; }
  try {
    await addConstraint({ Tangent: { line: ids[0], circle: ids[1] } });
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`相切约束添加失败: ${err}`);
  }
}

async function addConcentric() {
  const ids = pickTwoCircles();
  if (!ids) { toastStore.info("需要两个圆/弧"); return; }
  try {
    await addConstraint({ Concentric: { a: ids[0], b: ids[1] } });
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`同心约束添加失败: ${err}`);
  }
}

async function addEqual() {
  const ids = pickTwoLines() ?? pickTwoCircles();
  if (!ids) { toastStore.info("需要两个同类实体"); return; }
  try {
    await addConstraint({ Equal: { a: ids[0], b: ids[1] } });
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`等长约束添加失败: ${err}`);
  }
}

async function addMidpoint() {
  const ids = pickPointAndLine();
  if (!ids) { toastStore.info("需要一个点 + 一条线"); return; }
  try {
    await addConstraint({ Midpoint: { point: ids[0], line: ids[1] } });
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`中点约束添加失败: ${err}`);
  }
}

async function addFixPoint() {
  const id = sketchStore.selectedId;
  if (id === null) {
    toastStore.info("请先选择一个点");
    return;
  }
  try {
    await addConstraint({ Fix: { point: id } });
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`固定约束添加失败: ${err}`);
  }
}

async function addAngle() {
  const ids = pickTwoLines();
  if (!ids) { toastStore.info("需要两条线"); return; }
  try {
    await addConstraint({ Angle: { line_a: ids[0], line_b: ids[1], angle_deg: 90.0 } });
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`角度约束添加失败: ${err}`);
  }
}

async function addDiameter() {
  const circles = sketchStore.entities.filter(e => e.type === "Circle" || e.type === "Arc");
  if (circles.length === 0) { toastStore.info("需要圆/弧"); return; }
  const sel = sketchStore.selectedId;
  const target = (sel !== null && circles.some(c => c.id === sel))
    ? circles.find(c => c.id === sel)!
    : circles[0];
  const r = target.type === "Circle" ? target.radius : (target as any).radius;
  try {
    await addConstraint({ Diameter: { circle: target.id, diameter: r * 2 } });
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`直径约束添加失败: ${err}`);
  }
}

async function addDistance() {
  const ids = pickTwoPoints();
  if (!ids) { toastStore.info("需要两个点"); return; }
  try {
    await addConstraint({ Distance: { a: ids[0], b: ids[1], distance: 2.0 } });
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`距离约束添加失败: ${err}`);
  }
}

async function solve() {
  await solveSketch();
  await refreshSketch();
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

/// Delete a single constraint by its index in store.constraints. The
/// `removeConstraint` Tauri command takes an index (not an id) — the
/// PropertiesPanel passes the index from its v-for, which matches the
/// backend's current constraint ordering. After the deletion we re-sync
/// the store (read-your-write, design-principle #6.1) so the list and
/// the dimension/glyph overlay both update.
async function deleteConstraint(index: number) {
  try {
    await removeConstraint(index);
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`约束删除失败: ${err}`);
  }
}

/// Clear every constraint on the active sketch. Repeatedly remove index 0
/// (each removal shifts later indices down) until none remain. A single
/// refreshSketch at the end is enough since the backend mutation is
/// synchronous w.r.t. our await chain.
async function clearAllConstraints() {
  const n = sketchStore.constraints.length;
  try {
    for (let i = 0; i < n; i++) {
      await removeConstraint(0);
    }
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  } catch (err) {
    toastStore.error(`约束清除失败: ${err}`);
  }
}

// ── Entity editing ───────────────────────────────────────────────

async function updateEntityProp(id: EntityId, prop: string, event: Event) {
  const value = parseFloat((event.target as HTMLInputElement).value);
  if (Number.isNaN(value)) return;
  await updateEntityPropCmd(id, prop, value);
  await solveSketch();
  await refreshSketch();
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

async function deleteSelectedEntity() {
  const id = sketchStore.selectedId;
  if (id === null) return;
  await deleteEntityCmd(id);
  sketchStore.select(null);
  await refreshSketch();
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

async function toggleConstruction(id: EntityId, event: Event) {
  const checked = (event.target as HTMLInputElement).checked;
  await updateEntityPropCmd(id, "construction", checked ? 1 : 0);
  await refreshSketch();
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

// ── Context menu ─────────────────────────────────────────────────

const ctxMenu = ref<CtxTarget | null>(null);
function onFeatureContextMenu(event: MouseEvent, featureId: FeatureId) {
  event.preventDefault();
  ctxMenu.value = { x: event.clientX, y: event.clientY, featureId };
}
function closeCtxMenu() { ctxMenu.value = null; }
const ctxFeature = computed(() =>
  ctxMenu.value ? sketchStore.features.find(f => f.id === ctxMenu.value!.featureId) : null
);
const ctxFeatureSuppressed = computed(() => ctxFeature.value?.suppressed ?? false);

async function ctxRename() {
  if (!ctxMenu.value) return;
  const f = sketchStore.features.find(x => x.id === ctxMenu.value!.featureId);
  if (!f) return;
  const n = prompt("重命名:", f.name);
  if (n && n !== f.name) {
    try {
      await renameFeatureCmd(f.id, n);
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`重命名失败: ${err}`);
    }
  }
  closeCtxMenu();
}

async function ctxDelete() {
  if (!ctxMenu.value) return;
  const fid = ctxMenu.value.featureId;
  closeCtxMenu();
  await deleteFeature(fid);
}

function ctxEditSketch() {
  if (!ctxMenu.value) return;
  const fid = ctxMenu.value.featureId;
  const f = sketchStore.features.find(x => x.id === fid);
  if (f && (f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"))) {
    enterSketchEdit(fid);
  }
  closeCtxMenu();
}

async function ctxToggleSuppress() {
  if (!ctxMenu.value || !ctxFeature.value) return;
  await toggleSuppress(ctxFeature.value);
  closeCtxMenu();
}

function ctxFaceNormal() {
  if (!ctxMenu.value) return;
  const fid = ctxMenu.value.featureId;
  selectFeature(fid);
  nextTick(() => { unifiedViewport.value?.setView("face"); });
  closeCtxMenu();
}

async function exitSketchEdit() {
  if (sketchStore.editingSketchId === null) return;
  sketchStore.setEditingSketch(null);
  toolbarRef.value?.closeAllMenus();
  await unifiedViewport.value?.refreshViewport();
}

function onGlobalClick() {
  toolbarRef.value?.closeAllMenus();
  closeCtxMenu();
}

// ── Plugin dialog ────────────────────────────────────────────────

const activeDialog = ref<GeneratorInfo | null>(null);
const dialogLoading = ref(false);

function openPluginDialog(gen: GeneratorInfo) {
  sketchStore.setActivePluginTool(`${gen.plugin_id}:${gen.id}`);
  activeDialog.value = gen;
}

function closeDialog() {
  sketchStore.setActivePluginTool(null);
  activeDialog.value = null;
  dialogLoading.value = false;
}

async function runGenerator(params: Record<string, number>) {
  const gen = activeDialog.value;
  if (!gen || dialogLoading.value) return;
  dialogLoading.value = true;
  try {
    const id = await generatePluginFeature(gen.plugin_id, gen.id, { ...params });
    closeDialog();
    sketchStore.setActiveFeature(id);
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    sketchStore.selectedFeatureId = id;
    toastStore.success(`${gen.name} 生成成功`);
  } catch (e) {
    toastStore.error(`${gen.name} 生成失败: ${e}`);
  } finally {
    dialogLoading.value = false;
  }
}

// ── Keyboard shortcuts ───────────────────────────────────────────

async function onKeyDown(e: KeyboardEvent) {
  const tag = (e.target as HTMLElement).tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") return;

  if (e.ctrlKey && e.key === "s") { e.preventDefault(); saveProjectFile(); return; }
  if (e.ctrlKey && e.key === "o") { e.preventDefault(); loadProjectFile(); return; }
  if (e.ctrlKey && e.key === "z") { e.preventDefault(); undoAction(); return; }
  if (e.ctrlKey && (e.key === "y" || (e.key === "Z" && e.shiftKey))) {
    e.preventDefault(); redoAction(); return;
  }

  // Feature shortcuts work whenever a sketch is selected (edit mode not required).
  switch (e.key) {
    case "e": case "E": showExtrudePanel(); return;
    case "w": case "W": revolve(); return;
    case "n": case "N": newSketch(); return;
  }

  if (!isEditingSketch.value) return;

  switch (e.key) {
    case "l": case "L": sketchStore.setTool("line"); break;
    case "c": case "C": sketchStore.setTool("circle"); break;
    case "a": case "A": sketchStore.setTool("arc"); break;
    case "r": case "R": sketchStore.setTool("rectangle"); break;
    case "b": case "B": sketchStore.setTool("spline"); break;
    case "i": case "I": sketchStore.setTool("ellipse"); break;
    case "s": case "S": if (!e.ctrlKey) solve(); break;
    case "h": case "H": addHorizontal(); break;
    case "v": case "V": addVertical(); break;
    case "p": case "P": addParallel(); break;
    case "d": case "D": addDistance(); break;
    case "Delete": case "Backspace":
      if (sketchStore.selectedId !== null) {
        await deleteSelectedEntity();
        e.preventDefault();
      }
      break;
    case "Escape": sketchStore.setTool("select"); break;
  }
}

onMounted(async () => {
  await loadState();
  await refreshPlugins();
  await refreshRecent();
  layoutRoot.value?.focus();
  document.addEventListener("click", onGlobalClick);

  // Check for crash recovery file
  try {
    const hasRecovery = await checkRecovery();
    if (hasRecovery) {
      toastStore.info("检测到未保存的工作，使用 Ctrl+O 恢复");
    }
  } catch {
    // Ignore recovery check errors
  }
});
onUnmounted(() => {
  document.removeEventListener("click", onGlobalClick);
});
</script>

<style>
/*
 * These styles are intentionally NON-scoped: the markup for the toolbar,
 * feature tree, properties panel, dialogs, toasts and context menu now
 * lives in dedicated child components, but they all reuse the class names
 * defined here. Keeping the rules global (rather than scoped to HomeView)
 * lets them reach into those children without duplication. Specificity is
 * never higher than a single class, so any child that ships its own scoped
 * override (e.g. ExtrudePanel's .btn-primary) still wins.
 */
.main-layout {
  display: flex; flex-direction: column; width: 100%; height: 100%;
  background: #1e1e1e; color: #e0e0e0; outline: none;
}
.toolbar {
  height: 36px; display: flex; align-items: center; gap: 2px;
  padding: 0 8px; background: #2d2d2d; border-bottom: 1px solid #3c3c3c;
  flex-shrink: 0; user-select: none; z-index: 50;
}
.title { font-weight: 700; font-size: 13px; margin-right: 8px; }
.tb-sep { width: 1px; height: 22px; background: #555; margin: 0 3px; flex-shrink: 0; }
.tb-spacer { flex: 1; }

.tb-menu { position: relative; flex-shrink: 0; }
.tb-menu-btn {
  background: transparent; border: 1px solid transparent; color: #ccc;
  padding: 4px 10px; font-size: 12px; cursor: pointer; border-radius: 3px; white-space: nowrap;
}
.tb-menu-btn:hover, .tb-menu-btn.open { background: #444; border-color: #555; }
.tb-dropdown {
  position: absolute; top: 100%; left: 0; margin-top: 2px;
  background: #333; border: 1px solid #555; border-radius: 4px;
  min-width: 140px; box-shadow: 0 4px 16px rgba(0,0,0,.5); z-index: 100; padding: 4px;
}
.tb-dd-item {
  display: flex; justify-content: space-between; align-items: center;
  width: 100%; padding: 5px 12px; font-size: 12px; color: #ccc;
  background: transparent; border: none; cursor: pointer; border-radius: 2px; text-align: left;
}
.tb-dd-item:hover { background: #007acc; color: #fff; }
.mm-grid { display: grid; gap: 2px; padding: 2px; }
.mm-grid button, .mm-list button {
  padding: 5px 10px; font-size: 12px; color: #ccc; background: transparent;
  border: 1px solid transparent; cursor: pointer; border-radius: 2px;
  text-align: left; display: flex; justify-content: space-between; align-items: center;
}
.mm-grid button:hover, .mm-list button:hover { background: #007acc; color: #fff; }
.mm-grid button.active { background: #007acc; }
.mm-2col { grid-template-columns: 1fr 1fr; }
.mm-key { font-size: 10px; color: #888; margin-left: 12px; }
button:hover .mm-key { color: #aac; }

.recent-section { border-top: 1px solid #555; margin-top: 4px; padding-top: 4px; }
.recent-header { font-size: 10px; color: #888; padding: 4px 12px 2px; text-transform: uppercase; letter-spacing: 0.5px; }
.recent-item { font-size: 11px !important; }
.recent-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 200px; }
.recent-clear { color: #999 !important; font-size: 10px !important; font-style: italic; }
.recent-clear:hover { color: #f88 !important; background: transparent !important; }

.tb-quick-btn {
  background: #3c3c3c; border: 1px solid #555; color: #ddd;
  padding: 4px 10px; font-size: 12px; cursor: pointer; border-radius: 3px; white-space: nowrap;
}
.tb-quick-btn:hover { background: #007acc; color: #fff; border-color: #007acc; }

.tb-plane {
  background: #3c3c3c; border: 1px solid #555; color: #ccc;
  padding: 3px 6px; border-radius: 3px; font-size: 11px; cursor: pointer;
}
.tb-plane:disabled {
  opacity: 0.5; cursor: not-allowed; background: #2a2a2a;
}
.tb-icon-btn {
  background: transparent; border: 1px solid transparent; color: #ccc;
  padding: 3px 8px; font-size: 14px; cursor: pointer; border-radius: 3px;
}
.tb-icon-btn:hover { background: #444; }
.tb-exit-sketch {
  background: #5a2a2a; border: 1px solid #844; color: #f88;
  padding: 4px 12px; font-size: 12px; cursor: pointer; border-radius: 3px;
  white-space: nowrap; font-weight: 600;
}
.tb-exit-sketch:hover { background: #733; color: #faa; }

.workspace { flex: 1; display: flex; overflow: hidden; }
.sidebar-left, .sidebar-right { width: 240px; background: #252526; overflow-y: auto; flex-shrink: 0; }
.sidebar-left { border-right: 1px solid #3c3c3c; }
.sidebar-right { border-left: 1px solid #3c3c3c; }
.panel { padding: 12px; }
.panel h3 { font-size: 11px; text-transform: uppercase; color: #999; margin-bottom: 8px; letter-spacing: 0.5px; display: flex; align-items: center; gap: 8px; }
.err-badge { background: #c62828; color: #fff; padding: 1px 6px; border-radius: 8px; font-size: 10px; }
.placeholder { font-size: 12px; color: #777; }

.prop-section { font-size: 12px; }
.prop-row { display: flex; align-items: center; gap: 8px; margin: 6px 0; }
.prop-row label { width: 40px; color: #999; flex-shrink: 0; }
.prop-row span { color: #ccc; }
.prop-row input { width: 130px; background: #3c3c3c; border: 1px solid #555; color: #e0e0e0; padding: 2px 5px; border-radius: 2px; font-size: 12px; }
.prop-row input:focus { border-color: #007acc; outline: none; }
.ro-val { color: #777 !important; }
.prop-del-btn { margin-top: 8px; padding: 4px 12px; background: #5a1a1a; border: 1px solid #833; color: #f88; font-size: 11px; border-radius: 2px; cursor: pointer; }
.prop-del-btn:hover { background: #832; }
.prop-check { display: flex; align-items: center; gap: 6px; margin-top: 8px; font-size: 11px; color: #aaa; cursor: pointer; }
.prop-check input { accent-color: #007acc; }
.prop-error { background: #4a2020; border: 1px solid #833; color: #fcc; padding: 6px 10px; border-radius: 3px; font-size: 11px; margin: 6px 0; }
.has-dep { color: #ffd54f; font-size: 11px; font-style: italic; }

.feature-tree { list-style: none; font-size: 12px; margin: 0; padding: 0; }
.feature-tree li { display: flex; align-items: center; gap: 6px; padding: 5px 8px; border-radius: 3px; cursor: pointer; }
.feature-tree li:hover { background: #3c3c3c; }
.feature-tree li.active { background: #007acc; }
.feature-tree li.selected { outline: 1px solid #4fc3f7; outline-offset: -1px; }
.feature-tree li.suppressed { opacity: 0.45; font-style: italic; }
.feature-tree li.errored { background: #4a2020; }
.feature-tree li.errored.active { background: #6a3030; }
.suppress-btn {
  background: transparent; border: none; color: #aaa; cursor: pointer;
  font-size: 10px; padding: 0; line-height: 1; opacity: 0.6;
}
.suppress-btn:hover { color: #fff; opacity: 1; }
.feature-icon { font-size: 14px; flex-shrink: 0; }
.err-icon { color: #ff5252; font-size: 12px; }
.delete-btn {
  margin-left: auto; background: transparent; border: none; color: #999;
  cursor: pointer; font-size: 16px; line-height: 1; padding: 0 2px; opacity: 0; transition: opacity 0.15s;
}
.feature-tree li:hover .delete-btn { opacity: 1; }
.delete-btn:hover { color: #ff6b6b; }
.fi-arrow { color: #555; font-size: 10px; margin-right: 2px; flex-shrink: 0; }
.fi-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.ctx-menu {
  position: fixed; z-index: 200; background: #333; border: 1px solid #555; border-radius: 4px;
  padding: 4px; min-width: 120px; box-shadow: 0 4px 16px rgba(0,0,0,.6);
}
.ctx-menu button { display: block; width: 100%; padding: 6px 12px; font-size: 12px; color: #ccc; background: transparent; border: none; cursor: pointer; text-align: left; border-radius: 2px; }
.ctx-menu button:hover { background: #007acc; }
.ctx-danger:hover { background: #c62828 !important; }

.plugin-item { margin-bottom: 10px; padding: 6px 8px; background: #2a2a2a; border-radius: 4px; }
.plugin-header { display: flex; align-items: center; gap: 6px; margin-bottom: 2px; }
.plugin-name { font-size: 12px; font-weight: 600; }
.plugin-version { font-size: 10px; color: #777; }
.plugin-desc { font-size: 11px; color: #999; margin-bottom: 6px; }
.plugin-generator { font-size: 12px; color: #4fc3f7; cursor: pointer; padding: 3px 0; }
.plugin-generator:hover { color: #81d4fa; }

.viewport { flex: 1; position: relative; overflow: hidden; }

.statusbar {
  height: 26px; display: flex; align-items: center; padding: 0 12px;
  background: #007acc; font-size: 12px; color: white; flex-shrink: 0;
}

/* Toast */
.toast-container { position: fixed; bottom: 36px; right: 16px; z-index: 200; display: flex; flex-direction: column; gap: 6px; pointer-events: none; }
.toast { padding: 8px 16px; border-radius: 4px; font-size: 13px; box-shadow: 0 2px 12px rgba(0,0,0,.4); animation: toast-in 0.25s ease-out; }
.toast-info { background: #007acc; color: white; }
.toast-success { background: #2e7d32; color: white; }
.toast-error { background: #c62828; color: white; }
@keyframes toast-in { from { opacity: 0; transform: translateY(10px); } to { opacity: 1; transform: translateY(0); } }

/* Dialog */
.dialog-overlay { position: fixed; inset: 0; background: rgba(0,0,0,.6); display: flex; align-items: center; justify-content: center; z-index: 100; }
.dialog { background: #2d2d2d; border: 1px solid #555; border-radius: 6px; padding: 20px 24px; min-width: 320px; max-width: 420px; box-shadow: 0 8px 32px rgba(0,0,0,.5); }
.dialog h3 { margin: 0 0 4px; font-size: 16px; }
.dialog-desc { font-size: 12px; color: #999; margin: 0 0 16px; }
.dialog-param { display: flex; align-items: center; gap: 12px; margin-bottom: 10px; }
.dialog-param label { width: 120px; font-size: 13px; color: #ccc; }
.dialog-param input { flex: 1; background: #1e1e1e; border: 1px solid #555; color: #e0e0e0; padding: 5px 8px; border-radius: 3px; font-size: 13px; }
.dialog-actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 20px; }
.btn-primary { background: #007acc; border: none; color: white; padding: 7px 20px; border-radius: 3px; cursor: pointer; font-size: 13px; }
.btn-primary:hover { background: #0098ff; }
.btn-primary:disabled { background: #555; cursor: not-allowed; }
.btn-danger { background: #c62828; border: none; color: white; padding: 7px 20px; border-radius: 3px; cursor: pointer; font-size: 13px; }
.btn-danger:hover { background: #d32f2f; }
.btn-spinner { display: inline-block; width: 12px; height: 12px; border: 2px solid rgba(255,255,255,.3); border-top-color: #fff; border-radius: 50%; animation: spin 0.6s linear infinite; margin-right: 6px; vertical-align: middle; }
@keyframes spin { to { transform: rotate(360deg); } }
.btn-cancel { background: #3c3c3c; border: 1px solid #555; color: #e0e0e0; padding: 7px 20px; border-radius: 3px; cursor: pointer; font-size: 13px; }
.btn-cancel:hover { background: #505050; }
</style>
