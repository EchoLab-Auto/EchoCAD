<template>
  <div v-if="generator" class="dialog-overlay" @click.self="emit('cancel')">
    <div class="dialog">
      <h3>{{ generator.name }}</h3>
      <p class="dialog-desc">{{ generator.description }}</p>
      <div v-for="param in generator.parameters" :key="param.id" class="dialog-param">
        <label :title="param.description">{{ param.name }}</label>
        <input type="number" :min="param.min ?? undefined" :max="param.max ?? undefined"
          :step="param.step" v-model.number="localParams[param.id]" />
      </div>
      <div class="dialog-actions">
        <button class="btn-primary" @click="onRun" :disabled="loading">
          <span v-if="loading" class="btn-spinner"></span>
          {{ loading ? '生成中...' : '生成' }}
        </button>
        <button class="btn-cancel" @click="emit('cancel')" :disabled="loading">取消</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from "vue";
import type { GeneratorInfo } from "@/commands/sketch";

const props = defineProps<{
  generator: GeneratorInfo | null;
  loading: boolean;
}>();

const emit = defineEmits<{
  (e: "run", params: Record<string, number>): void;
  (e: "cancel"): void;
}>();

// Local copy of the parameter values, seeded from each param's default.
// Reset whenever a new generator is opened.
const localParams = ref<Record<string, number>>({});

watch(() => props.generator, (gen) => {
  const next: Record<string, number> = {};
  if (gen) {
    for (const param of gen.parameters) next[param.id] = param.default_value;
  }
  localParams.value = next;
}, { immediate: true });

function onRun() {
  // Hand the parent a shallow copy so it can't be mutated by the form afterwards.
  emit("run", { ...localParams.value });
}
</script>
