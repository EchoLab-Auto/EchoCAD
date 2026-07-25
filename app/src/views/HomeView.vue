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
      @toggle-brep="toggleBrep"
      @toggle-measure="toggleMeasure"
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
        <UnifiedViewport ref="unifiedViewport" @faceSelected="onFaceSelected" @drag-end="onDragEnd" @edgeSelected="onEdgeSelected" @dimension-edited="loadState" />
      </main>

      <!-- Right sidebar: Properties -->
      <aside class="sidebar-right">
        <PropertiesPanel
          :regions="extrudeRegions"
          @rename="renameSelectedFeature"
          @update-param="updateParam"
          @update-entity-prop="updateEntityProp"
          @toggle-construction="toggleConstruction"
          @delete-entity="deleteSelectedEntity"
          @delete-constraint="deleteConstraint"
          @clear-constraints="clearAllConstraints"
          @extrude-change="onExtrudeConfigChange"
          @extrude-confirm="(sel: number[] | null) => doExtrude(sel)"
          @extrude-cancel="cancelExtrude"
          @extrude-region-change="onExtrudeRegionChange"
          @enter-edge-pick="onEnterEdgePick"
          @exit-edge-pick="cancelEdgePick"
          @confirm-edge-pick="confirmEdgePick"
          @appearance-changed="loadState"
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
import { computed, ref, onMounted, onUnmounted } from "vue";
import UnifiedViewport from "@/components/UnifiedViewport.vue";
import Toolbar, {
  type FileAction,
} from "@/components/Toolbar.vue";
import FeatureTree from "@/components/FeatureTree.vue";
import ContextMenu from "@/components/ContextMenu.vue";
import PropertiesPanel from "@/components/PropertiesPanel.vue";
import ToastContainer from "@/components/ToastContainer.vue";
import PluginDialog from "@/components/PluginDialog.vue";
import CascadeDeleteDialog from "@/components/CascadeDeleteDialog.vue";
import BooleanDialog from "@/components/BooleanDialog.vue";
import OffsetPlaneDialog, { type OffsetPlaneResult } from "@/components/OffsetPlaneDialog.vue";
import { useSketchStore } from "@/stores/sketch";
import { useToastStore } from "@/stores/toast";
import { useFeatureActions } from "@/composables/useFeatureActions";
import { useSketchActions } from "@/composables/useSketchActions";

import {
  getFeatures, getSketchEntities,
  updateParameter,
  clearDocument,
  saveProject, loadProject, loadProjectFrom, exportStl, exportObj, exportGltf,
  checkRecovery,
  getRecentFiles, clearRecentFiles,
  undo, redo, listPlugins, listGenerators,
  generatePluginFeature,
  createOffsetPlane,
  type ParameterId,
  type GeneratorInfo,
} from "@/commands/sketch";

const sketchStore = useSketchStore();
const toastStore = useToastStore();
const unifiedViewport = ref<InstanceType<typeof UnifiedViewport> | null>(null);
const toolbarRef = ref<InstanceType<typeof Toolbar> | null>(null);
const layoutRoot = ref<HTMLDivElement | null>(null);

// ── Composables ───────────────────────────────────────────────────

const {
  doExtrude, cancelExtrude, onExtrudeConfigChange, showExtrudePanel,
  extrudeRegions, onExtrudeRegionChange,
  revolve,
  confirmEdgePick, cancelEdgePick,
  booleanDialog, confirmBoolean, cancelBoolean,
  onFeatureAction, onPatternAction,
} = useFeatureActions(unifiedViewport);

