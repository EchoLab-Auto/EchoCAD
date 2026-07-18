//! Shell: offset a mesh inward by a thickness to create a hollow shell.
//! Uses angle-weighted vertex normals for robust offsetting on both
//! convex and concave geometry.

use crate::extrude::Mesh;

/// Offset a mesh inward by `thickness` to create a hollow shell.
///
/// Computes angle-weighted vertex normals (weighted by the face angle
/// at each vertex) to produce a more accurate offset direction than
/// simple averaging. Caps open edges by connecting outer and inner
/// boundary vertices.
pub fn shell_mesh(mesh: &Mesh, thickness: f64) -> Mesh {
    if thickness <= 0.0 {
        return mesh.clone();
    }
    let t = thickness as f32;

    let vcount = mesh.vertex_count();

    // Compute angle-weighted vertex normals
    let mut vertex_normals: Vec<(f32, f32, f32)> = vec![(0.0, 0.0, 0.0); vcount];

    for tri in mesh.indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        let a = tri[0] as usize;
        let b = tri[1] as usize;
        let c = tri[2] as usize;

        let a3 = a * 3;
        let b3 = b * 3;
        let c3 = c * 3;

        // Edge vectors
        let ab_x = mesh.positions[b3] - mesh.positions[a3];
        let ab_y = mesh.positions[b3 + 1] - mesh.positions[a3 + 1];
        let ab_z = mesh.positions[b3 + 2] - mesh.positions[a3 + 2];

        let ac_x = mesh.positions[c3] - mesh.positions[a3];
        let ac_y = mesh.positions[c3 + 1] - mesh.positions[a3 + 1];
        let ac_z = mesh.positions[c3 + 2] - mesh.positions[a3 + 2];

        // Face normal via cross product
        let fnx = ab_y * ac_z - ab_z * ac_y;
        let fny = ab_z * ac_x - ab_x * ac_z;
        let fnz = ab_x * ac_y - ab_y * ac_x;
        let flen = (fnx * fnx + fny * fny + fnz * fnz).sqrt();
        if flen < 1e-10 {
            continue;
        }
        let fnx = fnx / flen;
        let fny = fny / flen;
        let fnz = fnz / flen;

        // Compute angles at each vertex (weighted by angle)
        let weights = [
            angle_at_vertex(
                (mesh.positions[a3], mesh.positions[a3 + 1], mesh.positions[a3 + 2]),
                (mesh.positions[b3], mesh.positions[b3 + 1], mesh.positions[b3 + 2]),
                (mesh.positions[c3], mesh.positions[c3 + 1], mesh.positions[c3 + 2]),
            ),
            angle_at_vertex(
                (mesh.positions[b3], mesh.positions[b3 + 1], mesh.positions[b3 + 2]),
                (mesh.positions[c3], mesh.positions[c3 + 1], mesh.positions[c3 + 2]),
                (mesh.positions[a3], mesh.positions[a3 + 1], mesh.positions[a3 + 2]),
            ),
            angle_at_vertex(
                (mesh.positions[c3], mesh.positions[c3 + 1], mesh.positions[c3 + 2]),
                (mesh.positions[a3], mesh.positions[a3 + 1], mesh.positions[a3 + 2]),
                (mesh.positions[b3], mesh.positions[b3 + 1], mesh.positions[b3 + 2]),
            ),
        ];

        for (idx, &vi) in [a, b, c].iter().enumerate() {
            let w = weights[idx];
            vertex_normals[vi].0 += fnx * w;
            vertex_normals[vi].1 += fny * w;
            vertex_normals[vi].2 += fnz * w;
        }
    }

    // Normalize vertex normals
    for i in 0..vcount {
        let (nx, ny, nz) = vertex_normals[i];
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        if len > 1e-10 {
            vertex_normals[i] = (nx / len, ny / len, nz / len);
        } else {
            // Fall back to stored normal
            vertex_normals[i] = (
                mesh.normals[i * 3],
                mesh.normals[i * 3 + 1],
                mesh.normals[i * 3 + 2],
            );
        }
    }

    // Detect open edges (edges used by only one triangle)
    let mut edge_map: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();
    let mut boundary_verts: std::collections::HashSet<u32> = std::collections::HashSet::new();

    for tri in mesh.indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        let edges = [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])];
        for &(v0, v1) in &edges {
            let key = if v0 < v1 { (v0, v1) } else { (v1, v0) };
            *edge_map.entry(key).or_insert(0) += 1;
        }
    }

    for ((v0, v1), count) in &edge_map {
        if *count == 1 {
            boundary_verts.insert(*v0);
            boundary_verts.insert(*v1);
        }
    }

    // Build result: outer shell + inner shell + side walls for boundary edges
    let mut result = Mesh::default();
    let _outer_start = 0u32;
    let inner_start = vcount as u32;

    // Copy outer vertices (original positions and normals)
    result.positions.extend_from_slice(&mesh.positions);
    result.normals.extend_from_slice(&mesh.normals);

    // Create inner vertices (offset inward along angle-weighted normal)
    for i in 0..vcount {
        let vx = mesh.positions[i * 3] - vertex_normals[i].0 * t;
        let vy = mesh.positions[i * 3 + 1] - vertex_normals[i].1 * t;
        let vz = mesh.positions[i * 3 + 2] - vertex_normals[i].2 * t;
        result.positions.push(vx);
        result.positions.push(vy);
        result.positions.push(vz);
        // Invert normals for inner shell
        result.normals.push(-vertex_normals[i].0);
        result.normals.push(-vertex_normals[i].1);
        result.normals.push(-vertex_normals[i].2);
    }

    // Copy outer triangles
    result.indices.extend_from_slice(&mesh.indices);

    // Add inner triangles (reversed winding)
    for tri in mesh.indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        result.indices.push(inner_start + tri[0]);
        result.indices.push(inner_start + tri[2]);
        result.indices.push(inner_start + tri[1]);
    }

    // Add side walls for boundary edges (connect outer boundary to inner boundary)
    for ((v0, v1), count) in &edge_map {
        if *count != 1 {
            continue;
        }
        let vo0 = *v0;
        let vo1 = *v1;
        let vi0 = inner_start + vo0;
        let vi1 = inner_start + vo1;

        // Two triangles forming a quad: outer[v0,v1] - inner[v0,v1]
        result.indices.push(vo0);
        result.indices.push(vo1);
        result.indices.push(vi0);

        result.indices.push(vo1);
        result.indices.push(vi1);
        result.indices.push(vi0);
    }

    let _ = boundary_verts;
    result
}

/// Compute the angle at vertex `v` of triangle (v, a, b).
/// Returns the angle in radians as a weight factor.
fn angle_at_vertex(
    v: (f32, f32, f32),
    a: (f32, f32, f32),
    b: (f32, f32, f32),
) -> f32 {
    let e1 = (a.0 - v.0, a.1 - v.1, a.2 - v.2);
    let e2 = (b.0 - v.0, b.1 - v.1, b.2 - v.2);
    let len1 = (e1.0 * e1.0 + e1.1 * e1.1 + e1.2 * e1.2).sqrt();
    let len2 = (e2.0 * e2.0 + e2.1 * e2.1 + e2.2 * e2.2).sqrt();
    if len1 < 1e-10 || len2 < 1e-10 {
        return 0.0;
    }
    let dot = (e1.0 * e2.0 + e1.1 * e2.1 + e1.2 * e2.2) / (len1 * len2);
    dot.clamp(-1.0, 1.0).acos()
}
