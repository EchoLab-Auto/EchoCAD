<template>
  <div ref="root" class="dim-overlay">
    <svg class="dim-svg">
      <g v-for="(d, i) in dimensions" :key="i">
        <template v-if="d.kind === 'distance'">
          <line
            :x1="d.fromScreen.x" :y1="d.fromScreen.y"
            :x2="d.toScreen.x" :y2="d.toScreen.y"
            class="dim-line"
          />
          <circle :cx="d.fromScreen.x" :cy="d.fromScreen.y" r="2" class="dim-tick" />
          <circle :cx="d.toScreen.x" :cy="d.toScreen.y" r="2" class="dim-tick" />
          <g :transform="`translate(${d.labelPos.x}, ${d.labelPos.y})`">
            <rect
              x="-22" y="-9" width="44" height="18" rx="2"
              class="dim-label-bg"
              @click="onLabelClick(d)"
            />
            <text
              class="dim-label"
              text-anchor="middle" dominant-baseline="central"
              @click="onLabelClick(d)"
            >{{ d.value.toFixed(2) }}</text>
          </g>
        </template>

        <template v-else-if="d.kind === 'radius'">
          <line
            :x1="d.centerScreen.x" :y1="d.centerScreen.y"
            :x2="d.edgeScreen.x" :y2="d.edgeScreen.y"
            class="dim-line dim-radius-line"
          />
          <g :transform="`translate(${d.labelPos.x}, ${d.labelPos.y})`">
            <rect
              x="-26" y="-9" width="52" height="18" rx="2"
              class="dim-label-bg"
              @click="onLabelClick(d)"
            />
            <text
              class="dim-label"
              text-anchor="middle" dominant-baseline="central"
              @click="onLabelClick(d)"
            >R {{ d.value.toFixed(2) }}</text>
          </g>
        </template>

        <template v-else-if="d.kind === 'angle'">
          <path :d="d.arcPath" class="dim-arc" />
          <g :transform="`translate(${d.labelPos.x}, ${d.labelPos.y})`">
            <rect
              x="-22" y="-9" width="44" height="18" rx="2"
              class="dim-label-bg"
              @click="onLabelClick(d)"
            />
            <text
              class="dim-label"
              text-anchor="middle" dominant-baseline="central"
              @click="onLabelClick(d)"
            >{{ d.value.toFixed(0) }}°</text>
          </g>
        </template>
      </g>
    </svg>

    <div
      v-if="editing"
      class="dim-edit"
      :style="{ left: editing.labelPos.x + 'px', top: editing.labelPos.y + 'px' }"
    >
      <input
        ref="editInput"
        type="number"
        step="0.1"
        :value="editing.value"
        @change="commitEdit(($event.target as HTMLInputElement).value)"
        @blur="cancelEdit"
        @keydown.enter="commitEdit(($event.target as HTMLInputElement).value)"
        @keydown.escape="cancelEdit"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, nextTick } from "vue";
import type { Constraint, SketchEntity } from "@/commands/sketch";
import { updateConstraintValue, solveSketch } from "@/commands/sketch";

/**
 * Projection function supplied by the parent. We deliberately accept a
 * function (instead of the raw THREE camera/renderer) to avoid Vue's
 * reactivity proxying Three.js objects, which breaks them.
 */
type ProjectFn = (sx: number, sy: number) => { x: number; y: number } | null;

const props = defineProps<{
  entities: SketchEntity[];
  constraints: Constraint[];
  visible: boolean;
  project: ProjectFn;
}>();

const emit = defineEmits<{ (e: "refresh"): void }>();

const root = ref<HTMLDivElement | null>(null);
const editing = ref<Dimension | null>(null);
const editInput = ref<HTMLInputElement | null>(null);

type Pt = { x: number; y: number };
type DistanceDim = {
  kind: "distance";
  value: number;
  fromScreen: Pt;
  toScreen: Pt;
  labelPos: Pt;
  constraint: Constraint;
};
type RadiusDim = {
  kind: "radius";
  value: number;
  centerScreen: Pt;
  edgeScreen: Pt;
  labelPos: Pt;
  constraint: Constraint;
};
type AngleDim = {
  kind: "angle";
  value: number;
  arcPath: string;
  labelPos: Pt;
  constraint: Constraint;
};
type Dimension = DistanceDim | RadiusDim | AngleDim;

