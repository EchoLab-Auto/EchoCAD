<template>
  <div v-if="targets" class="dialog-overlay" @click.self="emit('cancel')">
    <div class="dialog">
      <h3>布尔运算</h3>
      <p class="dialog-desc">
        合并两个实体。差集 / 交集会真实切割几何 (CSG)。
      </p>
      <div class="dialog-param">
        <label>运算类型</label>
        <select v-model="localOp" class="param-select">
          <option value="union">并集 (A ∪ B)</option>
          <option value="subtract">差集 (A − B)</option>
          <option value="intersect">交集 (A ∩ B)</option>
        </select>
      </div>
      <div class="dialog-param boolean-target">
        <label>目标 A</label>
        <span class="target-name">{{ swapped ? targets.nameB : targets.nameA }}</span>
        <button class="swap-btn" type="button" title="交换 A / B (差集运算顺序)" @click="swapped = !swapped">⇄</button>
      </div>
      <div class="dialog-param boolean-target">
        <label>目标 B</label>
        <span class="target-name">{{ swapped ? targets.nameA : targets.nameB }}</span>
      </div>
      <div class="dialog-actions">
        <button class="btn-primary" @click="onConfirm">确认</button>
        <button class="btn-cancel" @click="emit('cancel')">取消</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from "vue";
import type { FeatureId } from "@/commands/sketch";

export interface BooleanTargets {
  idA: FeatureId;
  nameA: string;
  idB: FeatureId;
  nameB: string;
}

export interface BooleanResult {
  op: string;
  idA: FeatureId;
  idB: FeatureId;
}

const props = defineProps<{
  targets: BooleanTargets | null;
}>();

const emit = defineEmits<{
  (e: "confirm", result: BooleanResult): void;
  (e: "cancel"): void;
}>();

const localOp = ref<string>("union");
const swapped = ref<boolean>(false);

// Reset the form each time a new pair of targets is presented so a previous
// session's op/swap choice can't leak into the next invocation.
watch(() => props.targets, () => {
  localOp.value = "union";
  swapped.value = false;
});

function onConfirm() {
  const a = swapped.value ? props.targets!.idB : props.targets!.idA;
  const b = swapped.value ? props.targets!.idA : props.targets!.idB;
  emit("confirm", { op: localOp.value, idA: a, idB: b });
}
</script>

<style scoped>
/* dialog-* / btn-* classes are global (style.css); only the target-row
   bits specific to this dialog live here. */
.boolean-target { align-items: center; }
.target-name { flex: 1; font-size: 13px; color: #ccc; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.swap-btn {
  background: #3c3c3c; border: 1px solid #555; color: #ccc;
  padding: 2px 8px; border-radius: 3px; font-size: 13px; cursor: pointer; line-height: 1;
}
.swap-btn:hover { background: #505050; color: #fff; }
</style>
