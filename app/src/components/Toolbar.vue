<template>
  <header class="toolbar">
    <!-- Row 1: menus + workspace tabs + global actions -->
    <div class="tb-row">
      <span class="title">EchoCAD</span>

      <div class="tb-menu">
        <button class="tb-menu-btn" :class="{ open: openMenuId==='file' }" @click.stop="toggleMenu('file')">文件 ▾</button>
        <div v-if="openMenuId==='file'" class="tb-dropdown" @click.stop>
          <button v-for="mi in menuFile" :key="mi.label" class="tb-dd-item" @click="emit('file-action', mi.action); closeAllMenus()">
            {{ mi.label }}<span class="mm-key">{{ mi.key }}</span>
          </button>
          <div v-if="recentFiles.length > 0" class="recent-section">
            <div class="recent-header">最近打开</div>
            <button v-for="(path, i) in recentFiles" :key="path" class="tb-dd-item recent-item" :title="path"
              @click="emit('open-recent', path); closeAllMenus()">
              <span class="recent-name">{{ basename(path) }}</span>
              <span v-if="i === 0" class="mm-key">最近</span>
            </button>
            <button class="tb-dd-item recent-clear" @click="emit('clear-recent'); closeAllMenus()">清空列表</button>
          </div>
        </div>
      </div>

      <div class="tb-menu">
        <button class="tb-menu-btn" :class="{ open: openMenuId==='edit' }" @click.stop="toggleMenu('edit')">编辑 ▾</button>
        <div v-if="openMenuId==='edit'" class="tb-dropdown" @click.stop>
          <button v-for="mi in menuEdit" :key="mi.label" class="tb-dd-item" @click="mi.handler(); closeAllMenus()">
            {{ mi.label }}<span class="mm-key">{{ mi.key }}</span>
          </button>
        </div>
      </div>

      <div class="tb-sep"></div>

      <!-- workspace tabs (inline in row 1) -->
      <button :class="{ active: !isEditingSketch }" class="tb-ws-btn" @click="onSolidTab">📦 实体</button>
      <button :class="{ active: isEditingSketch }" class="tb-ws-btn" @click="onSketchTab">📐 草图</button>

      <div class="tb-spacer"></div>

      <button class="tb-quick-btn" :class="{ active: sketchStore.measureMode }"
        @click="emit('toggle-measure')" title="测量模式">📏 测量</button>
      <button class="tb-icon-btn" @click="emit('undo')" title="撤销 Ctrl+Z" :disabled="!canUndo">↩</button>
      <button class="tb-icon-btn" @click="emit('redo')" title="重做 Ctrl+Y" :disabled="!canRedo">↪</button>
    </div>

    <!-- Row 2: tool groups + plane -->
    <div class="tb-row">
      <!-- 实体 tools -->
      <template v-if="!isEditingSketch">
        <div class="tb-group">
          <span class="tb-group-label">特征</span>
          <button class="tb-quick-btn" @click="emit('feature', 'extrude')" title="拉伸 (E)">⬆ 拉伸</button>
          <button class="tb-quick-btn" @click="emit('feature', 'revolve')" title="旋转 (W)">🔄 旋转</button>
          <button class="tb-quick-btn" @click="emit('feature', 'sweep')" title="扫描">〰 扫描</button>
          <button class="tb-quick-btn" @click="emit('feature', 'fillet')" title="圆角">🔵 圆角</button>
          <button class="tb-quick-btn" @click="emit('feature', 'chamfer')" title="倒角">🔻 倒角</button>
          <button class="tb-quick-btn" @click="emit('feature', 'shell')" title="抽壳">⬡ 抽壳</button>
          <button class="tb-quick-btn" @click="emit('feature', 'boolean')" title="布尔运算">➖ 布尔</button>
        </div>
        <div class="tb-group">
          <span class="tb-group-label">阵列</span>
          <button class="tb-quick-btn" @click="emit('pattern', 'linear')" title="线性阵列">⬌ 线性</button>
          <button class="tb-quick-btn" @click="emit('pattern', 'circular')" title="圆周阵列">🔁 圆周</button>
          <button class="tb-quick-btn" @click="emit('pattern', 'mirror')" title="镜像">🪞 镜像</button>
        </div>
      </template>

      <!-- 草图 tools -->
      <template v-if="isEditingSketch">
        <div class="tb-group">
          <span class="tb-group-label">绘图</span>
          <button v-for="t in sketchTools" :key="t.value"
            class="tb-quick-btn"
            :class="{ active: sketchStore.activeTool === t.value }"
            @click="sketchStore.setTool(t.value)"
            :title="t.label + ' (' + t.shortcut + ')'">
            {{ toolIcon(t.value) }} {{ t.label }}
          </button>
        </div>
        <div class="tb-group">
          <span class="tb-group-label">约束</span>
          <button v-for="c in constraintItems" :key="c.label"
            class="tb-quick-btn"
            @click="emit('constraint', c.kind)"
            :title="'约束: ' + c.label">
            {{ c.label }}
          </button>
        </div>
        <div class="tb-group">
          <span class="tb-group-label">操作</span>
          <button class="tb-quick-btn" @click="emit('feature', 'extrude')" title="拉伸 (E)">⬆ 拉伸</button>
          <button class="tb-quick-btn" @click="emit('feature', 'revolve')" title="旋转 (W)">🔄 旋转</button>
          <button class="tb-quick-btn" @click="emit('clear-sketch')" title="清空草图">🗑 清空</button>
          <button class="tb-exit-sketch" @click="emit('exit-sketch')" title="退出草图编辑模式 (Esc)">✕ 退出草图</button>
        </div>
      </template>

      <div v-if="sketchStore.generators.length>0" class="tb-menu">
        <button class="tb-menu-btn" :class="{ open: openMenuId==='plugins' }" @click.stop="toggleMenu('plugins')">插件 ▾</button>
        <div v-if="openMenuId==='plugins'" class="tb-dropdown" @click.stop>
          <div class="mm-list">
            <button v-for="g in sketchStore.generators" :key="g.id"
              @click="emit('open-plugin', g); closeAllMenus()">{{ g.name }}</button>
          </div>
        </div>
      </div>

      <select v-model="sketchStore.activePlane" class="tb-plane"
        :disabled="isEditingSketch"
        :title="isEditingSketch ? '草图平面在创建时确定，编辑中不可修改' : '新草图的基准平面'">
        <option v-for="p in planes" :key="p.value" :value="p.value">{{ p.label }}</option>
      </select>
      <button class="tb-quick-btn" @click="emit('offset-plane')"
        title="在基准平面上偏移创建新草图平面">➕ 偏移平面</button>
    </div>
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
  (e: "toggle-measure"): void;
}>();

