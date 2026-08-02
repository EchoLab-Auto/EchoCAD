<template>
  <div class="extrude-panel">
    <h3>拉伸设置</h3>
    <div class="param-row">
      <label>方向</label>
      <select v-model="localDir" class="param-select">
        <option value="one_side">单侧</option>
        <option value="midplane">中平面</option>
        <option value="two_sides">双向</option>
      </select>
    </div>
    <div class="param-row">
      <label>深度</label>
      <input type="range" min="0.1" max="20" step="0.1" v-model.number="localDepth" @input="onChange" />
      <input type="number" step="0.1" min="0.1" v-model.number="localDepth" @change="onChange" class="param-num" />
    </div>
    <div v-if="localDir === 'two_sides'" class="param-row">
      <label>反向深度</label>
      <input type="range" min="0.1" max="20" step="0.1" v-model.number="localDist2" @input="onChange" />
      <input type="number" step="0.1" min="0.1" v-model.number="localDist2" @change="onChange" class="param-num" />
    </div>
    <div class="param-row">
      <label>拔模角°</label>
      <input type="range" min="0" max="15" step="0.5" v-model.number="localDraft" @input="onChange" />
      <input type="number" step="0.5" min="0" max="15" v-model.number="localDraft" @change="onChange" class="param-num" />
    </div>

    <!-- Region selection -->
    <div v-if="regions.length > 1" class="region-section">
      <h3 class="region-title">拉伸选区 <span class="region-count">{{ regions.length }} 个轮廓</span></h3>
      <div class="region-toolbar">
        <button class="region-btn" @click="selectAll">全选</button>
        <button class="region-btn" @click="deselectAll">取消</button>
      </div>
      <div
        v-for="r in regions"
        :key="r.index"
        class="region-item"
        :class="{ selected: selectedRegions.includes(r.index) }"
        @click="toggleRegion(r.index)"
      >
        <span class="region-check">{{ selectedRegions.includes(r.index) ? '☑' : '☐' }}</span>
        <span class="region-label">轮廓 {{ r.index + 1 }}</span>
        <span class="region-area">{{ r.area.toFixed(3) }} mm²</span>
      </div>
      <div v-if="selectedRegions.length === 0" class="region-hint">
        未选中时拉伸全部轮廓。选中的轮廓可组成实体+孔的复合形状。
      </div>
    </div>

    <div class="param-actions">
      <button class="btn-primary" :disabled="loading" @click="onConfirm">
        {{ loading ? "处理中..." : "确认拉伸" }}
      </button>
      <button class="btn-cancel" :disabled="loading" @click="onCancel">取消</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from "vue";
import type { ExtrudeRegionInfo } from "@/commands/sketch";

const props = defineProps<{
  direction: string;
  depth: number;
  dist2: number;
  draft: number;
  regions: ExtrudeRegionInfo[];
}>();

const emit = defineEmits<{
  (e: "change", config: { direction: string; depth: number; dist2: number; draft: number }): void;
  (e: "confirm", selectedRegions: number[] | null): void;
  (e: "cancel"): void;
  /** Fired when the user toggles a region selection, so the 3D preview
   *  can re-render with only the selected loops (preview must match the
   *  final extrude — 原则2). */
  (e: "region-change", selectedRegions: number[] | null): void;
}>();

const localDir = ref(props.direction);
const localDepth = ref(props.depth);
const localDist2 = ref(props.dist2);
const localDraft = ref(props.draft);
const loading = ref(false);
const selectedRegions = ref<number[]>([]);

watch(() => props.direction, v => { localDir.value = v; });
watch(() => props.depth, v => { localDepth.value = v; });
watch(() => props.dist2, v => { localDist2.value = v; });
watch(() => props.draft, v => { localDraft.value = v; });
// Reset the region selection whenever the region list itself changes —
// reopening the panel on a different sketch must not leak the previous
// sketch's selection (stale indices target the wrong loops). Same pattern
// as BooleanDialog's targets watcher.
watch(() => props.regions, () => {
  selectedRegions.value = [];
  emit("region-change", null);
});