function project(x: number, y: number): Pt | null {
  return props.project(x, y);
}

const dimensions = computed<Dimension[]>(() => {
  if (!props.visible) return [];
  const result: Dimension[] = [];
  const ents = new Map(props.entities.map(e => [e.id, e]));

  for (const c of props.constraints) {
    if ("Distance" in c) {
      const a = ents.get(c.Distance.a);
      const b = ents.get(c.Distance.b);
      if (a?.type !== "Point" || b?.type !== "Point") continue;
      const sa = project(a.x, a.y);
      const sb = project(b.x, b.y);
      if (!sa || !sb) continue;
      const mx = (sa.x + sb.x) / 2;
      const my = (sa.y + sb.y) / 2;
      const dx = sb.x - sa.x;
      const dy = sb.y - sa.y;
      const len = Math.hypot(dx, dy) || 1;
      const off = 18;
      result.push({
        kind: "distance",
        value: c.Distance.distance,
        fromScreen: sa,
        toScreen: sb,
        labelPos: { x: mx + (-dy / len) * off, y: my + (dx / len) * off },
        constraint: c,
      });
    } else if ("Diameter" in c) {
      const circ = ents.get(c.Diameter.circle);
      if (circ?.type !== "Circle") continue;
      const sc = project(circ.cx, circ.cy);
      const se = project(circ.cx + circ.radius, circ.cy);
      if (!sc || !se) continue;
      result.push({
        kind: "radius",
        value: c.Diameter.diameter / 2,
        centerScreen: sc,
        edgeScreen: se,
        labelPos: { x: (sc.x + se.x) / 2, y: (sc.y + se.y) / 2 - 14 },
        constraint: c,
      });
    } else if ("Radius" in c) {
      const circ = ents.get(c.Radius.circle);
      if (circ?.type !== "Circle") continue;
      const sc = project(circ.cx, circ.cy);
      const se = project(circ.cx + circ.radius, circ.cy);
      if (!sc || !se) continue;
      result.push({
        kind: "radius",
        value: c.Radius.radius,
        centerScreen: sc,
        edgeScreen: se,
        labelPos: { x: (sc.x + se.x) / 2, y: (sc.y + se.y) / 2 - 14 },
        constraint: c,
      });
    } else if ("Angle" in c) {
      const la = ents.get(c.Angle.line_a);
      const lb = ents.get(c.Angle.line_b);
      if (la?.type !== "Line" || lb?.type !== "Line") continue;
      const inter = lineIntersect(la, lb);
      if (!inter) continue;
      const sint = project(inter.x, inter.y);
      if (!sint) continue;
      const a_end = pointAlongLine(la, inter, 1.0);
      const b_end = pointAlongLine(lb, inter, 1.0);
      const sea = project(a_end.x, a_end.y);
      const seb = project(b_end.x, b_end.y);
      if (!sea || !seb) continue;
      const r = 18;
      const arcPath = describeArc(sint, sea, seb, r);
      result.push({
        kind: "angle",
        value: c.Angle.angle_deg,
        arcPath,
        labelPos: {
          x: sint.x + (sea.x - sint.x + seb.x - sint.x) / 2 * 0.7,
          y: sint.y + (sea.y - sint.y + seb.y - sint.y) / 2 * 0.7,
        },
        constraint: c,
      });
    }
  }
  return result;
});

type LineEntity = Extract<SketchEntity, { type: "Line" }>;

function lineIntersect(a: LineEntity, b: LineEntity): Pt | null {
  const d = (a.x1 - a.x2) * (b.y1 - b.y2) - (a.y1 - a.y2) * (b.x1 - b.x2);
  if (Math.abs(d) < 1e-10) return null;
  const t = ((a.x1 - b.x1) * (b.y1 - b.y2) - (a.y1 - b.y1) * (b.x1 - b.x2)) / d;
  return { x: a.x1 + t * (a.x2 - a.x1), y: a.y1 + t * (a.y2 - a.y1) };
}

