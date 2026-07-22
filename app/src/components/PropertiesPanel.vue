<template>
  <div class="panel">
    <h3>属性</h3>

    <!-- Extrude configuration panel -->
    <ExtrudePanel
      v-if="sketchStore.showExtrudePanel"
      :direction="sketchStore.extrudeConfig.direction"
      :depth="sketchStore.extrudeConfig.depth"
      :dist2="sketchStore.extrudeConfig.dist2"
      :draft="sketchStore.extrudeConfig.draft"
      @change="onExtrudeConfigChange"
      @confirm="emit('extrude-confirm')"
      @cancel="emit('extrude-cancel')"
    />

    <!-- Feature properties -->
    <div v-else-if="selectedFeature" class="prop-section">
      <div class="prop-row"><label>类型</label><span>{{ selectedFeature.feature_type }}</span></div>
      <div class="prop-row">
        <label>名称</label>
        <input type="text" :value="selectedFeature.name"
          @change="emit('rename', ($event.target as HTMLInputElement).value)" />
      </div>
      <!-- Per-feature color picker (solid-producing features only).
           TODO: add backend command to persist feature color.
           Currently stored in sketchStore.featureColors (UI-only). -->
      <div v-if="isSolidFeature" class="prop-row color-row">
        <label>颜色</label>
        <div class="color-picker-wrap">
          <input
            type="color"
            :value="featureColorHex"
            @input="onColorChange($event)"
            class="color-input"
          />
          <span class="color-hex">{{ featureColorHex }}</span>
          <button
            v-if="hasCustomColor"
            class="color-reset-btn"
            @click="resetColor"
            title="重置为默认颜色"
          >↩</button>
        </div>
      </div>
      <div v-if="selectedFeature.errors" class="prop-error">
        ⚠ {{ selectedFeature.errors }}
      </div>

      <!-- Parameter list.
           Param-driven features (Extrude depth, Fillet radius, Chamfer
           distance, Shell thickness, Revolve angle) expose a *real*
           ParameterId (id !== 0) and update_parameter can mutate them —
           these get a slider+number scrubber. Virtual params (id === 0,
           e.g. LinearPattern count/spacing) are projections of fields the
           backend owns but can't yet mutate via update_parameter, so they
           render read-only to avoid a dead, misleading input. -->
      <div v-for="param in selectedFeature.parameters" :key="param.id + '-' + param.name" class="prop-row param-edit-row">
        <label>{{ param.name }}</label>
        <template v-if="canEditParam(param)">
          <input type="range" class="param-slider"
            :min="paramRange(param.name).min" :max="paramRange(param.name).max" :step="paramRange(param.name).step"
            :value="param.value" @input="emit('update-param', param.id, $event)" />
          <input type="number" class="param-num-input"
            :step="paramRange(param.name).step" :value="param.value"
            @change="emit('update-param', param.id, $event)" />
        </template>
        <span v-else class="ro-val">{{ formatParamValue(param.value) }}</span>
      </div>

      <div class="prop-row" v-if="selectedFeature.has_dependents">
        <label></label>
        <span class="has-dep" title="其他特征依赖于此特征">⚠ 有依赖</span>
      </div>

      <!-- Edge-pick section: shown for Fillet/Chamfer features (editing)
           and during edge-pick creation mode for fillet/chamfer. -->
      <div
        v-if="isEdgePickFeature"
        class="edge-pick-section"
      >
        <div class="edge-pick-header">边缘选择</div>

        <!-- In creation mode: show selected edges + params + confirm/cancel -->
        <template v-if="sketchStore.edgePickMode && sketchStore.pendingFilletChamferType">
          <div v-if="sketchStore.pendingEdges.length === 0" class="edge-pick-empty">
            点击视口中的边缘来选择（可选：留空则选择全部锐边）
          </div>
          <div v-else class="edge-list">
            <div
              v-for="(edge, i) in sketchStore.pendingEdges"
              :key="i"
              class="edge-item"
            >
              <span class="edge-label">边 ({{ edge[0] }}, {{ edge[1] }})</span>
              <button
                class="edge-remove-btn"
                title="移除此边"
                @click="sketchStore.removePickedEdge(i)"
              >✕</button>
            </div>
          </div>

          <!-- Radius / Distance parameter for the pending feature -->
          <div class="prop-row edge-param-row">
            <label>{{ sketchStore.pendingFilletChamferType === "fillet" ? "半径" : "距离" }}</label>
            <input
              type="number"
              step="0.1"
              min="0.1"
              :value="edgeParamValue"
              @input="edgeParamValue = parseFloat(($event.target as HTMLInputElement).value) || 0.5"
              class="edge-param-input"
            />
          </div>

          <div class="edge-pick-actions">
            <button class="btn-confirm-edge" @click="emit('confirm-edge-pick', edgeParamValue)">
              {{ sketchStore.pendingFilletChamferType === "fillet" ? "确认圆角" : "确认倒角" }}
            </button>
            <button class="btn-cancel-edge" @click="emit('exit-edge-pick')">取消</button>
          </div>
        </template>

        <!-- Editing an existing Fillet/Chamfer: show edge count + re-pick button -->
        <template v-else>
          <div class="edge-pick-info">
            <span v-if="selectedFeature">已选择全部锐边</span>
          </div>
          <button class="btn-pick-edges" @click="emit('enter-edge-pick')">
            选择边 / Pick edges
          </button>
        </template>
      </div>
    </div>

    <!-- Entity properties -->
    <div v-else-if="selectedEntity" class="prop-section">
      <div class="prop-row"><label>类型</label><span>{{ selectedEntity.type }}</span></div>
      <div class="prop-row"><label>ID</label><span>{{ selectedEntity.id }}</span></div>
      <div v-if="selectedPoint" class="prop-row">
        <label>X</label>
        <input type="number" step="0.1" :value="selectedPoint.x"
          @change="emit('update-entity-prop', selectedPoint.id, 'x', $event)" />
      </div>
      <div v-if="selectedPoint" class="prop-row">
        <label>Y</label>
        <input type="number" step="0.1" :value="selectedPoint.y"
          @change="emit('update-entity-prop', selectedPoint.id, 'y', $event)" />
      </div>
      <div v-if="selectedRound" class="prop-row">
        <label>半径</label>
        <input type="number" step="0.1" :value="selectedRound.radius"
          @change="emit('update-entity-prop', selectedRound.id, 'radius', $event)" />
      </div>
      <div v-if="selectedLine" class="prop-row">
        <label>长度</label><span>{{ lineLength(selectedLine).toFixed(2) }}</span>
      </div>
      <label v-if="selectedConstructible" class="prop-check">
        <input type="checkbox" :checked="!!selectedConstructible.construction"
          @change="emit('toggle-construction', selectedConstructible.id, $event)" />
        构造线
      </label>
      <button class="prop-del-btn" @click="emit('delete-entity')">删除实体 (Del)</button>
    </div>

    <p v-else class="placeholder">选择特征或草图实体</p>

    <!-- Constraint manager: shown only while editing a sketch (and not
         while the extrude config panel is up, where it would be noise).
         Each row is one constraint from store.constraints with a ✕ to
         delete it; the bottom button clears all of them. Deletion is
         delegated up to HomeView, which calls removeConstraint(index)
         then refreshSketch() so the store re-syncs before the next read
         (design-principle #6.1, read-your-write). -->
    <div v-if="showConstraintList" class="constraint-section">
      <div class="constraint-header">
        约束 <span class="constraint-count">{{ sketchStore.constraints.length }}</span>
      </div>
      <div v-if="sketchStore.constraints.length === 0" class="constraint-empty">
        当前草图无约束
      </div>
      <div v-else class="constraint-list">
        <div
          v-for="(c, i) in sketchStore.constraints"
          :key="i"
          class="constraint-row"
        >
          <span class="constraint-desc">{{ describeConstraint(c) }}</span>
          <button
            class="constraint-del-btn"
            title="删除该约束"
            @click="emit('delete-constraint', i)"
          >✕</button>
        </div>
        <button
          class="constraint-clear-btn"
          @click="emit('clear-constraints')"
        >清空全部约束</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from "vue";
import ExtrudePanel from "@/components/ExtrudePanel.vue";
import { useSketchStore } from "@/stores/sketch";
import type {
  Constraint,
  EntityId,
  ParameterId,
  ParameterInfo,
  SketchEntity,
} from "@/commands/sketch";

type ExtrudeConfig = { direction: string; depth: number; dist2: number; draft: number };

const emit = defineEmits<{
  (e: "rename", name: string): void;
  (e: "update-param", id: ParameterId, event: Event): void;
  (e: "update-entity-prop", id: EntityId, prop: string, event: Event): void;
  (e: "toggle-construction", id: EntityId, event: Event): void;
  (e: "delete-entity"): void;
  (e: "delete-constraint", index: number): void;
  (e: "clear-constraints"): void;
  (e: "extrude-change", config: ExtrudeConfig): void;
  (e: "extrude-confirm"): void;
  (e: "extrude-cancel"): void;
  (e: "enter-edge-pick"): void;
  (e: "exit-edge-pick"): void;
  (e: "confirm-edge-pick", radius: number): void;
  (e: "remove-picked-edge", index: number): void;
}>();

const sketchStore = useSketchStore();

const selectedFeature = computed(() => sketchStore.selectedFeature);

/// True when we should show the edge-pick section: either the selected
/// feature is a Fillet/Chamfer, or we are in edge-pick creation mode.
const isEdgePickFeature = computed(() => {
  if (sketchStore.edgePickMode) return true;
  const f = selectedFeature.value;
  if (!f) return false;
  return f.feature_type === "Fillet" || f.feature_type === "Chamfer";
});

/// Default radius/distance for the edge-pick creation UI.
const edgeParamValue = ref(0.5);

/// True when the selected feature is a solid-producing feature
/// (anything that isn't a Sketch or CustomSketch). Color picker is
/// only relevant for solids.
const isSolidFeature = computed(() => {
  if (!selectedFeature.value) return false;
  const ft = selectedFeature.value.feature_type;
  return ft !== "Sketch" && !ft.startsWith("Custom:");
});

/// Current hex color for the selected feature. Falls back to "#cccccc"
/// (default gray) when not set.
const featureColorHex = computed(() => {
  if (!selectedFeature.value) return "#cccccc";
  return sketchStore.featureColors[selectedFeature.value.id] ?? "#cccccc";
});

/// Whether this feature has a custom color set in the store.
const hasCustomColor = computed(() => {
  if (!selectedFeature.value) return false;
  return selectedFeature.value.id in sketchStore.featureColors;
});

/// Called when the native color input changes. Stores the selected color
/// in the featureColors map and persists it to the backend.
function onColorChange(event: Event) {
  const input = event.target as HTMLInputElement;
  const hex = input.value;
  if (selectedFeature.value) {
    sketchStore.persistFeatureColor(selectedFeature.value.id, hex);
  }
}

/// Remove the custom color override so the feature goes back to the
/// default palette color.
function resetColor() {
  if (!selectedFeature.value) return;
  sketchStore.persistFeatureColor(selectedFeature.value.id, null);
}

const selectedEntity = computed<SketchEntity | null>(() => {
  if (!sketchStore.selectedId) return null;
  return sketchStore.entities.find(e => e.id === sketchStore.selectedId) || null;
});

// Type-narrowed views of the selected entity, so the template never needs
// an `as any` cast (SketchEntity is a discriminated union on `type`).
type PointEntity = Extract<SketchEntity, { type: "Point" }>;
type LineEntity = Extract<SketchEntity, { type: "Line" }>;
type CircleEntity = Extract<SketchEntity, { type: "Circle" }>;
type ArcEntity = Extract<SketchEntity, { type: "Arc" }>;

const selectedPoint = computed<PointEntity | null>(() =>
  selectedEntity.value?.type === "Point" ? selectedEntity.value : null
);
const selectedLine = computed<LineEntity | null>(() =>
  selectedEntity.value?.type === "Line" ? selectedEntity.value : null
);
const selectedRound = computed<CircleEntity | ArcEntity | null>(() => {
  const e = selectedEntity.value;
  if (!e) return null;
  return e.type === "Circle" || e.type === "Arc" ? e : null;
});
const selectedConstructible = computed<LineEntity | CircleEntity | null>(() => {
  const e = selectedEntity.value;
  if (!e) return null;
  return e.type === "Line" || e.type === "Circle" ? e : null;
});

function lineLength(line: LineEntity): number {
  return Math.sqrt((line.x2 - line.x1) ** 2 + (line.y2 - line.y1) ** 2);
}

/// True only while a sketch is open for editing AND the extrude config
/// panel is not occupying the properties area. Drives the constraint list.
const showConstraintList = computed(() =>
  sketchStore.isEditingSketch() && !sketchStore.showExtrudePanel
);

/// Short, human-readable label for an entity, e.g. `P1`, `Line3`, `Circle2`.
/// Used by [`describeConstraint`] so the constraint list reads like the
/// examples in the spec ("Horizontal: Line3", "Distance(P1,P2)=2.0").
function entityLabel(e: SketchEntity): string {
  switch (e.type) {
    case "Point": return `P${e.id}`;
    case "Line": return `Line${e.id}`;
    case "Circle": return `Circle${e.id}`;
    case "Arc": return `Arc${e.id}`;
    case "Ellipse": return `Ellipse${e.id}`;
    case "Spline": return `Spline${e.id}`;
  }
}

/// Label for an entity id, looking the entity up in the current sketch.
/// Falls back to `#id` if the entity is missing (e.g. stale constraint).
function entityLabelById(id: EntityId): string {
  const e = sketchStore.entities.find(en => en.id === id);
  return e ? entityLabel(e) : `#${id}`;
}

/// Format `value` without a trailing `.00`, so 2 stays "2" and 2.5 stays "2.5".
function fmtNum(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(2);
}

/// Human-readable description of a constraint. Covers every variant of the
/// Constraint union — the trailing `_exhaustive: never` assertion makes the
/// compiler flag this function if a new variant is added without a case.
function describeConstraint(c: Constraint): string {
  if ("Coincident" in c) return `Coincident(${entityLabelById(c.Coincident.a)}, ${entityLabelById(c.Coincident.b)})`;
  if ("Horizontal" in c) return `Horizontal: ${entityLabelById(c.Horizontal.line)}`;
  if ("Vertical" in c) return `Vertical: ${entityLabelById(c.Vertical.line)}`;
  if ("Distance" in c) return `Distance(${entityLabelById(c.Distance.a)}, ${entityLabelById(c.Distance.b)}) = ${fmtNum(c.Distance.distance)}`;
  if ("Radius" in c) return `Radius(${entityLabelById(c.Radius.circle)}) = ${fmtNum(c.Radius.radius)}`;
  if ("Parallel" in c) return `Parallel(${entityLabelById(c.Parallel.line_a)}, ${entityLabelById(c.Parallel.line_b)})`;
  if ("Perpendicular" in c) return `Perpendicular(${entityLabelById(c.Perpendicular.line_a)}, ${entityLabelById(c.Perpendicular.line_b)})`;
  if ("Tangent" in c) return `Tangent(${entityLabelById(c.Tangent.line)}, ${entityLabelById(c.Tangent.circle)})`;
  if ("Concentric" in c) return `Concentric(${entityLabelById(c.Concentric.a)}, ${entityLabelById(c.Concentric.b)})`;
  if ("Equal" in c) return `Equal(${entityLabelById(c.Equal.a)}, ${entityLabelById(c.Equal.b)})`;
  if ("Fix" in c) return `Fix(${entityLabelById(c.Fix.point)})`;
  if ("Midpoint" in c) return `Midpoint(${entityLabelById(c.Midpoint.point)} @ ${entityLabelById(c.Midpoint.line)})`;
  if ("Symmetric" in c) return `Symmetric(${entityLabelById(c.Symmetric.a)}, ${entityLabelById(c.Symmetric.b)})`;
  if ("PointOnLine" in c) return `PointOnLine(${entityLabelById(c.PointOnLine.point)}, ${entityLabelById(c.PointOnLine.line)})`;
  if ("Collinear" in c) return `Collinear(${entityLabelById(c.Collinear.a)}, ${entityLabelById(c.Collinear.b)}, ${entityLabelById(c.Collinear.c)})`;
  if ("Angle" in c) return `Angle(${entityLabelById(c.Angle.line_a)}, ${entityLabelById(c.Angle.line_b)}) = ${fmtNum(c.Angle.angle_deg)}°`;
  if ("Diameter" in c) return `Diameter(${entityLabelById(c.Diameter.circle)}) = ${fmtNum(c.Diameter.diameter)}`;
  // Exhaustiveness guard: adding a new Constraint variant without a branch
  // above makes this assignment fail to type-check.
  const _exhaustive: never = c;
  void _exhaustive;
  return "Constraint";
}

function onExtrudeConfigChange(config: ExtrudeConfig) {
  emit("extrude-change", config);
}

/// A parameter is live-editable only when the backend gave it a real id —
/// virtual params (id === 0, e.g. LinearPattern's count/spacing projections)
/// have no `update_parameter` target, so editing them would be a no-op.
function canEditParam(param: ParameterInfo): boolean {
  return !param.readonly && param.id !== 0;
}

/// Slider/number bounds keyed off the parameter name. Angles go up to 360°;
/// dimensions (radius/distance/thickness/depth) top out at 20 — the number
/// input remains authoritative for values outside the slider's range.
function paramRange(name: string): { min: number; max: number; step: number } {
  const lower = name.toLowerCase();
  if (lower.includes("angle")) return { min: 0, max: 360, step: 1 };
  return { min: 0.1, max: 20, step: 0.1 };
}

/// Integer-valued params (count) display without a decimal tail.
function formatParamValue(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(2);
}
</script>

<style scoped>
/* The parameter scrubber row: label (40px, shared with global .prop-row) +
   a flexing range slider + a compact number input. Wraps on narrow widths
   so the sidebar never clips the number box. These rules override the
   global `.prop-row input { width: 130px }` via higher specificity. */
.prop-row.param-edit-row {
  flex-wrap: wrap;
}
.prop-row.param-edit-row .param-slider {
  flex: 1 1 80px;
  min-width: 60px;
  width: auto;
  accent-color: #007acc;
}
.prop-row.param-edit-row .param-num-input {
  width: 55px;
}

/* ── Constraint manager ───────────────────────────────────────── */
.constraint-section {
  margin-top: 14px;
  padding-top: 10px;
  border-top: 1px solid #3c3c3c;
}
.constraint-header {
  font-size: 11px;
  text-transform: uppercase;
  color: #999;
  letter-spacing: 0.5px;
  margin-bottom: 8px;
  display: flex;
  align-items: center;
  gap: 6px;
}
.constraint-count {
  background: #3c3c3c;
  color: #ccc;
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 8px;
  letter-spacing: 0;
}
.constraint-empty {
  font-size: 11px;
  color: #777;
  font-style: italic;
}
.constraint-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 260px;
  overflow-y: auto;
}
.constraint-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 6px;
  border-radius: 2px;
  font-size: 11px;
}
.constraint-row:hover {
  background: #2d2d2d;
}
.constraint-desc {
  flex: 1;
  color: #ccc;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.constraint-del-btn {
  background: transparent;
  border: none;
  color: #888;
  cursor: pointer;
  font-size: 12px;
  line-height: 1;
  padding: 2px 4px;
  border-radius: 2px;
  opacity: 0.6;
}
.constraint-del-btn:hover {
  color: #ff6b6b;
  background: #3a1a1a;
  opacity: 1;
}
.constraint-clear-btn {
  margin-top: 8px;
  padding: 4px 10px;
  background: #3c3c3c;
  border: 1px solid #555;
  color: #ddd;
  font-size: 11px;
  border-radius: 2px;
  cursor: pointer;
}
.constraint-clear-btn:hover {
  background: #5a2a2a;
  border-color: #833;
  color: #f88;
}

