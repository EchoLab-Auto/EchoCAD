//! `wry_plugin` bridge: drives the renderer from the Tauri event loop and
//! routes native window input (the viewport has no DOM) into the camera.
//!
//! Same mechanism tauri-plugin-egui uses — we register a plugin through
//! `tauri::Builder::wry_plugin` and hook `on_event`, so no hand-assembled
//! winit loop is needed. The plugin lazily initializes the GPU device on the
//! first event loop iteration and renders on `MainEventsCleared`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
use tauri::Manager;
use tauri_runtime_wry::{
    Context, EventLoopIterationContext, Message, Plugin, PluginBuilder, WebContextStore,
};
use tao::event::{ElementState, Event, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopProxy, EventLoopWindowTarget};
use tao::keyboard::KeyCode;

use crate::manager::RenderManager;
use crate::renderer::{PanelLayout, RenderMode, Renderer};

/// Owned display handle for wgpu's `InstanceDescriptor::display` (raw
/// window handle 0.6 only provides borrowed handles, and wgpu 30 requires
/// the display connection at instance creation time).
struct OwnedDisplay(RawDisplayHandle);

impl HasDisplayHandle for OwnedDisplay {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        // SAFETY: `OwnedDisplay` keeps the raw handle alive for `&self`.
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(self.0) })
    }
}

// SAFETY: `RawDisplayHandle` is a bag of opaque platform pointers treated as
// immutable data by the backends (same reasoning as winit's OwnedDisplayHandle).
unsafe impl Send for OwnedDisplay {}
unsafe impl Sync for OwnedDisplay {}

impl std::fmt::Debug for OwnedDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("OwnedDisplay").finish()
    }
}

/// Builder registered via `tauri::Builder::wry_plugin`.
pub struct WgpuPluginBuilder {
    layout: PanelLayout,
    app_handle: tauri::AppHandle,
    manager: Arc<RenderManager>,
}

impl WgpuPluginBuilder {
    pub fn new(app_handle: tauri::AppHandle, manager: Arc<RenderManager>) -> Self {
        Self {
            layout: PanelLayout::default(),
            app_handle,
            manager,
        }
    }

    pub fn with_layout(mut self, layout: PanelLayout) -> Self {
        self.layout = layout;
        self
    }
}

impl PluginBuilder<tauri::EventLoopMessage> for WgpuPluginBuilder {
    type Plugin = WgpuPlugin;

    fn build(self, _context: Context<tauri::EventLoopMessage>) -> WgpuPlugin {
        WgpuPlugin {
            renderer: None,
            layout: self.layout,
            app_handle: self.app_handle,
            manager: self.manager,
            last_init_attempt: None,
            cursor: None,
            dragging: None,
            press_pos: None,
            shift_down: false,
        }
    }
}

/// Event-loop plugin that owns the renderer.
pub struct WgpuPlugin {
    renderer: Option<Renderer>,
    layout: PanelLayout,
    app_handle: tauri::AppHandle,
    manager: Arc<RenderManager>,
    /// Throttle init retries after a failure (don't spam the log every frame).
    last_init_attempt: Option<Instant>,
    /// Last cursor position in physical pixels (None = outside viewport).
    cursor: Option<(f64, f64)>,
    /// Active camera drag: (start_x, start_y) in physical pixels.
    dragging: Option<(f64, f64)>,
    /// Left-button press position, to distinguish click from drag.
    press_pos: Option<(f64, f64)>,
    shift_down: bool,
}

