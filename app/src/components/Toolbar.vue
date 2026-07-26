<template>
  <header class="toolbar">
    <span class="title">EchoCAD</span>

    <!-- File menu -->
    <div class="tb-menu">
      <button class="tb-menu-btn" :class="{ open: openMenuId==='file' }" @click.stop="toggleMenu('file')">文件 ▾</button>
      <div v-if="openMenuId==='file'" class="tb-dropdown" @click.stop>
        <button v-for="mi in menuFile" :key="mi.label" class="tb-dd-item" @click="emit('file-action', mi.action); closeAllMenus()">
          {{ mi.label }}<span class="mm-key">{{ mi.key }}</span>
        </button>
        <div v-if="recentFiles.length > 0" class="recent-section">
          <div class="recent-header">最近打开</div>
          <button
            v-for="(path, i) in recentFiles"
            :key="path"
            class="tb-dd-item recent-item"
            :title="path"
            @click="emit('open-recent', path); closeAllMenus()"
          >
            <span class="recent-name">{{ basename(path) }}</span>
            <span v-if="i === 0" class="mm-key">最近</span>
          </button>
          <button class="tb-dd-item recent-clear" @click="emit('clear-recent'); closeAllMenus()">
            清空列表
          </button>
        </div>
      </div>
    </div>

    <!-- Edit menu -->
    <div class="tb-menu">
      <button class="tb-menu-btn" :class="{ open: openMenuId==='edit' }" @click.stop="toggleMenu('edit')">编辑 ▾</button>
      <div v-if="openMenuId==='edit'" class="tb-dropdown" @click.stop>
        <button v-for="mi in menuEdit" :key="mi.label" class="tb-dd-item" @click="mi.handler(); closeAllMenus()">
          {{ mi.label }}<span class="mm-key">{{ mi.key }}</span>
        </button>
      </div>
    </div>

    <div class="tb-sep"></div>

    <!-- Quick feature buttons (SolidWorks-like) -->
    <template v-if="!isEditingSketch">
      <button class="tb-quick-btn" @click="emit('feature', 'extrude')" title="拉伸 (E)">🧊 拉伸</button>
      <button class="tb-quick-btn" @click="emit('feature', 'revolve')" title="旋转 (W)">🔄 旋转</button>
      <button class="tb-quick-btn" @click="emit('feature', 'sweep')" title="扫描">〰️ 扫描</button>
      <button class="tb-quick-btn" @click="emit('feature', 'fillet')" title="圆角">🔵 圆角</button>
      <button class="tb-quick-btn" @click="emit('feature', 'chamfer')" title="倒角">🔻 倒角</button>
      <button class="tb-quick-btn" @click="emit('feature', 'shell')" title="抽壳">🫙 抽壳</button>
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
          <button class="tb-dropdown-item" style="color:var(--error);padding:4px 12px"
            :disabled="!sketchStore.isEditingSketch()"
            @click="emit('clear-sketch'); closeAllMenus()">
            清空草图
          </button>
        </div>
      </div>

      <div class="tb-menu">
        <button class="tb-menu-btn" :class="{ open: openMenuId==='constraints' }" @click.stop="toggleMenu('constraints')">约束 ▾</button>
        <div v-if="openMenuId==='constraints'" class="tb-dropdown" @click.stop>
          <div class="mm-grid mm-2col">
            <button v-for="c in constraintItems" :key="c.label" @click="emit('constraint', c.kind); closeAllMenus()">{{ c.label }}</button>
          </div>
        </div>
      </div>
      <div class="tb-sep"></div>
      <button class="tb-quick-btn" @click="emit('feature', 'extrude')" title="拉伸 (E)">🧊 拉伸</button>
      <button class="tb-quick-btn" @click="emit('feature', 'revolve')" title="旋转 (W)">🔄 旋转</button>
      <button class="tb-exit-sketch" @click="emit('exit-sketch')" title="退出草图编辑模式">✕ 退出草图</button>
    </template>

    <!-- Features -->
    <div class="tb-menu">
      <button class="tb-menu-btn" :class="{ open: openMenuId==='features' }" @click.stop="toggleMenu('features')">特征 ▾</button>
      <div v-if="openMenuId==='features'" class="tb-dropdown" @click.stop>
        <div class="mm-grid mm-2col">
          <button v-for="f in featureItems" :key="f.label" @click="emit('feature', f.kind); closeAllMenus()">
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
          <button v-for="p in patternItems" :key="p.label" @click="emit('pattern', p.kind); closeAllMenus()">{{ p.label }}</button>
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
            @click="emit('open-plugin', g); closeAllMenus()">{{ g.name }}</button>
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
    <button class="tb-quick-btn" @click="emit('offset-plane')"
      title="在基准平面上偏移创建新草图平面">
      ➕ 偏移平面
    </button>

    <div class="tb-spacer"></div>

    <button
      class="tb-quick-btn"
      :class="{ active: sketchStore.useBrep }"
      @click="emit('toggle-brep')"
      title="B-Rep 管线 (OCCT 精确几何)"
    >🧩 B-Rep</button>

    <button
      class="tb-quick-btn"
      :class="{ active: sketchStore.measureMode }"
      @click="emit('toggle-measure')"
      title="测量模式"
    >📏 测量</button>

    <button class="tb-icon-btn" @click="emit('undo')" title="撤销 Ctrl+Z" :disabled="!canUndo">↩</button>
    <button class="tb-icon-btn" @click="emit('redo')" title="重做 Ctrl+Y" :disabled="!canRedo">↪</button>
  </header>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from "vue";
