<template>
  <div class="main-layout" @keydown="onKeyDown" tabindex="0" ref="layoutRoot">
    <header class="toolbar">
      <span class="title">EchoCAD</span>

      <!-- File menu -->
      <div class="tb-menu">
        <button class="tb-menu-btn" :class="{ open: openMenuId==='file' }" @click.stop="toggleMenu('file')">文件 ▾</button>
        <div v-if="openMenuId==='file'" class="tb-dropdown" @click.stop>
          <button v-for="mi in menuFile" :key="mi.label" class="tb-dd-item" @click="mi.action(); closeAllMenus()">
            {{ mi.label }}<span class="mm-key">{{ mi.key }}</span>
          </button>
          <div v-if="recentFiles.length > 0" class="recent-section">
            <div class="recent-header">最近打开</div>
            <button
              v-for="(path, i) in recentFiles"
              :key="path"
              class="tb-dd-item recent-item"
              :title="path"
              @click="openRecent(path); closeAllMenus()"
            >
              <span class="recent-name">{{ basename(path) }}</span>
              <span v-if="i === 0" class="mm-key">最近</span>
            </button>
            <button class="tb-dd-item recent-clear" @click="clearRecent(); closeAllMenus()">
              清空列表
            </button>
          </div>
        </div>
      </div>

      <!-- Edit menu -->
      <div class="tb-menu">
        <button class="tb-menu-btn" :class="{ open: openMenuId==='edit' }" @click.stop="toggleMenu('edit')">编辑 ▾</button>
        <div v-if="openMenuId==='edit'" class="tb-dropdown" @click.stop>
          <button v-for="mi in menuEdit" :key="mi.label" class="tb-dd-item" @click="mi.action(); closeAllMenus()">
            {{ mi.label }}<span class="mm-key">{{ mi.key }}</span>
          </button>
        </div>
      </div>

      <div class="tb-sep"></div>

      <!-- Quick feature buttons (SolidWorks-like) -->
      <template v-if="!isEditingSketch">
        <button class="tb-quick-btn" @click="showExtrudePanel" title="拉伸 (E)">🧊 拉伸</button>
        <button class="tb-quick-btn" @click="revolve" title="旋转 (W)">🔄 旋转</button>
        <button class="tb-quick-btn" @click="addSweep" title="扫描">〰️ 扫描</button>
        <button class="tb-quick-btn" @click="addFillet" title="圆角">🔵 圆角</button>
        <button class="tb-quick-btn" @click="addChamfer" title="倒角">🔻 倒角</button>
        <button class="tb-quick-btn" @click="addShell" title="抽壳">🫙 抽壳</button>
      </template>

      <!-- Sketch tools — shown when editing a sketch -->
      <template v-if="isEditingSketch">
        <div class="tb-menu">
          <button class="tb-menu-btn" :class="{ open: openMenuId==='sketch' }" @click.stop="toggleMenu('sketch')">草图 ▾</button>
          <div v-if="openMenuId==='sketch'" class="tb-dropdown" @click.stop>
            <div class="mm-grid">
              <button v-for="t in sketchTools" :key="t.value"
                :class="{ active: sketchStore.activeTool === t.value }"
                @click="sketchStore.setTool(t.value); closeAllMenus()">
                {{ t.label }}<span class="mm-key">{{ t.shortcut }}</span>
              </button>
            </div>
          </div>
        </div>

        <div class="tb-menu">
          <button class="tb-menu-btn" :class="{ open: openMenuId==='constraints' }" @click.stop="toggleMenu('constraints')">约束 ▾</button>
          <div v-if="openMenuId==='constraints'" class="tb-dropdown" @click.stop>
            <div class="mm-grid mm-2col">
              <button v-for="c in constraintItems" :key="c.label" @click="c.action(); closeAllMenus()">{{ c.label }}</button>
            </div>
          </div>
        </div>
        <div class="tb-sep"></div>
        <button class="tb-quick-btn" @click="showExtrudePanel" title="拉伸 (E)">🧊 拉伸</button>
        <button class="tb-quick-btn" @click="revolve" title="旋转 (W)">🔄 旋转</button>
        <button class="tb-exit-sketch" @click="exitSketchEdit" title="退出草图编辑模式">✕ 退出草图</button>
      </template>

      <!-- Features -->
      <div class="tb-menu">
        <button class="tb-menu-btn" :class="{ open: openMenuId==='features' }" @click.stop="toggleMenu('features')">特征 ▾</button>
        <div v-if="openMenuId==='features'" class="tb-dropdown" @click.stop>
          <div class="mm-grid mm-2col">
            <button v-for="f in featureItems" :key="f.label" @click="f.action(); closeAllMenus()">
              {{ f.label }}<span class="mm-key">{{ f.key }}</span>
            </button>
          </div>
        </div>
      </div>

      <!-- Pattern -->
      <div class="tb-menu">
        <button class="tb-menu-btn" :class="{ open: openMenuId==='pattern' }" @click.stop="toggleMenu('pattern')">阵列 ▾</button>
        <div v-if="openMenuId==='pattern'" class="tb-dropdown" @click.stop>
          <div class="mm-list">
            <button v-for="p in patternItems" :key="p.label" @click="p.action(); closeAllMenus()">{{ p.label }}</button>
          </div>
        </div>
      </div>

      <div class="tb-sep"></div>

      <!-- Plugin -->
      <div v-if="sketchStore.generators.length>0" class="tb-menu">
        <button class="tb-menu-btn" :class="{ open: openMenuId==='plugins' }" @click.stop="toggleMenu('plugins')">插件 ▾</button>
        <div v-if="openMenuId==='plugins'" class="tb-dropdown" @click.stop>
          <div class="mm-list">
            <button v-for="g in sketchStore.generators" :key="g.id"
              @click="openPluginDialog(g); closeAllMenus()">{{ g.name }}</button>
          </div>
        </div>
      </div>

      <!-- Plane selector (disabled while editing — plane is fixed per sketch) -->
      <select
        v-model="sketchStore.activePlane"
        class="tb-plane"
        :disabled="isEditingSketch"
        :title="isEditingSketch ? '草图平面在创建时确定，编辑中不可修改' : '新草图的基准平面'"
      >
        <option v-for="p in planes" :key="p.value" :value="p.value">{{ p.label }}</option>
      </select>

      <div class="tb-spacer"></div>

      <button class="tb-icon-btn" @click="undoAction" title="撤销 Ctrl+Z">↩</button>
      <button class="tb-icon-btn" @click="redoAction" title="重做 Ctrl+Y">↪</button>
    </header>

    <div class="workspace">
      <!-- Left sidebar: Feature tree -->
      <aside class="sidebar-left">
        <div class="panel">
          <h3>特征树
            <span v-if="regenErrorCount > 0" class="err-badge" :title="`${regenErrorCount} 个特征失败`">
              {{ regenErrorCount }} 错误
            </span>
          </h3>
          <ul class="feature-tree">
            <li
              v-for="feature in sketchStore.features"
              :key="feature.id"
              :class="{
                active: sketchStore.activeFeatureId === feature.id,
                selected: selectedFeature?.id === feature.id,
                suppressed: feature.suppressed,
                errored: !!feature.errors,
              }"
              @click="selectFeature(feature.id)"
              @dblclick="editFeature(feature)"
              @contextmenu="onFeatureContextMenu($event, feature.id)"
            >
              <button class="suppress-btn" :title="feature.suppressed ? '取消抑制' : '抑制特征'"
                @click.stop="toggleSuppress(feature)">
                {{ feature.suppressed ? '◌' : '●' }}
              </button>
              <span class="fi-arrow" v-if="feature.feature_type!=='Sketch' && !feature.feature_type.startsWith('Custom:')">└</span>
              <span class="feature-icon">{{ featureIcon(feature.feature_type) }}</span>
              <span class="fi-name">{{ feature.name }}</span>
              <span v-if="feature.errors" class="err-icon" :title="feature.errors">⚠</span>
              <button class="delete-btn" @click.stop="deleteFeature(feature.id)" title="删除">×</button>
            </li>
          </ul>
          <!-- Context menu -->
          <div v-if="ctxMenu" class="ctx-menu" :style="{ left: ctxMenu.x+'px', top: ctxMenu.y+'px' }" @click.stop>
            <button @click="ctxEditSketch">编辑草图</button>
            <button @click="ctxFaceNormal">正视于草图</button>
            <button @click="ctxRename">重命名</button>
            <button @click="ctxToggleSuppress">{{ ctxFeatureSuppressed ? '取消抑制' : '抑制' }}</button>
            <button @click="ctxDelete" class="ctx-danger">删除</button>
          </div>
        </div>

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
        <UnifiedViewport ref="unifiedViewport" @faceSelected="onFaceSelected" />
      </main>

      <!-- Right sidebar: Properties -->
      <aside class="sidebar-right">
        <div class="panel">
          <h3>属性</h3>

          <!-- Extrude configuration panel -->
          <ExtrudePanel
            v-if="sketchStore.showExtrudePanel"
            :direction="sketchStore.extrudeConfig.direction"
            :depth="sketchStore.extrudeConfig.depth"
            :dist2="sketchStore.extrudeConfig.dist2"
            :draft="sketchStore.extrudeConfig.draft"
            @change="onExtrudeConfigChange"
            @confirm="doExtrude"
            @cancel="cancelExtrude"
          />

          <!-- Feature properties -->
          <div v-else-if="selectedFeature" class="prop-section">
            <div class="prop-row"><label>类型</label><span>{{ selectedFeature.feature_type }}</span></div>
            <div class="prop-row">
              <label>名称</label>
              <input type="text" :value="selectedFeature.name"
                @change="renameSelectedFeature(($event.target as HTMLInputElement).value)" />
            </div>
            <div v-if="selectedFeature.errors" class="prop-error">
              ⚠ {{ selectedFeature.errors }}
            </div>
            <div v-for="param in editableParameters(selectedFeature)" :key="param.id + '-' + param.name" class="prop-row">
              <label>{{ param.name }}</label>
              <input v-if="!param.readonly" type="number" step="0.1" :value="param.value"
                @change="updateParam(param.id, $event)" />
              <span v-else class="ro-val">{{ param.value.toFixed(2) }}</span>
            </div>
            <div class="prop-row" v-if="selectedFeature.has_dependents">
              <label></label>
              <span class="has-dep" title="其他特征依赖于此特征">⚠ 有依赖</span>
            </div>
          </div>

          <!-- Entity properties -->
          <div v-else-if="selectedEntity" class="prop-section">
            <div class="prop-row"><label>类型</label><span>{{ selectedEntity.type }}</span></div>
            <div class="prop-row"><label>ID</label><span>{{ selectedEntity.id }}</span></div>
            <div v-if="selectedEntity.type==='Point'" class="prop-row">
              <label>X</label>
              <input type="number" step="0.1" :value="(selectedEntity as any).x"
                @change="updateEntityProp(selectedEntity.id, 'x', $event)" />
            </div>
            <div v-if="selectedEntity.type==='Point'" class="prop-row">
              <label>Y</label>
              <input type="number" step="0.1" :value="(selectedEntity as any).y"
                @change="updateEntityProp(selectedEntity.id, 'y', $event)" />
            </div>
            <div v-if="selectedEntity.type==='Circle' || selectedEntity.type==='Arc'" class="prop-row">
              <label>半径</label>
              <input type="number" step="0.1" :value="(selectedEntity as any).radius"
                @change="updateEntityProp(selectedEntity.id, 'radius', $event)" />
            </div>
            <div v-if="selectedEntity.type==='Line'" class="prop-row">
              <label>长度</label><span>{{ lineLength(selectedEntity as any).toFixed(2) }}</span>
            </div>
            <label v-if="selectedEntity && (selectedEntity.type==='Line' || selectedEntity.type==='Circle')" class="prop-check">
              <input type="checkbox" :checked="!!(selectedEntity as any).construction"
                @change="toggleConstruction(selectedEntity.id, $event)" />
              构造线
            </label>
            <button v-if="selectedEntity" class="prop-del-btn" @click="deleteSelectedEntity">删除实体 (Del)</button>
          </div>

          <p v-else class="placeholder">选择特征或草图实体</p>
        </div>
      </aside>
    </div>

    <footer class="statusbar">
      <span class="status-text">{{ statusText }}</span>
    </footer>

    <!-- Toast notifications -->
    <div class="toast-container">
      <div v-for="toast in toasts" :key="toast.id" :class="['toast', 'toast-' + toast.type]">
        {{ toast.message }}
      </div>
    </div>

    <!-- Plugin generator dialog -->
    <div v-if="activeDialog" class="dialog-overlay" @click.self="closeDialog">
      <div class="dialog">
        <h3>{{ activeDialog.name }}</h3>
        <p class="dialog-desc">{{ activeDialog.description }}</p>
        <div v-for="param in activeDialog.parameters" :key="param.id" class="dialog-param">
          <label :title="param.description">{{ param.name }}</label>
          <input type="number" :min="param.min ?? undefined" :max="param.max ?? undefined"
            :step="param.step" v-model.number="dialogParams[param.id]" />
        </div>
        <div class="dialog-actions">
          <button class="btn-primary" @click="runGenerator" :disabled="dialogLoading">
            <span v-if="dialogLoading" class="btn-spinner"></span>
            {{ dialogLoading ? '生成中...' : '生成' }}
          </button>
          <button class="btn-cancel" @click="closeDialog" :disabled="dialogLoading">取消</button>
        </div>
      </div>
    </div>

    <!-- Confirm cascade delete dialog -->
    <div v-if="cascadeDialog" class="dialog-overlay" @click.self="cancelCascade">
      <div class="dialog">
        <h3>删除 "{{ cascadeDialog.name }}"？</h3>
        <p class="dialog-desc">
          该特征有 {{ cascadeDialog.dependents.length }} 个依赖项。删除它将同时删除：
          <strong>{{ cascadeDialog.dependents.join(', ') }}</strong>
        </p>
        <div class="dialog-actions">
          <button class="btn-danger" @click="confirmCascadeDelete">级联删除</button>
          <button class="btn-cancel" @click="cancelCascade">取消</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, nextTick, onMounted, onUnmounted } from "vue";
