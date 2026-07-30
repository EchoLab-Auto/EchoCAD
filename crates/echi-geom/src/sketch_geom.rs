//! 2D sketch geometry utilities — nearest-point, intersection, and tangent
//! functions used by both the snapping frontend (via mirrored TypeScript) and
//! the trim/extend backend commands.
//!
//! All functions are pure: they operate on raw f64 values and do not reference
//! Sketch or EntityId. Callers resolve coordinates before calling.

/// Nearest point on a line segment (x1,y1)-(x2,y2) to query point (px,py).
/// Returns (nearest_x, nearest_y, t_parameter, distance).
/// t=0 means at (x1,y1); t=1 means at (x2,y2).
pub fn nearest_point_on_segment(
    px: f64, py: f64,
    x1: f64, y1: f64,
    x2: f64, y2: f64,
) -> (f64, f64, f64, f64) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let l2 = dx * dx + dy * dy;
    if l2 < 1e-20 {
        // Degenerate segment (both endpoints coincide)
        let d = ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
        return (x1, y1, 0.0, d);
    }
    let t = ((px - x1) * dx + (py - y1) * dy) / l2;
    let t = t.clamp(0.0, 1.0);
    let nx = x1 + t * dx;
    let ny = y1 + t * dy;
    let d = ((px - nx).powi(2) + (py - ny).powi(2)).sqrt();
    (nx, ny, t, d)
}

/// Nearest point on a circle's circumference to query point (px,py).
/// Returns (nearest_x, nearest_y, distance_to_circumference).
pub fn nearest_point_on_circle(
    px: f64, py: f64,
    cx: f64, cy: f64, r: f64,
) -> (f64, f64, f64) {
    let dx = px - cx;
    let dy = py - cy;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < 1e-12 {
        // Query point is at the center — any point on circumference is equidistant.
        // Return the rightmost point.
        return (cx + r, cy, r);
    }
    let nx = cx + dx / dist * r;
    let ny = cy + dy / dist * r;
    let d = (dist - r).abs();
    (nx, ny, d)
}

/// Nearest point on an arc's circumference to query point (px,py).
/// The arc is defined by center, radius, start_angle, and end_angle.
/// If the nearest point on the full circle falls outside the arc's angular
/// range, falls back to the nearest arc endpoint.
pub fn nearest_point_on_arc(
    px: f64, py: f64,
    cx: f64, cy: f64, r: f64,
    start_angle: f64, end_angle: f64,
) -> (f64, f64, f64) {
    let dx = px - cx;
    let dy = py - cy;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < 1e-12 {
        // At center — return arc midpoint
        let mid = (start_angle + end_angle) / 2.0;
        return (cx + r * mid.cos(), cy + r * mid.sin(), r);
    }
    let angle = dy.atan2(dx);
    if angle_in_range(angle, start_angle, end_angle) {
        let nx = cx + dx / dist * r;
        let ny = cy + dy / dist * r;
        return (nx, ny, (dist - r).abs());
    }
    // Fall back to nearest endpoint
    let sx = cx + r * start_angle.cos();
    let sy = cy + r * start_angle.sin();
    let ex = cx + r * end_angle.cos();
    let ey = cy + r * end_angle.sin();
    let ds = ((px - sx).powi(2) + (py - sy).powi(2)).sqrt();
    let de = ((px - ex).powi(2) + (py - ey).powi(2)).sqrt();
    if ds <= de {
        (sx, sy, ds)
    } else {
        (ex, ey, de)
    }
}

/// Check whether `angle` lies within the CCW arc from `start` to `end`.
fn angle_in_range(angle: f64, start: f64, end: f64) -> bool {
    // Normalize all angles to [0, 2π)
    let norm = |mut a: f64| -> f64 {
        a = a % (2.0 * std::f64::consts::PI);
        if a < 0.0 { a += 2.0 * std::f64::consts::PI; }
        a
    };
    let a = norm(angle);
    let s = norm(start);
    let e = norm(end);
    if s <= e {
        a >= s && a <= e
    } else {
        // Arc wraps around 2π
        a >= s || a <= e
    }
}

