//! Sweep: extrude a profile along a 3D path with Frenet frame orientation.

use echi_core::sketch::Sketch;
use crate::extrude::{Mesh, Point2D};

/// Sweep a profile sketch along a path sketch.
///
/// The profile is placed perpendicular to the path tangent at each vertex,
/// oriented using Frenet frames. Adjacent profile copies are stitched with
/// triangle strips; open paths get ear-clipped end caps, closed paths are
/// stitched into a seamless torus-like surface.
pub fn sweep_mesh(profile_sketch: &Sketch, path_sketch: &Sketch) -> Option<Mesh> {
    let (path, path_closed) = extract_path(path_sketch)?;
    if path.len() < 2 {
        return None;
    }

    let profile = extract_closed_profile(profile_sketch)?;
    if profile.len() < 3 {
        return None;
    }

    // Convert 2D path to 3D (path lies in the sketch's local XY plane; the
    // caller transforms the result to the sketch's world plane afterwards).
    let path_3d: Vec<(f64, f64, f64)> = path.iter().map(|&(x, y)| (x, y, 0.0)).collect();

    // Compute arc-length parameterization of the path
    let segment_lengths: Vec<f64> = path_3d
        .windows(2)
        .map(|w| {
            let dx = w[1].0 - w[0].0;
            let dy = w[1].1 - w[0].1;
            let dz = w[1].2 - w[0].2;
            (dx * dx + dy * dy + dz * dz).sqrt()
        })
        .collect();

    let total_length: f64 = segment_lengths.iter().sum();
    if total_length < 1e-10 {
        return None;
    }

    // Compute tangents at each path vertex (using central differences for interior,
    // forward/backward for endpoints)
    let tangents = compute_path_tangents(&path_3d);

    // Compute Frenet frames at each path vertex
    let frames = compute_frenet_frames(&path_3d, &tangents);

    let n_profile = profile.len();
    let n_path = path_3d.len();

    // Profile centroid — side normals point radially outward from it
    // (the old code used the path tangent, which lies IN the surface plane).
    let (pcx, pcy) = profile.iter().fold((0.0, 0.0), |(ax, ay), p| (ax + p.0, ay + p.1));
    let pcx = pcx / n_profile as f64;
    let pcy = pcy / n_profile as f64;

    let mut positions = Vec::with_capacity(n_path * n_profile * 3);
    let mut normals = Vec::with_capacity(n_path * n_profile * 3);
    let mut indices = Vec::new();

    // Place profile copies along the path
    for (path_idx, &(px, py, pz)) in path_3d.iter().enumerate() {
        let frame = &frames[path_idx];
        let (nx, ny, nz) = frame.normal;
        let (bx, by, bz) = frame.binormal;

        for &(prof_x, prof_y) in &profile {
            // Profile point in world space: path_point + prof_x * N + prof_y * B
            let wx = px + prof_x * nx + prof_y * bx;
            let wy = py + prof_x * ny + prof_y * by;
            let wz = pz + prof_x * nz + prof_y * bz;

            positions.push(wx as f32);
            positions.push(wy as f32);
            positions.push(wz as f32);

            // Side normal: radial from the profile centroid, mapped into the
            // frame (lies perpendicular to the swept surface).
            let rx = prof_x - pcx;
            let ry = prof_y - pcy;
            let rlen = (rx * rx + ry * ry).sqrt().max(1e-10);
            let onx = (rx / rlen) * nx + (ry / rlen) * bx;
            let ony = (rx / rlen) * ny + (ry / rlen) * by;
            let onz = (rx / rlen) * nz + (ry / rlen) * bz;
            normals.push(onx as f32);
            normals.push(ony as f32);
            normals.push(onz as f32);
        }
    }

    // Stitch adjacent profiles with triangles. Closed paths wrap the last
    // profile back to the first (seamless torus); open paths stop at n-1.
    let seg_count = if path_closed { n_path } else { n_path - 1 };
    for path_i in 0..seg_count {
        let next_i = (path_i + 1) % n_path;
        for prof_j in 0..n_profile {
            let next_j = (prof_j + 1) % n_profile;

            let a = (path_i * n_profile + prof_j) as u32;
            let b = (path_i * n_profile + next_j) as u32;
            let c = (next_i * n_profile + prof_j) as u32;
            let d = (next_i * n_profile + next_j) as u32;

            indices.push(a);
            indices.push(c);
            indices.push(b);
            indices.push(b);
            indices.push(c);
            indices.push(d);
        }
    }

    // End caps only for OPEN paths, ear-clipped so non-convex profiles work
    // (the old centroid fan spilled outside L-shaped profiles).
    if !path_closed {
        let cap_tris = crate::extrude::triangulate_ear_clip(
            &profile.iter().map(|&(x, y)| Point2D { x, y }).collect::<Vec<_>>(),
        );

        // Start cap (faces −tangent): reverse winding
        for tri in &cap_tris {
            indices.push(tri[2] as u32);
            indices.push(tri[1] as u32);
            indices.push(tri[0] as u32);
        }
        let (tnx, tny, tnz) = frames[0].tangent;
        for j in 0..n_profile {
            normals[j * 3] = -tnx as f32;
            normals[j * 3 + 1] = -tny as f32;
            normals[j * 3 + 2] = -tnz as f32;
        }

        // End cap (faces +tangent)
        let end_offset = (n_path - 1) * n_profile;
        for tri in &cap_tris {
            indices.push((end_offset + tri[0]) as u32);
            indices.push((end_offset + tri[1]) as u32);
            indices.push((end_offset + tri[2]) as u32);
        }
        let (enx, eny, enz) = frames[n_path - 1].tangent;
        for j in 0..n_profile {
            let vi = end_offset + j;
            normals[vi * 3] = enx as f32;
            normals[vi * 3 + 1] = eny as f32;
            normals[vi * 3 + 2] = enz as f32;
        }
    }

    Some(Mesh {
        positions,
        normals,
        indices,
    })
}

