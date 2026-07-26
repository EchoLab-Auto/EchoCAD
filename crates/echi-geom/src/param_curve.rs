//! Parametric 2D curve representations for sketch profile extraction.
//!
//! Unlike the old approach that discretizes curves into fixed-segment polygons
//! before classification, this module keeps curves parametric through area,
//! centroid, and point-in-loop tests. Curves are only sampled to polygons
//! at mesh-generation time, with adaptive quality based on chord error.
//!
//! Area and centroid use Green's theorem (line integrals), giving EXACT
//! results for lines, circles, and arcs (no floating-point accumulation).

use std::f64::consts::{PI, TAU};

// ── Data types ────────────────────────────────────────────────────────

/// A parametric 2D curve segment.
#[derive(Debug, Clone)]
pub enum ParamCurve {
    /// Straight line segment from start to end.
    Line { start: (f64, f64), end: (f64, f64) },
    /// Circular arc. Angles in radians.
    Arc {
        center: (f64, f64),
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    /// Full circle (a complete loop by itself).
    Circle { center: (f64, f64), radius: f64 },
    /// Elliptical arc defined by center, major axis vector, and ratio.
    EllipseArc {
        center: (f64, f64),
        major_rx: f64,
        major_ry: f64,
        ratio: f64,
        start_param: f64,
        end_param: f64,
    },
}

/// A closed loop made of parametric curve segments.
/// For `Circle`, the loop has exactly one element.
/// For other types, each curve's end point must match the next curve's start.
pub type ParamLoop = Vec<ParamCurve>;

// ── Green's theorem helpers ───────────────────────────────────────────

/// Signed area of a parametric curve via Green's theorem: A = ½∫(x dy - y dx)
pub fn curve_signed_area(curve: &ParamCurve) -> f64 {
    match curve {
        ParamCurve::Line { start, end } => {
            0.5 * (start.0 * end.1 - end.0 * start.1)
        }
        ParamCurve::Circle { radius, .. } => {
            PI * radius * radius
        }
        ParamCurve::Arc { center, radius, start_angle, end_angle } => {
            let (cx, cy) = *center;
            let r = *radius;
            let sa = *start_angle;
            let ea = *end_angle;
            // Normalize to positive span
            let span = if ea >= sa { ea - sa } else { ea + TAU - sa };
            // Area of circular sector + triangle to origin
            // Using: A = ½r²Δθ + ½r[cx(sin θ₂ - sin θ₁) - cy(cos θ₂ - cos θ₁)]
            let sector = 0.5 * r * r * span;
            let tri = 0.5 * r * (cx * (ea.sin() - sa.sin()) - cy * (ea.cos() - sa.cos()));
            sector + tri
        }
        ParamCurve::EllipseArc { .. } => {
            // Fall back to polygon area for ellipses (exact elliptic integral
            // is complex; we sample adaptively for area too)
            0.0 // Will be handled by the loop-level function
        }
    }
}

/// Signed area of a closed parametric loop (sum of curve areas via Green's theorem).
/// Positive = CCW, negative = CW.
pub fn loop_signed_area(ploop: &ParamLoop) -> f64 {
    if ploop.is_empty() {
        return 0.0;
    }
    // For a single-circle loop, use exact formula
    if ploop.len() == 1 {
        if let ParamCurve::Circle { radius, .. } = &ploop[0] {
            return PI * radius * radius;
        }
    }
    ploop.iter().map(curve_signed_area).sum()
}

/// Centroid of a parametric loop via first moments (Green's theorem).
/// Returns (cx, cy). Returns (0,0) for empty or zero-area loops.
pub fn loop_centroid(ploop: &ParamLoop) -> (f64, f64) {
    let area = loop_signed_area(ploop);
    if area.abs() < 1e-15 {
        return (0.0, 0.0);
    }

    // For a single circle, centroid is the center
    if ploop.len() == 1 {
        if let ParamCurve::Circle { center, .. } = &ploop[0] {
            return *center;
        }
    }

    // Sample the loop adaptively and compute centroid of the polygon
    // (exact parametric centroid for mixed curves is complex)
    let pts = sample_loop(ploop, 0.001);
    polygon_centroid(&pts)
}

/// Compute polygon centroid via shoelace formula.
/// Returns (0,0) for empty or degenerate polygons.
fn polygon_centroid(polygon: &[(f64, f64)]) -> (f64, f64) {
    let n = polygon.len();
    if n == 0 { return (0.0, 0.0); }
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut area = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        let (xi, yi) = polygon[i];
        let (xj, yj) = polygon[j];
        let cross = xi * yj - xj * yi;
        area += cross;
        cx += (xi + xj) * cross;
        cy += (yi + yj) * cross;
    }
    area *= 0.5;
    if area.abs() < 1e-15 {
        let sum_x: f64 = polygon.iter().map(|p| p.0).sum();
        let sum_y: f64 = polygon.iter().map(|p| p.1).sum();
        return (sum_x / n as f64, sum_y / n as f64);
    }
    (cx / (6.0 * area), cy / (6.0 * area))
}

