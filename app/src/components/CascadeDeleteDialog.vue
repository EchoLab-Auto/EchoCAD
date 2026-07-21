<template>
  <div v-if="target" class="dialog-overlay" @click.self="emit('cancel')">
    <div class="dialog">
      <h3>删除 "{{ target.name }}"？</h3>
      <p class="dialog-desc">
        该特征有 {{ target.dependents.length }} 个依赖项。删除它将同时删除：
        <strong>{{ target.dependents.join(', ') }}</strong>
      </p>
      <div class="dialog-actions">
        <button class="btn-danger" @click="emit('confirm')">级联删除</button>
        <button class="btn-cancel" @click="emit('cancel')">取消</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { FeatureId } from "@/commands/sketch";

export interface CascadeTarget {
  id: FeatureId;
  name: string;
  dependents: string[];
}

defineProps<{
  target: CascadeTarget | null;
}>();

const emit = defineEmits<{
  (e: "confirm"): void;
  (e: "cancel"): void;
}>();
</script>
