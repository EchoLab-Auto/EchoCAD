//! EchoCAD plugin system: traits and registry for extensible CAD features.

mod types;
mod registry;

// Re-export the geometry Mesh (and the extrude helper) so plugins can
// implement `generate_solid` without declaring a direct dependency on
// `echi-geom`.
pub use echi_geom::Mesh;
pub use echi_geom::extrude::extrude;

pub use types::*;
pub use registry::PluginRegistry;
