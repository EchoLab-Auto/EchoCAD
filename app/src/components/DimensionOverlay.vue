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

      <!-- Non-numeric constraint glyphs. Informational only — every element
           has pointer-events:none so it never steals clicks from entities. -->
      <g v-for="g in glyphs" :key="g.key">
        <line v-if="g.axis"
          :x1="g.axis.x1" :y1="g.axis.y1"
          :x2="g.axis.x2" :y2="g.axis.y2"
          class="glyph-axis" />
        <template v-if="g.dot">
          <circle :cx="g.dot.x" :cy="g.dot.y" r="3.5" class="glyph-dot-ring" />
          <circle :cx="g.dot.x" :cy="g.dot.y" r="1.6" class="glyph-dot" />
        </template>
        <template v-for="(c, ci) in g.circles ?? []" :key="'c' + ci">
          <circle :cx="c.x" :cy="c.y" r="7" class="glyph-circle-outer" />
          <circle :cx="c.x" :cy="c.y" r="3.5" class="glyph-circle" />
        </template>
        <g v-if="g.text && g.textPos" :transform="`translate(${g.textPos.x}, ${g.textPos.y})`">
          <circle r="8" class="glyph-bg" />
          <text
            class="glyph-text"
            text-anchor="middle" dominant-baseline="central"
          >{{ g.text }}</text>
        </g>
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
import type {
  Constraint,
  SketchCircleEntity,
  SketchArcEntity,
  SketchEntity,
} from "@/commands/sketch";
import { updateConstraintValue, solveSketch } from "@/commands/sketch";
import { useToastStore } from "@/stores/toast";

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
const toast = useToastStore();

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

/**
 * A non-numeric constraint glyph. Each glyph is a subtle marker placed at a
 * meaningful location derived from the constraint's referenced entity ids.
 * `text` is rendered on a small disc background; `axis` is a dashed line
 * (used by Symmetric / Collinear); `circles` are stroked rings (Concentric);
 * `dot` is a filled locked-point marker (Coincident). Every glyph element
 * has `pointer-events: none` so it never steals mouse interaction — they
 * are informational only.
 */
type Glyph = {
  key: string;
  text?: string;
  textPos?: Pt;
  axis?: { x1: number; y1: number; x2: number; y2: number };
  circles?: Pt[];
  dot?: Pt;
};

/// Shared id → entity lookup, rebuilt whenever `props.entities` changes.
/// Both `dimensions` and `glyphs` consume it so we don't build two maps.
const entityMap = computed(() => new Map(props.entities.map(e => [e.id, e])));

function project(x: number, y: number): Pt | null {
  return props.project(x, y);
}

/// Geometric centroid of any sketch entity, used to anchor glyphs whose
/// referenced entity is not a Point. Returns null for entities we can't
/// place (e.g. empty spline).
function entityCenter(e: SketchEntity | undefined): { x: number; y: number } | null {
  if (!e) return null;
  switch (e.type) {
    case "Point": return { x: e.x, y: e.y };
    case "Line": return { x: (e.x1 + e.x2) / 2, y: (e.y1 + e.y2) / 2 };
    case "Circle":
    case "Arc":
    case "Ellipse":
      return { x: e.cx, y: e.cy };
    case "Spline": {
      if (e.points.length === 0) return null;
      let sx = 0, sy = 0;
      for (const [x, y] of e.points) { sx += x; sy += y; }
      return { x: sx / e.points.length, y: sy / e.points.length };
    }
  }
  return null;
}

/// Foot of the perpendicular from `circle`'s center to `line` — the natural
/// "tangency point" to anchor a Tangent glyph at.
function tangentPointOnLine(line: LineEntity, circle: SketchCircleEntity | SketchArcEntity): { x: number; y: number } | null {
  const dx = line.x2 - line.x1, dy = line.y2 - line.y1;
  const len = Math.hypot(dx, dy);
  if (len < 1e-9) return null;
  const dirx = dx / len, diry = dy / len;
  const t = (circle.cx - line.x1) * dirx + (circle.cy - line.y1) * diry;
  return { x: line.x1 + t * dirx, y: line.y1 + t * diry };
}

