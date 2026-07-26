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
