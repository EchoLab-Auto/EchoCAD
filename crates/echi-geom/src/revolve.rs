//! Revolve: rotate a 2D sketch profile around an axis to create a solid.

use echi_core::sketch::Sketch;
use crate::extrude::{Mesh, Point2D};

/// Revolve a sketch profile around a user-defined axis.
///
/// `axis_start` and `axis_end` define the axis of revolution as a line in the sketch (XY) plane.
/// If both are `None`, defaults to Y axis (x=0).
/// `total_angle` in radians; `segments` controls angular resolution.
pub fn revolve(
    sketch: &Sketch,
    total_angle: f64,
    segments: u32,
    axis_start: Option<(f64, f64)>,
    axis_end: Option<(f64, f64)>,
) -> Option<Mesh> {
    let (profile, profile_closed) = extract_profile(sketch)?;
    if profile.len() < 2 {
        return None;
    }

    // Determine axis in the XY sketch plane
    let (ax, ay, bx, by) = match (axis_start, axis_end) {
        (Some((x1, y1)), Some((x2, y2))) => (x1, y1, x2, y2),
        _ => (0.0, 0.0, 0.0, 1.0), // Default Y axis
    };

    let axis_dx = bx - ax;
    let axis_dy = by - ay;
    let axis_len = (axis_dx * axis_dx + axis_dy * axis_dy).sqrt();
    if axis_len < 1e-10 {
        return None;
    }
    // Unit axis direction in XY plane
    let ux = axis_dx / axis_len;
    let uy = axis_dy / axis_len;
    // Perpendicular direction in XY plane (rotate axis by +90 deg in XY)
    let px = -uy;
    let py = ux;

    let n_segments = segments.max(8);
    let angle_step = total_angle / n_segments as f64;
    let n_profile = profile.len();

    // A full 360° revolution closes onto itself: generate only n_segments
    // rings and stitch the last segment back to ring 0 (instead of emitting
    // a duplicated final ring that leaves the mesh boundary open).
    let is_full_revolution = (total_angle.abs() - 2.0 * std::f64::consts::PI).abs() < 1e-6;
    let n_rings = if is_full_revolution {
        n_segments as usize
    } else {
        n_segments as usize + 1
    };

    let mut positions = Vec::with_capacity(n_rings * n_profile * 3);
    let mut normals = Vec::with_capacity(n_rings * n_profile * 3);
    let mut indices = Vec::new();

    // Precompute profile decomposition relative to the axis
    struct ProfilePoint {
        along: f64,  // projection along axis direction
        radial: f64, // signed perpendicular distance from axis
    }
    let decomp: Vec<ProfilePoint> = profile
        .iter()
        .map(|(px, py)| {
            let rx = px - ax;
            let ry = py - ay;
            // along = dot((rx, ry), (ux, uy))
            let along = rx * ux + ry * uy;
            // radial = cross(axis_dir, (rx, ry)) = ux*ry - uy*rx
            // Positive means counter-clockwise from axis in XY plane
            let radial = ux * ry - uy * rx;
            ProfilePoint { along, radial }
        })
        .collect();

    // Generate vertices for each angular ring
    for i in 0..n_rings {
        let theta = i as f64 * angle_step;
        let cos_t = theta.cos();
        let sin_t = theta.sin();

        for pp in &decomp {
            // Radial component rotates into 3D:
            // In XY plane: pp.radial * cos_t along perp direction (px, py, 0)
            // Out of XY plane: pp.radial * sin_t along Z axis
            // Add the along-axis component in XY plane
            let x = ax + pp.along * ux + pp.radial * cos_t * px;
            let y = ay + pp.along * uy + pp.radial * cos_t * py;
            let z = pp.radial * sin_t;

            positions.push(x as f32);
            positions.push(y as f32);
            positions.push(z as f32);

            // Normal: radial direction rotated by theta
            let nr_x = cos_t * px;
            let nr_y = cos_t * py;
            let nr_z = sin_t;
            let nlen = (nr_x * nr_x + nr_y * nr_y + nr_z * nr_z).sqrt();
            if nlen > 1e-10 {
                normals.push((nr_x / nlen) as f32);
                normals.push((nr_y / nlen) as f32);
                normals.push((nr_z / nlen) as f32);
            } else {
                normals.push(0.0);
                normals.push(0.0);
                normals.push(1.0);
            }
        }
    }

    // Stitch adjacent profiles with quad strips (two triangles per quad).
    // Full revolution: the last segment wraps back to ring 0.
    let ring_below = |i: usize| -> usize {
        if is_full_revolution { (i + 1) % n_rings } else { i + 1 }
    };
    for i in 0..n_segments as usize {
        let r1 = ring_below(i);
        for j in 0..(n_profile - 1) {
            let a = (i * n_profile + j) as u32;
            let b = a + 1;
            let c = (r1 * n_profile + j) as u32;
            let d = c + 1;
            indices.push(a);
            indices.push(c);
            indices.push(b);
            indices.push(b);
            indices.push(c);
            indices.push(d);
        }
    }

    // Closed profile: stitch the last profile point back to the first on
    // every ring (only when the extracted loop is actually closed).
    if profile_closed {
        for i in 0..n_segments as usize {
            let r1 = ring_below(i);
            let a = (i * n_profile + n_profile - 1) as u32; // last vertex
            let b = (i * n_profile) as u32; // first vertex
            let c = (r1 * n_profile + n_profile - 1) as u32;
            let d = (r1 * n_profile) as u32;
            indices.push(a);
            indices.push(c);
            indices.push(b);
            indices.push(b);
            indices.push(c);
            indices.push(d);
        }
    }

    // End caps: only for PARTIAL revolves — a full 360° revolution closes
    // onto itself (the old code added interior fin caps there and left
    // partial revolves open, exactly backwards). Caps triangulate the
    // profile polygon with ear-clipping, so non-convex profiles work too.
    if !is_full_revolution && profile_closed && profile.len() >= 3 {
        let cap_tris = crate::extrude::triangulate_ear_clip(
            &profile.iter().map(|&(x, y)| Point2D { x, y }).collect::<Vec<_>>(),
        );
        let sign = if total_angle >= 0.0 { 1.0 } else { -1.0 };

        // Start cap (θ=0): outward normal is −Z × sign (against the sweep).
        for tri in &cap_tris {
            indices.push(tri[2] as u32);
            indices.push(tri[1] as u32);
            indices.push(tri[0] as u32);
        }

        // End cap (θ=total): outward normal is the sweep direction at the end.
        let ring0 = 0usize;
        let ring_end = n_segments as usize * n_profile;
        // Fix start-cap vertex normals to point along −Z × sign.
        for j in 0..n_profile {
            let vi = ring0 + j;
            normals[vi * 3] = 0.0;
            normals[vi * 3 + 1] = 0.0;
            normals[vi * 3 + 2] = (-sign) as f32;
        }
        for tri in &cap_tris {
            indices.push((ring_end + tri[0]) as u32);
            indices.push((ring_end + tri[1]) as u32);
            indices.push((ring_end + tri[2]) as u32);
        }
        // End-cap normals: sweep direction at θ=total is
        // d(pos)/dθ = radial·(−sinθ·perp + cosθ·ẑ) — uniform (0,0,sign)
        // direction component dominates for the cap face; use +Z × sign.
        let theta = total_angle;
        let (s, c) = (theta.sin(), theta.cos());
        for j in 0..n_profile {
            let vi = ring_end + j;
            // Outward = rotate −Z by the remaining sweep: (−sinθ·px, −sinθ·py, cosθ)·sign
            let nx = (-s * px) * sign;
            let ny = (-s * py) * sign;
            let nz = c * sign;
            let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-10);
            normals[vi * 3] = (nx / len) as f32;
            normals[vi * 3 + 1] = (ny / len) as f32;
            normals[vi * 3 + 2] = (nz / len) as f32;
        }
    }

    Some(Mesh {
        positions,
        normals,
        indices,
    })
}