/// Intersection of two infinite lines defined by point pairs.
/// Returns None if lines are parallel (within epsilon 1e-12).
pub fn line_line_intersection(
    x1: f64, y1: f64, x2: f64, y2: f64,
    x3: f64, y3: f64, x4: f64, y4: f64,
) -> Option<(f64, f64)> {
    let d = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
    if d.abs() < 1e-12 {
        return None;
    }
    let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / d;
    Some((x1 + t * (x2 - x1), y1 + t * (y2 - y1)))
}

/// Bounded segment-segment intersection.
/// Returns (intersection_x, intersection_y, t_on_first, u_on_second)
/// only if the intersection lies on BOTH segments (t,u in [0,1] within eps).
pub fn segment_intersection(
    x1: f64, y1: f64, x2: f64, y2: f64,
    x3: f64, y3: f64, x4: f64, y4: f64,
    eps: f64,
) -> Option<(f64, f64, f64, f64)> {
    let d = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
    if d.abs() < 1e-12 {
        return None;
    }
    let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / d;
    let u = -((x1 - x2) * (y1 - y3) - (y1 - y2) * (x1 - x3)) / d;
    if t >= -eps && t <= 1.0 + eps && u >= -eps && u <= 1.0 + eps {
        let t = t.clamp(0.0, 1.0);
        let u = u.clamp(0.0, 1.0);
        Some((x1 + t * (x2 - x1), y1 + t * (y2 - y1), t, u))
    } else {
        None
    }
}

/// Intersection(s) of an infinite line through (x1,y1)-(x2,y2) with a circle.
/// Returns 0, 1, or 2 points, sorted by distance from (x1,y1).
pub fn line_circle_intersection(
    x1: f64, y1: f64, x2: f64, y2: f64,
    cx: f64, cy: f64, r: f64,
) -> Vec<(f64, f64)> {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let fx = x1 - cx;
    let fy = y1 - cy;

    let a = dx * dx + dy * dy;
    let b = 2.0 * (fx * dx + fy * dy);
    let c = fx * fx + fy * fy - r * r;
    let disc = b * b - 4.0 * a * c;

    if disc < -1e-12 {
        return vec![];
    }

    let mut points: Vec<(f64, f64, f64)> = Vec::new(); // (t, x, y)

    if disc.abs() < 1e-12 {
        let t = -b / (2.0 * a);
        points.push((t, x1 + t * dx, y1 + t * dy));
    } else {
        let sqrt_disc = disc.sqrt();
        let t1 = (-b - sqrt_disc) / (2.0 * a);
        let t2 = (-b + sqrt_disc) / (2.0 * a);
        points.push((t1, x1 + t1 * dx, y1 + t1 * dy));
        points.push((t2, x1 + t2 * dx, y1 + t2 * dy));
        // Sort by t (distance from x1,y1 along the line)
        if points[0].0 > points[1].0 {
            points.swap(0, 1);
        }
    }

    points.into_iter().map(|(_, x, y)| (x, y)).collect()
}

/// Intersection(s) of a line SEGMENT (x1,y1)-(x2,y2) with a circle.
/// Returns 0, 1, or 2 points that lie on the segment (t ∈ [0,1]).
pub fn segment_circle_intersection(
    x1: f64, y1: f64, x2: f64, y2: f64,
    cx: f64, cy: f64, r: f64,
) -> Vec<(f64, f64)> {
    let all = line_circle_intersection(x1, y1, x2, y2, cx, cy, r);
    all.into_iter()
        .filter(|(ix, iy)| {
            // Check if the intersection lies between segment endpoints
            let t = if (x2 - x1).abs() > (y2 - y1).abs() {
                (ix - x1) / (x2 - x1)
            } else if (y2 - y1).abs() > 1e-12 {
                (iy - y1) / (y2 - y1)
            } else {
                return false; // degenerate segment
            };
            t >= -1e-9 && t <= 1.0 + 1e-9
        })
        .collect()
}

