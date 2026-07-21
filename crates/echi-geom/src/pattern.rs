//! Linear pattern, circular pattern, and mirror feature geometry.

use crate::extrude::Mesh;

/// Repeat a mesh N times along a direction vector.
pub fn linear_pattern(mesh: &Mesh, dir_x: f64, dir_y: f64, dir_z: f64, count: u32, spacing: f64) -> Mesh {
    let len = (dir_x * dir_x + dir_y * dir_y + dir_z * dir_z).sqrt();
    if len < 1e-10 || count < 2 {
        return mesh.clone();
    }
    let ux = dir_x / len;
    let uy = dir_y / len;
    let uz = dir_z / len;

    let mut result = Mesh::default();
    let base_verts = mesh.vertex_count();
    let base_offset = 0u32;

    // Copy original
    copy_mesh_into(mesh, &mut result);

    for i in 1..count {
        let d = i as f64 * spacing;
        let tx = (ux * d) as f32;
        let ty = (uy * d) as f32;
        let tz = (uz * d) as f32;

        let off = (result.positions.len() / 3) as u32;
        for vi in 0..base_verts {
            let idx = vi * 3;
            result.positions.push(mesh.positions[idx] + tx);
            result.positions.push(mesh.positions[idx + 1] + ty);
            result.positions.push(mesh.positions[idx + 2] + tz);
            result.normals.push(mesh.normals[idx]);
            result.normals.push(mesh.normals[idx + 1]);
            result.normals.push(mesh.normals[idx + 2]);
        }
        for &idx in &mesh.indices {
            result.indices.push(off + idx);
        }
    }
    let _ = base_offset;
    result
}

/// Repeat a mesh N times around an axis.
pub fn circular_pattern(
    mesh: &Mesh,
    axis_ox: f64, axis_oy: f64, axis_oz: f64,
    axis_dx: f64, axis_dy: f64, axis_dz: f64,
    count: u32, total_angle_deg: f64,
) -> Mesh {
    let alen = (axis_dx * axis_dx + axis_dy * axis_dy + axis_dz * axis_dz).sqrt();
    if alen < 1e-10 || count < 2 {
        return mesh.clone();
    }
    let aux = axis_dx / alen;
    let auy = axis_dy / alen;
    let auz = axis_dz / alen;
    let angle_step = total_angle_deg.to_radians() / (count - 1).max(1) as f64;

    let mut result = Mesh::default();
    let base_verts = mesh.vertex_count();
    copy_mesh_into(mesh, &mut result);

    for i in 1..count {
        let angle = i as f64 * angle_step;
        let (cos_a, sin_a) = (angle.cos(), angle.sin());
        let off = (result.positions.len() / 3) as u32;

        for vi in 0..base_verts {
            let idx = vi * 3;
            let px = mesh.positions[idx] as f64 - axis_ox;
            let py = mesh.positions[idx + 1] as f64 - axis_oy;
            let pz = mesh.positions[idx + 2] as f64 - axis_oz;

            // Rodrigues' rotation formula
            let dot = px * aux + py * auy + pz * auz;
            let cross_x = auy * pz - auz * py;
            let cross_y = auz * px - aux * pz;
            let cross_z = aux * py - auy * px;

            let rx = px * cos_a + cross_x * sin_a + dot * (1.0 - cos_a) * aux;
            let ry = py * cos_a + cross_y * sin_a + dot * (1.0 - cos_a) * auy;
            let rz = pz * cos_a + cross_z * sin_a + dot * (1.0 - cos_a) * auz;

            result.positions.push((axis_ox + rx) as f32);
            result.positions.push((axis_oy + ry) as f32);
            result.positions.push((axis_oz + rz) as f32);

            // Rotate normal
            let nx = mesh.normals[idx] as f64;
            let ny = mesh.normals[idx + 1] as f64;
            let nz = mesh.normals[idx + 2] as f64;
            let ndot = nx * aux + ny * auy + nz * auz;
            let ncross_x = auy * nz - auz * ny;
            let ncross_y = auz * nx - aux * nz;
            let ncross_z = aux * ny - auy * nx;
            result.normals.push((nx * cos_a + ncross_x * sin_a + ndot * (1.0 - cos_a) * aux) as f32);
            result.normals.push((ny * cos_a + ncross_y * sin_a + ndot * (1.0 - cos_a) * auy) as f32);
            result.normals.push((nz * cos_a + ncross_z * sin_a + ndot * (1.0 - cos_a) * auz) as f32);
        }
        for &idx in &mesh.indices {
            result.indices.push(off + idx);
        }
    }
    result
}