import UnifiedViewport from "@/components/UnifiedViewport.vue";
import ExtrudePanel from "@/components/ExtrudePanel.vue";
import { useSketchStore } from "@/stores/sketch";

// ── Dropdown menu state ──────────────────────────────────────────

const openMenuId = ref<string | null>(null);
function toggleMenu(id: string) {
  openMenuId.value = openMenuId.value === id ? null : id;
}
function closeAllMenus() {
  openMenuId.value = null;
}

interface MenuItem { label: string; action: () => void; key?: string; }

const menuFile: MenuItem[] = [
  { label: "新建", action: () => { clearDoc(); }, key: "Ctrl+N" },
  { label: "打开", action: () => { loadProjectFile(); }, key: "Ctrl+O" },
  { label: "保存", action: () => { saveProjectFile(); }, key: "Ctrl+S" },
  { label: "另存为...", action: () => { saveProjectFile(); } },
  { label: "导出 STL", action: () => { exportStlFile(); } },
  { label: "导出 OBJ", action: () => { exportObjFile(); } },
];

const recentFiles = ref<string[]>([]);

async function refreshRecent() {
  try {
    recentFiles.value = await getRecentFiles();
  } catch {
    recentFiles.value = [];
  }
}

function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

async function openRecent(path: string) {
  try {
    await loadProjectFrom(path);
    selectedFeatureId.value = null;
    const features = await getFeatures();
    const firstSketch = features.find(f => f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"));
    if (firstSketch) sketchStore.setActiveFeature(firstSketch.id);
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    showToast("success", `已加载: ${basename(path)}`);
  } catch (err) {
    showToast("error", `加载失败: ${err}`);
  }
}

async function clearRecent() {
  await clearRecentFiles();
  recentFiles.value = [];
}

const menuEdit: MenuItem[] = [
  { label: "撤销", action: () => { undoAction(); }, key: "Ctrl+Z" },
  { label: "重做", action: () => { redoAction(); }, key: "Ctrl+Y" },
  { label: "新草图", action: () => { newSketch(); }, key: "N" },
  { label: "清空文档", action: () => { clearDoc(); } },
];

const constraintItems = [
  { label: "水平", action: addHorizontal }, { label: "竖直", action: addVertical },
  { label: "平行", action: addParallel }, { label: "垂直", action: addPerpendicular },
  { label: "相切", action: addTangent }, { label: "同心", action: addConcentric },
  { label: "相等", action: addEqual }, { label: "中点", action: addMidpoint },
  { label: "固定", action: addFixPoint }, { label: "角度", action: addAngle },
  { label: "直径", action: addDiameter }, { label: "距离", action: addDistance },
  { label: "求解", action: solve },
];

const featureItems = [
  { label: "拉伸", action: showExtrudePanel, key: "E" },
  { label: "旋转", action: revolve, key: "W" },
  { label: "扫描", action: addSweep },
  { label: "圆角", action: addFillet },
  { label: "倒角", action: addChamfer },
  { label: "抽壳", action: addShell },
  { label: "布尔运算", action: addBoolean },
];

const patternItems = [
  { label: "线性阵列", action: addLinearPattern },
  { label: "圆周阵列", action: addCircularPattern },
  { label: "镜像", action: addMirror },
];

import {
  addConstraint, solveSketch,
  updateEntityProp as updateEntityPropCmd,
  deleteEntity as deleteEntityCmd,
  getSketchEntities, clearDocument,
  getFeatures, setActiveSketch,
  addSketchFeature, deleteFeature as deleteFeatureCmd,
  renameFeature as renameFeatureCmd,
  setFeatureSuppressed as setFeatureSuppressedCmd,
  updateParameter, addExtrudeFeature,
  addRevolveFeature, addFilletFeature, addChamferFeature,
  addLinearPattern as addLinearPatternCmd,
  addCircularPattern as addCircularPatternCmd,
  addMirrorFeature, addSweepFeature, addShellFeature,
  addBooleanFeature as addBooleanFeatureCmd,
  previewExtrude,
  saveProject, loadProject, loadProjectFrom, exportStl, exportObj,
  getRecentFiles, clearRecentFiles,
  undo, redo, listPlugins, listGenerators,
  generatePluginFeature,
  type EntityId, type FeatureId, type FeatureNode,
  type ParameterId, type GeneratorInfo,
} from "@/commands/sketch";

const sketchStore = useSketchStore();
const unifiedViewport = ref<InstanceType<typeof UnifiedViewport> | null>(null);
const layoutRoot = ref<HTMLDivElement | null>(null);

// Derive selectedFeature from the store so it always reflects the latest state.
// We track an explicit "selection" separate from `activeFeatureId` because the
// user can select a solid for inspection while a sketch is still being edited.
const selectedFeatureId = ref<FeatureId | null>(null);
const selectedFeature = computed<FeatureNode | null>(() => {
  if (selectedFeatureId.value === null) return null;
  return sketchStore.features.find(f => f.id === selectedFeatureId.value) ?? null;
});

// Toast notifications
interface Toast { id: number; type: "info" | "success" | "error"; message: string; }
const toasts = ref<Toast[]>([]);
let toastId = 0;

function showToast(type: Toast["type"], message: string, duration = 3000) {
  const id = ++toastId;
  toasts.value.push({ id, type, message });
  setTimeout(() => {
    toasts.value = toasts.value.filter(t => t.id !== id);
  }, duration);
}

const sketchTools = [
  { value: "select" as const, label: "选择", shortcut: "Esc" },
  { value: "line" as const, label: "直线", shortcut: "L" },
  { value: "circle" as const, label: "圆", shortcut: "C" },
  { value: "arc" as const, label: "弧线", shortcut: "A" },
  { value: "rectangle" as const, label: "矩形", shortcut: "R" },
  { value: "spline" as const, label: "样条", shortcut: "B" },
  { value: "ellipse" as const, label: "椭圆", shortcut: "I" },
];

const isEditingSketch = computed(() => sketchStore.isEditingSketch());

const selectedEntity = computed(() => {
  if (!sketchStore.selectedId) return null;
  return sketchStore.entities.find(e => e.id === sketchStore.selectedId) || null;
});

const regenErrorCount = computed(() =>
  sketchStore.features.filter(f => f.errors).length
);

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

const planes = [
  { value: "xy", label: "XY 平面" },
  { value: "yz", label: "YZ 平面" },
  { value: "zx", label: "ZX 平面" },
];

function featureIcon(type: string): string {
  if (type === "Sketch") return "📐";
  if (type === "Extrude") return "🧊";
  if (type === "Revolve") return "🔄";
  if (type === "Fillet") return "🔵";
  if (type === "Chamfer") return "🔻";
  if (type === "LinearPattern") return "↔️";
  if (type === "CircularPattern") return "🔁";
  if (type === "Mirror") return "🪞";
  if (type === "Sweep") return "〰️";
  if (type === "Shell") return "🫙";
  if (type.startsWith("Custom:") || type.startsWith("CustomSolid:")) {
    const lower = type.toLowerCase();
    if (lower.includes("gear") || lower.includes("spur")) return "⚙️";
    if (lower.includes("spring")) return "🌀";
    return "🔧";
  }
  return "📦";
}

/// Parameters with a non-zero `id` are real (linked to a ParameterId).
/// Parameters with id=0 are virtual (e.g., display-only count, or non-editable).
function editableParameters(f: FeatureNode) {
  return f.parameters;
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
  await refreshFeatures();
  await refreshSketch();
}

// ── Feature tree ─────────────────────────────────────────────────

/**
 * Left-click selection: highlight in the tree and show properties.
 * Does NOT enter sketch edit mode — that's only via double-click or the
 * right-click "编辑草图" menu (SolidWorks behavior).
 */
async function selectFeature(id: FeatureId) {
  selectedFeatureId.value = id;
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

  selectedFeatureId.value = id;
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
  selectedFeatureId.value = featureId;
}

async function toggleSuppress(feature: FeatureNode) {
  await setFeatureSuppressedCmd(feature.id, !feature.suppressed);
  await loadState();
  await unifiedViewport.value?.refreshViewport();
}

async function renameSelectedFeature(name: string) {
  if (!selectedFeature.value) return;
  if (!name || name === selectedFeature.value.name) return;
  await renameFeatureCmd(selectedFeature.value.id, name);
  await loadState();
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
    if (selectedFeature.value?.id === id) selectedFeatureId.value = null;
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    showToast("success", `已删除 "${feature.name}"`);
  } catch (err: any) {
    // Parse dependent names from the error message
    const msg = String(err);
    const dependentNames = msg.startsWith("dependents:")
      ? msg.slice("dependents:".length).split(",").filter(Boolean)
      : [];
    cascadeDialog.value = { id, name: feature.name, dependents: dependentNames };
  }
}