function pointAlongLine(line: LineEntity, from: Pt, dist: number): Pt {
  const d1 = Math.hypot(line.x1 - from.x, line.y1 - from.y);
  const d2 = Math.hypot(line.x2 - from.x, line.y2 - from.y);
  const fx = d2 > d1 ? line.x2 : line.x1;
  const fy = d2 > d1 ? line.y2 : line.y1;
  const dx = fx - from.x;
  const dy = fy - from.y;
  const len = Math.hypot(dx, dy) || 1;
  return { x: from.x + (dx / len) * dist, y: from.y + (dy / len) * dist };
}

function describeArc(center: Pt, a: Pt, b: Pt, r: number): string {
  const ua = Math.atan2(a.y - center.y, a.x - center.x);
  const ub = Math.atan2(b.y - center.y, b.x - center.x);
  const start = { x: center.x + r * Math.cos(ua), y: center.y + r * Math.sin(ua) };
  const end = { x: center.x + r * Math.cos(ub), y: center.y + r * Math.sin(ub) };
  let delta = ub - ua;
  if (delta < -Math.PI) delta += 2 * Math.PI;
  if (delta > Math.PI) delta -= 2 * Math.PI;
  const largeArc = Math.abs(delta) > Math.PI ? 1 : 0;
  const sweep = delta > 0 ? 1 : 0;
  return `M ${start.x} ${start.y} A ${r} ${r} 0 ${largeArc} ${sweep} ${end.x} ${end.y}`;
}

function onLabelClick(d: Dimension) {
  editing.value = d;
  nextTick(() => {
    editInput.value?.focus();
    editInput.value?.select();
  });
}

async function commitEdit(raw: string) {
  const v = parseFloat(raw);
  const d = editing.value;
  editing.value = null;
  if (Number.isNaN(v) || !d) return;
  const c = d.constraint;
  // Use updateConstraintValue so the existing constraint is edited in place —
  // plain addConstraint would stack a duplicate on top of the old one.
  if ("Distance" in c) {
    await updateConstraintValue({ Distance: { ...c.Distance, distance: v } });
  } else if ("Radius" in c) {
    await updateConstraintValue({ Radius: { ...c.Radius, radius: v } });
  } else if ("Diameter" in c) {
    await updateConstraintValue({ Diameter: { ...c.Diameter, diameter: v } });
  } else if ("Angle" in c) {
    await updateConstraintValue({ Angle: { ...c.Angle, angle_deg: v } });
  }
  await solveSketch();
  emit("refresh");
}

function cancelEdit() {
  editing.value = null;
}
</script>

<style scoped>
.dim-overlay {
  position: absolute; inset: 0; pointer-events: none; z-index: 5;
}
.dim-svg {
  position: absolute; inset: 0; width: 100%; height: 100%;
  pointer-events: none;
}
.dim-line {
  stroke: #4fc3f7; stroke-width: 1; fill: none;
  stroke-dasharray: 4 2; opacity: 0.85;
}
.dim-radius-line { stroke-dasharray: none; }
.dim-arc { stroke: #ffd54f; stroke-width: 1.2; fill: none; opacity: 0.9; }
.dim-tick { fill: #4fc3f7; }
.dim-label-bg {
  fill: #1e1e1e; stroke: #4fc3f7; stroke-width: 0.6;
  pointer-events: auto; cursor: pointer;
}
.dim-label {
  fill: #4fc3f7; font-size: 10px; pointer-events: auto; cursor: pointer;
  user-select: none;
}
.dim-edit {
  position: absolute; transform: translate(-50%, -50%);
  pointer-events: auto;
}
.dim-edit input {
  width: 60px; padding: 1px 3px;
  background: #007acc; border: 1px solid #4fc3f7; color: #fff;
  font-size: 11px; border-radius: 2px; text-align: center;
}
</style>
