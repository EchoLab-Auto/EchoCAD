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

pub fn triangulate_solid(_mesh: &Mesh) -> RenderMesh {
    RenderMesh {
        positions: Vec::new(),
        normals: Vec::new(),
        indices: Vec::new(),
    }
}