/// Offset `mid` perpendicular to `line` in *screen* space by `off` px, so a
/// glyph anchored at a line midpoint sits just off the line instead of on it.
function offsetPerp(mid: Pt, line: LineEntity, off: number): Pt {
  const s1 = project(line.x1, line.y1);
  const s2 = project(line.x2, line.y2);
  if (!s1 || !s2) return mid;
  const dx = s2.x - s1.x, dy = s2.y - s1.y;
  const len = Math.hypot(dx, dy) || 1;
  return { x: mid.x + (-dy / len) * off, y: mid.y + (dx / len) * off };
}

const dimensions = computed<Dimension[]>(() => {
  if (!props.visible) return [];
  const result: Dimension[] = [];
  const ents = entityMap.value;

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

/// Non-numeric constraint glyphs. Numeric dimensions (Distance, Radius,
/// Diameter, Angle) are deliberately NOT included here — `dimensions`
/// already visualizes them. Every other constraint kind gets a small,
/// semi-transparent marker placed where the user can see "this constraint
/// exists". Locations are derived purely from the referenced entity ids
/// resolved through `entityMap` + the parent-supplied `project` function —
/// no THREE objects cross the prop boundary (design-principle #6).
const glyphs = computed<Glyph[]>(() => {
  if (!props.visible) return [];
  const result: Glyph[] = [];
  const ents = entityMap.value;

  let counter = 0;
  for (const c of props.constraints) {
    const key = "glyph-" + counter++;
    if ("Horizontal" in c) {
      const line = ents.get(c.Horizontal.line);
      if (line?.type !== "Line") continue;
      const mid = project((line.x1 + line.x2) / 2, (line.y1 + line.y2) / 2);
      if (mid) result.push({ key, text: "H", textPos: offsetPerp(mid, line, 12) });
    } else if ("Vertical" in c) {
      const line = ents.get(c.Vertical.line);
      if (line?.type !== "Line") continue;
      const mid = project((line.x1 + line.x2) / 2, (line.y1 + line.y2) / 2);
      if (mid) result.push({ key, text: "V", textPos: offsetPerp(mid, line, 12) });
    } else if ("Parallel" in c) {
      const la = ents.get(c.Parallel.line_a);
      if (la?.type !== "Line") continue;
      const mid = project((la.x1 + la.x2) / 2, (la.y1 + la.y2) / 2);
      if (mid) result.push({ key, text: "//", textPos: offsetPerp(mid, la, 12) });
    } else if ("Perpendicular" in c) {
      const la = ents.get(c.Perpendicular.line_a);
      const lb = ents.get(c.Perpendicular.line_b);
      if (la?.type !== "Line" || lb?.type !== "Line") continue;
      const inter = lineIntersect(la, lb);
      if (!inter) continue;
      const sint = project(inter.x, inter.y);
      if (sint) result.push({ key, text: "⊥", textPos: sint });
    } else if ("Tangent" in c) {
      const line = ents.get(c.Tangent.line);
      const circle = ents.get(c.Tangent.circle);
      if (line?.type !== "Line") continue;
      if (circle?.type !== "Circle" && circle?.type !== "Arc") continue;
      const t = tangentPointOnLine(line, circle);
      if (!t) continue;
      const st = project(t.x, t.y);
      if (st) result.push({ key, text: "T", textPos: st });
    } else if ("Concentric" in c) {
      const ca = entityCenter(ents.get(c.Concentric.a));
      const cb = entityCenter(ents.get(c.Concentric.b));
      if (!ca || !cb) continue;
      const sm = project((ca.x + cb.x) / 2, (ca.y + cb.y) / 2);
      if (sm) result.push({ key, text: "◎", textPos: sm, circles: [sm] });
    } else if ("Equal" in c) {
      const ca = entityCenter(ents.get(c.Equal.a));
      const cb = entityCenter(ents.get(c.Equal.b));
      if (!ca || !cb) continue;
      const sm = project((ca.x + cb.x) / 2, (ca.y + cb.y) / 2);
      if (sm) result.push({ key, text: "=", textPos: sm });
    } else if ("Coincident" in c) {
      const ca = entityCenter(ents.get(c.Coincident.a));
      const cb = entityCenter(ents.get(c.Coincident.b));
      if (!ca || !cb) continue;
      const sm = project((ca.x + cb.x) / 2, (ca.y + cb.y) / 2);
      if (sm) result.push({ key, dot: sm });
    } else if ("Fix" in c) {
      const p = ents.get(c.Fix.point);
      if (p?.type !== "Point") continue;
      const sp = project(p.x, p.y);
      if (sp) result.push({ key, text: "🔒", textPos: sp });
    } else if ("Midpoint" in c) {
      const line = ents.get(c.Midpoint.line);
      if (line?.type !== "Line") continue;
      const mid = project((line.x1 + line.x2) / 2, (line.y1 + line.y2) / 2);
      if (mid) result.push({ key, text: "Δ", textPos: mid });
    } else if ("Symmetric" in c) {
      const axis = ents.get(c.Symmetric.axis);
      if (axis?.type !== "Line") continue;
      const sa1 = project(axis.x1, axis.y1);
      const sa2 = project(axis.x2, axis.y2);
      if (!sa1 || !sa2) continue;
      result.push({ key, axis: { x1: sa1.x, y1: sa1.y, x2: sa2.x, y2: sa2.y } });
    } else if ("PointOnLine" in c) {
      const p = ents.get(c.PointOnLine.point);
      if (p?.type !== "Point") continue;
      const sp = project(p.x, p.y);
      if (sp) result.push({ key, text: "×", textPos: sp });
    } else if ("Collinear" in c) {
      const ca = entityCenter(ents.get(c.Collinear.a));
      const cb = entityCenter(ents.get(c.Collinear.b));
      if (!ca || !cb) continue;
      const sa = project(ca.x, ca.y);
      const sb = project(cb.x, cb.y);
      if (!sa || !sb) continue;
      // Extend the dashed axis line a bit beyond both endpoints.
      const dx = sb.x - sa.x, dy = sb.y - sa.y;
      const len = Math.hypot(dx, dy) || 1;
      const ex = (dx / len) * 10, ey = (dy / len) * 10;
      result.push({ key, axis: { x1: sa.x - ex, y1: sa.y - ey, x2: sb.x + ex, y2: sb.y + ey } });
    }
    // Numeric kinds (Distance, Radius, Diameter, Angle) are intentionally
    // skipped — they are already drawn by the `dimensions` computed above.
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
  try {
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
  } catch (err) {
    toast.error("修改尺寸失败: " + String(err));
  }
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

/* ── Constraint glyphs ──────────────────────────────────────────
   Subtle markers for non-numeric constraints. A single accent color
   (#b39ddb, light purple) keeps them visually distinct from blue
   dimensions and yellow angles without competing for attention. */
.glyph-axis {
  stroke: #b39ddb; stroke-width: 1; fill: none;
  stroke-dasharray: 3 3; opacity: 0.6; pointer-events: none;
}
.glyph-dot {
  fill: #b39ddb; opacity: 0.9; pointer-events: none;
}
.glyph-dot-ring {
  fill: none; stroke: #b39ddb; stroke-width: 0.8; opacity: 0.55;
  pointer-events: none;
}
.glyph-circle {
  fill: none; stroke: #b39ddb; stroke-width: 1; opacity: 0.8;
  pointer-events: none;
}
.glyph-circle-outer {
  fill: none; stroke: #b39ddb; stroke-width: 0.6; opacity: 0.45;
  pointer-events: none;
}
.glyph-bg {
  fill: #1e1e1e; stroke: #b39ddb; stroke-width: 0.5; opacity: 0.85;
  pointer-events: none;
}
.glyph-text {
  fill: #b39ddb; font-size: 10px; user-select: none;
  pointer-events: none;
}
</style>
