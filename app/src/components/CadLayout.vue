<template>
  <!-- App shell: toolbar (top) / feature tree + plugins (left) /
       properties (right) / viewport (center, native wgpu surface) /
       status bar (bottom). Overlays: context menu, dialogs, toasts. -->
  <div class="cad-layout" @keydown="onKeyDown" tabindex="-1">
    <header class="toolbar-area">
      <Toolbar
        ref="toolbarRef"
        :recent-files="recentFiles"
        @file-action="onFileAction"
        @open-recent="onOpenRecent"
        @clear-recent="onClearRecent"
        @undo="undoAction"
        @redo="redoAction"
        @new-sketch="newSketch"
        @clear-doc="clearDocument"
        @feature="onFeature"
        @pattern="onPattern"
        @constraint="onConstraint"
        @exit-sketch="exitSketchEdit"
        @open-plugin="openPluginDialog"
        @offset-plane="offsetDialogOpen = true"
        @clear-sketch="clearSketchAction"
        @toggle-measure="sketchStore.toggleMeasure"
      />
    </header>

    <aside class="sidebar-left">
      <FeatureTree
        @select="selectFeature"
        @edit="editFeature"
        @contextmenu="onFeatureContextMenu"
        @toggle-suppress="toggleSuppress"
        @delete="deleteFeature"
        @reparent="onReparentFeature"
      />

      <div class="panel plugins-section">
        <h3>插件</h3>
        <template v-if="sketchStore.plugins.length > 0">
          <div v-for="plugin in sketchStore.plugins" :key="plugin.id" class="plugin-item">
            <div class="plugin-header">
              <span class="plugin-name">{{ plugin.name }}</span>
              <span class="plugin-version">v{{ plugin.version }}</span>
            </div>
            <div class="plugin-desc">{{ plugin.description }}</div>
            <div
              v-for="gen in plugin.generators"
              :key="gen.id"
              class="plugin-generator"
              @click="openPluginDialog(gen)"
            >
              + {{ gen.name }}
            </div>
          </div>
        </template>
        <p v-else class="placeholder">暂无可用插件</p>
      </div>
    </aside>

    <main class="viewport-area">
      <!-- The native wgpu surface owns this rectangle (see LAYOUT in
           wgpu_viewport.rs). Until macOS compositing lands, this area is
           painted with the renderer's clear color as a placeholder. -->
      <div class="viewport-placeholder"></div>
    </main>

    <aside class="sidebar-right">
      <PropertiesPanel
        :regions="extrudeRegions"
        @rename="renameSelectedFeature"
        @update-param="onUpdateParam"
        @update-entity-prop="onUpdateEntityProp"
        @toggle-construction="onToggleConstruction"
        @delete-entity="deleteSelectedEntity"
        @delete-constraint="onDeleteConstraint"
        @clear-constraints="onClearConstraints"
        @extrude-change="onExtrudeChange"
        @extrude-confirm="onExtrudeConfirm"
        @extrude-cancel="cancelExtrude"
        @extrude-region-change="onExtrudeRegionChange"
        @enter-edge-pick="onEnterEdgePick"
        @exit-edge-pick="sketchStore.exitEdgePickMode()"
        @confirm-edge-pick="confirmEdgePick"
        @remove-picked-edge="(i: number) => sketchStore.removePickedEdge(i)"
        @appearance-changed="refreshFeatures"
      />
    </aside>

    <footer class="statusbar">
      <span class="status-text">{{ statusText }}</span>
    </footer>

    <!-- ── Overlays ──────────────────────────────────────────── -->
    <ToastContainer />

    <ContextMenu
      :target="ctxMenu"
      :suppressed="ctxFeature?.suppressed ?? false"
      :feature-type="ctxFeature?.feature_type"
      @edit-sketch="ctxEditSketch"
      @rename="ctxRename"
      @toggle-suppress="ctxToggleSuppress"
      @delete="ctxDelete"
    />

    <!-- Rename dialog (window.prompt is unsupported in WKWebView). -->
    <div v-if="renameDialog" class="dialog-overlay" @click.self="renameDialog = null">
      <div class="dialog">
        <h3>重命名</h3>
        <div class="dialog-param">
          <label>名称</label>
          <input
            ref="renameInputRef"
            v-model="renameDialog.name"
            @keydown.enter="confirmRename"
            @keydown.esc="renameDialog = null"
          />
        </div>
        <div class="dialog-actions">
          <button class="btn-primary" @click="confirmRename">确定</button>
          <button class="btn-cancel" @click="renameDialog = null">取消</button>
        </div>
      </div>
    </div>

    <PluginDialog
      :generator="pluginDialog"
      :loading="pluginDialogLoading"
      @run="runGenerator"
      @cancel="closePluginDialog"
    />

    <OffsetPlaneDialog
      :open="offsetDialogOpen"
      @confirm="confirmOffsetPlane"
      @cancel="offsetDialogOpen = false"
    />

    <CascadeDeleteDialog
      :target="cascadeDialog"
      @confirm="confirmCascadeDelete"
      @cancel="cascadeDialog = null"
    />

    <BooleanDialog
      :targets="booleanDialog"
      @confirm="confirmBoolean"
      @cancel="booleanDialog = null"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { useSketchStore } from "@/stores/sketch";
