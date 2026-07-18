//! EchoCAD geometry primitives and constraint solving.

pub mod boolean;
pub mod extrude;
pub mod fillet;
pub mod pattern;
pub mod revolve;
pub mod shell;
pub mod solid;
pub mod solver;
pub mod sweep;

pub use boolean::boolean_op;
pub use echi_core::sketch::{Constraint, EntityId, Sketch, SketchEntity, SketchPoint};
pub use extrude::{Mesh, extrude};
pub use fillet::{apply_chamfer, apply_fillet};
pub use pattern::{circular_pattern, linear_pattern, mirror_across_plane};
pub use revolve::revolve;
pub use shell::shell_mesh;
pub use solid::Solid;
pub use solver::solve;
pub use sweep::sweep_mesh;
