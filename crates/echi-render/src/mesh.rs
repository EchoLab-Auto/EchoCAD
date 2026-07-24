use echi_geom::extrude::Mesh;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderMesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
}

impl From<&Mesh> for RenderMesh {
    fn from(mesh: &Mesh) -> Self {
        Self {
            positions: mesh.positions.clone(),
            normals: mesh.normals.clone(),
            indices: mesh.indices.clone(),
        }
    }
}

/// Copy mesh vertices (with Z offset) into destination, with index offset.
#[cfg(feature = "brep")]
pub fn copy_mesh(src: &Mesh, dst: &mut Mesh, z_offset: f64) {
    let offset = dst.vertex_count() as u32;
    for i in (0..src.positions.len()).step_by(3) {
        dst.positions.push(src.positions[i]);
        dst.positions.push(src.positions[i + 1]);
        dst.positions.push(src.positions[i + 2] + z_offset as f32);
    }
    dst.normals.extend_from_slice(&src.normals);
    for &idx in &src.indices {
        dst.indices.push(offset + idx);
    }
}

/// Transform a mesh from local XY sketch space to world coordinates.
#[cfg(feature = "brep")]
pub fn transform_mesh_to_world(mesh: &mut Mesh, plane: &echi_core::feature::PlaneDefinition) {
    let (origin, u, v, normal) = plane.frame();
    let ox = origin[0] as f32;
    let oy = origin[1] as f32;
    let oz = origin[2] as f32;
    let ux = u[0] as f32;
    let uy = u[1] as f32;
    let uz = u[2] as f32;
    let vx = v[0] as f32;
    let vy = v[1] as f32;
    let vz = v[2] as f32;
    let nx = normal[0] as f32;
    let ny = normal[1] as f32;
    let nz = normal[2] as f32;

    for i in (0..mesh.positions.len()).step_by(3) {
        let x = mesh.positions[i];
        let y = mesh.positions[i + 1];
        let z = mesh.positions[i + 2];
        mesh.positions[i]     = ox + x * ux + y * vx + z * nx;
        mesh.positions[i + 1] = oy + x * uy + y * vy + z * ny;
        mesh.positions[i + 2] = oz + x * uz + y * vz + z * nz;
    }
    for i in (0..mesh.normals.len()).step_by(3) {
        let x = mesh.normals[i];
        let y = mesh.normals[i + 1];
        let z = mesh.normals[i + 2];
        mesh.normals[i]     = x * ux + y * vx + z * nx;
        mesh.normals[i + 1] = x * uy + y * vy + z * ny;
        mesh.normals[i + 2] = x * uz + y * vz + z * nz;
    }
}