import { useToastStore } from "@/stores/toast";
import Toolbar from "./Toolbar.vue";
import FeatureTree from "./FeatureTree.vue";
import PropertiesPanel from "./PropertiesPanel.vue";
import ToastContainer from "./ToastContainer.vue";
import ContextMenu, { type CtxTarget } from "./ContextMenu.vue";
import PluginDialog from "./PluginDialog.vue";
import OffsetPlaneDialog, { type OffsetPlaneResult } from "./OffsetPlaneDialog.vue";
import CascadeDeleteDialog, { type CascadeTarget } from "./CascadeDeleteDialog.vue";
import BooleanDialog, { type BooleanTargets, type BooleanResult } from "./BooleanDialog.vue";
import {
  addBooleanFeature,
  addChamferEdgesFeature,
  addCircularPattern,
  addConstraint,
  addExtrudeFeature,
  addFilletEdgesFeature,
  addLinearPattern,
  addMirrorFeature,
  addRevolveFeature,
  addShellFeature,
  addSketchFeature,
  addSweepFeature,
  checkRecovery,
  clearDocument as clearDocumentCmd,
  clearRecentFiles,
  clearSketch,
  createOffsetPlane,
  deleteEntity,
  deleteFeature as deleteFeatureCmd,
  generatePluginFeature,
  getExtrudeRegions,
  getFeatures,
  getRecentFiles,
  getSketchConstraints,
  getSketchEntities,
  listGenerators,
  listPlugins,
  loadProjectFrom,
  redo,
  removeConstraint,
  renameFeature,
  reparentFeature,
  saveProject,
  saveProjectTo,
  setActiveSketch,
  setFeatureSuppressed,
  solveSketch,
  undo,
  updateEntityProp,
  updateParameter,
  type Constraint,
  type ExtrudeRegionInfo,
  type FeatureId,
  type FeatureNode,
  type GeneratorInfo,
  type ParameterId,
  type EntityId,
} from "@/commands/sketch";
import { type ConstraintKind, type FeatureKind, type PatternKind } from "./Toolbar.vue";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

const sketchStore = useSketchStore();
const toastStore = useToastStore();
const toolbarRef = ref<InstanceType<typeof Toolbar> | null>(null);

const recentFiles = ref<string[]>([]);
const extrudeRegions = ref<ExtrudeRegionInfo[]>([]);

// ── Status bar ─────────────────────────────────────────────────

const isEditingSketch = computed(() => sketchStore.isEditingSketch());

const statusText = computed(() => {
  if (sketchStore.edgePickMode) {
    const label = sketchStore.pendingFilletChamferType === "chamfer" ? "倒角" : "圆角";
    return `${label} — 在视口中拾取边，然后在右侧面板确认`;
  }
  if (sketchStore.showExtrudePanel) return "拉伸 — 在右侧面板调整参数并确认";
  if (isEditingSketch.value) {
    const toolLabels: Record<string, string> = {
      select: "选择", line: "直线", circle: "圆", arc: "弧线",
      rectangle: "矩形", spline: "样条", ellipse: "椭圆",
      trim: "裁剪", extend: "延伸", plugin: "插件工具",
    };
    const tool = toolLabels[sketchStore.activeTool] ?? sketchStore.activeTool;
    return `草图编辑 · ${tool} · ${sketchStore.entities.length} 实体 · ${sketchStore.constraints.length} 约束`;
  }
  const n = sketchStore.features.length;
  return n > 0 ? `3D 视口 · ${n} 个特征` : "就绪 — 通过「编辑 → 新草图」开始";
});

