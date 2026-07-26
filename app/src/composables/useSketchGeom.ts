// 2D sketch geometry utilities — TypeScript mirrors of the Rust sketch_geom.rs
// functions. Used by the snapping frontend for low-latency (60fps) snap computation
// on mouse move.

/** Nearest point on a line segment to a query point.
 *  Returns { x, y, t, dist } where t ∈ [0,1] is the parameter along the segment. */
export function nearestPointOnSegment(
  px: number, py: number,
  x1: number, y1: number, x2: number, y2: number,
): { x: number; y: number; t: number; dist: number } {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const l2 = dx * dx + dy * dy;
  if (l2 < 1e-20) {
    const d = Math.hypot(px - x1, py - y1);
    return { x: x1, y: y1, t: 0, dist: d };
  }
  let t = ((px - x1) * dx + (py - y1) * dy) / l2;
  t = Math.max(0, Math.min(1, t));
  const nx = x1 + t * dx;
  const ny = y1 + t * dy;
  const d = Math.hypot(px - nx, py - ny);
  return { x: nx, y: ny, t, dist: d };
}

/** Nearest point on a circle's circumference to a query point. */
export function nearestPointOnCircle(
  px: number, py: number,
  cx: number, cy: number, r: number,
): { x: number; y: number; dist: number } {
  const dx = px - cx;
  const dy = py - cy;
  const dist = Math.hypot(dx, dy);
  if (dist < 1e-12) {
    return { x: cx + r, y: cy, dist: r };
  }
  const nx = cx + (dx / dist) * r;
  const ny = cy + (dy / dist) * r;
  return { x: nx, y: ny, dist: Math.abs(dist - r) };
}

/** Nearest point on an arc's circumference, clamped to the arc's angular range. */
export function nearestPointOnArc(
  px: number, py: number,
  cx: number, cy: number, r: number,
  startAngle: number, endAngle: number,
): { x: number; y: number; dist: number } {
  const dx = px - cx;
  const dy = py - cy;
  const dist = Math.hypot(dx, dy);
  if (dist < 1e-12) {
    const mid = (startAngle + endAngle) / 2;
    return { x: cx + r * Math.cos(mid), y: cy + r * Math.sin(mid), dist: r };
  }
  const angle = Math.atan2(dy, dx);
  if (angleInRange(angle, startAngle, endAngle)) {
    const nx = cx + (dx / dist) * r;
    const ny = cy + (dy / dist) * r;
    return { x: nx, y: ny, dist: Math.abs(dist - r) };
  }
  // Fall back to nearest endpoint
  const sx = cx + r * Math.cos(startAngle);
  const sy = cy + r * Math.sin(startAngle);
  const ex = cx + r * Math.cos(endAngle);
  const ey = cy + r * Math.sin(endAngle);
  const ds = Math.hypot(px - sx, py - sy);
  const de = Math.hypot(px - ex, py - ey);
  return ds <= de ? { x: sx, y: sy, dist: ds } : { x: ex, y: ey, dist: de };
}

function angleInRange(angle: number, start: number, end: number): boolean {
  const norm = (a: number) => {
    a = a % (2 * Math.PI);
    if (a < 0) a += 2 * Math.PI;
    return a;
  };
  const a = norm(angle);
  const s = norm(start);
  const e = norm(end);
  if (s <= e) {
    return a >= s && a <= e;
  } else {
    return a >= s || a <= e;
  }
}

/** Tangent points from an external point to a circle.
 *  Returns 2 points if point is outside, 1 if on circumference, empty if inside. */
export function tangentPointsFromPoint(
  px: number, py: number,
  cx: number, cy: number, r: number,
): { x: number; y: number }[] {
  const dx = px - cx;
  const dy = py - cy;
  const d = Math.hypot(dx, dy);

  if (d < 1e-12 || d < r - 1e-12) return [];
  if (Math.abs(d - r) < 1e-9) return [{ x: px, y: py }];

  const phi = Math.atan2(dy, dx);
  const theta = Math.acos(r / d);

  return [
    { x: cx + r * Math.cos(phi + theta), y: cy + r * Math.sin(phi + theta) },
    { x: cx + r * Math.cos(phi - theta), y: cy + r * Math.sin(phi - theta) },
  ];
}

/** Perpendicular foot from a point to an infinite line through (x1,y1)-(x2,y2). */
export function perpendicularFoot(
  px: number, py: number,
  x1: number, y1: number, x2: number, y2: number,
): { x: number; y: number; t: number } {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const l2 = dx * dx + dy * dy;
  if (l2 < 1e-20) return { x: x1, y: y1, t: 0 };
  const t = ((px - x1) * dx + (py - y1) * dy) / l2;
  return { x: x1 + t * dx, y: y1 + t * dy, t };
}

/** Infinite line-line intersection. Returns null if parallel. */
export function lineIntersection2(
  x1: number, y1: number, x2: number, y2: number,
  x3: number, y3: number, x4: number, y4: number,
): { x: number; y: number } | null {
  const d = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
  if (Math.abs(d) < 1e-12) return null;
  const t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / d;
  return { x: x1 + t * (x2 - x1), y: y1 + t * (y2 - y1) };
}
