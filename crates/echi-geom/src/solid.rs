use crate::extrude::Mesh;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Solid {
    pub mesh: Mesh,
}
