<template>
  <div
    v-if="target"
    class="ctx-menu"
    :style="{ left: target.x + 'px', top: target.y + 'px' }"
    @click.stop
  >
    <button v-if="isSketchLike" @click="emit('edit-sketch')">编辑草图</button>
    <button v-if="isSketchLike" @click="emit('face-normal')">正视于草图</button>
    <button @click="emit('rename')">重命名</button>
    <button @click="emit('toggle-suppress')">{{ suppressed ? '取消抑制' : '抑制' }}</button>
    <button @click="emit('delete')" class="ctx-danger">删除</button>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type { FeatureId, FeatureNode } from "@/commands/sketch";

export interface CtxTarget {
  x: number;
  y: number;
  featureId: FeatureId;
}

const props = defineProps<{
  target: CtxTarget | null;
  suppressed: boolean;
  featureType?: string | null;
}>();

const emit = defineEmits<{
  (e: "edit-sketch"): void;
  (e: "face-normal"): void;
  (e: "rename"): void;
  (e: "toggle-suppress"): void;
  (e: "delete"): void;
}>();

const isSketchLike = computed(() => {
  const t = props.featureType ?? "";
  return t === "Sketch" || t === "SketchModule" || t.startsWith("Module:") || t.startsWith("Custom:");
});
</script>