const sketchStore = useSketchStore();

const openMenuId = ref<string | null>(null);
function toggleMenu(id: string) { openMenuId.value = openMenuId.value === id ? null : id; }
function closeAllMenus() { openMenuId.value = null; }
defineExpose({ closeAllMenus });

const isEditingSketch = computed(() => sketchStore.isEditingSketch());

function onSolidTab() { if (isEditingSketch.value) emit("exit-sketch"); }
function onSketchTab() { if (!isEditingSketch.value) emit("new-sketch"); }

const canUndo = ref(false);
const canRedo = ref(false);
let undoRedoPoll: ReturnType<typeof setInterval> | null = null;

async function refreshUndoRedo() {
  try { const [u, r] = await canUndoRedo(); canUndo.value = u; canRedo.value = r; } catch { /* */ }
}

watch(() => sketchStore.features, () => { refreshUndoRedo(); });
watch(() => sketchStore.entities, () => { refreshUndoRedo(); });

onMounted(() => { refreshUndoRedo(); undoRedoPoll = setInterval(refreshUndoRedo, 2000); });
onUnmounted(() => { if (undoRedoPoll !== null) { clearInterval(undoRedoPoll); undoRedoPoll = null; } });

function basename(path: string): string { const p = path.split(/[\\/]/); return p[p.length - 1] || path; }

interface MenuItem { label: string; action: FileAction; key?: string; }
interface EditMenuItem { label: string; handler: () => void; key?: string; }

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

const constraintItems: { label: string; kind: ConstraintKind }[] = [
  { label: "水平", kind: "horizontal" }, { label: "竖直", kind: "vertical" },
  { label: "平行", kind: "parallel" }, { label: "垂直", kind: "perpendicular" },
  { label: "相切", kind: "tangent" }, { label: "同心", kind: "concentric" },
  { label: "相等", kind: "equal" }, { label: "中点", kind: "midpoint" },
  { label: "固定", kind: "fix" }, { label: "角度", kind: "angle" },
  { label: "直径", kind: "diameter" }, { label: "距离", kind: "distance" },
  { label: "求解", kind: "solve" },
];

function toolIcon(tool: string): string {
  const icons: Record<string, string> = {
    select: "🖱", line: "📏", circle: "⭕", arc: "🌙",
    rectangle: "⬜", spline: "〰", ellipse: "🥚", trim: "✂", extend: "⤏",
  };
  return icons[tool] ?? "";
}

const planes = [
  { value: "xy", label: "XY 平面" },
  { value: "yz", label: "YZ 平面" },
  { value: "zx", label: "ZX 平面" },
];
</script>

<style scoped>
/*
 * The toolbar has a FIXED total height (var(--toolbar-height)): the native
 * renderer subtracts exactly this many logical pixels from the top of the
 * window to place the 3D viewport. Rows must never wrap — row 2 scrolls
 * horizontally instead when the window is narrow.
 */
.toolbar {
  display: flex;
  flex-direction: column;
  height: var(--toolbar-height);
  background: var(--bg-toolbar);
  border-bottom: 1px solid var(--border);
  user-select: none;
}

.tb-row {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-wrap: nowrap;
  padding: 0 8px;
  flex-shrink: 0;
}
.tb-row:first-child {
  height: 34px;
}
.tb-row:last-child {
  height: 41px;
  overflow-x: auto;
  scrollbar-width: none; /* keep the row height exact (Firefox) */
}
.tb-row:last-child::-webkit-scrollbar {
  display: none; /* keep the row height exact (WebKit) */
}