struct FrenetFrame {
    tangent: (f64, f64, f64),
    normal: (f64, f64, f64),
    binormal: (f64, f64, f64),
}

/// Compute path tangents using central differences for smooth results.
fn compute_path_tangents(path: &[(f64, f64, f64)]) -> Vec<(f64, f64, f64)> {
    let n = path.len();
    let mut tangents = Vec::with_capacity(n);

    for i in 0..n {
        let (tx, ty, tz) = if i == 0 {
            // Forward difference for first point
            let dx = path[1].0 - path[0].0;
            let dy = path[1].1 - path[0].1;
            let dz = path[1].2 - path[0].2;
            (dx, dy, dz)
        } else if i == n - 1 {
            // Backward difference for last point
            let dx = path[n - 1].0 - path[n - 2].0;
            let dy = path[n - 1].1 - path[n - 2].1;
            let dz = path[n - 1].2 - path[n - 2].2;
            (dx, dy, dz)
        } else {
            // Central difference for interior points
            let dx = path[i + 1].0 - path[i - 1].0;
            let dy = path[i + 1].1 - path[i - 1].1;
            let dz = path[i + 1].2 - path[i - 1].2;
            (dx, dy, dz)
        };

        let len = (tx * tx + ty * ty + tz * tz).sqrt();
        if len > 1e-10 {
            tangents.push((tx / len, ty / len, tz / len));
        } else {
            tangents.push((1.0, 0.0, 0.0));
        }
    }

    tangents
}

