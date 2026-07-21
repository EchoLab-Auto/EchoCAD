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

const props = defineProps<{
  direction: string;
  depth: number;
  dist2: number;
  draft: number;
}>();

const emit = defineEmits<{
  (e: "change", config: { direction: string; depth: number; dist2: number; draft: number }): void;
  (e: "confirm"): void;
  (e: "cancel"): void;
}>();

const localDir = ref(props.direction);
const localDepth = ref(props.depth);
const localDist2 = ref(props.dist2);
const localDraft = ref(props.draft);
const loading = ref(false);

watch(() => props.direction, v => { localDir.value = v; });
watch(() => props.depth, v => { localDepth.value = v; });
watch(() => props.dist2, v => { localDist2.value = v; });
watch(() => props.draft, v => { localDraft.value = v; });

function onChange() {
  emit("change", {
    direction: localDir.value,
    depth: localDepth.value,
    dist2: localDist2.value,
    draft: localDraft.value,
  });
}

function onConfirm() {
  loading.value = true;
  emit("confirm");
}
function onCancel() { emit("cancel"); }

/** Allow the parent to reset the loading state after the async operation completes. */
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
.param-row input[type="range"] {
  flex: 1; min-width: 60px; accent-color: #007acc;
}
.param-num {
  width: 55px; background: #3c3c3c; border: 1px solid #555; color: #e0e0e0;
  padding: 3px 5px; border-radius: 3px; font-size: 12px; text-align: right;
}
.param-num:focus { border-color: #007acc; outline: none; }
.param-actions { display: flex; gap: 8px; margin-top: 16px; }
.btn-primary {
  background: #007acc; border: none; color: white; padding: 6px 16px;
  border-radius: 3px; cursor: pointer; font-size: 12px; flex: 1;
}
.btn-primary:hover { background: #0098ff; }
.btn-primary:disabled {
  background: #555; cursor: not-allowed; opacity: 0.6;
}
.btn-cancel {
  background: #3c3c3c; border: 1px solid #555; color: #e0e0e0;
  padding: 6px 16px; border-radius: 3px; cursor: pointer; font-size: 12px;
}
.btn-cancel:hover { background: #505050; }
.btn-cancel:disabled {
  opacity: 0.5; cursor: not-allowed;
}
</style>
