//! EchoCAD rendering data generation.

pub mod mesh;
#[cfg(feature = "occt")]
pub mod occt_ops;
pub mod regenerate;

pub use mesh::RenderMesh;
#[cfg(feature = "brep")]
pub use mesh::{copy_mesh, transform_mesh_to_world};
pub use regenerate::{RegenResult, SolidGenerator, regenerate, regenerate_with};
#[cfg(feature = "occt")]
pub use occt_ops::build_occt_solid_for_mass;

// Re-export B-rep types for downstream consumers
pub use echi_brep::{BrepKernel, MockBrepKernel, ProfilePoint};
#[cfg(feature = "occt")]
pub use echi_brep::OcctBrepKernel;
