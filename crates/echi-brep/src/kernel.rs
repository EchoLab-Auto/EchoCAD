use echi_geom::{
    Mesh, Sketch, boolean_op, circular_pattern, linear_pattern, mirror_across_plane as geom_mirror,
    shell_mesh, sweep_mesh,
};
use thiserror::Error;

use crate::types::Solid;

/// Errors produced by any BrepKernel implementation.
#[derive(Debug, Error)]
pub enum BrepError {
    #[error("kernel error: {0}")]
    Kernel(String),

    #[error("invalid geometry: {0}")]
    InvalidGeometry(String),

    #[error("operation not supported: {0}")]
    Unsupported(String),
}

/// A 2D point in the profile plane (for extrude/revolve inputs).
///
/// Mirrors `echi_geom::extrude::Point2D` but lives in echi-brep so callers do
/// not couple to the mesh geometry crate for profile data. The mock kernel
/// converts to/from this type internally.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfilePoint {
    pub x: f64,
    pub y: f64,
}

impl ProfilePoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// The B-rep kernel trait — the central abstraction for solid modeling.
///
/// Implementations produce `Solid` B-rep data (faces, edges, vertices) for
/// feature operations and can tessellate those solids back into triangle meshes
/// for rendering.
pub trait BrepKernel {
    /// Extrude a closed 2D profile along the Z axis by `height`.
    ///
    /// The profile is in the XY plane; positive height extrudes toward +Z.
    /// The profile must be a closed polygon with at least 3 points, ordered
    /// counter-clockwise for outward-facing normals.
    fn extrude(&self, profile: &[ProfilePoint], height: f64) -> Result<Solid, BrepError>;

    /// Extrude a profile to a mesh directly (bypassing B-rep Solid).
    ///
    /// The default implementation calls `extrude` + `tessellate`. Kernels
    /// with native mesh output (e.g., OCCT) can override this for better
    /// performance and precision.
    fn extrude_mesh(&self, profile: &[ProfilePoint], height: f64) -> Result<Mesh, BrepError> {
        let solid = self.extrude(profile, height)?;
        self.tessellate(&solid, 0.01)
    }

    /// Revolve a 2D profile around an axis in the XY plane.
    ///
    /// `axis_start` and `axis_end` define the axis of revolution in the
    /// sketch (XY) plane. The profile is rotated by `total_angle` radians
    /// with `segments` angular steps. For a 360° revolve, end caps are
    /// generated. The profile should be on one side of the axis.
    fn revolve(
        &self,
        profile: &[ProfilePoint],
        axis_start: (f64, f64),
        axis_end: (f64, f64),
        total_angle: f64,
        segments: u32,
    ) -> Result<Solid, BrepError>;

    /// Tessellate a solid into a triangle mesh using the given chordal
    /// tolerance (in model units).
    fn tessellate(&self, solid: &Solid, tolerance: f64) -> Result<Mesh, BrepError>;

    /// Sweep a profile sketch along a path sketch to produce a mesh.
    ///
    /// Both sketches are in the XY plane. Returns `None` if either sketch
    /// is empty or the path is degenerate. The default implementation
    /// delegates to `echi_geom::sweep_mesh`.
    fn sweep_mesh(
        &self,
        profile_sketch: &Sketch,
        path_sketch: &Sketch,
    ) -> Result<Mesh, BrepError> {
        sweep_mesh(profile_sketch, path_sketch)
            .ok_or_else(|| BrepError::Kernel("sweep produced no mesh".into()))
    }

    /// Hollow a solid by offsetting its surface inward by `thickness`.
    ///
    /// The default implementation delegates to `echi_geom::shell_mesh`.
    fn shell_mesh(&self, mesh: &Mesh, thickness: f64) -> Result<Mesh, BrepError> {
        Ok(shell_mesh(mesh, thickness))
    }

    /// Boolean union of two meshes.
    ///
    /// The default implementation delegates to `echi_geom::boolean_op`.
    fn boolean_union(&self, a: &Mesh, b: &Mesh) -> Result<Mesh, BrepError> {
        Ok(boolean_op(a, b, "union"))
    }

    /// Boolean subtraction (a - b) of two meshes.
    ///
    /// The default implementation delegates to `echi_geom::boolean_op`.
    fn boolean_subtract(&self, a: &Mesh, b: &Mesh) -> Result<Mesh, BrepError> {
        Ok(boolean_op(a, b, "subtract"))
    }

    /// Boolean intersection of two meshes.
    ///
    /// The default implementation delegates to `echi_geom::boolean_op`.
    fn boolean_intersect(&self, a: &Mesh, b: &Mesh) -> Result<Mesh, BrepError> {
        Ok(boolean_op(a, b, "intersect"))
    }

    /// Translate a solid by `(dx, dy, dz)`.
    fn translate(&self, solid: &Solid, dx: f64, dy: f64, dz: f64) -> Result<Solid, BrepError>;

    /// Rotate a solid around an axis defined by `axis_origin` and `axis_dir`
    /// by `angle_rad` radians (right-hand rule).
    fn rotate(
        &self,
        solid: &Solid,
        axis_origin: [f64; 3],
        axis_dir: [f64; 3],
        angle_rad: f64,
    ) -> Result<Solid, BrepError>;