impl WgpuPlugin {
    /// Lazy init: the first event loop iteration happens after the window
    /// exists, so this is the safe point to enumerate adapters.
    fn ensure_renderer(&mut self) {
        if self.renderer.is_some() {
            return;
        }
        // Retry at most every second; failures are usually transient
        // (e.g. window still being created in the first iterations).
        if let Some(t) = self.last_init_attempt {
            if t.elapsed() < Duration::from_secs(1) {
                return;
            }
        }
        self.last_init_attempt = Some(Instant::now());
        let Some(window) = self.app_handle.get_window("main") else {
            return;
        };
        let size = window
            .inner_size()
            .unwrap_or(tao::dpi::PhysicalSize::new(800, 600));
        // wgpu 30 requires the display connection in InstanceDescriptor.
        // raw-window-handle 0.6 has no owned display handle, so we own the
        // raw handle ourselves (it's a plain Copy struct on every platform).
        let display = match window.display_handle() {
            Ok(h) => {
                let owned = OwnedDisplay(h.as_raw().clone());
                Box::new(owned) as Box<dyn wgpu::wgt::WgpuHasDisplayHandle>
            }
            Err(e) => {
                log::error!("echi-wgpu: cannot get display handle: {e}");
                return;
            }
        };
        match Renderer::new(window.clone(), display, (size.width, size.height)) {
            Ok(mut r) => {
                // Initial camera fit once meshes arrive; nothing yet.
                log::info!(
                    "echi-wgpu initialized at {}x{} (layout: {:.0}/{:.0}/{:.0})",
                    size.width,
                    size.height,
                    self.layout.left_width,
                    self.layout.right_width,
                    self.layout.top_height
                );
                if self.manager.scene_len() > 0 {
                    if let Some(b) = self.manager.scene_bounds() {
                        r.fit_view(b);
                    }
                }
                self.renderer = Some(r);
            }
            Err(e) => log::error!("echi-wgpu init failed: {e}"),
        }
    }

    /// Is the cursor inside the viewport (physical pixels)?
    fn cursor_in_viewport(&self, x: f64, y: f64, window_w: f64, window_h: f64, scale: f64) -> bool {
        let (vx, vy, vw, vh) = self.layout.viewport_rect_physical(window_w, window_h, scale);
        x >= vx && x <= vx + vw && y >= vy && y <= vy + vh
    }
}