interface CascadeDialog { id: FeatureId; name: string; dependents: string[] }
const cascadeDialog = ref<CascadeDialog | null>(null);

async function confirmCascadeDelete() {
  if (!cascadeDialog.value) return;
  const { id, name } = cascadeDialog.value;
  cascadeDialog.value = null;
  try {
    await deleteFeatureCmd(id, true);
    if (selectedFeature.value?.id === id) selectedFeatureId.value = null;
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    showToast("success", `已级联删除 "${name}" 及其依赖项`);
  } catch (err: any) {
    showToast("error", `删除失败: ${err}`);
  }
}

function cancelCascade() {
  cascadeDialog.value = null;
}

async function clearDoc() {
  await clearDocument();
  selectedFeatureId.value = null;
  const features = await getFeatures();
  const firstSketch = features.find(f => f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"));
  if (firstSketch) sketchStore.setActiveFeature(firstSketch.id);
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
    showToast("info", "请先选择一个草图");
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
    showToast("success", "拉伸特征已创建");
  } catch (err) {
    showToast("error", `拉伸失败: ${err}`);
  }
}

// ── Other features ───────────────────────────────────────────────

/// Wrap a feature-creation call: shows toast on error, refreshes state on success.
async function withFeatureAction<T>(label: string, fn: () => Promise<T>): Promise<T | null> {
  try {
    const result = await fn();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    showToast("success", `${label}已创建`);
    return result;
  } catch (err) {
    showToast("error", `${label}失败: ${err}`);
    return null;
  }
}