function onChange() {
  emit("change", {
    direction: localDir.value,
    depth: localDepth.value,
    dist2: localDist2.value,
    draft: localDraft.value,
  });
}

function toggleRegion(index: number) {
  const idx = selectedRegions.value.indexOf(index);
  if (idx >= 0) {
    selectedRegions.value.splice(idx, 1);
  } else {
    selectedRegions.value.push(index);
  }
  emitRegionChange();
}

function emitRegionChange() {
  emit("region-change", selectedRegions.value.length > 0 ? [...selectedRegions.value] : null);
}

function selectAll() {
  selectedRegions.value = props.regions.map(r => r.index);
  emitRegionChange();
}

function deselectAll() {
  selectedRegions.value = [];
  emitRegionChange();
}

function onConfirm() {
  loading.value = true;
  // null = extrude all, [] = also extrude all, non-empty = selected
  const regions = selectedRegions.value.length > 0 ? selectedRegions.value : null;
  emit("confirm", regions);
}

function onCancel() { emit("cancel"); }

function resetLoading() { loading.value = false; }

defineExpose({ resetLoading });
</script>

<style scoped>
.extrude-panel { padding: 4px 0; }
.extrude-panel h3 {
  font-size: 11px; text-transform: uppercase; color: #999;
  margin-bottom: 10px; letter-spacing: 0.5px;
}
.param-row { margin-bottom: 10px; display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
.param-row label { width: 60px; font-size: 12px; color: #ccc; flex-shrink: 0; }
.param-select {
  flex: 1; min-width: 100px;
  background: #3c3c3c; border: 1px solid #555; color: #e0e0e0;
  padding: 4px 6px; border-radius: 3px; font-size: 12px; cursor: pointer;
}
.param-select:focus { border-color: #007acc; outline: none; }
.param-row input[type="range"] { flex: 1; min-width: 60px; accent-color: #007acc; }
.param-num {
  width: 55px; background: #3c3c3c; border: 1px solid #555; color: #e0e0e0;
  padding: 3px 5px; border-radius: 3px; font-size: 12px; text-align: right;
}
.param-num:focus { border-color: #007acc; outline: none; }
.region-section { margin: 12px 0; border-top: 1px solid #3c3c3c; padding-top: 10px; }
.region-title { margin-bottom: 8px !important; }
.region-toolbar { display: flex; gap: 4px; margin-bottom: 6px; }
.region-btn {
  font-size: 10px; padding: 2px 8px; background: #3c3c3c; border: 1px solid #555;
  color: #ccc; cursor: pointer; border-radius: 2px;
}
.region-btn:hover { background: #007acc; color: #fff; }
.region-item {
  display: flex; align-items: center; gap: 6px; padding: 4px 6px;
  border-radius: 3px; cursor: pointer; font-size: 12px;
}
.region-item:hover { background: #3c3c3c; }
.region-item.selected { background: #1a3a5c; border: 1px solid #007acc; }
.region-item.hole { opacity: 0.6; }
.region-check { font-size: 12px; flex-shrink: 0; }
.region-label { color: #ccc; flex: 1; }
.region-area { color: #888; font-size: 11px; }
.region-hint { font-size: 11px; color: #888; padding: 4px 6px; font-style: italic; }
.param-actions { display: flex; gap: 8px; margin-top: 16px; }
.btn-primary {
  background: #007acc; border: none; color: white; padding: 6px 16px;
  border-radius: 3px; cursor: pointer; font-size: 12px; flex: 1;
}
.btn-primary:hover { background: #0098ff; }
.btn-primary:disabled { background: #555; cursor: not-allowed; opacity: 0.6; }
.btn-cancel {
  background: #3c3c3c; border: 1px solid #555; color: #e0e0e0;
  padding: 6px 16px; border-radius: 3px; cursor: pointer; font-size: 12px;
}
.btn-cancel:hover { background: #505050; }
.btn-cancel:disabled { opacity: 0.5; cursor: not-allowed; }
.region-count { font-size: 10px; color: var(--fg-faint); font-weight: 400; }
</style>
