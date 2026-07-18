import { ref, type Ref } from "vue";
import type { EntityId, SketchEntity } from "@/commands/sketch";
import type { Tool } from "@/stores/sketch";

export const SNAP_PX = 12;
export const SNAP_THRESHOLD_DEG = 4;
export const STANDARD_ANGLES = [0, 45, 90, 135, 180, 225, 270, 315];

export type SnapType = "endpoint" | "midpoint" | "center" | "intersection" | "grid" | "tangent";

export interface SnapResult {
  type: SnapType;
  world: { x: number; y: number };
  screen: { x: number; y: number };
}

/// Mutable drawing state shared between mouse handlers. Each tool resets
/// the relevant pieces when activated.
export interface SketchInteractionState {
  // Line drawing
  lineStartId: Ref<EntityId | null>;
  // Rectangle drawing
  rectStart: Ref<{ x: number; y: number } | null>;
  // Arc drawing (3-step)
  arcCenterId: Ref<EntityId | null>;
  arcRadiusPoint: Ref<{ x: number; y: number } | null>;
  arcRadiusSet: Ref<boolean>;
  // Circle drawing
  circleCenterId: Ref<EntityId | null>;
  // Spline / multi-point
  splinePoints: Ref<EntityId[]>;
  // Ellipse
  ellipseCenterId: Ref<EntityId | null>;
  // Drag/select
  isDragging: Ref<boolean>;
  dragId: Ref<EntityId | null>;
  // Snap result
  activeSnap: Ref<SnapResult | null>;
  hoverSketch: Ref<{ x: number; y: number }>;
  isHovering: Ref<boolean>;
}

export function createSketchState(): SketchInteractionState {
  return {
    lineStartId: ref(null),
    rectStart: ref(null),
    arcCenterId: ref(null),
    arcRadiusPoint: ref(null),
    arcRadiusSet: ref(false),
    circleCenterId: ref(null),
    splinePoints: ref([]),
    ellipseCenterId: ref(null),
    isDragging: ref(false),
    dragId: ref(null),
    activeSnap: ref(null),
    hoverSketch: ref({ x: 0, y: 0 }),
    isHovering: ref(false),
  };
}

/// Reset all drawing state. Called when the active tool changes or the
/// user presses Esc / right-click in empty space.
export function resetDrawingState(state: SketchInteractionState) {
  state.lineStartId.value = null;
  state.rectStart.value = null;
  state.arcCenterId.value = null;
  state.arcRadiusPoint.value = null;
  state.arcRadiusSet.value = false;
  state.circleCenterId.value = null;
  state.ellipseCenterId.value = null;
  state.splinePoints.value = [];
}

/// Find the closest existing point to (wx, wy) within `threshold` sketch units.
export function findPointAt(entities: SketchEntity[], wx: number, wy: number, threshold = 0.35): EntityId | null {
  for (const entity of entities) {
    if (entity.type === "Point") {
      if (Math.hypot(entity.x - wx, entity.y - wy) < threshold) return entity.id;
    }
  }
  return null;
}

/// Find the closest entity (any kind) to (wx, wy) within `threshold`.
export function findClosestEntity(entities: SketchEntity[], wx: number, wy: number, threshold = 0.4): EntityId | null {
  let best: EntityId | null = null;
  let bestDist = threshold;
  for (const entity of entities) {
    let d = Infinity;
    if (entity.type === "Point") {
      d = Math.hypot(entity.x - wx, entity.y - wy);
    } else if (entity.type === "Line") {
      d = pointToSegmentDist(wx, wy, entity.x1, entity.y1, entity.x2, entity.y2);
    } else if (entity.type === "Circle" || entity.type === "Arc") {
      d = Math.abs(Math.hypot(wx - entity.cx, wy - entity.cy) - entity.radius);
    }
    if (d < bestDist) {
      bestDist = d;
      best = entity.id;
    }
  }
  return best;
}

export function pointToSegmentDist(
  px: number, py: number,
  x1: number, y1: number,
  x2: number, y2: number,
): number {
  const l2 = (x2 - x1) ** 2 + (y2 - y1) ** 2;
  if (l2 === 0) return Math.hypot(px - x1, py - y1);
  let t = ((px - x1) * (x2 - x1) + (py - y1) * (y2 - y1)) / l2;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(px - (x1 + t * (x2 - x1)), py - (y1 + t * (y2 - y1)));
}

/// Snap cursor `angle` to the nearest of STANDARD_ANGLES if within threshold.
/// Returns the snapped end-point.
export function snapAngle(start: { x: number; y: number }, end: { x: number; y: number }, applySnap: boolean): { x: number; y: number } {
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  const dist = Math.hypot(dx, dy);
  if (dist < 1e-9) return { ...end };
  let angle = Math.atan2(dy, dx) * (180 / Math.PI);
  if (angle < 0) angle += 360;
  let nearestAngle = angle;
  let minDiff = Infinity;
  for (const std of STANDARD_ANGLES) {
    let diff = Math.abs(angle - std);
    if (diff > 180) diff = 360 - diff;
    if (diff < minDiff) {
      minDiff = diff;
      nearestAngle = std;
    }
  }
  if (applySnap && minDiff <= SNAP_THRESHOLD_DEG) {
    const rad = nearestAngle * (Math.PI / 180);
    return { x: start.x + dist * Math.cos(rad), y: start.y + dist * Math.sin(rad) };
  }
  return { ...end };
}

export function lineIntersection(
  x1: number, y1: number, x2: number, y2: number,
  x3: number, y3: number, x4: number, y4: number,
): { x: number; y: number } | null {
  const d = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
  if (Math.abs(d) < 1e-10) return null;
  const t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / d;
  return { x: x1 + t * (x2 - x1), y: y1 + t * (y2 - y1) };
}

export const SNAP_LABELS: Record<SnapType, string> = {
  endpoint: "端点",
  midpoint: "中点",
  center: "圆心",
  intersection: "交点",
  grid: "网格",
  tangent: "相切",
};

export const SNAP_PRIORITY: Record<SnapType, number> = {
  endpoint: 0,
  center: 1,
  midpoint: 2,
  intersection: 3,
  grid: 4,
  tangent: 5,
};

export function toolLabel(tool: Tool): string {
  const labels: Record<Tool, string> = {
    select: "选择/拖拽",
    line: "直线",
    circle: "圆",
    arc: "弧线",
    rectangle: "矩形",
    spline: "样条",
    ellipse: "椭圆",
    plugin: "插件",
  };
  return labels[tool];
}