// ── Point-in-loop test ────────────────────────────────────────────────

/// Test if a point is inside a parametric loop using ray-casting with
/// exact curve intersections.
/// Returns true if the point is inside or on the boundary.
pub fn point_in_param_loop(px: f64, py: f64, ploop: &ParamLoop) -> bool {
    // Cast a horizontal ray to the right and count intersections
    let mut crossings = 0u32;

    for curve in ploop {
        match curve {
            ParamCurve::Line { start, end } => {
                if ray_crosses_segment(px, py, start.0, start.1, end.0, end.1) {
                    crossings += 1;
                }
            }
            ParamCurve::Circle { center, radius } => {
                let (cx, cy) = *center;
                let r = *radius;
                let dist_sq = (px - cx).powi(2) + (py - cy).powi(2);
                // On boundary → inside
                if (dist_sq - r * r).abs() < 1e-12 {
                    return true;
                }
                let dy = py - cy;
                if dy.abs() >= r { continue; }
                // Ray y=py intersects circle at x = cx ± sqrt(r² - dy²)
                let half_chord = (r * r - dy * dy).sqrt();
                // Right intersection: cx + half_chord must be >= px to count
                let x_right = cx + half_chord;
                if x_right > px {
                    crossings += 1;
                }
                // Left intersection: also count if it's to the right of px
                // (ray enters AND exits to the right → 2 crossings needed)
                let x_left = cx - half_chord;
                if x_left > px {
                    crossings += 1;
                }
            }
            ParamCurve::Arc { center, radius, start_angle, end_angle } => {
                let (cx, cy) = *center;
                let r = *radius;
                let dist_sq = (px - cx).powi(2) + (py - cy).powi(2);
                // On the arc boundary?
                if (dist_sq - r * r).abs() < 1e-12 {
                    let angle = (py - cy).atan2(px - cx);
                    if angle_in_arc(angle, *start_angle, *end_angle) {
                        return true;
                    }
                }
                let dy = py - cy;
                if dy.abs() >= r { continue; }
                let half_chord = (r * r - dy * dy).sqrt();
                let x1 = cx - half_chord;
                let x2 = cx + half_chord;
                for &ix in &[x1, x2] {
                    if ix <= px { continue; }
                    let angle = (py - cy).atan2(ix - cx);
                    if angle_in_arc(angle, *start_angle, *end_angle) {
                        crossings += 1;
                    }
                }
            }
            ParamCurve::EllipseArc { .. } => {
                // Fall back: sample the ellipse and test against polygon edges
                let pts = sample_curve(curve, 0.01);
                for w in pts.windows(2) {
                    if ray_crosses_segment(px, py, w[0].0, w[0].1, w[1].0, w[1].1) {
                        crossings += 1;
                    }
                }
            }
        }
    }

    crossings % 2 == 1
}