    /// Mirror a solid across a plane defined by a point and normal.
    fn mirror_across_plane(
        &self,
        solid: &Solid,
        plane_normal: [f64; 3],
        plane_point: [f64; 3],
    ) -> Result<Solid, BrepError>;

    /// Create a linear pattern by translating `count` copies of the solid
    /// along direction `(dx, dy, dz)` with `spacing` between copies, then
    /// tessellating and combining all meshes.
    fn linear_pattern_mesh(
        &self,
        solid: &Solid,
        dx: f64,
        dy: f64,
        dz: f64,
        count: u32,
        spacing: f64,
    ) -> Result<Mesh, BrepError> {
        if count < 2 {
            return self.tessellate(solid, 0.01);
        }
        let mut mesh = self.tessellate(solid, 0.01)?;
        let step_x = dx * spacing;
        let step_y = dy * spacing;
        let step_z = dz * spacing;
        for i in 1..count {
            let translated = self.translate(solid, step_x * i as f64, step_y * i as f64, step_z * i as f64)?;
            let t_mesh = self.tessellate(&translated, 0.01)?;
            mesh = merge_meshes(mesh, t_mesh);
        }
        Ok(mesh)
    }

    /// Create a circular pattern by rotating `count` copies of the solid
    /// around the given axis, tessellating and combining all meshes.
    fn circular_pattern_mesh(
        &self,
        solid: &Solid,
        axis_ox: f64,
        axis_oy: f64,
        axis_oz: f64,
        axis_dx: f64,
        axis_dy: f64,
        axis_dz: f64,
        count: u32,
        total_angle_deg: f64,
    ) -> Result<Mesh, BrepError> {
        if count < 2 {
            return self.tessellate(solid, 0.01);
        }
        let angle_step = total_angle_deg.to_radians() / count as f64;
        let axis_origin = [axis_ox, axis_oy, axis_oz];
        let axis_dir = [axis_dx, axis_dy, axis_dz];
        let mut mesh = self.tessellate(solid, 0.01)?;
        for i in 1..count {
            let rotated = self.rotate(solid, axis_origin, axis_dir, angle_step * i as f64)?;
            let r_mesh = self.tessellate(&rotated, 0.01)?;
            mesh = merge_meshes(mesh, r_mesh);
        }
        Ok(mesh)
    }

    /// Mirror a solid across a plane and combine original + mirrored meshes.
    fn mirror_mesh(
        &self,
        solid: &Solid,
        nx: f64,
        ny: f64,
        nz: f64,
        px: f64,
        py: f64,
        pz: f64,
    ) -> Result<Mesh, BrepError> {
        let original = self.tessellate(solid, 0.01)?;
        let mirrored = self.mirror_across_plane(solid, [nx, ny, nz], [px, py, pz])?;
        let m_mesh = self.tessellate(&mirrored, 0.01)?;
        Ok(merge_meshes(original, m_mesh))
    }

    // ── Mesh-level pattern methods (delegate to echi_geom) ──────────
    //
    // These take a Mesh directly (not a Solid) because RegenResult stores
    // meshes. When OCCT arrives, they can be upgraded to operate on Solids
    // for proper B-rep pattern operations.

    /// Linear pattern on a mesh. Default delegates to echi_geom::linear_pattern.
    fn linear_pattern_mesh_for(
        &self,
        target: &Mesh,
        dir_x: f64,
        dir_y: f64,
        dir_z: f64,
        count: u32,
        spacing: f64,
    ) -> Result<Mesh, BrepError> {
        Ok(linear_pattern(target, dir_x, dir_y, dir_z, count, spacing))
    }

    /// Circular pattern on a mesh. Default delegates to echi_geom::circular_pattern.
    fn circular_pattern_mesh_for(
        &self,
        target: &Mesh,
        axis_x: f64,
        axis_y: f64,
        axis_z: f64,
        axis_dx: f64,
        axis_dy: f64,
        axis_dz: f64,
        count: u32,
        total_angle_deg: f64,
    ) -> Result<Mesh, BrepError> {
        Ok(circular_pattern(
            target, axis_x, axis_y, axis_z, axis_dx, axis_dy, axis_dz, count, total_angle_deg,
        ))
    }

    /// Mirror on a mesh. Default delegates to echi_geom::mirror_across_plane.
    fn mirror_mesh_for(
        &self,
        target: &Mesh,
        nx: f64,
        ny: f64,
        nz: f64,
        px: f64,
        py: f64,
        pz: f64,
    ) -> Result<Mesh, BrepError> {
        Ok(geom_mirror(target, nx, ny, nz, px, py, pz))
    }
}

/// Merge two meshes by concatenating positions, normals, and offset indices.
fn merge_meshes(mut a: Mesh, b: Mesh) -> Mesh {
    let offset = (a.positions.len() / 3) as u32;
    a.positions.extend_from_slice(&b.positions);
    a.normals.extend_from_slice(&b.normals);
    for &idx in &b.indices {
        a.indices.push(offset + idx);
    }
    a
}
