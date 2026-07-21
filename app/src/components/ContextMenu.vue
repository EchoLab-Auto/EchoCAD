<template>
  <div
    v-if="target"
    class="ctx-menu"
    :style="{ left: target.x + 'px', top: target.y + 'px' }"
    @click.stop
  >
    <button @click="emit('edit-sketch')">编辑草图</button>
    <button @click="emit('face-normal')">正视于草图</button>
    <button @click="emit('rename')">重命名</button>
    <button @click="emit('toggle-suppress')">{{ suppressed ? '取消抑制' : '抑制' }}</button>
    <button @click="emit('delete')" class="ctx-danger">删除</button>
  </div>
</template>

<script setup lang="ts">
import type { FeatureId } from "@/commands/sketch";

export interface CtxTarget {
  x: number;
  y: number;
  featureId: FeatureId;
}

defineProps<{
  target: CtxTarget | null;
  suppressed: boolean;
}>();

const emit = defineEmits<{
  (e: "edit-sketch"): void;
  (e: "face-normal"): void;
  (e: "rename"): void;
  (e: "toggle-suppress"): void;
  (e: "delete"): void;
}>();
</script>