import { useSketchStore } from "@/stores/sketch";
import { canUndoRedo } from "@/commands/sketch";
import type { GeneratorInfo } from "@/commands/sketch";

export type FileAction = "new" | "open" | "save" | "save-as" | "export-stl" | "export-obj" | "export-gltf";
export type FeatureKind = "extrude" | "revolve" | "sweep" | "fillet" | "chamfer" | "shell" | "boolean";
export type PatternKind = "linear" | "circular" | "mirror";
export type ConstraintKind =
  | "horizontal" | "vertical" | "parallel" | "perpendicular" | "tangent"
  | "concentric" | "equal" | "midpoint" | "fix" | "angle" | "diameter"
  | "distance" | "solve";

defineProps<{
  recentFiles: string[];
}>();

const emit = defineEmits<{
  (e: "file-action", action: FileAction): void;
  (e: "open-recent", path: string): void;
  (e: "clear-recent"): void;
  (e: "undo"): void;
  (e: "redo"): void;
  (e: "new-sketch"): void;
  (e: "clear-doc"): void;
  (e: "feature", kind: FeatureKind): void;
  (e: "pattern", kind: PatternKind): void;
  (e: "constraint", kind: ConstraintKind): void;
  (e: "exit-sketch"): void;
  (e: "open-plugin", gen: GeneratorInfo): void;
  (e: "offset-plane"): void;
  (e: "clear-sketch"): void;
  (e: "toggle-brep"): void;
  (e: "toggle-measure"): void;
}>();

const sketchStore = useSketchStore();

// ── Dropdown menu state ──────────────────────────────────────────
const openMenuId = ref<string | null>(null);
function toggleMenu(id: string) {
  openMenuId.value = openMenuId.value === id ? null : id;
}
function closeAllMenus() {
  openMenuId.value = null;
}
// Exposed so the parent's global click handler can close dropdowns.
defineExpose({ closeAllMenus });

const isEditingSketch = computed(() => sketchStore.isEditingSketch());

// ── Undo/Redo availability ──────────────────────────────────────
// The backend exposes a single `can_undo_redo` command returning
// `[can_undo, can_redo]`. We mirror it into two local refs so the
// toolbar buttons can be disabled when their stack is empty (instead
// of being always-enabled, silently no-op'ing buttons).
//
// Refresh strategy: every document mutation in HomeView goes through
// `loadState()`, which reassigns `store.features` / `.entities`. Those
// reassignments fire the watchers below, so the button state updates
// right after any undo/redo/edit. A low-frequency poll covers any
// stack change that happens to skip a loadState (safety net only).
const canUndo = ref(false);
const canRedo = ref(false);
let undoRedoPoll: ReturnType<typeof setInterval> | null = null;