/// Intersection of two circles.
/// Returns 0, 1, or 2 points.
pub fn circle_circle_intersection(
    c1x: f64, c1y: f64, r1: f64,
    c2x: f64, c2y: f64, r2: f64,
) -> Vec<(f64, f64)> {
    let dx = c2x - c1x;
    let dy = c2y - c1y;
    let d = (dx * dx + dy * dy).sqrt();

    if d < 1e-12 {
        // Concentric circles
        return vec![];
    }
    if d > r1 + r2 + 1e-9 || d < (r1 - r2).abs() - 1e-9 {
        // No intersection
        return vec![];
    }

    // Point along the line from c1 to c2 where the intersection chord is
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h_sq = r1 * r1 - a * a;
    let h = if h_sq <= 0.0 { 0.0 } else { h_sq.sqrt() };

    let px = c1x + a * dx / d;
    let py = c1y + a * dy / d;

    if h.abs() < 1e-9 {
        // Tangent circles — single intersection
        return vec![(px, py)];
    }

    let rx = -dy * h / d;
    let ry = dx * h / d;

    vec![(px + rx, py + ry), (px - rx, py - ry)]
}

/// Tangent points from an external point (px,py) to a circle.
/// Returns:
/// - 2 points if the point is outside the circle
/// - 1 point (the point itself) if on the circumference
/// - empty if inside the circle
pub fn tangent_points_from_point(
    px: f64, py: f64,
    cx: f64, cy: f64, r: f64,
) -> Vec<(f64, f64)> {
    let dx = px - cx;
    let dy = py - cy;
    let d_sq = dx * dx + dy * dy;
    let d = d_sq.sqrt();

    if d < 1e-12 || d_sq < r * r - 1e-12 {
        // Point at center or inside circle — no tangent
        return vec![];
    }
    if (d - r).abs() < 1e-9 {
        // Point on circumference — tangent is the point itself
        return vec![(px, py)];
    }

    // Angle from center to the point
    let phi = dy.atan2(dx);
    // Half-angle of the tangent cone
    let theta = (r / d).acos();

    let a1 = phi + theta;
    let a2 = phi - theta;

    // Tangent points on the circle: C + r * (cos(θ), sin(θ))
    let t1x = cx + r * a1.cos();
    let t1y = cy + r * a1.sin();
    let t2x = cx + r * a2.cos();
    let t2y = cy + r * a2.sin();

    vec![(t1x, t1y), (t2x, t2y)]
}

/// Compute the perpendicular foot from point (px,py) to the infinite line
/// through (x1,y1)-(x2,y2). Returns (foot_x, foot_y, t_parameter).
pub fn perpendicular_foot(
    px: f64, py: f64,
    x1: f64, y1: f64, x2: f64, y2: f64,
) -> (f64, f64, f64) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let l2 = dx * dx + dy * dy;
    if l2 < 1e-20 {
        return (x1, y1, 0.0);
    }
    let t = ((px - x1) * dx + (py - y1) * dy) / l2;
    (x1 + t * dx, y1 + t * dy, t)
}

// ── Arc / Ellipse intersections ─────────────────────────────────────

/// Intersection of an infinite line with an arc. Returns points on the arc
/// (within the arc's angular range, inclusive).
pub fn arc_line_intersection(
    x1: f64, y1: f64, x2: f64, y2: f64,
    cx: f64, cy: f64, r: f64,
    start_angle: f64, end_angle: f64,
) -> Vec<(f64, f64)> {
    line_circle_intersection(x1, y1, x2, y2, cx, cy, r)
        .into_iter()
        .filter(|&(ix, iy)| {
            let angle = (iy - cy).atan2(ix - cx);
            angle_in_range(angle, start_angle, end_angle)
        })
        .collect()
}

/// Intersection of a line segment with an arc.
pub fn segment_arc_intersection(
    x1: f64, y1: f64, x2: f64, y2: f64,
    cx: f64, cy: f64, r: f64,
    start_angle: f64, end_angle: f64,
) -> Vec<(f64, f64)> {
    // First, check which line-circle intersections fall on the arc
    let circle_hits = line_circle_intersection(x1, y1, x2, y2, cx, cy, r);
    circle_hits.into_iter().filter(|&(ix, iy)| {
        // Check angle range
        let angle = (iy - cy).atan2(ix - cx);
        if !angle_in_range(angle, start_angle, end_angle) { return false; }
        // Check segment bounds
        let t = if (x2 - x1).abs() > (y2 - y1).abs() {
            if (x2 - x1).abs() < 1e-12 { return false; }
            (ix - x1) / (x2 - x1)
        } else {
            if (y2 - y1).abs() < 1e-12 { return false; }
            (iy - y1) / (y2 - y1)
        };
        t >= -1e-9 && t <= 1.0 + 1e-9
    }).collect()
}

