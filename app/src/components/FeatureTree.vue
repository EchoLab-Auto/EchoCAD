<template>
  <div class="panel">
    <h3>特征树
      <span v-if="regenErrorCount > 0" class="err-badge" :title="`${regenErrorCount} 个特征失败`">
        {{ regenErrorCount }} 错误
      </span>
    </h3>
    <ul class="feature-tree">
      <template v-for="feature in treeNodes" :key="feature.id">
        <li
          :class="{
            active: sketchStore.activeFeatureId === feature.id,
            selected: sketchStore.selectedFeatureId === feature.id,
            suppressed: feature.suppressed,
            errored: !!feature.errors,
            'drop-target': dropTargetId === feature.id,
          }"
          :style="{ paddingLeft: (feature.depth * 16 + 8) + 'px' }"
          draggable="true"
          @dragstart="onDragStart($event, feature)"
          @dragover.prevent="onDragOver($event, feature)"
          @dragleave="onDragLeave(feature)"
          @drop.prevent="onDrop($event, feature)"
          @click="emit('select', feature.id)"
          @dblclick="emit('edit', feature)"
          @contextmenu="emit('contextmenu', $event, feature.id)"
        >
          <button class="suppress-btn" :title="feature.suppressed ? '取消抑制' : '抑制特征'"
            @click.stop="emit('toggle-suppress', feature)">
            {{ feature.suppressed ? '◌' : '●' }}
          </button>
          <span class="fi-arrow" v-if="showArrow(feature)">└</span>
          <span class="feature-icon">{{ featureIcon(feature.feature_type) }}</span>
          <span class="fi-name">{{ feature.name }}</span>
          <span v-if="feature.errors" class="err-icon" :title="feature.errors">⚠</span>
          <button class="delete-btn" @click.stop="emit('delete', feature.id)" title="删除">×</button>
        </li>
      </template>
    </ul>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { useSketchStore } from "@/stores/sketch";
import { reparentFeature } from "@/commands/sketch";
import type { FeatureId, FeatureNode } from "@/commands/sketch";

const emit = defineEmits<{
  (e: "select", id: FeatureId): void;
  (e: "edit", feature: FeatureNode): void;
  (e: "contextmenu", event: MouseEvent, featureId: FeatureId): void;
  (e: "toggle-suppress", feature: FeatureNode): void;
  (e: "delete", id: FeatureId): void;
  (e: "reparent", childId: FeatureId, newParentId: FeatureId): void;
}>();

const sketchStore = useSketchStore();

// ── Tree computation ────────────────────────────────────────────────

interface TreeNode extends FeatureNode {
  depth: number;
}

const treeNodes = computed<TreeNode[]>(() => {
  const features = sketchStore.features;
  const result: TreeNode[] = [];
  const visited = new Set<FeatureId>();

  function addWithChildren(f: FeatureNode, depth: number) {
    if (visited.has(f.id)) return;
    visited.add(f.id);
    result.push({ ...f, depth });
    const children = features.filter(c => c.parent_id === f.id);
    for (const child of children) {
      addWithChildren(child, depth + 1);
    }
  }

  for (const f of features) {
    if (!f.parent_id) {
      addWithChildren(f, 0);
    }
  }
  // Catch any orphans (parent_id set but parent not found)
  for (const f of features) {
    if (!visited.has(f.id)) {
      addWithChildren(f, 0);
    }
  }
  return result;
});

function showArrow(f: FeatureNode): boolean {
  const t = f.feature_type;
  return t !== "Sketch" && t !== "SketchModule"
    && !t.startsWith("Custom:") && !t.startsWith("Module:");
}

// ── Drag-and-drop ───────────────────────────────────────────────────

const dragFeatureId = ref<FeatureId | null>(null);
const dropTargetId = ref<FeatureId | null>(null);

function onDragStart(e: DragEvent, feature: FeatureNode) {
  dragFeatureId.value = feature.id;
  if (e.dataTransfer) {
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", String(feature.id));
  }
}

function onDragOver(_e: DragEvent, feature: FeatureNode) {
  if (!dragFeatureId.value || dragFeatureId.value === feature.id) return;
  // Only sketch-like features can be drop targets (accept children)
  const t = feature.feature_type;
  if (t === "Sketch" || t === "SketchModule" || t.startsWith("Module:") || t.startsWith("Custom:")) {
    dropTargetId.value = feature.id;
  }
}

function onDragLeave(feature: FeatureNode) {
  if (dropTargetId.value === feature.id) {
    dropTargetId.value = null;
  }
}

async function onDrop(_e: DragEvent, target: FeatureNode) {
  dropTargetId.value = null;
  if (!dragFeatureId.value || dragFeatureId.value === target.id) return;
  const childId = dragFeatureId.value;
  dragFeatureId.value = null;
  emit("reparent", childId, target.id);
}

const regenErrorCount = computed(() =>
  sketchStore.features.filter(f => f.errors).length
);

function featureIcon(type: string): string {
  // Each feature type has a unique, identifiable icon.
  // 草图 / 拉伸 / 切除(布尔差) / 草图模块 — the four primary types.
  if (type === "Sketch")         return "📐"; // 草图
  if (type === "SketchModule")   return "🧩"; // 草图模块
  if (type === "Extrude")        return "⬆";  // 拉伸
  if (type === "Revolve")        return "🔄"; // 旋转
  if (type === "Boolean")        return "➖"; // 切除/布尔
  if (type === "Fillet")         return "🔵"; // 圆角
  if (type === "Chamfer")        return "🔻"; // 倒角
  if (type === "Sweep")          return "〰"; // 扫描
  if (type === "Shell")          return "⬡"; // 抽壳
  if (type === "LinearPattern")  return "⬌"; // 线性阵列
  if (type === "CircularPattern")return "🔁"; // 圆周阵列
  if (type === "Mirror")         return "🪞"; // 镜像
  if (type === "CustomSolid")    return "🔧"; // 自定义实体
  if (type.startsWith("Module:") || type.startsWith("Custom:")) {
    const lower = type.toLowerCase();
    if (lower.includes("gear") || lower.includes("spur")) return "⚙️";
    if (lower.includes("spring")) return "🌀";
    return "🧩";
  }
  return "📦"; // fallback
}
</script>