const {
  addHorizontal, addVertical, addParallel,
  addDistance,
  onConstraintAction, solve,
  deleteConstraint, clearAllConstraints,
  updateEntityProp, deleteSelectedEntity, toggleConstruction,
  selectFeature, editFeature, enterSketchEdit, exitSketchEdit,
  newSketch, clearSketchAction,
  deleteFeature, confirmCascadeDelete, cancelCascade, cascadeDialog,
  toggleSuppress, renameSelectedFeature,
  onFeatureContextMenu, closeCtxMenu,
  ctxMenu, ctxFeatureSuppressed,
  ctxRename, ctxDelete, ctxEditSketch, ctxToggleSuppress, ctxFaceNormal,
} = useSketchActions(unifiedViewport, toolbarRef);

// ── Computed ──────────────────────────────────────────────────────

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
    const planeLabels: Record<string, string> = { xy: "XY", yz: "YZ", zx: "ZX" };
    const plane = planeLabels[sketchStore.activePlane] || sketchStore.activePlane;
    return `${toolLabels[sketchStore.activeTool] || sketchStore.activeTool} · 平面 ${plane} · ${sketchStore.entities.length} 实体 · ${sketchStore.constraints.length} 约束`;
  }
  const featureCount = sketchStore.features.length;
  return `3D 视口 · ${featureCount} 个特征 · 选择特征进行编辑`;
});

// ── Helpers ───────────────────────────────────────────────────────

function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

// ── Recent files ──────────────────────────────────────────────────

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
    resetPerDocumentState();
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

/// Sync sketch entities + constraints exactly once. The viewport's
/// refreshSketch fetches both AND re-renders; a bare fetch is the fallback
/// only before the viewport mounts.
async function refreshSketch() {
  if (unifiedViewport.value) {
    await unifiedViewport.value.refreshSketch();
  } else {
    sketchStore.setEntities(await getSketchEntities());
  }
}

async function refreshPlugins() {
  const [plugins, generators] = await Promise.all([listPlugins(), listGenerators()]);
  sketchStore.setPlugins(plugins);
  sketchStore.setGenerators(generators);
}

async function loadState() {
  sketchStore.isLoading = true;
  try {
    await refreshFeatures();
    await refreshSketch();
  } finally {
    sketchStore.isLoading = false;
  }
}

// ── Viewport callbacks ────────────────────────────────────────────

function onFaceSelected(featureId: number, _faceIndex: number) {
  sketchStore.selectedFeatureId = featureId;
}

function onDragEnd() {
  unifiedViewport.value?.refreshViewport();
}

function onEdgeSelected(_featureId: number, _vA: number, _vB: number) {
  // Edge selection is handled inside UnifiedViewport via store.addPickedEdge().
  // This callback exists for potential future use (logging, analytics, etc.).
}

function toggleMeasure() {
  sketchStore.toggleMeasure();
}