/// Mirror a mesh across a plane defined by a point and normal.
pub fn mirror_across_plane(
    mesh: &Mesh,
    nx: f64, ny: f64, nz: f64,
    px: f64, py: f64, pz: f64,
) -> Mesh {
    let nlen = (nx * nx + ny * ny + nz * nz).sqrt();
    if nlen < 1e-10 {
        return mesh.clone();
    }
    let ux = nx / nlen;
    let uy = ny / nlen;
    let uz = nz / nlen;

    let mut result = Mesh::default();
    copy_mesh_into(mesh, &mut result);

    let base_verts = mesh.vertex_count();
    let off = (result.positions.len() / 3) as u32;

    for vi in 0..base_verts {
        let idx = vi * 3;
        let vx = mesh.positions[idx] as f64;
        let vy = mesh.positions[idx + 1] as f64;
        let vz = mesh.positions[idx + 2] as f64;

        // Signed distance from point to plane
        let dist = (vx - px) * ux + (vy - py) * uy + (vz - pz) * uz;
        let mx = vx - 2.0 * dist * ux;
        let my = vy - 2.0 * dist * uy;
        let mz = vz - 2.0 * dist * uz;

        result.positions.push(mx as f32);
        result.positions.push(my as f32);
        result.positions.push(mz as f32);

        // Reflect normal
        let ndist = mesh.normals[idx] as f64 * ux + mesh.normals[idx + 1] as f64 * uy + mesh.normals[idx + 2] as f64 * uz;
        result.normals.push((mesh.normals[idx] as f64 - 2.0 * ndist * ux) as f32);
        result.normals.push((mesh.normals[idx + 1] as f64 - 2.0 * ndist * uy) as f32);
        result.normals.push((mesh.normals[idx + 2] as f64 - 2.0 * ndist * uz) as f32);
    }
    for &idx in &mesh.indices {
        result.indices.push(off + idx);
    }
    result
}

