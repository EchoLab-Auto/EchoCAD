//! EchoCAD rendering data generation.

pub mod mesh;
pub mod regenerate;

pub use mesh::RenderMesh;
pub use regenerate::{RegenResult, SolidGenerator, regenerate, regenerate_with};