async function revolve() {
  const activeId = activeSketchForFeature();
  if (activeId === null) {
    showToast("info", "请先选择一个草图");
    return;
  }
  const axisId = sketchStore.selectedId ?? null;
  await withFeatureAction("旋转特征", () => addRevolveFeature(activeId, axisId));
}

async function addFillet() {
  const tid = lastSolidFeatureId();
  if (tid === null) {
    showToast("info", "请先创建一个实体");
    return;
  }
  await withFeatureAction("圆角特征", () => addFilletFeature(tid, 0.5));
}

async function addChamfer() {
  const tid = lastSolidFeatureId();
  if (tid === null) {
    showToast("info", "请先创建一个实体");
    return;
  }
  await withFeatureAction("倒角特征", () => addChamferFeature(tid, 0.5));
}

function lastSolidFeatureId(): FeatureId | null {
  const solids = sketchStore.features.filter(f =>
    !f.suppressed && (
      ["Extrude", "Revolve", "Fillet", "Chamfer", "LinearPattern", "CircularPattern", "Mirror", "Sweep", "Shell"].includes(f.feature_type) ||
      f.feature_type.startsWith("CustomSolid")
    )
  );
  return solids.length > 0 ? solids[solids.length - 1].id : null;
}

async function addLinearPattern() {
  const tid = lastSolidFeatureId();
  if (tid === null) {
    showToast("info", "请先创建一个实体");
    return;
  }
  await withFeatureAction("线性阵列", () => addLinearPatternCmd(tid, 1.0, 0.0, 0.0, 3, 2.0));
}

