//! EchoCAD plugin system: traits and registry for extensible CAD features.
//!
//! ## Plugin API
//!
//! Plugins implement the [`Plugin`] trait and are registered with a
//! [`PluginRegistry`]. They can generate sketches and solid meshes,
//! contribute UI tools, and use the built-in geometry operations.
//!
//! ## Geometry Operations
//!
//! The following operations are re-exported so plugins can build complex
//! solids without declaring a direct dependency on `echi-geom`:
//!
//! | Function | Description |
//! |----------|-------------|
//! | [`extrude`] | Extrude a 2D sketch into a 3D solid |
//! | [`revolve`] | Revolve a sketch around an axis |
//! | [`sweep`] | Sweep a profile along a 3D path |
//! | [`union_meshes`] | CSG union of two meshes |
//! | [`subtract_meshes`] | CSG subtract (A - B) |
//! | [`intersect_meshes`] | CSG intersection |
//! | [`shell_mesh`] | Hollow a solid to a given wall thickness |
//! | [`apply_fillet`] | Fillet all sharp edges |
//! | [`apply_fillet_edges`] | Fillet specific edges |
//! | [`apply_chamfer`] | Chamfer all sharp edges |
//! | [`apply_chamfer_edges`] | Chamfer specific edges |
//! | [`linear_pattern`] | Linear array of a mesh |
//! | [`circular_pattern`] | Circular array of a mesh |
//! | [`mirror_mesh`] | Mirror a mesh across a plane |
//! | [`compute_mass_props`] | Compute volume, surface area, centroid |
//! | [`extract_loops`] | Extract closed polygon loops from a sketch |

mod types;
mod registry;

// Re-export geometry types and operations so plugins stay decoupled from echi-geom.
pub use echi_geom::Mesh;
pub use echi_geom::extrude::{extrude, extract_loops};
pub use echi_geom::revolve::revolve;
pub use echi_geom::sweep::sweep_mesh;
pub use echi_geom::shell::shell_mesh;
pub use echi_geom::fillet::{apply_fillet, apply_fillet_edges, apply_chamfer, apply_chamfer_edges};
pub use echi_geom::pattern::{linear_pattern, circular_pattern};
pub use echi_geom::mirror_across_plane as mirror_mesh;
pub use echi_geom::compute_mass_properties as compute_mass_props;
pub use echi_geom::MassProperties;

/// CSG union of two meshes (A ∪ B).
pub fn union_meshes(a: &Mesh, b: &Mesh) -> Mesh {
    echi_geom::boolean::boolean_op(a, b, "union")
}

/// CSG subtract (A - B).
pub fn subtract_meshes(a: &Mesh, b: &Mesh) -> Mesh {
    echi_geom::boolean::boolean_op(a, b, "subtract")
}

/// CSG intersect (A ∩ B).
pub fn intersect_meshes(a: &Mesh, b: &Mesh) -> Mesh {
    echi_geom::boolean::boolean_op(a, b, "intersect")
}

pub use types::*;
pub use registry::PluginRegistry;