/// Extract a 2D profile (ordered point list) from sketch entities, plus a
/// flag telling whether the profile forms a CLOSED loop. Uses the shared
/// loop extractor, so arcs, circles, splines and mixed line/arc chains all
/// work — previously only straight lines were followed and arcs silently
/// truncated the profile (原则8).
fn extract_profile(sketch: &Sketch) -> Option<(Vec<(f64, f64)>, bool)> {
    let loops = crate::extrude::extract_loops(sketch)?;
    // Revolve uses the largest-area loop as the profile.
    let mut best: Option<(Vec<(f64, f64)>, bool)> = None;
    let mut best_area = 0.0f64;
    for lp in loops {
        let mut area = 0.0;
        let n = lp.len();
        for i in 0..n {
            let j = (i + 1) % n;
            area += lp[i].x * lp[j].y - lp[j].x * lp[i].y;
        }
        let area = area.abs() / 2.0;
        if area > best_area {
            best_area = area;
            // A closed loop from extract_loops ends where it started
            // (closing edge tessellated back to the start point). Strip the
            // duplicate and remember closure.
            let mut pts: Vec<(f64, f64)> = lp.iter().map(|p| (p.x, p.y)).collect();
            let mut closed = false;
            if pts.len() >= 2 {
                let f = pts[0];
                let l = pts[pts.len() - 1];
                if ((f.0 - l.0).powi(2) + (f.1 - l.1).powi(2)).sqrt() < 1e-9 {
                    pts.pop();
                    closed = true;
                }
            }
            best = Some((pts, closed));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use echi_core::sketch::Sketch;

    fn make_rect_profile_right_of_y_axis() -> Sketch {
        // A rectangle offset from Y axis: (1,0) to (2,1)
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(1.0, 0.0);
        let p1 = sketch.add_point(2.0, 0.0);
        let p2 = sketch.add_point(2.0, 1.0);
        let p3 = sketch.add_point(1.0, 1.0);
        sketch.add_line(p0, p1);
        sketch.add_line(p1, p2);
        sketch.add_line(p2, p3);
        sketch.add_line(p3, p0);
        sketch
    }

    #[test]
    fn revolve_square_360_around_y_axis() {
        let sketch = make_rect_profile_right_of_y_axis();
        let mesh = revolve(&sketch, 2.0 * std::f64::consts::PI, 32, None, None)
            .expect("should revolve square 360 degrees");
        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
        let vc = mesh.vertex_count() as u32;
        for &idx in &mesh.indices {
            assert!(idx < vc, "index {} out of bounds", idx);
        }
        // Full revolution: closed side surface, no caps needed — but the
        // stitch count should still be substantial.
        assert!(mesh.indices.len() > 100);
    }

    #[test]
    fn revolve_partial_has_caps_full_does_not() {
        use std::collections::HashMap;
        let sketch = make_rect_profile_right_of_y_axis();

        // Partial (180°): must have caps — the mesh should be closed, i.e.
        // every undirected edge is shared by exactly 2 triangles.
        let partial = revolve(&sketch, std::f64::consts::PI, 16, None, None)
            .expect("partial revolve");
        let mut edge_count: HashMap<(u32, u32), u32> = HashMap::new();
        for tri in partial.indices.chunks_exact(3) {
            for e in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                let key = if e.0 < e.1 { (e.0, e.1) } else { (e.1, e.0) };
                *edge_count.entry(key).or_default() += 1;
            }
        }
        let open_edges = edge_count.values().filter(|&&c| c != 2).count();
        assert_eq!(open_edges, 0, "partial revolve must be watertight (caps present)");

        // Full (360°): no caps — side surface closes onto itself; also watertight.
        let full = revolve(&sketch, 2.0 * std::f64::consts::PI, 16, None, None)
            .expect("full revolve");
        let mut edge_count2: HashMap<(u32, u32), u32> = HashMap::new();
        for tri in full.indices.chunks_exact(3) {
            for e in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                let key = if e.0 < e.1 { (e.0, e.1) } else { (e.1, e.0) };
                *edge_count2.entry(key).or_default() += 1;
            }
        }
        let open_edges2 = edge_count2.values().filter(|&&c| c != 2).count();
        assert_eq!(open_edges2, 0, "full revolve must be watertight (no cap fins)");
    }

    #[test]
    fn revolve_profile_with_arc() {
        // Profile with a rounded (arc) edge: lines + semicircle arc forming
        // a "D" shape right of the Y axis. Arc must be tessellated into the
        // profile — previously line-only extraction silently dropped it.
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(1.0, 0.0);
        let p1 = sketch.add_point(2.0, 0.0);
        let pc = sketch.add_point(2.0, 0.5);
        sketch.add_line(p0, p1);
        sketch.add_arc(pc, 0.5, -std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
        // Close back to p0 with a line from arc end (2,1) to (1,0)? Use a
        // simple closed chain: line p0->p1, arc p1(2,0)→(2,1) via semicircle,
        // line (2,1)->(1,1), line (1,1)->(1,0).
        let p2 = sketch.add_point(2.0, 1.0);
        let p3 = sketch.add_point(1.0, 1.0);
        sketch.add_line(p2, p3);
        sketch.add_line(p3, p0);

        let mesh = revolve(&sketch, 2.0 * std::f64::consts::PI, 16, None, None)
            .expect("D-profile revolve should succeed");
        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
        // Arc tessellation means many more than 4 profile points (was 4 with
        // line-only extraction → coarse chord shape).
        let rings = mesh.vertex_count();
        assert!(rings > 16 * 8, "arc should add tessellation points (got {rings} verts)");
    }

    #[test]
    fn revolve_partial_angle_180() {
        let sketch = make_rect_profile_right_of_y_axis();
        let mesh = revolve(&sketch, std::f64::consts::PI, 16, None, None)
            .expect("should revolve 180 degrees");
        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
    }

    #[test]
    fn revolve_empty_sketch_returns_none() {
        let sketch = Sketch::new();
        assert!(revolve(&sketch, 2.0 * std::f64::consts::PI, 32, None, None).is_none());
    }

    #[test]
    fn revolve_zero_angle_returns_mesh() {
        let sketch = make_rect_profile_right_of_y_axis();
        let result = revolve(&sketch, 0.0, 32, None, None);
        // Zero angle produces a degenerate mesh (all slices at theta=0)
        // rather than None — the function does not short-circuit on zero angle.
        let mesh = result.expect("zero angle should still return a mesh");
        assert!(mesh.vertex_count() > 0);
    }

    #[test]
    fn revolve_degenerate_axis_returns_none() {
        let sketch = make_rect_profile_right_of_y_axis();
        // Axis start == axis end
        let result = revolve(
            &sketch,
            2.0 * std::f64::consts::PI,
            32,
            Some((0.0, 0.0)),
            Some((0.0, 0.0)),
        );
        assert!(result.is_none());
    }

    #[test]
    fn revolve_around_x_axis() {
        // Profile offset from X axis in Y: (0, 1) to (1, 2)
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(0.0, 1.0);
        let p1 = sketch.add_point(1.0, 1.0);
        let p2 = sketch.add_point(1.0, 2.0);
        let p3 = sketch.add_point(0.0, 2.0);
        sketch.add_line(p0, p1);
        sketch.add_line(p1, p2);
        sketch.add_line(p2, p3);
        sketch.add_line(p3, p0);

        let mesh = revolve(
            &sketch,
            2.0 * std::f64::consts::PI,
            32,
            Some((0.0, 0.0)),
            Some((1.0, 0.0)),
        )
        .expect("should revolve around X axis");
        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
    }

    #[test]
    fn revolve_no_nan_invariant() {
        let sketch = make_rect_profile_right_of_y_axis();
        let mesh = revolve(&sketch, 2.0 * std::f64::consts::PI, 32, None, None)
            .expect("should produce mesh");
        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
        for &v in &mesh.positions {
            assert!(!v.is_nan(), "NaN in positions");
        }
        for &n in &mesh.normals {
            assert!(!n.is_nan(), "NaN in normals");
        }
        let vc = mesh.vertex_count() as u32;
        for &idx in &mesh.indices {
            assert!(idx < vc, "index {} out of bounds", idx);
        }
    }
}