// ── Data refresh ───────────────────────────────────────────────
// Sync invariant (原则2): every mutation path ends in a refresh of the
// feature tree (+ sketch entities/constraints while editing a sketch).

async function refreshRecent() {
  recentFiles.value = await getRecentFiles();
}

async function refreshFeatures() {
  sketchStore.setFeatures(await getFeatures());
}

async function refreshSketch() {
  if (sketchStore.editingSketchId === null) return;
  sketchStore.setEntities(await getSketchEntities());
  sketchStore.setConstraints(await getSketchConstraints());
}

async function refreshAll() {
  await refreshFeatures();
  await refreshSketch();
}

// ── Selection / editing ────────────────────────────────────────

function selectFeature(id: FeatureId) {
  sketchStore.selectedFeatureId = id;
  sketchStore.setActiveFeature(id);
  // Switching to a different sketch leaves edit mode (selection ≠ editing).
  if (sketchStore.editingSketchId !== null && sketchStore.editingSketchId !== id) {
    sketchStore.setEditingSketch(null);
  }
  const f = sketchStore.features.find((x) => x.id === id);
  if (f?.plane) {
    sketchStore.activePlane = f.plane;
    if (f.plane === "offset") {
      sketchStore.activePlaneBase = f.plane_base ?? null;
      sketchStore.activePlaneDistance = f.plane_distance ?? null;
    }
  }
}

/** Double-click: sketches enter edit mode; other features just select. */
function editFeature(feature: FeatureNode) {
  if (feature.feature_type === "Sketch" || feature.feature_type.startsWith("Custom:")) {
    enterSketchEdit(feature.id);
  } else {
    selectFeature(feature.id);
  }
}

