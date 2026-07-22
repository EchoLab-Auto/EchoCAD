//! Mass properties computed from triangle meshes.
//!
//! Uses the divergence theorem to compute volume and centroid from
//! triangle-mesh boundary representations. The mesh must be watertight
//! (closed, oriented consistently outward) for meaningful results.

use crate::extrude::Mesh;

/// Mass properties computed from a closed triangle mesh.
#[derive(Debug, Clone, Default)]
pub struct MassProperties {
    /// Absolute volume (mm³).
    pub volume: f64,
    /// Total surface area (mm²).
    pub surface_area: f64,
    /// Center of mass in world coordinates (mm).
    pub centroid: [f64; 3],
}

/// Compute mass properties (volume, surface area, centroid) from a
/// triangle mesh.
///
/// For each triangle (i0, i1, i2) we compute:
///
/// - **surface area** from half the magnitude of
///   `(v1 - v0) × (v2 - v0)`.
/// - **signed volume** via the divergence theorem: the tetrahedron
///   formed by the origin and the triangle contributes
///   `(v0 · ((v1 - v0) × (v2 - v0))) / 6`.  Summing over all faces
///   of a watertight solid yields the exact enclosed volume.
/// - **centroid** via tetrahedron-centroid weighting: each triangle
///   contributes `vol_contrib × (v0 + v1 + v2) / 4` to the
///   first-moment accumulator.  Dividing by the total volume after
///   the loop yields the centroid.
///
/// Returns `None` when the mesh has no indices or no positions.
///
/// # Correctness
///
/// The mesh must be **watertight** (every edge belongs to exactly two
/// faces traversed in opposite directions) and consistently oriented
/// outward.  For non-watertight meshes the volume and centroid are
/// meaningless; surface area is the naive sum of triangle areas.
pub fn compute_mass_properties(mesh: &Mesh) -> Option<MassProperties> {
    if mesh.indices.is_empty() || mesh.positions.is_empty() {
        return None;
    }

    let positions = &mesh.positions;
    let indices = &mesh.indices;

    let mut total_volume: f64 = 0.0;
    let mut total_surface_area: f64 = 0.0;
    let mut first_moment: [f64; 3] = [0.0; 3];

    for tri in indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        let i0 = tri[0] as usize * 3;
        let i1 = tri[1] as usize * 3;
        let i2 = tri[2] as usize * 3;

        let v0x = positions[i0] as f64;
        let v0y = positions[i0 + 1] as f64;
        let v0z = positions[i0 + 2] as f64;
        let v1x = positions[i1] as f64;
        let v1y = positions[i1 + 1] as f64;
        let v1z = positions[i1 + 2] as f64;
        let v2x = positions[i2] as f64;
        let v2y = positions[i2 + 1] as f64;
        let v2z = positions[i2 + 2] as f64;

        // Edge vectors
        let e1x = v1x - v0x;
        let e1y = v1y - v0y;
        let e1z = v1z - v0z;
        let e2x = v2x - v0x;
        let e2y = v2y - v0y;
        let e2z = v2z - v0z;

        // N = (v1 - v0) × (v2 - v0) — magnitude is 2 × triangle area
        let nx = e1y * e2z - e1z * e2y;
        let ny = e1z * e2x - e1x * e2z;
        let nz = e1x * e2y - e1y * e2x;

        // Surface area
        total_surface_area += 0.5 * (nx * nx + ny * ny + nz * nz).sqrt();

        // Signed volume of tetrahedron (origin, v0, v1, v2).
        // vol = det([v0, v1, v2]) / 6 = v0 · ((v1-v0) × (v2-v0)) / 6
        let vol_contrib = (v0x * nx + v0y * ny + v0z * nz) / 6.0;
        total_volume += vol_contrib;

        // First-moment contribution: volume-weighted tetrahedron centroid.
        // Centroid of tet (origin, v0, v1, v2) = (v0 + v1 + v2) / 4
        let cx = (v0x + v1x + v2x) / 4.0;
        let cy = (v0y + v1y + v2y) / 4.0;
        let cz = (v0z + v1z + v2z) / 4.0;
        first_moment[0] += cx * vol_contrib;
        first_moment[1] += cy * vol_contrib;
        first_moment[2] += cz * vol_contrib;
    }

    let volume = total_volume.abs();
    if volume < 1e-15 {
        return Some(MassProperties {
            volume: 0.0,
            surface_area: total_surface_area,
            centroid: [0.0; 3],
        });
    }

    let centroid = [
        first_moment[0] / total_volume,
        first_moment[1] / total_volume,
        first_moment[2] / total_volume,
    ];

    Some(MassProperties {
        volume,
        surface_area: total_surface_area,
        centroid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    /// Build an axis-aligned unit cube from two triangles per face, centered
    /// at the origin.  Extent: [-0.5, 0.5] in each axis.  All face normals
    /// point outward.
    fn unit_cube_mesh() -> Mesh {
        let mut m = Mesh {
            positions: Vec::new(),
            normals: Vec::new(),
            indices: Vec::new(),
        };

        let mut v = |x: f64, y: f64, z: f64| -> u32 {
            m.positions.extend([x as f32, y as f32, z as f32]);
            m.normals.extend([0.0f32; 3]); // placeholder; not consumed by mass props
            (m.vertex_count() - 1) as u32
        };

        let ppp = v(0.5, 0.5, 0.5);
        let mpp = v(-0.5, 0.5, 0.5);
        let mmp = v(-0.5, -0.5, 0.5);
        let pmp = v(0.5, -0.5, 0.5);
        let ppm = v(0.5, 0.5, -0.5);
        let mpm = v(-0.5, 0.5, -0.5);
        let mmm = v(-0.5, -0.5, -0.5);
        let pmm = v(0.5, -0.5, -0.5);

        // +Z (front) — outward normal (0,0,1)
        m.indices.extend(&[ppp, mpp, mmp, ppp, mmp, pmp]);
        // -Z (back) — outward normal (0,0,-1)
        m.indices.extend(&[mpm, ppm, pmm, mpm, pmm, mmm]);
        // +Y (top) — outward normal (0,1,0)
        m.indices.extend(&[ppp, ppm, mpm, ppp, mpm, mpp]);
        // -Y (bottom) — outward normal (0,-1,0)
        m.indices.extend(&[mmp, mmm, pmm, mmp, pmm, pmp]);
        // +X (right) — outward normal (1,0,0)
        m.indices.extend(&[ppp, pmp, pmm, ppp, pmm, ppm]);
        // -X (left) — outward normal (-1,0,0)
        m.indices.extend(&[mpp, mpm, mmm, mpp, mmm, mmp]);

        m
    }

    #[test]
    fn cube_volume() {
        let mesh = unit_cube_mesh();
        let props = compute_mass_properties(&mesh).unwrap();
        assert_relative_eq!(props.volume, 1.0, epsilon = 1e-6);
    }

    #[test]
    fn cube_surface_area() {
        let mesh = unit_cube_mesh();
        let props = compute_mass_properties(&mesh).unwrap();
        assert_relative_eq!(props.surface_area, 6.0, epsilon = 1e-6);
    }

    #[test]
    fn centroid_at_origin() {
        let mesh = unit_cube_mesh();
        let props = compute_mass_properties(&mesh).unwrap();
        assert_relative_eq!(props.centroid[0], 0.0, epsilon = 1e-6);
        assert_relative_eq!(props.centroid[1], 0.0, epsilon = 1e-6);
        assert_relative_eq!(props.centroid[2], 0.0, epsilon = 1e-6);
    }

    #[test]
    fn empty_mesh_returns_none() {
        let mesh = Mesh::default();
        assert!(compute_mass_properties(&mesh).is_none());
    }
}