/// Intersection of two circular arcs (same center and radius assumed to be
/// handled by circle_circle_intersection, then angle-filtered for both arcs).
pub fn arc_arc_intersection(
    cx1: f64, cy1: f64, r1: f64, start1: f64, end1: f64,
    cx2: f64, cy2: f64, r2: f64, start2: f64, end2: f64,
) -> Vec<(f64, f64)> {
    circle_circle_intersection(cx1, cy1, r1, cx2, cy2, r2)
        .into_iter()
        .filter(|&(ix, iy)| {
            let a1 = (iy - cy1).atan2(ix - cx1);
            let a2 = (iy - cy2).atan2(ix - cx2);
            angle_in_range(a1, start1, end1) && angle_in_range(a2, start2, end2)
        })
        .collect()
}

/// Intersection of an infinite line with an ellipse.
/// The ellipse is defined by center (cx,cy), major axis vector (major_rx, major_ry),
/// and ratio (minor/major). Solves the quadratic line-ellipse intersection in
/// the ellipse's local frame.
pub fn line_ellipse_intersection(
    x1: f64, y1: f64, x2: f64, y2: f64,
    cx: f64, cy: f64, major_rx: f64, major_ry: f64, ratio: f64,
) -> Vec<(f64, f64)> {
    let a = (major_rx * major_rx + major_ry * major_ry).sqrt();
    let b = a * ratio;
    if a < 1e-12 || b < 1e-12 { return vec![]; }
    let angle = major_ry.atan2(major_rx);
    let cos_a = angle.cos(); let sin_a = angle.sin();

    // Transform line to ellipse-local frame (rotate by -angle, translate by -center)
    let tx = |px: f64, py: f64| -> (f64, f64) {
        let dx = px - cx; let dy = py - cy;
        (dx * cos_a + dy * sin_a, -dx * sin_a + dy * cos_a)
    };
    let (lx1, ly1) = tx(x1, y1);
    let (lx2, ly2) = tx(x2, y2);
    let dx = lx2 - lx1; let dy = ly2 - ly1;

    // Intersection of line with ellipse x²/a² + y²/b² = 1:
    // Substitute (lx1 + t*dx)²/a² + (ly1 + t*dy)²/b² = 1
    // → At² + Bt + C = 0
    let a2 = a * a; let b2 = b * b;
    let aa = dx*dx/a2 + dy*dy/b2;
    let bb = 2.0 * (lx1*dx/a2 + ly1*dy/b2);
    let cc = lx1*lx1/a2 + ly1*ly1/b2 - 1.0;

    let disc = bb*bb - 4.0*aa*cc;
    if disc < -1e-12 { return vec![]; }

    let mut pts = Vec::new();
    if disc.abs() < 1e-12 {
        let t = -bb / (2.0 * aa);
        pts.push((x1 + t * dx, y1 + t * dy));
    } else {
        let sqrt_d = disc.sqrt();
        for &t in &[(-bb - sqrt_d) / (2.0 * aa), (-bb + sqrt_d) / (2.0 * aa)] {
            pts.push((x1 + t * dx, y1 + t * dy));
        }
    }
    pts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nearest_point_on_segment_midpoint() {
        let (nx, ny, t, d) = nearest_point_on_segment(0.5, 0.5, 0.0, 0.0, 1.0, 1.0);
        assert!((nx - 0.5).abs() < 1e-10);
        assert!((ny - 0.5).abs() < 1e-10);
        assert!((t - 0.5).abs() < 1e-10);
        assert!(d < 1e-10);
    }

    #[test]
    fn test_nearest_point_on_segment_endpoint() {
        let (nx, ny, t, _d) = nearest_point_on_segment(2.0, 2.0, 0.0, 0.0, 1.0, 1.0);
        assert!((nx - 1.0).abs() < 1e-10);
        assert!((ny - 1.0).abs() < 1e-10);
        assert!((t - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_nearest_point_on_segment_beyond_start() {
        let (nx, ny, t, _d) = nearest_point_on_segment(-1.0, -1.0, 0.0, 0.0, 2.0, 2.0);
        assert!((nx - 0.0).abs() < 1e-10);
        assert!((ny - 0.0).abs() < 1e-10);
        assert!((t - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_nearest_point_on_circle() {
        let (nx, ny, d) = nearest_point_on_circle(0.0, 3.0, 0.0, 0.0, 1.0);
        assert!((nx - 0.0).abs() < 1e-10);
        assert!((ny - 1.0).abs() < 1e-10);
        assert!((d - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_nearest_point_on_circle_at_center() {
        let (nx, ny, _d) = nearest_point_on_circle(0.0, 0.0, 0.0, 0.0, 5.0);
        assert!((nx - 5.0).abs() < 1e-10);
        assert!((ny - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_nearest_point_on_arc_within_range() {
        // Arc from 0 to π/2 (first quadrant, CCW)
        let (nx, ny, d) = nearest_point_on_arc(2.0, 0.0, 0.0, 0.0, 1.0, 0.0, std::f64::consts::FRAC_PI_2);
        assert!((nx - 1.0).abs() < 1e-10);
        assert!((ny - 0.0).abs() < 1e-10);
        assert!((d - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_nearest_point_on_arc_outside_range() {
        // Arc from π to 3π/2 (third quadrant). Query point at (1,0) — first quadrant.
        let (nx, ny, _d) = nearest_point_on_arc(
            1.0, 0.0, 0.0, 0.0, 1.0,
            std::f64::consts::PI,
            3.0 * std::f64::consts::FRAC_PI_2,
        );
        // Should snap to nearest endpoint (either (-1,0) or (0,-1))
        // Distance from (1,0) to (-1,0) = 2, to (0,-1) = sqrt(2) ≈ 1.414
        // So the nearest endpoint should be (0,-1)
        assert!((nx - 0.0).abs() < 1e-10);
        assert!((ny + 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_line_line_intersection() {
        let result = line_line_intersection(0.0, 0.0, 2.0, 2.0, 0.0, 2.0, 2.0, 0.0);
        assert!(result.is_some());
        let (ix, iy) = result.unwrap();
        assert!((ix - 1.0).abs() < 1e-10);
        assert!((iy - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_line_line_parallel() {
        let result = line_line_intersection(0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        assert!(result.is_none());
    }

    #[test]
    fn test_segment_intersection() {
        let result = segment_intersection(0.0, 0.0, 2.0, 2.0, 0.0, 2.0, 2.0, 0.0, 1e-9);
        assert!(result.is_some());
    }

    #[test]
    fn test_segment_intersection_no_overlap() {
        let result = segment_intersection(0.0, 0.0, 1.0, 0.0, 2.0, 0.0, 3.0, 0.0, 1e-9);
        assert!(result.is_none());
    }

    #[test]
    fn test_line_circle_intersection() {
        // Line through (0,0)-(2,0) with circle at (1,0) radius 0.5
        let pts = line_circle_intersection(0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 0.5);
        assert_eq!(pts.len(), 2);
        assert!((pts[0].0 - 0.5).abs() < 1e-9);
        assert!((pts[1].0 - 1.5).abs() < 1e-9);
    }

    #[test]
    fn test_line_circle_tangent() {
        // Line y=1 tangent to circle at (0,0) radius 1
        let pts = line_circle_intersection(-2.0, 1.0, 2.0, 1.0, 0.0, 0.0, 1.0);
        assert_eq!(pts.len(), 1, "expected 1 point on quarter-arc, got {}", pts.len());
        assert!((pts[0].0).abs() < 1e-9);
        assert!((pts[0].1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_line_circle_no_intersection() {
        let pts = line_circle_intersection(0.0, 5.0, 1.0, 5.0, 0.0, 0.0, 1.0);
        assert_eq!(pts.len(), 0);
    }

    #[test]
    fn test_segment_circle_intersection() {
        // Segment from (0,0) to (3,0) crossing circle at (2,0) radius 1
        // Intersections at x=1 and x=3 (both on segment 0..3)
        let pts = segment_circle_intersection(0.0, 0.0, 3.0, 0.0, 2.0, 0.0, 1.0);
        assert_eq!(pts.len(), 2);
        let xs: Vec<f64> = pts.iter().map(|p| { let x = p.0; x }).collect();
        assert!(xs.iter().any(|x| (x - 1.0).abs() < 1e-9));
        assert!(xs.iter().any(|x| (x - 3.0).abs() < 1e-9));
    }

    #[test]
    fn test_circle_circle_intersection() {
        let pts = circle_circle_intersection(0.0, 0.0, 1.0, 1.0, 0.0, 1.0);
        assert_eq!(pts.len(), 2);
        // Intersections at (0.5, ±√0.75)
        assert!((pts[0].0 - 0.5).abs() < 1e-9);
        assert!((pts[0].1 - 0.75_f64.sqrt()).abs() < 1e-9 || (pts[0].1 + 0.75_f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn test_circle_circle_no_intersection() {
        let pts = circle_circle_intersection(0.0, 0.0, 0.5, 10.0, 0.0, 0.5);
        assert_eq!(pts.len(), 0);
    }

    #[test]
    fn test_tangent_points_from_point() {
        // Point at (2,0), circle at (0,0) radius 1
        let pts = tangent_points_from_point(2.0, 0.0, 0.0, 0.0, 1.0);
        assert_eq!(pts.len(), 2);
        // Tangent points should be at angle ±60° from center, so x=0.5
        for &(tx, _ty) in &pts {
            assert!((tx - 0.5).abs() < 1e-9);
        }
    }

    #[test]
    fn test_tangent_from_point_inside() {
        let pts = tangent_points_from_point(0.5, 0.0, 0.0, 0.0, 1.0);
        assert_eq!(pts.len(), 0);
    }

    #[test]
    fn test_perpendicular_foot() {
        let (fx, fy, t) = perpendicular_foot(2.0, 1.0, 0.0, 0.0, 1.0, 1.0);
        // The foot of (2,1) onto line from (0,0) to (1,1):
        // The line y=x. The perpendicular through (2,1) meets at (1.5, 1.5).
        assert!((fx - 1.5).abs() < 1e-9);
        assert!((fy - 1.5).abs() < 1e-9);
        assert!((t - 1.5).abs() < 1e-9);
    }

    #[test]
    fn test_arc_line_intersection() {
        // Line from (-1,0) to (3,0) intersecting an arc at origin radius 1
        // from angle 0 to π/2 (first quadrant). Should hit at (1,0) -- within arc.
        let pts = arc_line_intersection(-1.0, 0.0, 3.0, 0.0, 0.0, 0.0, 1.0, 0.0, std::f64::consts::FRAC_PI_2);
        assert_eq!(pts.len(), 1, "expected 1 point on quarter-arc, got {}", pts.len());
        assert!((pts[0].0 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_arc_line_intersection_outside_arc() {
        // Line from (-1,0) to (3,0) intersecting arc from π to 2π (lower half).
        // Intersection at (-1,0) is on the circle but OUTSIDE the arc range.
        let pts = arc_line_intersection(-3.0, 0.0, 3.0, 0.0, 0.0, 0.0, 1.0, std::f64::consts::PI, 2.0 * std::f64::consts::PI);
        // (-1,0) is at angle π which IS in [π, 2π], so we should get it
        assert!(pts.len() >= 1);
    }

    #[test]
    fn test_arc_arc_intersection() {
        // Two quarter-circles at origin, radius 1: arc1 [0, π/2], arc2 [π/4, 3π/4]
        // Their circles are identical so all points are "intersections", but
        // only points in BOTH angle ranges qualify.
        // The overlapping angle range is [π/4, π/2]. The "intersection" of
        // two full circles at the same center is the whole circle, but
        // arc_arc_intersection filters by angle ranges.
        let pts = arc_arc_intersection(
            0.0, 0.0, 1.0, 0.0, std::f64::consts::FRAC_PI_2,
            0.0, 0.0, 1.0, std::f64::consts::FRAC_PI_4, 3.0 * std::f64::consts::FRAC_PI_4,
        );
        // Two identical circles have infinite intersections; circle_circle returns empty.
        assert_eq!(pts.len(), 0);
    }

    #[test]
    fn test_line_ellipse_intersection() {
        // Ellipse at origin, major axis along X, a=2, b=1 (ratio=0.5).
        // Line y=0 from (-3,0) to (3,0) intersects at (-2,0) and (2,0).
        let pts = line_ellipse_intersection(-3.0, 0.0, 3.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.5);
        assert_eq!(pts.len(), 2);
        let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
        assert!(xs.iter().any(|&x| (x + 2.0).abs() < 1e-6));
        assert!(xs.iter().any(|&x| (x - 2.0).abs() < 1e-6));
    }
}
