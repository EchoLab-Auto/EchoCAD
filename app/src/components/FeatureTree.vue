<template>
  <div class="panel">
    <h3>特征树
      <span v-if="regenErrorCount > 0" class="err-badge" :title="`${regenErrorCount} 个特征失败`">
        {{ regenErrorCount }} 错误
      </span>
    </h3>
    <ul class="feature-tree">
      <li
        v-for="feature in sketchStore.features"
        :key="feature.id"
        :class="{
          active: sketchStore.activeFeatureId === feature.id,
          selected: sketchStore.selectedFeatureId === feature.id,
          suppressed: feature.suppressed,
          errored: !!feature.errors,
        }"
        @click="emit('select', feature.id)"
        @dblclick="emit('edit', feature)"
        @contextmenu="emit('contextmenu', $event, feature.id)"
      >
        <button class="suppress-btn" :title="feature.suppressed ? '取消抑制' : '抑制特征'"
          @click.stop="emit('toggle-suppress', feature)">
          {{ feature.suppressed ? '◌' : '●' }}
        </button>
        <span class="fi-arrow" v-if="feature.feature_type!=='Sketch' && !feature.feature_type.startsWith('Custom:')">└</span>
        <span class="feature-icon">{{ featureIcon(feature.feature_type) }}</span>
        <span class="fi-name">{{ feature.name }}</span>
        <span v-if="feature.errors" class="err-icon" :title="feature.errors">⚠</span>
        <button class="delete-btn" @click.stop="emit('delete', feature.id)" title="删除">×</button>
      </li>
    </ul>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useSketchStore } from "@/stores/sketch";
import type { FeatureId, FeatureNode } from "@/commands/sketch";

const emit = defineEmits<{
  (e: "select", id: FeatureId): void;
  (e: "edit", feature: FeatureNode): void;
  (e: "contextmenu", event: MouseEvent, featureId: FeatureId): void;
  (e: "toggle-suppress", feature: FeatureNode): void;
  (e: "delete", id: FeatureId): void;
}>();

const sketchStore = useSketchStore();

const regenErrorCount = computed(() =>
  sketchStore.features.filter(f => f.errors).length
);

function featureIcon(type: string): string {
  if (type === "Sketch") return "📐";
  if (type === "Extrude") return "🧊";
  if (type === "Revolve") return "🔄";
  if (type === "Fillet") return "🔵";
  if (type === "Chamfer") return "🔻";
  if (type === "LinearPattern") return "↔️";
  if (type === "CircularPattern") return "🔁";
  if (type === "Mirror") return "🪞";
  if (type === "Sweep") return "〰️";
  if (type === "Shell") return "🫙";
  if (type.startsWith("Custom:") || type.startsWith("CustomSolid:")) {
    const lower = type.toLowerCase();
    if (lower.includes("gear") || lower.includes("spur")) return "⚙️";
    if (lower.includes("spring")) return "🌀";
    return "🔧";
  }
  return "📦";
}
</script>
