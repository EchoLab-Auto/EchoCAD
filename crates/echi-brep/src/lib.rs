//! EchoCAD boundary-representation kernel abstraction.
//!
//! This crate defines the `BrepKernel` trait and the B-rep data model
//! (`Solid`, `Face`, `Edge`, `Vertex`) that all kernel implementations
//! produce and consume.

pub mod kernel;
pub mod mock;
#[cfg(feature = "occt")]
pub mod occt;
pub mod types;

pub use kernel::{BrepError, BrepKernel, ProfilePoint};
pub use mock::MockBrepKernel;
#[cfg(feature = "occt")]
pub use occt::OcctBrepKernel;
pub use types::{Curve, Edge, Face, Solid, Surface, Vertex, Wire, WireEdge};
