//! EchoCAD rendering data generation.

pub mod mesh;
pub mod regenerate;

pub use mesh::{RenderMesh, triangulate_solid};
pub use regenerate::{RegenResult, regenerate};