async function toggleBrep() {
  await sketchStore.toggleBrep();
  // B-rep toggle triggers a full backend regen — feature errors can change
  // (a feature may fail on one path and succeed on the other), so the
  // feature tree must re-sync too (原则2).
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

/// Enter edge-pick mode from the PropertiesPanel (when editing an existing
/// Fillet/Chamfer feature). Uses the feature's recorded target solid —
/// never a positional guess, which can silently pick the wrong solid when
/// the tree has multiple solids (原则1; target_id exposed per 原则14).
function onEnterEdgePick() {
  const f = sketchStore.selectedFeature;
  if (!f) return;
  const type = f.feature_type === "Fillet" ? "fillet" : "chamfer";
  const targetId = f.target_id ?? null;
  if (targetId === null) {
    toastStore.info("找不到目标实体");
    return;
  }
  sketchStore.enterEdgePickMode(type, targetId);
}

// ── Parameter editing (PropertiesPanel) ──────────────────────────

async function updateParam(id: ParameterId, event: Event) {
  if (id === 0) return;
  const value = parseFloat((event.target as HTMLInputElement).value);
  if (Number.isNaN(value)) return;
  await updateParameter(id, value);
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

// ── Document operations ───────────────────────────────────────────

/// Reset all per-document UI state when the document is REPLACED
/// (new/open/undo/redo). Backend feature ids restart from 1, so any state
/// keyed by FeatureId (appearance overrides, selection, editing session)
/// would alias onto unrelated features of the new document (原则1/2.1).
function resetPerDocumentState() {
  sketchStore.selectedFeatureId = null;
  sketchStore.clearAppearanceOverrides();
  // Exit any sketch-edit session — the user never chose to edit the new
  // document's same-id sketch.
  sketchStore.setEditingSketch(null);
  sketchStore.setTool("select");
  sketchStore.select(null);
}

async function clearDoc() {
  await clearDocument();
  resetPerDocumentState();
  const features = await getFeatures();
  const firstSketch = features.find(f => f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"));
  if (firstSketch) sketchStore.setActiveFeature(firstSketch.id);
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

// ── File operations ──────────────────────────────────────────────

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
    resetPerDocumentState();
    const features = await getFeatures();
    const firstSketch = features.find(f => f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"));
    if (firstSketch) sketchStore.setActiveFeature(firstSketch.id);
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    await refreshRecent();
    toastStore.success("项目已加载");
  } catch (err) {
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
    resetPerDocumentState();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  }
}

async function redoAction() {
  if (await redo()) {
    resetPerDocumentState();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  }
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
    await enterSketchEdit(id);
    await unifiedViewport.value?.refreshViewport();
    toastStore.success("偏移平面已创建");
  } catch (err) {
    toastStore.error(`偏移平面创建失败: ${err}`);
  }
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

// ── Global clicks & keyboard ─────────────────────────────────────

function onGlobalClick() {
  toolbarRef.value?.closeAllMenus();
  closeCtxMenu();
}

async function onKeyDown(e: KeyboardEvent) {
  const tag = (e.target as HTMLElement).tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") return;

  if (e.ctrlKey && e.key === "s") { e.preventDefault(); saveProjectFile(); return; }
  if (e.ctrlKey && e.key === "o") { e.preventDefault(); loadProjectFile(); return; }
  if (e.ctrlKey && e.key === "z") { e.preventDefault(); undoAction(); return; }
  if (e.ctrlKey && (e.key === "y" || (e.key === "Z" && e.shiftKey))) {
    e.preventDefault(); redoAction(); return;
  }
  if (e.ctrlKey && e.key === "0") { e.preventDefault(); unifiedViewport.value?.fitView(); return; }

  // Space: fit view (only when not in input fields)
  if (e.key === " " && !isEditingSketch.value) {
    e.preventDefault();
    unifiedViewport.value?.fitView();
    return;
  }

  // Feature shortcuts work whenever a sketch is selected (edit mode not required).
  switch (e.key) {
    case "e": case "E": showExtrudePanel(); return;
    case "w": case "W": revolve(); return;
    case "f": case "F": onFeatureAction("fillet"); return;
    case "n": case "N": newSketch(); return;
  }

  if (!isEditingSketch.value) {
    // Outside sketch mode: Delete key deletes selected feature
    if ((e.key === "Delete" || e.key === "Backspace") && sketchStore.selectedFeatureId !== null) {
      e.preventDefault();
      const f = sketchStore.features.find(x => x.id === sketchStore.selectedFeatureId);
      if (f) await deleteFeature(f.id);
      return;
    }
    return;
  }

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
    case "Escape":
      // First Escape: deselect / switch to select tool.
      // Second Escape (when already in select mode): exit sketch editing.
      if (sketchStore.activeTool === "select" && sketchStore.selectedId === null) {
        exitSketchEdit();
      } else {
        sketchStore.setTool("select");
      }
      break;
  }
}

// ── Lifecycle ─────────────────────────────────────────────────────

onMounted(async () => {
  await loadState();
  await sketchStore.initBrep();
  await refreshPlugins();
  await refreshRecent();
  layoutRoot.value?.focus();
  document.addEventListener("click", onGlobalClick);

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