async function addCircularPattern() {
  const tid = lastSolidFeatureId();
  if (tid === null) {
    showToast("info", "请先创建一个实体");
    return;
  }
  await withFeatureAction("圆周阵列", () => addCircularPatternCmd(tid, 0, 0, 0, 0, 0, 1, 4, 360));
}

async function addMirror() {
  const tid = lastSolidFeatureId();
  if (tid === null) {
    showToast("info", "请先创建一个实体");
    return;
  }
  await withFeatureAction("镜像特征", () => addMirrorFeature(tid, 1, 0, 0, 0, 0, 0));
}

async function addSweep() {
  const activeId = activeSketchForFeature();
  if (activeId === null) {
    showToast("info", "请先选择一个草图");
    return;
  }
  const otherSketches = sketchStore.features.filter(f => f.feature_type === "Sketch" && f.id !== activeId);
  if (otherSketches.length === 0) {
    showToast("info", "需要两个草图：轮廓 + 路径");
    return;
  }
  await withFeatureAction("扫描特征", () => addSweepFeature(activeId, otherSketches[0].id));
}

async function addShell() {
  const tid = lastSolidFeatureId();
  if (tid === null) {
    showToast("info", "请先创建一个实体");
    return;
  }
  await withFeatureAction("抽壳特征", () => addShellFeature(tid, 0.5));
}

