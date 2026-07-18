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
