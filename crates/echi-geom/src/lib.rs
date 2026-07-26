//! EchoCAD geometry primitives and constraint solving.

pub mod boolean;
pub mod extrude;
pub mod fillet;
pub mod mass_props;
pub mod param_curve;
pub mod pattern;
pub mod revolve;
pub mod shell;
pub mod sketch_geom;
pub mod solver;
pub mod sweep;

pub use boolean::boolean_op;
pub use echi_core::sketch::{Constraint, EntityId, Sketch, SketchEntity, SketchPoint};
pub use extrude::{Mesh, extrude};
pub use fillet::{apply_chamfer, apply_chamfer_edges, apply_fillet, apply_fillet_edges};
pub use mass_props::{MassProperties, compute_mass_properties};
pub use pattern::{circular_pattern, linear_pattern, mirror_across_plane};
pub use revolve::revolve;
pub use shell::shell_mesh;
pub use solver::{check_overconstrained, solve};
pub use sweep::sweep_mesh;