impl Plugin<tauri::EventLoopMessage> for WgpuPlugin {
    fn on_event(
        &mut self,
        event: &Event<Message<tauri::EventLoopMessage>>,
        _event_loop: &EventLoopWindowTarget<Message<tauri::EventLoopMessage>>,
        _proxy: &EventLoopProxy<Message<tauri::EventLoopMessage>>,
        control_flow: &mut ControlFlow,
        _context: EventLoopIterationContext<'_, tauri::EventLoopMessage>,
        _web_context: &WebContextStore,
    ) -> bool {
        match event {
            // Keep the animation timer alive between iterations.
            Event::NewEvents(StartCause::Init)
            | Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                if self.renderer.is_some() {
                    *control_flow =
                        ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16));
                }
            }
            // Surface changed (resize, minimize/restore, DPI change).
            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
            }
            // ── Input routing (viewport has no DOM) ──────────────────────
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                // Orbit / pan while dragging; update cursor otherwise.
                if let Some((sx, sy)) = self.dragging {
                    if let Some(r) = &mut self.renderer {
                        let (dx, dy) = (position.x - sx, position.y - sy);
                        let viewport_h = r.size().1 as f64;
                        if self.shift_down {
                            r.camera.pan(dx as f32, dy as f32, viewport_h as f32);
                        } else {
                            r.camera.orbit(dx as f32, dy as f32, viewport_h as f32);
                        }
                    }
                    self.dragging = Some((position.x, position.y));
                }
                self.cursor = Some((position.x, position.y));
            }
            Event::WindowEvent {
                event: WindowEvent::MouseInput { state, button, .. },
                ..
            } => {
                if let Some(r) = &mut self.renderer {
                    let pressed = matches!(state, ElementState::Pressed);
                    let in_viewport = self.cursor.map_or(false, |(x, y)| {
                        let win = self
                            .app_handle
                            .get_window("main")
                            .and_then(|w| w.inner_size().ok())
                            .unwrap_or(tao::dpi::PhysicalSize::new(0, 0));
                        let scale = self
                            .app_handle
                            .get_window("main")
                            .and_then(|w| w.scale_factor().ok())
                            .unwrap_or(1.0);
                        self.cursor_in_viewport(x, y, win.width as f64, win.height as f64, scale)
                    });
                    if !in_viewport {
                        self.dragging = None;
                        return false;
                    }
                    match button {
                        MouseButton::Left | MouseButton::Middle => {
                            if pressed {
                                if let Some(c) = self.cursor {
                                    self.dragging = Some(c);
                                    self.press_pos = Some(c);
                                }
                            } else {
                                // Release: if the cursor barely moved, this
                                // was a click → GPU pick.
                                let is_click = match (self.press_pos, self.cursor) {
                                    (Some(p), Some(c)) => {
                                        (c.0 - p.0).hypot(c.1 - p.1) < 6.0
                                    }
                                    _ => false,
                                };
                                self.press_pos = None;
                                self.dragging = None;
                                if is_click && *button == MouseButton::Left {
                                    if let Some((x, y)) = self.cursor {
                                        if let Some(r) = &mut self.renderer {
                                            r.request_pick(x as u32, y as u32);
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::WindowEvent {
                event: WindowEvent::MouseWheel { delta, .. },
                ..
            } => {
                if let Some(r) = &mut self.renderer {
                    let y = match delta {
                        MouseScrollDelta::LineDelta(_, dy) => *dy * 40.0,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32,
                        _ => 0.0,
                    };
                    if y != 0.0 {
                        let factor = (-y * 0.008).exp();
                        r.camera.zoom(factor);
                    }
                }
            }
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        event: key_event, ..
                    },
                ..
            } => {
                if key_event.state == ElementState::Pressed {
                    if key_event.physical_key == KeyCode::ShiftLeft
                        || key_event.physical_key == KeyCode::ShiftRight
                    {
                        self.shift_down = true;
                    }
                    // F = fit view.
                    if key_event.physical_key == KeyCode::KeyF
                        || matches!(key_event.logical_key, tao::keyboard::Key::Character(c) if c == "f")
                    {
                        if let Some(b) = self.manager.scene_bounds() {
                            if let Some(r) = &mut self.renderer {
                                r.fit_view(b);
                            }
                        }
                    }
                    // W = wireframe toggle.
                    if key_event.physical_key == KeyCode::KeyW
                        || matches!(key_event.logical_key, tao::keyboard::Key::Character(c) if c == "w")
                    {
                        if let Some(r) = &mut self.renderer {
                            r.mode = match r.mode {
                                RenderMode::Solid => RenderMode::Wireframe,
                                RenderMode::Wireframe => RenderMode::Solid,
                            };
                        }
                    }
                    // V = toggle four-viewport mode.
                    if key_event.physical_key == KeyCode::KeyV
                        || matches!(key_event.logical_key, tao::keyboard::Key::Character(c) if c == "v")
                    {
                        if let Some(r) = &mut self.renderer {
                            r.view_mode = match r.view_mode {
                                crate::renderer::ViewMode::Perspective => {
                                    crate::renderer::ViewMode::Four
                                }
                                crate::renderer::ViewMode::Four => {
                                    crate::renderer::ViewMode::Perspective
                                }
                            };
                            log::info!("echi-wgpu: view mode = {:?}", r.view_mode);
                        }
                    }
                    // C = toggle section clip plane.
                    if key_event.physical_key == KeyCode::KeyC
                        || matches!(key_event.logical_key, tao::keyboard::Key::Character(c) if c == "c")
                    {
                        if let Some(r) = &mut self.renderer {
                            r.clip_enabled = !r.clip_enabled;
                            log::info!("echi-wgpu: clip = {}", r.clip_enabled);
                        }
                    }
                } else {
                    if key_event.physical_key == KeyCode::ShiftLeft
                        || key_event.physical_key == KeyCode::ShiftRight
                    {
                        self.shift_down = false;
                    }
                }
            }
            // All input processed — apply staged mesh updates and render.
            Event::MainEventsCleared => {
                self.ensure_renderer();
                if let Some(r) = &mut self.renderer {
                    let uploaded = self.manager.apply_pending(r);
                    if uploaded > 0 {
                        log::info!(
                            "echi-wgpu: uploaded {uploaded} mesh(es) (scene: {} on GPU)",
                            r.meshes.len()
                        );
                    }
                    // Resolve a pending GPU pick and notify the frontend.
                    if let Some(result) = r.poll_pick() {
                        r.set_highlight(result.feature_id);
                        use tauri::Emitter;
                        let _ = self.app_handle.emit(
                            "feature_picked",
                            serde_json::json!({ "id": result.feature_id }),
                        );
                        log::info!(
                            "echi-wgpu: picked {:?}",
                            result.feature_id
                        );
                    }
                    *control_flow =
                        ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16));
                    r.render();
                }
            }
            _ => {}
        }
        // Never consume: tauri must keep handling webview events.
        false
    }
}
