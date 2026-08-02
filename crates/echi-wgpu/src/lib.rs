//! EchoCAD native wgpu renderer.
//!
//! Architecture (see doc/wgpu-rendering-migration-plan.md):
//! - The wgpu surface covers the whole native window.
//! - Vue UI panels live in child webviews (`Window::add_child`), placed over
//!   non-overlapping opaque rectangles of the window. The viewport area has no
//!   DOM at all: geometry lives in Rust/GPU, input goes through native window
//!   events, and the JS side only receives small signals (revision numbers,
//!   cursor rays, selection state).
//! - The render loop is driven through the `wry_plugin` mechanism (the same
//!   hook tauri-plugin-egui uses), so we never hand-assemble a winit loop.

pub mod manager;
pub mod plugin;
pub mod renderer;
pub mod scene;

pub use manager::RenderManager;
pub use plugin::{WgpuPlugin, WgpuPluginBuilder};
pub use renderer::{PanelLayout, RenderMode, Renderer, RendererError};
pub use scene::{OrbitCamera, SceneMesh};