async function refreshUndoRedo() {
  try {
    const [u, r] = await canUndoRedo();
    canUndo.value = u;
    canRedo.value = r;
  } catch {
    // The backend may be unreachable during teardown; ignore.
  }
}

watch(() => sketchStore.features, () => { refreshUndoRedo(); });
watch(() => sketchStore.entities, () => { refreshUndoRedo(); });

onMounted(() => {
  refreshUndoRedo();
  undoRedoPoll = setInterval(refreshUndoRedo, 2000);
});
onUnmounted(() => {
  if (undoRedoPoll !== null) {
    clearInterval(undoRedoPoll);
    undoRedoPoll = null;
  }
});

function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

interface MenuItem { label: string; action: FileAction; key?: string; }
interface EditMenuItem { label: string; handler: () => void; key?: string; }
interface FeatureItem { label: string; kind: FeatureKind; key?: string; }
interface PatternItem { label: string; kind: PatternKind; }
interface ConstraintItem { label: string; kind: ConstraintKind; }

const menuFile: MenuItem[] = [
  { label: "新建", action: "new", key: "Ctrl+N" },
  { label: "打开", action: "open", key: "Ctrl+O" },
  { label: "保存", action: "save", key: "Ctrl+S" },
  { label: "另存为...", action: "save-as" },
  { label: "导出 STL", action: "export-stl" },
  { label: "导出 OBJ", action: "export-obj" },
  { label: "导出 glTF", action: "export-gltf" },
];

const menuEdit: EditMenuItem[] = [
  { label: "撤销", handler: () => emit("undo"), key: "Ctrl+Z" },
  { label: "重做", handler: () => emit("redo"), key: "Ctrl+Y" },
  { label: "新草图", handler: () => emit("new-sketch"), key: "N" },
  { label: "清空文档", handler: () => emit("clear-doc") },
];

const sketchTools = [
  { value: "select" as const, label: "选择", shortcut: "Esc" },
  { value: "line" as const, label: "直线", shortcut: "L" },
  { value: "circle" as const, label: "圆", shortcut: "C" },
  { value: "arc" as const, label: "弧线", shortcut: "A" },
  { value: "rectangle" as const, label: "矩形", shortcut: "R" },
  { value: "spline" as const, label: "样条", shortcut: "B" },
  { value: "ellipse" as const, label: "椭圆", shortcut: "I" },
  { value: "trim" as const, label: "裁剪", shortcut: "T" },
  { value: "extend" as const, label: "延伸", shortcut: "X" },
];

const constraintItems: ConstraintItem[] = [
  { label: "水平", kind: "horizontal" }, { label: "竖直", kind: "vertical" },
  { label: "平行", kind: "parallel" }, { label: "垂直", kind: "perpendicular" },
  { label: "相切", kind: "tangent" }, { label: "同心", kind: "concentric" },
  { label: "相等", kind: "equal" }, { label: "中点", kind: "midpoint" },
  { label: "固定", kind: "fix" }, { label: "角度", kind: "angle" },
  { label: "直径", kind: "diameter" }, { label: "距离", kind: "distance" },
  { label: "求解", kind: "solve" },
];

const featureItems: FeatureItem[] = [
  { label: "拉伸", kind: "extrude", key: "E" },
  { label: "旋转", kind: "revolve", key: "W" },
  { label: "扫描", kind: "sweep" },
  { label: "圆角", kind: "fillet" },
  { label: "倒角", kind: "chamfer" },
  { label: "抽壳", kind: "shell" },
  { label: "布尔运算", kind: "boolean" },
];

const patternItems: PatternItem[] = [
  { label: "线性阵列", kind: "linear" },
  { label: "圆周阵列", kind: "circular" },
  { label: "镜像", kind: "mirror" },
];

const planes = [
  { value: "xy", label: "XY 平面" },
  { value: "yz", label: "YZ 平面" },
  { value: "zx", label: "ZX 平面" },
];
</script>
