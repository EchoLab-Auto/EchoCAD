//! Revolve: rotate a 2D sketch profile around an axis to create a solid.

use echi_core::sketch::{Sketch, SketchEntity};
use crate::extrude::Mesh;

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
    let profile = extract_profile(sketch)?;
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

    let total_verts = (n_segments as usize + 1) * n_profile;
    let mut positions = Vec::with_capacity(total_verts * 3);
    let mut normals = Vec::with_capacity(total_verts * 3);
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

    // Generate vertices for each angular segment
    for i in 0..=n_segments {
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

    // Stitch adjacent profiles with quad strips (two triangles per quad)
    for i in 0..n_segments as usize {
        for j in 0..(n_profile - 1) {
            let a = (i * n_profile + j) as u32;
            let b = a + 1;
            let c = a + n_profile as u32;
            let d = c + 1;
            indices.push(a);
            indices.push(c);
            indices.push(b);
            indices.push(b);
            indices.push(c);
            indices.push(d);
        }
    }

    // Closed profile: stitch the last vertices back to the first
    if let (Some(first), Some(last)) = (profile.first(), profile.last()) {
        let dx = first.0 - last.0;
        let dy = first.1 - last.1;
        if (dx * dx + dy * dy).sqrt() < 1e-6 {
            // Profile is closed
            for i in 0..n_segments as usize {
                let a = (i * n_profile + n_profile - 1) as u32; // last vertex
                let b = (i * n_profile) as u32; // first vertex
                let c = ((i + 1) * n_profile + n_profile - 1) as u32;
                let d = ((i + 1) * n_profile) as u32;
                indices.push(a);
                indices.push(c);
                indices.push(b);
                indices.push(b);
                indices.push(c);
                indices.push(d);
            }
        }
    }

    // End caps: generate fan triangles for start (theta=0) and end (theta=total_angle)
    // Only cap if the revolution is full 360° (closed surface)
    let is_full_revolution = (total_angle - 2.0 * std::f64::consts::PI).abs() < 1e-6;

    if is_full_revolution {
        // Start cap (theta = 0, cos_t=1, sin_t=0):
        // Every profile point lies in the XY sketch plane. Triangulate the profile
        // polygon in its original position.
        let start_cap_center_idx = positions.len() / 3;
        // Add center point on the axis at average along position
        let avg_along: f64 = decomp.iter().map(|pp| pp.along).sum::<f64>() / decomp.len() as f64;
        let cx = ax + avg_along * ux;
        let cy = ay + avg_along * uy;
        positions.push(cx as f32);
        positions.push(cy as f32);
        positions.push(0.0_f32);
        // Cap normal points along -Z (into the start face)
        let cap_nx = -0.0_f64;
        let cap_ny = -0.0_f64;
        let cap_nz = -1.0_f64 * (if total_angle > 0.0 { 1.0 } else { -1.0 });
        normals.push(cap_nx as f32);
        normals.push(cap_ny as f32);
        normals.push(cap_nz as f32);

        for j in 0..n_profile {
            let a = (j) as u32;
            let b = ((j + 1) % n_profile) as u32;
            let c = start_cap_center_idx as u32;
            indices.push(c);
            indices.push(b);
            indices.push(a);
        }

        // End cap (theta = 2π) - identical position to start for full revolution
        let end_cap_center_idx = positions.len() / 3;
        positions.push(cx as f32);
        positions.push(cy as f32);
        positions.push(0.0_f32);
        // Cap normal points along +Z (out of the end face)
        normals.push(0.0_f32);
        normals.push(0.0_f32);
        normals.push(1.0_f32 * (if total_angle > 0.0 { 1.0 } else { -1.0 }));

        let last_ring_start = n_segments as usize * n_profile;
        for j in 0..n_profile {
            let a = (last_ring_start + j) as u32;
            let b = (last_ring_start + (j + 1) % n_profile) as u32;
            let c = end_cap_center_idx as u32;
            indices.push(c);
            indices.push(a);
            indices.push(b);
        }
    }

    Some(Mesh {
        positions,
        normals,
        indices,
    })
}

/// Extract a 2D profile (ordered point list) from sketch entities.
/// The profile follows the contour formed by connected lines,
/// or a standalone circle/arc tessellated into a polygon.
fn extract_profile(sketch: &Sketch) -> Option<Vec<(f64, f64)>> {
    // First, check for standalone circle — tessellate it as the profile
    if let Some(circle_points) = tessellate_circle_revolve_profile(sketch) {
        return Some(circle_points);
    }

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

    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut used = vec![false; lines.len()];

    // Start with the first line
    let first = lines[0];
    used[0] = true;
    if let (Some(p_start), Some(p_end)) =
        (sketch.get_point(first.0), sketch.get_point(first.1))
    {
        points.push((p_start.x, p_start.y));
        points.push((p_end.x, p_end.y));
    } else {
        return None;
    }

    // Follow the chain
    let mut current_id = first.1;
    loop {
        let mut found = false;
        for (i, &(start, end)) in lines.iter().enumerate() {
            if used[i] {
                continue;
            }
            if start == current_id {
                used[i] = true;
                if let Some(p) = sketch.get_point(end) {
                    points.push((p.x, p.y));
                    current_id = end;
                    found = true;
                }
            } else if end == current_id {
                used[i] = true;
                if let Some(p) = sketch.get_point(start) {
                    points.push((p.x, p.y));
                    current_id = start;
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

    // Close the profile by removing the last duplicate if it matches the first
    if points.len() >= 3 {
        let first_pt = points[0];
        let last_pt = points[points.len() - 1];
        let dx = first_pt.0 - last_pt.0;
        let dy = first_pt.1 - last_pt.1;
        if (dx * dx + dy * dy).sqrt() < 1e-6 {
            points.pop();
        }
    }

    Some(points)
}

/// Tessellate a standalone circle into profile points for revolve.
fn tessellate_circle_revolve_profile(sketch: &Sketch) -> Option<Vec<(f64, f64)>> {
    let mut circle: Option<(f64, f64, f64)> = None;

    for entity in sketch.entities.values() {
        match entity {
            SketchEntity::Line { construction, .. } | SketchEntity::Spline { construction, .. } => {
                if !*construction { return None; } // Has a real line — not a standalone circle
            }
            SketchEntity::Circle { center, radius, construction } => {
                if *construction { continue; }
                if circle.is_some() { return None; }
                if let Some(cp) = sketch.get_point(*center) {
                    circle = Some((cp.x, cp.y, *radius));
                }
            }
            _ => {}
        }
    }

    let (cx, cy, r) = circle?;
    if r <= 1e-10 { return None; }

    let n_seg = 64;
    let points: Vec<(f64, f64)> = (0..=n_seg)
        .map(|i| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n_seg as f64;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();

    Some(points)
}
