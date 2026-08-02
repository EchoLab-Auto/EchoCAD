//! P0 wgpu viewport window layout.
//!
//! Architecture (doc/wgpu-rendering-migration-plan.md, mode 5):
//! - The wgpu surface covers the whole native window.
//! - The single main webview hosts the whole Vue app (CadLayout: toolbar +
//!   left feature tree + right properties + status bar). The center region
//!   between the panels is where the 3D viewport lives.
//! - The renderer carves the viewport rect out of the window using
//!   `LAYOUT` below; those numbers MUST match the CSS custom properties in
//!   `app/src/style.css` (--toolbar-height / --sidebar-*-width).
//! - Panel bounds are recomputed on every window resize (physical pixels).

use std::time::Duration;

use tauri::{App, AppHandle, Manager};

use echi_wgpu::{PanelLayout, WgpuPluginBuilder};

use crate::commands::AppState;

/// Viewport margins in logical pixels — MUST match the CSS variables in
/// `app/src/style.css`. `top_height` is the full two-row toolbar height.
const LAYOUT: PanelLayout = PanelLayout {
    left_width: 260.0,
    right_width: 300.0,
    top_height: 76.0,
};

pub fn setup_panels(app: &mut App) -> tauri::Result<()> {
    // The renderer bridge lives in AppState; give it the handle so regen
    // hooks can emit `viewport_updated` events.
    let state = app.state::<AppState>();
    *state.app_handle.lock().unwrap() = Some(app.handle().clone());
    let manager = state
        .render_manager
        .clone()
        .ok_or_else(|| tauri::Error::AssetNotFound("render manager".into()))?;

    // Renderer plugin: drives wgpu from the event loop (same hook egui uses).
    app.wry_plugin(
        WgpuPluginBuilder::new(app.handle().clone(), manager).with_layout(LAYOUT),
    );

    // ECHO_DEMO=1: build a demo box through the real command pipeline so the
    // renderer has a scene to show (validates regen → sync → GPU end to end).
    if std::env::var("ECHO_DEMO").is_ok() {
        spawn_demo(app.handle().clone());
    }

    Ok(())
}

/// Dev-only demo scene: rectangle sketch + extrude via the real commands,
/// exercising the full regen → RenderManager → GPU path.
fn spawn_demo(handle: AppHandle) {
    log::info!("ECHO_DEMO: spawning demo scene builder");
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        log::info!("ECHO_DEMO: building");
        use crate::commands as c;
        let st = || handle.state::<AppState>();
        let p1 = c::add_point(0.0, 0.0, st());
        let p2 = c::add_point(10.0, 0.0, st());
        let p3 = c::add_point(10.0, 8.0, st());
        let p4 = c::add_point(0.0, 8.0, st());
        log::info!("ECHO_DEMO: points = {p1:?} {p2:?} {p3:?} {p4:?}");
        if let (Some(a), Some(b), Some(cc), Some(d)) = (p1, p2, p3, p4) {
            c::add_line(a, b, st());
            c::add_line(b, cc, st());
            c::add_line(cc, d, st());
            c::add_line(d, a, st());
        }
        log::info!("ECHO_DEMO: lines done");
        // Copy the id out first: a MutexGuard held across the if-let block
        // would deadlock when add_extrude_feature → snapshot locks again.
        let sketch_id = *st().active_sketch.lock().unwrap();
        if let Some(sketch_id) = sketch_id {
            let r = c::add_extrude_feature(
                sketch_id,
                "up".into(),
                0.0,
                0.0,
                5.0,
                None,
                st(),
            );
            log::info!("ECHO_DEMO: extrude = {r:?}");
        }
        log::info!("ECHO_DEMO: demo scene created");
    });
}