/// Compute Frenet frames at each path vertex.
/// Uses the "up vector" method with Z-axis as the initial reference,
/// with parallel transport to avoid twisting.
fn compute_frenet_frames(
    path: &[(f64, f64, f64)],
    tangents: &[(f64, f64, f64)],
) -> Vec<FrenetFrame> {
    let n = path.len();

    // Start with a reference normal: choose a vector perpendicular to the first tangent
    let t0 = tangents[0];
    let ref_normal = if t0.2.abs() < 0.9 {
        // T is not close to Z, use Z as up
        let nx = -t0.1;
        let ny = t0.0;
        let len = (nx * nx + ny * ny).sqrt();
        if len > 1e-10 {
            (-t0.2 * t0.0 / len, -t0.2 * t0.1 / len, len)
        } else {
            (0.0, 1.0, 0.0)
        }
    } else {
        // T is close to Z, use X as up
        (1.0, 0.0, 0.0)
    };

    // Normalize reference normal and ensure it's perpendicular to tangent
    let rlen = (ref_normal.0 * ref_normal.0 + ref_normal.1 * ref_normal.1 + ref_normal.2 * ref_normal.2).sqrt();
    let (mut nx, mut ny, mut nz) = if rlen > 1e-10 {
        (ref_normal.0 / rlen, ref_normal.1 / rlen, ref_normal.2 / rlen)
    } else {
        (1.0, 0.0, 0.0)
    };

    // Make perpendicular to tangent
    let dot = nx * t0.0 + ny * t0.1 + nz * t0.2;
    nx -= dot * t0.0;
    ny -= dot * t0.1;
    nz -= dot * t0.2;
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    if len > 1e-10 {
        nx /= len;
        ny /= len;
        nz /= len;
    }

    let mut frames = Vec::with_capacity(n);

    for i in 0..n {
        let t = tangents[i];

        // Binormal = T × N
        let bx = t.1 * nz - t.2 * ny;
        let by = t.2 * nx - t.0 * nz;
        let bz = t.0 * ny - t.1 * nx;

        frames.push(FrenetFrame {
            tangent: t,
            normal: (nx, ny, nz),
            binormal: (bx, by, bz),
        });

        // Parallel transport the normal to the next segment (if not last)
        if i < n - 1 {
            let t_next = tangents[i + 1];
            // Rotate the normal around the axis T[i] × T[i+1]
            let cross_x = t.1 * t_next.2 - t.2 * t_next.1;
            let cross_y = t.2 * t_next.0 - t.0 * t_next.2;
            let cross_z = t.0 * t_next.1 - t.1 * t_next.0;
            let cross_len = (cross_x * cross_x + cross_y * cross_y + cross_z * cross_z).sqrt();

            if cross_len > 1e-10 {
                let ax = cross_x / cross_len;
                let ay = cross_y / cross_len;
                let az = cross_z / cross_len;

                let dot = t.0 * t_next.0 + t.1 * t_next.1 + t.2 * t_next.2;
                let angle = dot.clamp(-1.0, 1.0).acos();

                let cos_a = angle.cos();
                let sin_a = angle.sin();

                // Rodrigues rotation of normal around axis
                let ndot = nx * ax + ny * ay + nz * az;
                let rnx = nx * cos_a + (ay * nz - az * ny) * sin_a + ndot * (1.0 - cos_a) * ax;
                let rny = ny * cos_a + (az * nx - ax * nz) * sin_a + ndot * (1.0 - cos_a) * ay;
                let rnz = nz * cos_a + (ax * ny - ay * nx) * sin_a + ndot * (1.0 - cos_a) * az;

                nx = rnx;
                ny = rny;
                nz = rnz;
            }
        }
    }

    frames
}