fn copy_mesh_into(src: &Mesh, dst: &mut Mesh) {
    dst.positions.extend_from_slice(&src.positions);
    dst.normals.extend_from_slice(&src.normals);
    dst.indices.extend_from_slice(&src.indices);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extrude::{Mesh, extrude};
    use echi_core::feature::{ExtrudeDirection, PlaneDefinition};
    use echi_core::sketch::Sketch;

    /// Build a small 1x1x1 cube mesh by extruding a square sketch.
    fn cube_mesh() -> Mesh {
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(0.0, 0.0);
        let p1 = sketch.add_point(1.0, 0.0);
        let p2 = sketch.add_point(1.0, 1.0);
        let p3 = sketch.add_point(0.0, 1.0);
        sketch.add_line(p0, p1);
        sketch.add_line(p1, p2);
        sketch.add_line(p2, p3);
        sketch.add_line(p3, p0);
        extrude(
            &sketch,
            1.0,
            ExtrudeDirection::OneSide,
            0.0,
            &PlaneDefinition::XY,
        )
        .expect("cube extrude should succeed")
    }

    fn assert_no_nan(m: &Mesh) {
        assert!(
            !m.positions.iter().any(|v| v.is_nan()),
            "NaN in positions"
        );
        assert!(!m.normals.iter().any(|v| v.is_nan()), "NaN in normals");
    }

    fn assert_indices_in_bounds(m: &Mesh) {
        let n = m.vertex_count() as u32;
        for &i in &m.indices {
            assert!(i < n, "index {} out of bounds ({})", i, n);
        }
    }

    #[test]
    fn linear_pattern_basic() {
        let cube = cube_mesh();
        let input_verts = cube.vertex_count();
        let result = linear_pattern(&cube, 1.0, 0.0, 0.0, 3, 2.0);
        // 3 instances (original + 2 copies)
        assert!(
            result.vertex_count() >= 3 * input_verts,
            "linear pattern should have at least 3x input vertices, got {} vs {}",
            result.vertex_count(),
            3 * input_verts
        );
        assert_no_nan(&result);
        assert_indices_in_bounds(&result);
    }

    #[test]
    fn circular_pattern_basic() {
        let cube = cube_mesh();
        let input_verts = cube.vertex_count();
        let result = circular_pattern(&cube, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 4, 360.0);
        // 4 instances
        assert!(
            result.vertex_count() >= 4 * input_verts,
            "circular pattern should have at least 4x input vertices, got {} vs {}",
            result.vertex_count(),
            4 * input_verts
        );
        assert_no_nan(&result);
        assert_indices_in_bounds(&result);

        // First instance (original) should have its vertices at original positions.
        // First vertex of the input cube should be at (0,0,0) as the first point added.
        assert!(
            (result.positions[0] - 0.0).abs() < 1e-4,
            "first instance vertex x should be 0.0"
        );
        assert!(
            (result.positions[1] - 0.0).abs() < 1e-4,
            "first instance vertex y should be 0.0"
        );
    }

    #[test]
    fn mirror_across_plane_basic() {
        let cube = cube_mesh();
        let input_verts = cube.vertex_count();
        // Mirror across YZ plane (normal = (1,0,0), through origin).
        let result = mirror_across_plane(&cube, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        // Should have original + mirrored copy
        assert!(
            result.vertex_count() >= 2 * input_verts,
            "mirror should have at least 2x input vertices, got {} vs {}",
            result.vertex_count(),
            2 * input_verts
        );
        assert_no_nan(&result);
        assert_indices_in_bounds(&result);

        // The mirrored half should have positive-x versions of original positive-x vertices,
        // and negative-x versions of original negative-x vertices.
        // Since the cube is [0,1]^3, all its x >= 0. The mirrored copy should have x <= 0.
        let mut found_negative_x = false;
        for vi in input_verts..result.vertex_count() {
            let x = result.positions[vi * 3];
            if x < -0.01 {
                found_negative_x = true;
                break;
            }
        }
        assert!(found_negative_x, "mirrored half should have negative-x vertices");
    }

    #[test]
    fn linear_pattern_count_zero_returns_identity() {
        let cube = cube_mesh();
        let result = linear_pattern(&cube, 1.0, 0.0, 0.0, 0, 2.0);
        assert_eq!(result.vertex_count(), cube.vertex_count());
        assert_no_nan(&result);
    }

    #[test]
    fn linear_pattern_count_one_returns_identity() {
        let cube = cube_mesh();
        let result = linear_pattern(&cube, 1.0, 0.0, 0.0, 1, 2.0);
        assert_eq!(result.vertex_count(), cube.vertex_count());
        assert_no_nan(&result);
    }

    #[test]
    fn linear_pattern_zero_spacing_all_overlap() {
        let cube = cube_mesh();
        let input_verts = cube.vertex_count();
        let result = linear_pattern(&cube, 1.0, 0.0, 0.0, 3, 0.0);
        // Still produces 3 instances worth of vertices, all at same location.
        assert_eq!(result.vertex_count(), 3 * input_verts);
        assert_no_nan(&result);
        assert_indices_in_bounds(&result);
    }

    #[test]
    fn circular_pattern_degenerate_axis_returns_identity() {
        let cube = cube_mesh();
        // Zero-length axis direction → falls back to clone.
        let result = circular_pattern(&cube, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 4, 360.0);
        assert_eq!(result.vertex_count(), cube.vertex_count());
        assert_no_nan(&result);
    }

    #[test]
    fn circular_pattern_count_one_returns_identity() {
        let cube = cube_mesh();
        let result = circular_pattern(&cube, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1, 360.0);
        assert_eq!(result.vertex_count(), cube.vertex_count());
        assert_no_nan(&result);
    }

    #[test]
    fn mirror_degenerate_normal_returns_identity() {
        let cube = cube_mesh();
        let result = mirror_across_plane(&cube, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(result.vertex_count(), cube.vertex_count());
        assert_no_nan(&result);
    }

    #[test]
    fn mirror_xy_plane_flips_z() {
        let cube = cube_mesh();
        let input_verts = cube.vertex_count();
        // Mirror across XY plane (normal (0,0,1), through origin).
        // Cube is [0,1]^3 so all z >= 0. Mirrored half should have z <= 0.
        let result = mirror_across_plane(&cube, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0);
        assert!(
            result.vertex_count() >= 2 * input_verts,
            "mirror across XY should double vertices"
        );
        let mut found_negative_z = false;
        for vi in input_verts..result.vertex_count() {
            let z = result.positions[vi * 3 + 2];
            if z < -0.01 {
                found_negative_z = true;
                break;
            }
        }
        assert!(found_negative_z, "mirrored half should have negative-z vertices");
        assert_no_nan(&result);
        assert_indices_in_bounds(&result);
    }
}