async function enterSketchEdit(id: FeatureId) {
  const f = sketchStore.features.find((x) => x.id === id);
  if (!f || !(f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"))) return;

  sketchStore.selectedFeatureId = id;
  sketchStore.setActiveFeature(id);
  sketchStore.setEditingSketch(id);
  sketchStore.setTool("select");
  if (f.plane) {
    sketchStore.activePlane = f.plane;
    if (f.plane === "offset") {
      sketchStore.activePlaneBase = f.plane_base ?? null;
      sketchStore.activePlaneDistance = f.plane_distance ?? null;
    }
  }
  sketchStore.showExtrudePanel = false;
  await setActiveSketch(id);
  await refreshSketch();
}

function exitSketchEdit() {
  sketchStore.setEditingSketch(null);
  sketchStore.setEntities([]);
  sketchStore.setConstraints([]);
  toolbarRef.value?.closeAllMenus();
}

// ── Feature-tree actions ───────────────────────────────────────

async function toggleSuppress(feature: FeatureNode) {
  await setFeatureSuppressed(feature.id, !feature.suppressed);
  await refreshFeatures();
}

async function renameSelectedFeature(name: string) {
  const id = sketchStore.selectedFeatureId ?? sketchStore.activeFeatureId;
  if (id == null) return;
  try {
    await renameFeature(id, name);
    await refreshFeatures();
  } catch (err) {
    toastStore.error(`重命名失败: ${err}`);
  }
}

/** Delete, asking for confirmation when dependents exist (cascade). */
async function deleteFeature(id: FeatureId) {
  const feature = sketchStore.features.find((f) => f.id === id);
  if (!feature) return;
  try {
    await deleteFeatureCmd(id, false);
    if (sketchStore.selectedFeatureId === id) sketchStore.selectedFeatureId = null;
    await refreshAll();
    toastStore.success(`已删除 "${feature.name}"`);
  } catch (err) {
    // Backend rejects with a structured "dependents" error → offer cascade.
    const msg = String(err);
    let dependentNames: string[] = [];
    try {
      const parsed = JSON.parse(msg);
      if (parsed.kind === "dependents" && Array.isArray(parsed.names)) {
        dependentNames = parsed.names;
      }
    } catch {
      if (msg.startsWith("dependents:")) {
        dependentNames = msg.slice("dependents:".length).split(",").filter(Boolean);
      }
    }
    if (dependentNames.length > 0) {
      cascadeDialog.value = { id, name: feature.name, dependents: dependentNames };
    } else {
      toastStore.error(`删除失败: ${msg}`);
    }
  }
}

const cascadeDialog = ref<CascadeTarget | null>(null);

async function confirmCascadeDelete() {
  const dialog = cascadeDialog.value;
  if (!dialog) return;
  cascadeDialog.value = null;
  try {
    await deleteFeatureCmd(dialog.id, true);
    if (sketchStore.selectedFeatureId === dialog.id) sketchStore.selectedFeatureId = null;
    await refreshAll();
    toastStore.success(`已级联删除 "${dialog.name}" 及其依赖项`);
  } catch (err) {
    toastStore.error(`删除失败: ${err}`);
  }
}

async function onReparentFeature(childId: FeatureId, newParentId: FeatureId) {
  try {
    const ok = await reparentFeature(childId, newParentId);
    if (ok) {
      await refreshFeatures();
      toastStore.success("已移动特征");
    } else {
      toastStore.error("无法移动：可能造成循环依赖");
    }
  } catch (err) {
    toastStore.error(`移动失败: ${err}`);
  }
}

// ── Context menu (feature tree right-click) ────────────────────

const ctxMenu = ref<CtxTarget | null>(null);
const ctxFeature = computed(() =>
  ctxMenu.value
    ? sketchStore.features.find((f) => f.id === ctxMenu.value!.featureId) ?? null
    : null,
);

function onFeatureContextMenu(event: MouseEvent, featureId: FeatureId) {
  selectFeature(featureId);
  ctxMenu.value = { x: event.clientX, y: event.clientY, featureId };
}

function closeCtxMenu() {
  ctxMenu.value = null;
}

function ctxEditSketch() {
  const f = ctxFeature.value;
  closeCtxMenu();
  if (f) editFeature(f);
}

const renameDialog = ref<{ id: FeatureId; name: string } | null>(null);
const renameInputRef = ref<HTMLInputElement | null>(null);

function ctxRename() {
  const f = ctxFeature.value;
  closeCtxMenu();
  if (!f) return;
  renameDialog.value = { id: f.id, name: f.name };
  nextTick(() => {
    renameInputRef.value?.focus();
    renameInputRef.value?.select();
  });
}

async function confirmRename() {
  const dialog = renameDialog.value;
  if (!dialog) return;
  renameDialog.value = null;
  const name = dialog.name.trim();
  if (!name) return;
  try {
    await renameFeature(dialog.id, name);
    await refreshFeatures();
  } catch (err) {
    toastStore.error(`重命名失败: ${err}`);
  }
}

async function ctxToggleSuppress() {
  const f = ctxFeature.value;
  closeCtxMenu();
  if (f) await toggleSuppress(f);
}

async function ctxDelete() {
  const f = ctxFeature.value;
  closeCtxMenu();
  if (f) await deleteFeature(f.id);
}

// ── File actions ───────────────────────────────────────────────

function resetTransientUi() {
  sketchStore.resetPerDocumentState();
  extrudeRegions.value = [];
  extrudeTargetId.value = null;
  booleanDialog.value = null;
  cascadeDialog.value = null;
  renameDialog.value = null;
  ctxMenu.value = null;
}

async function onFileAction(action: string) {
  switch (action) {
    case "new":
      await clearDocument();
      break;
    case "open": {
      const path = await invoke<string | null>("load_project_cmd");
      if (path) {
        resetTransientUi();
        await refreshAll();
        await refreshRecent();
      }
      break;
    }
    case "save": {
      const path = await saveProject();
      if (path) {
        await refreshRecent();
        toastStore.success("已保存");
      }
      break;
    }
    case "save-as": {
      await saveProjectTo("");
      await refreshRecent();
      break;
    }
    case "export-stl":
      await invoke("export_stl");
      break;
    case "export-obj":
      await invoke("export_obj");
      break;
    case "export-gltf":
      await invoke("export_gltf_cmd");
      break;
  }
}

async function onOpenRecent(path: string) {
  await loadProjectFrom(path);
  resetTransientUi();
  await refreshAll();
  await refreshRecent();
}

async function onClearRecent() {
  await clearRecentFiles();
  await refreshRecent();
}

async function clearDocument() {
  await clearDocumentCmd();
  resetTransientUi();
  await refreshAll();
}

// ── Edit actions ───────────────────────────────────────────────

async function undoAction() {
  if (await undo()) {
    sketchStore.resetPerDocumentState();
    await refreshAll();
  }
}

async function redoAction() {
  if (await redo()) {
    sketchStore.resetPerDocumentState();
    await refreshAll();
  }
}

async function newSketch() {
  try {
    const id = await addSketchFeature(sketchStore.activePlane);
    await refreshFeatures();
    await enterSketchEdit(id);
  } catch (err) {
    toastStore.error(`创建草图失败: ${err}`);
  }
}

async function clearSketchAction() {
  if (sketchStore.editingSketchId === null) return;
  await clearSketch();
  await refreshAll();
}

// ── Feature creation ───────────────────────────────────────────

function isSolidFeature(f: FeatureNode): boolean {
  return (
    !f.suppressed &&
    (["Extrude", "Revolve", "Fillet", "Chamfer", "LinearPattern", "CircularPattern",
      "Mirror", "Sweep", "Shell", "Boolean"].includes(f.feature_type) ||
      f.feature_type.startsWith("CustomSolid"))
  );
}

function selectedSolidFeatureId(): FeatureId | null {
  const sel = sketchStore.selectedFeatureId;
  if (sel === null) return null;
  const f = sketchStore.features.find((x) => x.id === sel);
  return f && isSolidFeature(f) ? f.id : null;
}

function lastSolidFeatureId(): FeatureId | null {
  const solids = sketchStore.features.filter(isSolidFeature);
  return solids.length > 0 ? solids[solids.length - 1].id : null;
}

/** The sketch feature operations should target: the one being edited,
 *  else the selected/active one if it is a sketch. */
function activeSketchForFeature(): FeatureId | null {
  if (sketchStore.editingSketchId !== null) return sketchStore.editingSketchId;
  const f = sketchStore.features.find((x) => x.id === sketchStore.activeFeatureId);
  if (f && (f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"))) return f.id;
  return null;
}

/** The sketch the extrude panel was opened for (region queries + confirm
 *  must target the same sketch, not whatever happens to be active later). */
const extrudeTargetId = ref<FeatureId | null>(null);
const extrudeSelectedRegions = ref<number[] | null>(null);

async function showExtrudePanel() {
  const targetId = activeSketchForFeature();
  if (targetId === null) {
    toastStore.info("请先选择一个草图");
    return;
  }
  extrudeTargetId.value = targetId;
  extrudeSelectedRegions.value = null;
  try {
    extrudeRegions.value = await getExtrudeRegions(targetId);
  } catch {
    extrudeRegions.value = [];
  }
  sketchStore.showExtrudePanel = true;
}

function onExtrudeChange(config: { direction: string; depth: number; dist2: number; draft: number }) {
  sketchStore.extrudeConfig = { ...config };
}

function onExtrudeRegionChange(selected: number[] | null) {
  extrudeSelectedRegions.value = selected;
}

function cancelExtrude() {
  sketchStore.showExtrudePanel = false;
  extrudeRegions.value = [];
  extrudeTargetId.value = null;
  extrudeSelectedRegions.value = null;
}

async function onExtrudeConfirm(selectedRegions: number[] | null) {
  const targetId = extrudeTargetId.value;
  if (targetId === null) return;
  const c = sketchStore.extrudeConfig;
  const regions = selectedRegions ?? extrudeSelectedRegions.value;
  cancelExtrude();
  try {
    await addExtrudeFeature(targetId, c.direction, c.dist2, c.draft, c.depth, regions);
    await refreshFeatures();
    toastStore.success("拉伸特征已创建");
  } catch (err) {
    toastStore.error(`拉伸失败: ${err}`);
  }
}

async function onFeature(kind: FeatureKind) {
  switch (kind) {
    case "extrude":
      await showExtrudePanel();
      return;
    case "revolve": {
      const sketchId = activeSketchForFeature();
      if (sketchId === null) {
        toastStore.info("请先选择一个草图");
        return;
      }
      await withFeature("旋转特征", () =>
        addRevolveFeature(sketchId, sketchStore.selectedId ?? null));
      return;
    }
    case "sweep": {
      const profile = activeSketchForFeature();
      if (profile === null) {
        toastStore.info("请先选择一个草图作为轮廓");
        return;
      }
      const path = sketchStore.features.find(
        (f) => f.id !== profile &&
          (f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:")),
      );
      if (!path) {
        toastStore.info("扫描需要两个草图：轮廓 + 路径");
        return;
      }
      await withFeature("扫描特征", () => addSweepFeature(profile, path.id));
      return;
    }
    case "fillet":
    case "chamfer": {
      const target = selectedSolidFeatureId() ?? lastSolidFeatureId();
      if (target === null) {
        toastStore.info("请先创建一个实体");
        return;
      }
      sketchStore.enterEdgePickMode(kind, target);
      return;
    }
    case "shell": {
      const target = selectedSolidFeatureId() ?? lastSolidFeatureId();
      if (target === null) {
        toastStore.info("请先创建一个实体");
        return;
      }
      await withFeature("抽壳特征", () => addShellFeature(target, 1));
      return;
    }
    case "boolean":
      openBooleanDialog();
      return;
  }
}

/** Shared wrapper: run a feature command, refresh, toast the outcome. */
async function withFeature<T>(label: string, fn: () => Promise<T>): Promise<T | null> {
  try {
    const result = await fn();
    await refreshFeatures();
    toastStore.success(`${label}已创建`);
    return result;
  } catch (err) {
    toastStore.error(`${label}失败: ${err}`);
    return null;
  }
}

// ── Boolean dialog ─────────────────────────────────────────────

const booleanDialog = ref<BooleanTargets | null>(null);

function openBooleanDialog() {
  const solids = sketchStore.features.filter(isSolidFeature);
  if (solids.length < 2) {
    toastStore.info("布尔运算需要两个实体");
    return;
  }
  const sel = selectedSolidFeatureId();
  const a = solids.find((f) => f.id === sel) ?? solids[solids.length - 1];
  const others = solids.filter((f) => f.id !== a.id);
  const b = others[others.length - 1];
  booleanDialog.value = { idA: a.id, nameA: a.name, idB: b.id, nameB: b.name };
}

async function confirmBoolean(result: BooleanResult) {
  booleanDialog.value = null;
  const id = await withFeature("布尔特征", () =>
    addBooleanFeature(result.idA, result.idB, result.op));
  if (id !== null) sketchStore.selectedFeatureId = id;
}

// ── Edge picking (fillet / chamfer) ────────────────────────────

function onEnterEdgePick() {
  const target = selectedSolidFeatureId() ?? lastSolidFeatureId();
  if (target === null) {
    toastStore.info("请先创建一个实体");
    return;
  }
  sketchStore.enterEdgePickMode("fillet", target);
}

async function confirmEdgePick(radiusOrDistance: number) {
  const targetId = sketchStore.pendingFilletChamferTarget;
  const edgeType = sketchStore.pendingFilletChamferType;
  if (targetId === null || edgeType === null) return;
  // Capture before exiting (exitEdgePickMode clears the list).
  const edges = [...sketchStore.pendingEdges];
  sketchStore.exitEdgePickMode();
  const label = edgeType === "fillet" ? "圆角特征" : "倒角特征";
  await withFeature(label, () =>
    edgeType === "fillet"
      ? addFilletEdgesFeature(targetId, radiusOrDistance, edges)
      : addChamferEdgesFeature(targetId, radiusOrDistance, edges));
}

// ── Patterns ───────────────────────────────────────────────────

async function onPattern(kind: PatternKind) {
  const target = selectedSolidFeatureId() ?? lastSolidFeatureId();
  if (target === null) {
    toastStore.info("请先创建一个实体");
    return;
  }
  switch (kind) {
    case "linear":
      await withFeature("线性阵列", () => addLinearPattern(target, 1, 0, 0, 3, 20));
      break;
    case "circular":
      await withFeature("圆周阵列", () => addCircularPattern(target, 0, 0, 0, 1, 0, 0, 4, 90));
      break;
    case "mirror":
      await withFeature("镜像特征", () => addMirrorFeature(target, 0, 0, 1, 0, 0, 0));
      break;
  }
}

// ── Constraints ────────────────────────────────────────────────

async function onConstraint(kind: ConstraintKind) {
  const a = sketchStore.selectedId;
  if (kind === "solve") {
    await solveSketch();
    await refreshSketch();
    return;
  }
  if (a == null) {
    toastStore.info("请先在草图中选择一个实体");
    return;
  }
  let constraint: Constraint;
  switch (kind) {
    case "horizontal":
      constraint = { Horizontal: { line: a } };
      break;
    case "vertical":
      constraint = { Vertical: { line: a } };
      break;
    case "fix":
      constraint = { Fix: { point: a } };
      break;
    case "diameter":
      constraint = { Diameter: { circle: a, diameter: 10 } };
      break;
    default:
      // Two-entity constraints need pair picking in the native viewport.
      toastStore.info("该约束需要选择两个实体（视口拾取即将支持）");
      return;
  }
  try {
    await addConstraint(constraint);
    await refreshSketch();
  } catch (err) {
    toastStore.error(`添加约束失败: ${err}`);
  }
}

// ── Properties panel ───────────────────────────────────────────

async function onUpdateParam(id: ParameterId, event: Event) {
  const value = parseFloat((event.target as HTMLInputElement).value);
  if (Number.isFinite(value)) {
    await updateParameter(id, value);
    await refreshFeatures();
  }
}

async function onUpdateEntityProp(id: EntityId, prop: string, event: Event) {
  const value = parseFloat((event.target as HTMLInputElement).value);
  if (Number.isFinite(value)) {
    await updateEntityProp(id, prop, value);
    await refreshAll();
  }
}

async function onToggleConstruction(id: EntityId, event: Event) {
  const value = (event.target as HTMLInputElement).checked ? 1 : 0;
  await updateEntityProp(id, "construction", value);
  await refreshAll();
}

async function deleteSelectedEntity() {
  const id = sketchStore.selectedId;
  if (id == null) return;
  await deleteEntity(id);
  sketchStore.select(null);
  await refreshAll();
}

async function onDeleteConstraint(index: number) {
  await removeConstraint(index);
  await refreshSketch();
}

async function onClearConstraints() {
  await refreshSketch();
}

// ── Plugin dialog ──────────────────────────────────────────────

const pluginDialog = ref<GeneratorInfo | null>(null);
const pluginDialogLoading = ref(false);

function openPluginDialog(gen: GeneratorInfo) {
  sketchStore.setActivePluginTool(`${gen.plugin_id}:${gen.id}`);
  pluginDialog.value = gen;
}

function closePluginDialog() {
  sketchStore.setActivePluginTool(null);
  pluginDialog.value = null;
  pluginDialogLoading.value = false;
}

async function runGenerator(params: Record<string, number>) {
  const gen = pluginDialog.value;
  if (!gen || pluginDialogLoading.value) return;
  pluginDialogLoading.value = true;
  try {
    const id = await generatePluginFeature(gen.plugin_id, gen.id, { ...params });
    closePluginDialog();
    sketchStore.setActiveFeature(id);
    sketchStore.selectedFeatureId = id;
    await refreshFeatures();
    toastStore.success(`${gen.name} 生成成功`);
  } catch (err) {
    toastStore.error(`${gen.name} 生成失败: ${err}`);
  } finally {
    pluginDialogLoading.value = false;
  }
}

// ── Offset plane dialog ────────────────────────────────────────

const offsetDialogOpen = ref(false);

async function confirmOffsetPlane(result: OffsetPlaneResult) {
  offsetDialogOpen.value = false;
  try {
    const id = await createOffsetPlane(result.basePlane, result.distance);
    await refreshFeatures();
    await enterSketchEdit(id);
    toastStore.success("偏移平面已创建");
  } catch (err) {
    toastStore.error(`偏移平面创建失败: ${err}`);
  }
}

// ── Keyboard shortcuts ─────────────────────────────────────────

async function onKeyDown(e: KeyboardEvent) {
  // Never hijack typing in inputs/selects.
  const tag = (e.target as HTMLElement | null)?.tagName;
  if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;

  const mod = e.ctrlKey || e.metaKey;
  if (mod) {
    switch (e.key.toLowerCase()) {
      case "z":
        e.preventDefault();
        if (e.shiftKey) await redoAction();
        else await undoAction();
        return;
      case "y":
        e.preventDefault();
        await redoAction();
        return;
      case "s":
        e.preventDefault();
        await onFileAction("save");
        return;
    }
    return;
  }

  switch (e.key) {
    case "Escape":
      if (ctxMenu.value) {
        closeCtxMenu();
      } else if (sketchStore.edgePickMode) {
        sketchStore.exitEdgePickMode();
      } else if (sketchStore.isEditingSketch()) {
        exitSketchEdit();
      }
      break;
    case "Delete":
    case "Backspace":
      if (sketchStore.selectedId !== null) {
        e.preventDefault();
        await deleteSelectedEntity();
      }
      break;
  }
}

function onGlobalClick() {
  toolbarRef.value?.closeAllMenus();
  closeCtxMenu();
}

// ── Lifecycle ──────────────────────────────────────────────────

let unlisteners: Array<() => void> = [];

onMounted(async () => {
  await Promise.all([
    refreshAll(),
    refreshRecent(),
    listPlugins().then((p) => sketchStore.setPlugins(p)).catch(() => {}),
    listGenerators().then((g) => sketchStore.setGenerators(g)).catch(() => {}),
  ]);
  document.addEventListener("click", onGlobalClick);

  try {
    if (await checkRecovery()) {
      toastStore.info("检测到未保存的工作，可通过「文件 → 打开」恢复");
    }
  } catch {
    /* recovery check is best-effort */
  }

  // The Rust side emits `viewport_updated` after every regen — refresh the
  // tree (+ sketch data while editing) to keep UI in sync (原则2).
  unlisteners.push(
    await listen("viewport_updated", () => {
      refreshAll().catch((err) => console.error("refresh failed:", err));
    }),
  );
  // Clicking a body in the native viewport selects its feature here.
  unlisteners.push(
    await listen<{ id: number | null }>("feature_picked", (event) => {
      const id = event.payload?.id ?? null;
      if (id != null) {
        sketchStore.setActiveFeature(id);
        sketchStore.selectedFeatureId = id;
      }
    }),
  );
});

onUnmounted(() => {
  document.removeEventListener("click", onGlobalClick);
  unlisteners.forEach((fn) => fn());
  unlisteners = [];
});
</script>

<style scoped>
/*
 * Grid metrics MUST match the native renderer's PanelLayout
 * (wgpu_viewport.rs): it carves the 3D viewport out of the window using
 * the same widths/heights. The tokens live in style.css.
 */
.cad-layout {
  display: grid;
  grid-template-areas:
    "toolbar toolbar toolbar"
    "left viewport right"
    "status status status";
  grid-template-columns: var(--sidebar-left-width) 1fr var(--sidebar-right-width);
  grid-template-rows: var(--toolbar-height) 1fr var(--statusbar-height);
  width: 100%;
  height: 100%;
  background: var(--bg-app);
  color: var(--fg);
  overflow: hidden;
  outline: none;
}

.toolbar-area {
  grid-area: toolbar;
  overflow: visible; /* dropdown menus must escape the row */
  z-index: 20;
}

.sidebar-left {
  grid-area: left;
  background: var(--bg-panel);
  border-right: 1px solid var(--border);
  overflow-y: auto;
  overflow-x: hidden;
}

.sidebar-right {
  grid-area: right;
  background: var(--bg-panel);
  border-left: 1px solid var(--border);
  overflow-y: auto;
  overflow-x: hidden;
}

.plugins-section {
  border-top: 1px solid var(--border);
}
.plugin-item {
  margin-bottom: 10px;
  padding: 6px 8px;
  background: #2a2a2a;
  border-radius: var(--radius);
}
.plugin-header {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 2px;
}
.plugin-name {
  font-size: 12px;
  font-weight: 600;
}
.plugin-version {
  font-size: 10px;
  color: var(--fg-faint);
}
.plugin-desc {
  font-size: 11px;
  color: var(--fg-dim);
  margin-bottom: 6px;
}
.plugin-generator {
  font-size: 12px;
  color: var(--accent-outline);
  cursor: pointer;
  padding: 3px 0;
}
.plugin-generator:hover {
  color: #81d4fa;
}

.viewport-area {
  grid-area: viewport;
  position: relative;
  overflow: hidden;
  /* Matches the renderer clear color; becomes transparent once the macOS
     compositing issue is resolved and the surface shows through. */
  background: var(--bg-viewport);
}

.viewport-placeholder {
  position: absolute;
  inset: 0;
  pointer-events: none;
}

.statusbar {
  grid-area: status;
  background: var(--accent);
  color: #fff;
  display: flex;
  align-items: center;
  padding: 0 12px;
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
}
.status-text {
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