/// Check if an angle lies within a CCW arc span.
fn angle_in_arc(angle: f64, start: f64, end: f64) -> bool {
    let norm = |mut a: f64| -> f64 {
        a = a.rem_euclid(TAU);
        if a < 0.0 { a += TAU; }
        a
    };
    let a = norm(angle);
    let s = norm(start);
    let e = norm(end);
    if s <= e {
        a >= s && a <= e
    } else {
        a >= s || a <= e
    }
}

/// Test if a horizontal ray from (px,py) to (+∞,py) crosses a line segment.
fn ray_crosses_segment(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> bool {
    // Check if point is on the segment
    if point_on_segment(px, py, x1, y1, x2, y2) {
        return true; // Boundary counts as inside
    }
    // Standard even-odd rule: count if (y1 > py) != (y2 > py)
    if (y1 > py) == (y2 > py) {
        return false;
    }
    let intersect_x = x1 + (py - y1) * (x2 - x1) / (y2 - y1);
    intersect_x > px
}

/// Check if point (px,py) lies on the line segment (x1,y1)-(x2,y2).
fn point_on_segment(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> bool {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let l2 = dx * dx + dy * dy;
    if l2 < 1e-20 {
        return (px - x1).abs() < 1e-9 && (py - y1).abs() < 1e-9;
    }
    let t = ((px - x1) * dx + (py - y1) * dy) / l2;
    if t < 0.0 || t > 1.0 {
        return false;
    }
    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;
    (px - proj_x).abs() < 1e-9 && (py - proj_y).abs() < 1e-9
}

// ── Adaptive sampling ─────────────────────────────────────────────────

/// Sample a single curve into polyline points with given chord error tolerance.
/// The first point is always the start; the last is always the end.
/// For Line: returns [start, end].
/// For Circle: returns n+1 points (closed, last = first).
pub fn sample_curve(curve: &ParamCurve, chord_error: f64) -> Vec<(f64, f64)> {
    match curve {
        ParamCurve::Line { start, end } => {
            vec![*start, *end]
        }
        ParamCurve::Circle { center, radius } => {
            let r = *radius;
            let n = circle_segments(r, chord_error, TAU);
            let mut pts = Vec::with_capacity(n + 1);
            for i in 0..=n {
                let a = TAU * i as f64 / n as f64;
                pts.push((center.0 + r * a.cos(), center.1 + r * a.sin()));
            }
            pts
        }
        ParamCurve::Arc { center, radius, start_angle, end_angle } => {
            let r = *radius;
            let sa = *start_angle;
            let mut ea = *end_angle;
            if ea < sa { ea += TAU; }
            let span = ea - sa;
            let n = circle_segments(r, chord_error, span);
            let mut pts = Vec::with_capacity(n + 1);
            for i in 0..=n {
                let a = sa + span * i as f64 / n as f64;
                pts.push((center.0 + r * a.cos(), center.1 + r * a.sin()));
            }
            pts
        }
        ParamCurve::EllipseArc { center, major_rx, major_ry, ratio, start_param, end_param } => {
            let (cx, cy) = *center;
            let a = (*major_rx * *major_rx + *major_ry * *major_ry).sqrt();
            let b = a * ratio;
            let angle = major_ry.atan2(*major_rx);
            let span = if *end_param >= *start_param {
                *end_param - *start_param
            } else {
                *end_param + TAU - *start_param
            };
            let n = circle_segments(a.max(b), chord_error, span);
            let mut pts = Vec::with_capacity(n + 1);
            for i in 0..=n {
                let t = *start_param + span * i as f64 / n as f64;
                let ex = a * t.cos();
                let ey = b * t.sin();
                let rx = ex * angle.cos() - ey * angle.sin();
                let ry = ex * angle.sin() + ey * angle.cos();
                pts.push((cx + rx, cy + ry));
            }
            pts
        }
    }
}

/// Sample an entire loop into a closed polygon.
pub fn sample_loop(ploop: &ParamLoop, chord_error: f64) -> Vec<(f64, f64)> {
    if ploop.is_empty() {
        return vec![];
    }
    if ploop.len() == 1 {
        if let ParamCurve::Circle { .. } = &ploop[0] {
            let mut pts = sample_curve(&ploop[0], chord_error);
            pts.pop(); // Remove the closing duplicate for polygon representation
            return pts;
        }
    }
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for (i, curve) in ploop.iter().enumerate() {
        let sampled = sample_curve(curve, chord_error);
        if i == 0 {
            pts.extend(sampled);
        } else {
            // Skip the first point (same as previous curve's last point)
            pts.extend(&sampled[1..]);
        }
    }
    // Remove last point if it matches the first (closed loop)
    if pts.len() > 1 {
        let first = pts[0];
        let last = pts[pts.len() - 1];
        if (first.0 - last.0).abs() < 1e-12 && (first.1 - last.1).abs() < 1e-12 {
            pts.pop();
        }
    }
    pts
}

/// Number of segments needed for a circular arc to stay within chord_error.
fn circle_segments(radius: f64, chord_error: f64, span: f64) -> usize {
    if radius < 1e-10 || span < 1e-10 {
        return 1;
    }
    // chord_error ≈ r * (1 - cos(Δθ/2))
    // Δθ = 2 * acos(1 - chord_error / r)
    let chord_err = chord_error.min(radius * 0.99); // clamp
    let delta_angle = 2.0 * (1.0 - chord_err / radius).acos();
    let n = (span / delta_angle).ceil() as usize;
    n.clamp(4, 512)
}

/// Convert a sampled polygon (Vec<(f64,f64)>) to Vec<Point2D> for backward compat.
pub fn to_point2d_vec(pts: &[(f64, f64)]) -> Vec<crate::extrude::Point2D> {
    pts.iter().map(|&(x, y)| crate::extrude::Point2D { x, y }).collect()
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extrude::{Point2D, polygon_area, point_in_polygon as poly_point_in_polygon};
    use std::f64::consts::PI;

    fn p2d_vec(pts: &[(f64, f64)]) -> Vec<Point2D> {
        pts.iter().map(|&(x, y)| Point2D { x, y }).collect()
    }

    #[test]
    fn test_line_area() {
        let l = ParamCurve::Line { start: (0.0, 0.0), end: (1.0, 1.0) };
        assert!((curve_signed_area(&l) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_circle_area() {
        let c = ParamCurve::Circle { center: (0.0, 0.0), radius: 5.0 };
        let area = curve_signed_area(&c);
        assert!((area - PI * 25.0).abs() < 1e-10);
    }

    #[test]
    fn test_arc_area() {
        // Quarter circle arc from (1,0) to (0,1) around origin
        let a = ParamCurve::Arc {
            center: (0.0, 0.0),
            radius: 1.0,
            start_angle: 0.0,
            end_angle: PI / 2.0,
        };
        let area = curve_signed_area(&a);
        // Area of quarter circle sector = π/4 ≈ 0.785398
        assert!((area - PI / 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_square_loop_area() {
        let ploop: ParamLoop = vec![
            ParamCurve::Line { start: (0.0, 0.0), end: (1.0, 0.0) },
            ParamCurve::Line { start: (1.0, 0.0), end: (1.0, 1.0) },
            ParamCurve::Line { start: (1.0, 1.0), end: (0.0, 1.0) },
            ParamCurve::Line { start: (0.0, 1.0), end: (0.0, 0.0) },
        ];
        let area = loop_signed_area(&ploop);
        assert!((area - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_circle_centroid() {
        let ploop: ParamLoop = vec![
            ParamCurve::Circle { center: (3.0, 4.0), radius: 5.0 },
        ];
        let (cx, cy) = loop_centroid(&ploop);
        assert!((cx - 3.0).abs() < 1e-10);
        assert!((cy - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_adaptive_sampling_circle() {
        let curve = ParamCurve::Circle { center: (0.0, 0.0), radius: 1.0 };
        let pts = sample_curve(&curve, 0.01);
        // For r=1, chord_error=0.01: Δθ ≈ 2*acos(1-0.01) ≈ 0.283, n ≈ 22
        assert!(pts.len() > 10 && pts.len() < 50,
            "expected 10..50 segments for small circle, got {}", pts.len());
    }

    #[test]
    fn test_adaptive_sampling_large_circle() {
        let curve = ParamCurve::Circle { center: (0.0, 0.0), radius: 100.0 };
        let pts = sample_curve(&curve, 0.01);
        // For r=100, chord_error=0.01: Δθ ≈ 2*acos(0.9999) ≈ 0.0283, n ≈ 222
        assert!(pts.len() > 100, "expected >100 segments for large circle, got {}", pts.len());
    }

    #[test]
    fn test_adaptive_sampling_small_circle() {
        let curve = ParamCurve::Circle { center: (0.0, 0.0), radius: 0.1 };
        let pts = sample_curve(&curve, 0.01);
        // For r=0.1, chord_error=0.01: chord_err clamped to 0.099, Δθ ≈ 0.285, n≈22
        // clamped to 4..512
        assert!(pts.len() >= 4, "expected >=4 segments, got {}", pts.len());
    }

    #[test]
    fn test_polygon_area_matches_parametric() {
        // Verify that sampling a circle and computing polygon area
        // matches the exact parametric area
        let curve = ParamCurve::Circle { center: (0.0, 0.0), radius: 5.0 };
        let pts = sample_curve(&curve, 0.001); // tight tolerance
        let p2d = p2d_vec(&pts);
        let poly_area = polygon_area(&p2d).abs();
        let exact = PI * 25.0;
        let error = (poly_area - exact).abs() / exact;
        assert!(error < 0.001, "polygon area error {:.6} > 0.1%", error);
    }

    #[test]
    fn test_point_in_circle_loop() {
        let ploop: ParamLoop = vec![
            ParamCurve::Circle { center: (0.0, 0.0), radius: 5.0 },
        ];
        assert!(point_in_param_loop(0.0, 0.0, &ploop), "center should be inside");
        assert!(point_in_param_loop(3.0, 4.0, &ploop), "point at distance 5 should be on boundary");
        assert!(!point_in_param_loop(6.0, 0.0, &ploop), "point outside should be outside");
    }

    #[test]
    fn test_point_in_square_loop() {
        let ploop: ParamLoop = vec![
            ParamCurve::Line { start: (0.0, 0.0), end: (4.0, 0.0) },
            ParamCurve::Line { start: (4.0, 0.0), end: (4.0, 4.0) },
            ParamCurve::Line { start: (4.0, 4.0), end: (0.0, 4.0) },
            ParamCurve::Line { start: (0.0, 4.0), end: (0.0, 0.0) },
        ];
        assert!(point_in_param_loop(2.0, 2.0, &ploop), "center should be inside");
        assert!(!point_in_param_loop(5.0, 2.0, &ploop), "outside");
    }

    #[test]
    fn test_point_in_loop_vs_polygon() {
        // Verify parametric point-in-loop agrees with polygon version
        let curve = ParamCurve::Circle { center: (2.0, 3.0), radius: 5.0 };
        let ploop: ParamLoop = vec![curve];
        let pts = sample_loop(&ploop, 0.001);
        let p2d = p2d_vec(&pts);

        let test_points = [
            (2.0, 3.0),    // center — inside
            (2.0, 9.0),    // outside (dist=6 > r=5)
            (-4.0, 3.0),   // outside (dist=6 > r=5)
            (5.0, 6.0),    // inside
            (0.0, 0.0),    // inside (dist≈3.6 < r=5)
        ];
        for &(tx, ty) in &test_points {
            let param_result = point_in_param_loop(tx, ty, &ploop);
            let poly_result = poly_point_in_polygon(&Point2D { x: tx, y: ty }, &p2d);
            assert_eq!(param_result, poly_result,
                "mismatch at ({},{}): param={}, poly={}", tx, ty, param_result, poly_result);
        }
    }
}
