<template>
  <div
    v-if="target"
    class="ctx-menu"
    :style="{ left: target.x + 'px', top: target.y + 'px' }"
    @click.stop
  >
    <button v-if="isSketchLike" @click="emit('edit-sketch')">编辑草图</button>
    <button @click="emit('rename')">重命名</button>
    <button @click="emit('toggle-suppress')">{{ suppressed ? '取消抑制' : '抑制' }}</button>
    <button class="ctx-danger" @click="emit('delete')">删除</button>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type { FeatureId } from "@/commands/sketch";

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
  (e: "rename"): void;
  (e: "toggle-suppress"): void;
  (e: "delete"): void;
}>();

const isSketchLike = computed(() => {
  const t = props.featureType ?? "";
  return t === "Sketch" || t === "SketchModule" || t.startsWith("Module:") || t.startsWith("Custom:");
});
</script>

<style scoped>
/* position: fixed — the menu floats above panels and the viewport. */
.ctx-menu {
  position: fixed;
  z-index: 400;
  background: var(--bg-menu);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  padding: 4px;
  min-width: 120px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.6);
}
.ctx-menu button {
  display: block;
  width: 100%;
  padding: 6px 12px;
  font-size: 12px;
  color: #ccc;
  background: transparent;
  border: none;
  cursor: pointer;
  text-align: left;
  border-radius: 2px;
  white-space: nowrap;
}
.ctx-menu button:hover {
  background: var(--accent);
  color: #fff;
}
.ctx-menu button.ctx-danger:hover {
  background: var(--danger);
}
</style>