/// Extract the sweep path as an ordered point list, plus whether it is a
/// CLOSED loop. Uses the shared loop extractor so arcs, splines and circles
/// all work (a circular path yields a closed loop — torus-like sweeps).
/// Falls back to a raw line-chain walk for degenerate 2-point paths that
/// the loop extractor drops (a polygon needs ≥3 points to count as a loop).
fn extract_path(sketch: &Sketch) -> Option<(Vec<(f64, f64)>, bool)> {
    if let Some(loops) = crate::extrude::extract_loops(sketch) {
        // The path is the chain with the most points.
        let mut best: Option<(Vec<(f64, f64)>, bool)> = None;
        for lp in loops {
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
            if pts.len() >= 2 && best.as_ref().map_or(true, |(b, _)| pts.len() > b.len()) {
                best = Some((pts, closed));
            }
        }
        if let Some(b) = best {
            return Some(b);
        }
    }

    // Fallback: single-line or 2-point chains the loop extractor ignores.
    use echi_core::sketch::SketchEntity;
    let lines: Vec<_> = sketch
        .entities
        .iter()
        .filter_map(|(_, e)| match e {
            SketchEntity::Line { start, end, .. } => Some((*start, *end)),
            _ => None,
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    let mut points = Vec::new();
    let mut used = vec![false; lines.len()];
    let first = lines[0];
    used[0] = true;
    if let (Some(ps), Some(pe)) = (sketch.get_point(first.0), sketch.get_point(first.1)) {
        points.push((ps.x, ps.y));
        points.push((pe.x, pe.y));
    } else {
        return None;
    }
    let mut current = first.1;
    loop {
        let mut found = false;
        for (i, &(s, e)) in lines.iter().enumerate() {
            if used[i] {
                continue;
            }
            if s == current {
                used[i] = true;
                if let Some(p) = sketch.get_point(e) {
                    points.push((p.x, p.y));
                    current = e;
                    found = true;
                }
            } else if e == current {
                used[i] = true;
                if let Some(p) = sketch.get_point(s) {
                    points.push((p.x, p.y));
                    current = s;
                    found = true;
                }
            }
            if found {
                break;
            }
        }
        if !found {
            break;
        }
    }
    if points.len() >= 2 {
        Some((points, false))
    } else {
        None
    }
}

/// Extract a closed 2D profile from sketch entities. Uses the shared loop
/// extractor (arcs/circles/splines supported); takes the largest-area loop.
fn extract_closed_profile(sketch: &Sketch) -> Option<Vec<(f64, f64)>> {
    let loops = crate::extrude::extract_loops(sketch)?;
    let mut best: Option<Vec<(f64, f64)>> = None;
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
            let mut pts: Vec<(f64, f64)> = lp.iter().map(|p| (p.x, p.y)).collect();
            if pts.len() >= 2 {
                let f = pts[0];
                let l = pts[pts.len() - 1];
                if ((f.0 - l.0).powi(2) + (f.1 - l.1).powi(2)).sqrt() < 1e-9 {
                    pts.pop();
                }
            }
            best = Some(pts);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use echi_core::sketch::Sketch;

    fn make_square_profile() -> Sketch {
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(0.0, 0.0);
        let p1 = sketch.add_point(1.0, 0.0);
        let p2 = sketch.add_point(1.0, 1.0);
        let p3 = sketch.add_point(0.0, 1.0);
        sketch.add_line(p0, p1);
        sketch.add_line(p1, p2);
        sketch.add_line(p2, p3);
        sketch.add_line(p3, p0);
        sketch
    }

    fn assert_watertight(mesh: &Mesh) {
        use std::collections::HashMap;
        let mut edge_count: HashMap<(u32, u32), u32> = HashMap::new();
        for tri in mesh.indices.chunks_exact(3) {
            for e in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                let key = if e.0 < e.1 { (e.0, e.1) } else { (e.1, e.0) };
                *edge_count.entry(key).or_default() += 1;
            }
        }
        let open = edge_count.values().filter(|&&c| c != 2).count();
        assert_eq!(open, 0, "mesh must be watertight");
    }

    #[test]
    fn sweep_circle_path_is_seamless_torus() {
        // Profile: small square offset from origin; Path: standalone circle
        // (closed). The sweep must be a seamless torus — no caps, no seam.
        let profile = make_square_profile();
        let mut path = Sketch::new();
        let pc = path.add_point(5.0, 5.0);
        path.add_circle(pc, 3.0);

        let mesh = sweep_mesh(&profile, &path).expect("torus sweep should succeed");
        assert!(mesh.vertex_count() > 0);
        assert_watertight(&mesh);
    }

    #[test]
    fn sweep_along_arc_path() {
        // Path with an arc: quarter-arc between two points. Previously the
        // arc was silently dropped (line-only extraction), producing a
        // straight chord sweep.
        let profile = make_square_profile();
        let mut path = Sketch::new();
        let p0 = path.add_point(0.0, 0.0);
        let p1 = path.add_point(3.0, 0.0);
        let pc = path.add_point(3.0, 3.0);
        path.add_line(p0, p1);
        path.add_arc(pc, 3.0, -std::f64::consts::FRAC_PI_2, 0.0);

        let mesh = sweep_mesh(&profile, &path).expect("arc path sweep should succeed");
        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
        // Arc tessellation must add path vertices beyond the 2 line endpoints.
        // 4 profile points × (line pts + arc tessellation > 4) rings.
        assert!(
            mesh.vertex_count() > 4 * 4,
            "arc should tessellate into path rings (got {})",
            mesh.vertex_count()
        );
        let vc = mesh.vertex_count() as u32;
        for &idx in &mesh.indices {
            assert!(idx < vc, "index out of bounds");
        }
    }

    fn make_straight_path() -> Sketch {
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(0.0, 0.0);
        let p1 = sketch.add_point(5.0, 0.0);
        sketch.add_line(p0, p1);
        sketch
    }

    #[test]
    fn sweep_square_along_line() {
        let profile = make_square_profile();
        let path = make_straight_path();
        let mesh = sweep_mesh(&profile, &path).expect("should sweep square along line");
        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
        let vcount = mesh.vertex_count() as u32;
        for &idx in &mesh.indices {
            assert!(idx < vcount, "index {} out of bounds (vertex_count {})", idx, vcount);
        }
    }

    #[test]
    fn sweep_triangle_profile() {
        let mut profile = Sketch::new();
        let p0 = profile.add_point(0.0, 0.0);
        let p1 = profile.add_point(1.0, 0.0);
        let p2 = profile.add_point(0.5, 1.0);
        profile.add_line(p0, p1);
        profile.add_line(p1, p2);
        profile.add_line(p2, p0);

        let mut path = Sketch::new();
        let a = path.add_point(0.0, 0.0);
        let b = path.add_point(0.0, 3.0);
        path.add_line(a, b);

        let mesh = sweep_mesh(&profile, &path).expect("should sweep triangle");
        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
    }

    #[test]
    fn sweep_empty_profile_returns_none() {
        let profile = Sketch::new();
        let path = make_straight_path();
        assert!(sweep_mesh(&profile, &path).is_none());
    }

    #[test]
    fn sweep_empty_path_returns_none() {
        let profile = make_square_profile();
        let path = Sketch::new();
        assert!(sweep_mesh(&profile, &path).is_none());
    }

    #[test]
    fn sweep_single_point_path_returns_none() {
        let profile = make_square_profile();
        let mut path = Sketch::new();
        path.add_point(0.0, 0.0);
        assert!(sweep_mesh(&profile, &path).is_none());
    }

    #[test]
    fn sweep_no_nan_invariant() {
        let profile = make_square_profile();
        let path = make_straight_path();
        let mesh = sweep_mesh(&profile, &path).expect("should produce mesh");
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

    #[test]
    fn sweep_curved_path() {
        let profile = make_square_profile();
        let mut path = Sketch::new();
        let a = path.add_point(0.0, 0.0);
        let b = path.add_point(3.0, 2.0);
        let c = path.add_point(6.0, 0.0);
        path.add_line(a, b);
        path.add_line(b, c);

        let mesh = sweep_mesh(&profile, &path).expect("should sweep along multi-segment path");
        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
        let vc = mesh.vertex_count() as u32;
        for &idx in &mesh.indices {
            assert!(idx < vc, "index {} out of bounds", idx);
        }
    }
}