.title {
  font-weight: 700;
  font-size: 13px;
  margin-right: 8px;
  flex-shrink: 0;
}

.tb-sep {
  width: 1px;
  height: 20px;
  background: var(--border-strong);
  margin: 0 4px;
  flex-shrink: 0;
}
.tb-spacer {
  flex: 1;
}

/* ── Menus (文件 / 编辑 / 插件) ─────────────────────────────── */
.tb-menu {
  position: relative;
  flex-shrink: 0;
}
.tb-menu-btn {
  background: transparent;
  border: 1px solid transparent;
  color: #ccc;
  padding: 4px 10px;
  font-size: 12px;
  cursor: pointer;
  border-radius: 3px;
  white-space: nowrap;
}
.tb-menu-btn:hover,
.tb-menu-btn.open {
  background: #444;
  border-color: var(--border-strong);
}
.tb-dropdown {
  position: absolute;
  top: 100%;
  left: 0;
  margin-top: 2px;
  background: var(--bg-menu);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  min-width: 150px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
  z-index: 200;
  padding: 4px;
}
.tb-dd-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
  padding: 5px 12px;
  font-size: 12px;
  color: #ccc;
  background: transparent;
  border: none;
  cursor: pointer;
  border-radius: 2px;
  text-align: left;
  white-space: nowrap;
}
.tb-dd-item:hover {
  background: var(--accent);
  color: #fff;
}
.mm-key {
  font-size: 10px;
  color: #888;
  margin-left: 12px;
}
.tb-dd-item:hover .mm-key {
  color: #aac;
}
.mm-list button {
  display: flex;
  width: 100%;
  padding: 5px 10px;
  font-size: 12px;
  color: #ccc;
  background: transparent;
  border: 1px solid transparent;
  cursor: pointer;
  border-radius: 2px;
  text-align: left;
}
.mm-list button:hover {
  background: var(--accent);
  color: #fff;
}

.recent-section {
  border-top: 1px solid var(--border-strong);
  margin-top: 4px;
  padding-top: 4px;
}
.recent-header {
  font-size: 10px;
  color: #888;
  padding: 4px 12px 2px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.recent-item {
  font-size: 11px;
}
.recent-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 200px;
}
.recent-clear {
  color: var(--fg-dim);
  font-size: 10px;
  font-style: italic;
}
.recent-clear:hover {
  color: var(--danger-fg);
  background: transparent;
}

/* ── Workspace tabs (实体 / 草图) ───────────────────────────── */
.tb-ws-btn {
  padding: 4px 14px;
  font-size: 12px;
  color: var(--fg-dim);
  background: transparent;
  border: 1px solid transparent;
  border-radius: 3px;
  cursor: pointer;
  transition: color 0.15s, background 0.15s;
  flex-shrink: 0;
  white-space: nowrap;
}
.tb-ws-btn:hover {
  color: #ccc;
  background: var(--bg-hover);
}
.tb-ws-btn.active {
  color: #fff;
  background: var(--accent);
  border-color: var(--accent);
}

/* ── Tool groups + buttons ──────────────────────────────────── */
.tb-group {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
  border: 1px solid #444;
  border-radius: var(--radius);
  padding: 2px 4px;
}
.tb-group-label {
  font-size: 10px;
  color: #888;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin: 0 3px;
  flex-shrink: 0;
}
.tb-quick-btn {
  background: var(--bg-input);
  border: 1px solid var(--border-strong);
  color: #ddd;
  padding: 4px 10px;
  font-size: 12px;
  cursor: pointer;
  border-radius: 3px;
  white-space: nowrap;
  flex-shrink: 0;
}
.tb-quick-btn:hover {
  background: var(--accent);
  color: #fff;
  border-color: var(--accent);
}
.tb-quick-btn.active {
  background: var(--accent);
  color: #fff;
  border-color: var(--accent);
}
.tb-icon-btn {
  background: transparent;
  border: 1px solid transparent;
  color: #ccc;
  padding: 3px 8px;
  font-size: 14px;
  cursor: pointer;
  border-radius: 3px;
  flex-shrink: 0;
}
.tb-icon-btn:hover {
  background: #444;
}
.tb-icon-btn:disabled {
  opacity: 0.35;
  cursor: default;
}
.tb-icon-btn:disabled:hover {
  background: transparent;
}
.tb-exit-sketch {
  background: #5a2a2a;
  border: 1px solid #844;
  color: var(--danger-fg);
  padding: 4px 12px;
  font-size: 12px;
  cursor: pointer;
  border-radius: 3px;
  white-space: nowrap;
  font-weight: 600;
  flex-shrink: 0;
}
.tb-exit-sketch:hover {
  background: #733;
  color: #ffaaaa;
}

.tb-plane {
  background: var(--bg-input);
  border: 1px solid var(--border-strong);
  color: #ccc;
  padding: 3px 6px;
  border-radius: 3px;
  font-size: 11px;
  cursor: pointer;
  flex-shrink: 0;
}
.tb-plane:disabled {
  opacity: 0.5;
  cursor: not-allowed;
  background: #2a2a2a;
}
</style>
