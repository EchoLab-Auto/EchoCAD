//! EchoCAD plugin system: traits and registry for extensible CAD features.

mod types;
mod registry;

pub use types::*;
pub use registry::PluginRegistry;
