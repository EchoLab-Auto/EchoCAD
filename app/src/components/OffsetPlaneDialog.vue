<template>
  <div v-if="open" class="dialog-overlay" @click.self="emit('cancel')">
    <div class="dialog">
      <h3>偏移平面</h3>
      <p class="dialog-desc">
        在基准平面之上 / 之下创建新的草图平面，并立即进入草图编辑。
      </p>
      <div class="dialog-param">
        <label>基准平面</label>
        <select v-model="localBase" class="param-select">
          <option value="xy">XY 平面</option>
          <option value="yz">YZ 平面</option>
          <option value="zx">ZX 平面</option>
        </select>
      </div>
      <div class="dialog-param">
        <label>偏移距离</label>
        <input type="number" step="0.1" v-model.number="localDistance" />
      </div>
      <div class="dialog-actions">
        <button class="btn-primary" @click="onConfirm">创建并编辑</button>
        <button class="btn-cancel" @click="emit('cancel')">取消</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from "vue";

export interface OffsetPlaneResult {
  basePlane: string;
  distance: number;
}

const props = defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  (e: "confirm", result: OffsetPlaneResult): void;
  (e: "cancel"): void;
}>();

const localBase = ref<string>("xy");
const localDistance = ref<number>(1.0);

// Re-seed defaults each time the dialog opens, so a prior session's values
// don't persist into the next creation.
watch(() => props.open, (isOpen) => {
  if (isOpen) {
    localBase.value = "xy";
    localDistance.value = 1.0;
  }
});

function onConfirm() {
  // Guard against NaN if the user clears the number field.
  const distance = Number.isFinite(localDistance.value) ? localDistance.value : 0;
  emit("confirm", { basePlane: localBase.value, distance });
}
</script>