async function addBoolean() {
  const solids = sketchStore.features.filter(f =>
    ["Extrude", "Revolve", "Fillet", "Chamfer", "Sweep", "Shell"].includes(f.feature_type)
  );
  if (solids.length < 2) {
    showToast("info", "布尔运算需要两个实体");
    return;
  }
  await withFeatureAction("布尔特征", () => addBooleanFeatureCmd(solids[0].id, solids[1].id, "union"));
}

// ── File operations ──────────────────────────────────────────────

async function saveProjectFile() {
  try {
    const path = await saveProject();
    showToast("success", `已保存: ${basename(path)}`);
    await refreshRecent();
  } catch (err) {
    if (String(err).includes("No file selected")) return;
    showToast("error", `保存失败: ${err}`);
  }
}

async function loadProjectFile() {
  try {
    await loadProject();
    selectedFeatureId.value = null;
    const features = await getFeatures();
    const firstSketch = features.find(f => f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"));
    if (firstSketch) sketchStore.setActiveFeature(firstSketch.id);
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    await refreshRecent();
    showToast("success", "项目已加载");
  } catch (err) {
    // Don't error if user just cancelled the file picker
    if (String(err).includes("No file selected")) return;
    showToast("error", `加载失败: ${err}`);
  }
}

async function exportStlFile() {
  try {
    const path = await exportStl();
    showToast("success", `已导出 STL: ${path}`);
  } catch (err) {
    if (String(err).includes("No file selected")) return;
    showToast("error", `STL 导出失败: ${err}`);
  }
}

async function exportObjFile() {
  try {
    const path = await exportObj();
    showToast("success", `已导出 OBJ: ${path}`);
  } catch (err) {
    if (String(err).includes("No file selected")) return;
    showToast("error", `OBJ 导出失败: ${err}`);
  }
}

async function undoAction() {
  if (await undo()) {
    selectedFeatureId.value = null;
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  }
}

async function redoAction() {
  if (await redo()) {
    selectedFeatureId.value = null;
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
    showToast("info", "请先选择一条线");
    return;
  }
  await addConstraint({ Horizontal: { line: id } });
  await solveSketch();
  await refreshSketch();
}

async function addVertical() {
  const id = sketchStore.selectedId;
  if (id === null) {
    showToast("info", "请先选择一条线");
    return;
  }
  await addConstraint({ Vertical: { line: id } });
  await solveSketch();
  await refreshSketch();
}

async function addParallel() {
  const ids = pickTwoLines();
  if (!ids) { showToast("info", "需要两条线"); return; }
  await addConstraint({ Parallel: { line_a: ids[0], line_b: ids[1] } });
  await solveSketch(); await refreshSketch();
}

async function addPerpendicular() {
  const ids = pickTwoLines();
  if (!ids) { showToast("info", "需要两条线"); return; }
  await addConstraint({ Perpendicular: { line_a: ids[0], line_b: ids[1] } });
  await solveSketch(); await refreshSketch();
}

async function addTangent() {
  const ids = pickLineAndCircle();
  if (!ids) { showToast("info", "需要一条线和一个圆/弧"); return; }
  await addConstraint({ Tangent: { line: ids[0], circle: ids[1] } });
  await solveSketch(); await refreshSketch();
}

async function addConcentric() {
  const ids = pickTwoCircles();
  if (!ids) { showToast("info", "需要两个圆/弧"); return; }
  await addConstraint({ Concentric: { a: ids[0], b: ids[1] } });
  await solveSketch(); await refreshSketch();
}

async function addEqual() {
  const ids = pickTwoLines() ?? pickTwoCircles();
  if (!ids) { showToast("info", "需要两个同类实体"); return; }
  await addConstraint({ Equal: { a: ids[0], b: ids[1] } });
  await solveSketch(); await refreshSketch();
}

async function addMidpoint() {
  const ids = pickPointAndLine();
  if (!ids) { showToast("info", "需要一个点 + 一条线"); return; }
  await addConstraint({ Midpoint: { point: ids[0], line: ids[1] } });
  await solveSketch(); await refreshSketch();
}

async function addFixPoint() {
  const id = sketchStore.selectedId;
  if (id === null) {
    showToast("info", "请先选择一个点");
    return;
  }
  await addConstraint({ Fix: { point: id } });
  await refreshSketch();
}

async function addAngle() {
  const ids = pickTwoLines();
  if (!ids) { showToast("info", "需要两条线"); return; }
  await addConstraint({ Angle: { line_a: ids[0], line_b: ids[1], angle_deg: 90.0 } });
  await solveSketch(); await refreshSketch();
}

async function addDiameter() {
  const circles = sketchStore.entities.filter(e => e.type === "Circle" || e.type === "Arc");
  if (circles.length === 0) { showToast("info", "需要圆/弧"); return; }
  const sel = sketchStore.selectedId;
  const target = (sel !== null && circles.some(c => c.id === sel))
    ? circles.find(c => c.id === sel)!
    : circles[0];
  const r = target.type === "Circle" ? target.radius : (target as any).radius;
  await addConstraint({ Diameter: { circle: target.id, diameter: r * 2 } });
  await solveSketch(); await refreshSketch();
}

async function addDistance() {
  const ids = pickTwoPoints();
  if (!ids) { showToast("info", "需要两个点"); return; }
  await addConstraint({ Distance: { a: ids[0], b: ids[1], distance: 2.0 } });
  await solveSketch(); await refreshSketch();
}

async function solve() {
  await solveSketch();
  await refreshSketch();
}

// ── Entity editing ───────────────────────────────────────────────

function lineLength(line: { x1: number; y1: number; x2: number; y2: number }): number {
  return Math.sqrt((line.x2 - line.x1) ** 2 + (line.y2 - line.y1) ** 2);
}

async function updateEntityProp(id: EntityId, prop: string, event: Event) {
  const value = parseFloat((event.target as HTMLInputElement).value);
  if (Number.isNaN(value)) return;
  await updateEntityPropCmd(id, prop, value);
  await solveSketch();
  await refreshSketch();
}

async function deleteSelectedEntity() {
  const id = sketchStore.selectedId;
  if (id === null) return;
  await deleteEntityCmd(id);
  sketchStore.select(null);
  await refreshSketch();
}

async function toggleConstruction(id: EntityId, event: Event) {
  const checked = (event.target as HTMLInputElement).checked;
  await updateEntityPropCmd(id, "construction", checked ? 1 : 0);
  await refreshSketch();
}

// ── Context menu ─────────────────────────────────────────────────

const ctxMenu = ref<{ x: number; y: number; featureId: FeatureId } | null>(null);
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
    await renameFeatureCmd(f.id, n);
    await loadState();
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
  closeAllMenus();
  await unifiedViewport.value?.refreshViewport();
}