/* ── Color picker ──────────────────────────────────────────────── */
.prop-row.color-row {
  align-items: center;
}
.color-picker-wrap {
  display: flex;
  align-items: center;
  gap: 6px;
}
.color-input {
  width: 28px;
  height: 24px;
  border: 1px solid #555;
  border-radius: 2px;
  padding: 0;
  cursor: pointer;
  background: transparent;
}
.color-input::-webkit-color-swatch-wrapper {
  padding: 0;
}
.color-input::-webkit-color-swatch {
  border: none;
}
.color-hex {
  font-size: 11px;
  color: #999;
  font-family: monospace;
}
.color-reset-btn {
  background: transparent;
  border: 1px solid #555;
  color: #999;
  font-size: 12px;
  cursor: pointer;
  border-radius: 2px;
  padding: 1px 5px;
  line-height: 1.4;
}
.color-reset-btn:hover {
  background: #444;
  color: #fff;
}

/* ── Edge-pick section ─────────────────────────────────────────── */
.edge-pick-section {
  margin-top: 12px;
  padding-top: 10px;
  border-top: 1px solid #3c3c3c;
}
.edge-pick-header {
  font-size: 11px;
  text-transform: uppercase;
  color: #999;
  letter-spacing: 0.5px;
  margin-bottom: 8px;
}
.edge-pick-empty {
  font-size: 11px;
  color: #777;
  font-style: italic;
  margin-bottom: 8px;
}
.edge-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 160px;
  overflow-y: auto;
  margin-bottom: 8px;
}
.edge-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 2px 6px;
  border-radius: 2px;
  font-size: 11px;
  background: #2a2a2a;
}
.edge-label {
  color: #4fc3f7;
  font-family: monospace;
}
.edge-remove-btn {
  background: transparent;
  border: none;
  color: #888;
  cursor: pointer;
  font-size: 12px;
  padding: 2px 4px;
  border-radius: 2px;
  opacity: 0.6;
}
.edge-remove-btn:hover {
  color: #ff6b6b;
  background: #3a1a1a;
  opacity: 1;
}
.edge-param-row {
  margin: 8px 0;
}
.edge-param-input {
  width: 80px !important;
}
.edge-pick-actions {
  display: flex;
  gap: 8px;
  margin-top: 10px;
}
.btn-confirm-edge {
  background: #007acc;
  border: none;
  color: white;
  padding: 5px 14px;
  font-size: 12px;
  border-radius: 3px;
  cursor: pointer;
}
.btn-confirm-edge:hover {
  background: #0098ff;
}
.btn-cancel-edge {
  background: #3c3c3c;
  border: 1px solid #555;
  color: #ddd;
  padding: 5px 14px;
  font-size: 12px;
  border-radius: 3px;
  cursor: pointer;
}
.btn-cancel-edge:hover {
  background: #555;
}
.btn-pick-edges {
  margin-top: 6px;
  padding: 4px 12px;
  background: #3c3c3c;
  border: 1px solid #4fc3f7;
  color: #4fc3f7;
  font-size: 11px;
  border-radius: 2px;
  cursor: pointer;
}
.btn-pick-edges:hover {
  background: #007acc;
  border-color: #007acc;
  color: #fff;
}
.edge-pick-info {
  font-size: 11px;
  color: #888;
  margin-bottom: 4px;
}
</style>
