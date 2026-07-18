//! Mesh boolean operations: union, subtract, intersect.
//! Simplified implementation — merges meshes for union, inverts-and-merges for subtract.

use crate::extrude::Mesh;

/// Apply a boolean operation to two meshes.
pub fn boolean_op(a: &Mesh, b: &Mesh, op: &str) -> Mesh {
    match op {
        "union" => union_mesh(a, b),
        "subtract" => subtract_mesh(a, b),
        "intersect" => intersect_mesh(a, b),
        _ => a.clone(),
    }
}

/// Union: combine both meshes into one.
fn union_mesh(a: &Mesh, b: &Mesh) -> Mesh {
    let mut result = Mesh::default();
    copy_all(a, &mut result);
    let offset = (result.positions.len() / 3) as u32;
    copy_all_with_offset(b, &mut result, offset);
    result
}

/// Subtract: invert B's normals and combine (visual-only, not true CSG).
fn subtract_mesh(a: &Mesh, b: &Mesh) -> Mesh {
    // Simplified: just show A (typically the tool body would be subtracted).
    // A true CSG subtraction requires complex mesh boolean algorithms.
    // For now, return A as-is (the user can see both bodies and this is
    // documented as a simplified approximation).
    let mut result = Mesh::default();
    copy_all(a, &mut result);
    // Add B with inverted normals so it appears "cut away" in rendering
    let offset = (result.positions.len() / 3) as u32;
    copy_all_inverted(b, &mut result, offset);
    result
}

/// Intersect: keep only the overlapping region (placeholder).
fn intersect_mesh(a: &Mesh, _b: &Mesh) -> Mesh {
    // Full mesh-mesh intersection requires BSP or octree traversal.
    // Simplified: return A (documented limitation).
    a.clone()
}

fn copy_all(src: &Mesh, dst: &mut Mesh) {
    dst.positions.extend_from_slice(&src.positions);
    dst.normals.extend_from_slice(&src.normals);
    dst.indices.extend_from_slice(&src.indices);
}

fn copy_all_with_offset(src: &Mesh, dst: &mut Mesh, offset: u32) {
    for i in (0..src.positions.len()).step_by(3) {
        dst.positions.push(src.positions[i]);
        dst.positions.push(src.positions[i + 1]);
        dst.positions.push(src.positions[i + 2]);
    }
    dst.normals.extend_from_slice(&src.normals);
    for &idx in &src.indices {
        dst.indices.push(offset + idx);
    }
}

fn copy_all_inverted(src: &Mesh, dst: &mut Mesh, offset: u32) {
    for i in (0..src.positions.len()).step_by(3) {
        dst.positions.push(src.positions[i]);
        dst.positions.push(src.positions[i + 1]);
        dst.positions.push(src.positions[i + 2]);
    }
    for i in (0..src.normals.len()).step_by(3) {
        dst.normals.push(-src.normals[i]);
        dst.normals.push(-src.normals[i + 1]);
        dst.normals.push(-src.normals[i + 2]);
    }
    for tri in src.indices.chunks(3) {
        dst.indices.push(offset + tri[0]);
        dst.indices.push(offset + tri[2]);
        dst.indices.push(offset + tri[1]);
    }
}