function onGlobalClick() {
  closeAllMenus();
  closeCtxMenu();
}

// ── Plugin dialog ────────────────────────────────────────────────

const activeDialog = ref<GeneratorInfo | null>(null);
const dialogParams = ref<Record<string, number>>({});
const dialogLoading = ref(false);

function openPluginDialog(gen: GeneratorInfo) {
  sketchStore.setActivePluginTool(`${gen.plugin_id}:${gen.id}`);
  activeDialog.value = gen;
  dialogParams.value = {};
  for (const param of gen.parameters) dialogParams.value[param.id] = param.default_value;
}

function closeDialog() {
  sketchStore.setActivePluginTool(null);
  activeDialog.value = null;
  dialogLoading.value = false;
}

async function runGenerator() {
  const gen = activeDialog.value;
  if (!gen || dialogLoading.value) return;
  dialogLoading.value = true;
  try {
    const id = await generatePluginFeature(gen.plugin_id, gen.id, { ...dialogParams.value });
    closeDialog();
    sketchStore.setActiveFeature(id);
    await loadState();
    await unifiedViewport.value?.refreshViewport();
    selectedFeatureId.value = id;
    showToast("success", `${gen.name} 生成成功`);
  } catch (e) {
    showToast("error", `${gen.name} 生成失败: ${e}`);
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
});
onUnmounted(() => {
  document.removeEventListener("click", onGlobalClick);
});
</script>

<style scoped>
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
