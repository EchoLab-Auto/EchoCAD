//! EchoCAD application state and Tauri commands.

use echi_core::{
    BoolOp, CustomFeatureData, Document, ExtrudeDirection, Feature, FeatureId, FeatureKind,
    ParameterId, PlaneDefinition, Sketch,
    sketch::{Constraint, EntityId, SketchEntity, SketchPoint},
};
use echi_plugin::PluginRegistry;
use echi_render::{RegenResult, RenderMesh, SolidGenerator, regenerate_with, BrepKernel};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_UNDO: usize = 50;

pub(crate) struct UndoManager {
    undo_stack: Vec<(Vec<u8>, Option<FeatureId>)>,
    redo_stack: Vec<(Vec<u8>, Option<FeatureId>)>,
}

impl UndoManager {
    fn new() -> Self {
        Self { undo_stack: Vec::new(), redo_stack: Vec::new() }
    }
    fn push_snapshot(&mut self, doc: &Document, active: Option<FeatureId>) {
        if let Ok(data) = serde_json::to_vec(doc) {
            self.undo_stack.push((data, active));
            self.redo_stack.clear();
            if self.undo_stack.len() > MAX_UNDO {
                self.undo_stack.remove(0);
            }
        }
    }
    fn can_undo(&self) -> bool { !self.undo_stack.is_empty() }
    fn can_redo(&self) -> bool { !self.redo_stack.is_empty() }
    /// Drop all undo/redo history. Used when the document is REPLACED
    /// wholesale (project load) — snapshots of the previous document would
    /// restore the wrong state.
    fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
    fn undo(
        &mut self,
        current_doc: &Document,
        current_active: Option<FeatureId>,
    ) -> Option<(Document, Option<FeatureId>)> {
        let (data, active) = self.undo_stack.pop()?;
        if let Ok(data) = serde_json::to_vec(current_doc) {
            self.redo_stack.push((data, current_active));
        }
        let doc: Document = serde_json::from_slice(&data).ok()?;
        Some((doc, active))
    }
    fn redo(
        &mut self,
        current_doc: &Document,
        current_active: Option<FeatureId>,
    ) -> Option<(Document, Option<FeatureId>)> {
        let (data, active) = self.redo_stack.pop()?;
        if let Ok(data) = serde_json::to_vec(current_doc) {
            self.undo_stack.push((data, current_active));
        }
        let doc: Document = serde_json::from_slice(&data).ok()?;
        Some((doc, active))
    }
}

pub struct AppState {
    pub document: Mutex<Document>,
    pub active_sketch: Mutex<Option<FeatureId>>,
    pub regen_result: Mutex<RegenResult>,
    pub plugin_registry: PluginRegistry,
    pub(crate) undo_manager: Mutex<UndoManager>,
    pub autosave_enabled: AtomicBool,
    pub has_recovery_file: AtomicBool,
    pub use_brep: AtomicBool,
    /// Set whenever sketch data changes without an immediate regen.
    /// Mesh-reading commands (`get_all_solid_meshes`, `get_solid_mesh`, …)
    /// check this flag and regenerate lazily, so sketch edits propagate to
    /// dependent solids exactly once — never per mousemove during drags.
    pub regen_dirty: AtomicBool,
    /// Native wgpu renderer bridge (P1). `None` in legacy mode. Mesh data is
    /// staged here after every regen — it never crosses IPC to the frontend.
    pub render_manager: Option<std::sync::Arc<echi_wgpu::RenderManager>>,
    /// App handle for emitting `viewport_updated` events after regen (wgpu mode).
    pub(crate) app_handle: std::sync::Mutex<Option<tauri::AppHandle>>,
    /// Meshes imported from external formats (STEP). They are not backed by
    /// document features, so the regen pipeline would drop them — every
    /// regen helper re-injects them into the fresh result afterwards.
    imported_meshes: Mutex<HashMap<FeatureId, echi_geom::Mesh>>,
}

impl AppState {
    pub fn new() -> Self {
        Self::new_with_render_manager(None)
    }

    /// Create the state with the native renderer bridge.
    pub fn new_with_render_manager(
        render_manager: Option<std::sync::Arc<echi_wgpu::RenderManager>>,
    ) -> Self {
        let mut state = Self::new_inner();
        state.render_manager = render_manager;
        state
    }

    fn new_inner() -> Self {
        let mut doc = Document::new("Part1");
        let sketch_id = doc.new_feature_id();
        doc.add_feature(Feature::new(
            sketch_id,
            "Sketch1",
            FeatureKind::Sketch { sketch: Sketch::new(), plane: PlaneDefinition::default() },
        ));
        let mut registry = PluginRegistry::new();
        if let Err(e) = registry.register(echi_plugin_gear::create_plugin()) {
            log::error!("Failed to register gear plugin: {}", e);
        }
        if let Err(e) = registry.register(echi_plugin_cycloidal::create_plugin()) {
            log::error!("Failed to register cycloidal plugin: {}", e);
        }
        Self {
            document: Mutex::new(doc),
            active_sketch: Mutex::new(Some(sketch_id)),
            regen_result: Mutex::new(RegenResult::default()),
            plugin_registry: registry,
            undo_manager: Mutex::new(UndoManager::new()),
            autosave_enabled: AtomicBool::new(true),
            has_recovery_file: AtomicBool::new(false),
            use_brep: AtomicBool::new(true),
            regen_dirty: AtomicBool::new(false),
            imported_meshes: Mutex::new(HashMap::new()),
            render_manager: None,
            app_handle: std::sync::Mutex::new(None),
        }
    }

    fn snapshot(&self) {
        let doc = self.lock_doc();
        let active = *self.lock_sketch();
        self.lock_undo().push_snapshot(&doc, active);
    }

    /// Snapshot only when an active sketch exists. Sketch-entity commands
    /// are no-ops without one; pushing an undo entry anyway would pollute
    /// the stack with dead steps (principle #3).
    fn snapshot_if_sketch(&self) {
        if self.lock_sketch().is_some() {
            self.snapshot();
        }
    }

    /// Like [`snapshot`] but returns a token (the undo stack length *before*
    /// the push) so the caller can safely discard ONLY this snapshot via
    /// [`discard_snapshot`]. Used in paths that may need to roll back the
    /// snapshot (e.g. `delete_feature` when cascade is false and dependents
    /// exist), preventing U1-style corruption where a concurrent snapshot
    /// push between our `snapshot()` and `pop()` would cause us to pop the
    /// *wrong* snapshot.
    fn snapshot_with_token(&self) -> usize {
        let doc = self.lock_doc();
        let active = *self.lock_sketch();
        let mut mgr = self.lock_undo();
        let len = mgr.undo_stack.len();
        mgr.push_snapshot(&doc, active);
        len
    }

    /// Discard the last undo snapshot, but ONLY if the stack length is
    /// exactly `expected_before_len + 1` — i.e. exactly one snapshot was
    /// pushed since the [`snapshot_with_token`] call. If the length differs,
    /// another snapshot was pushed concurrently and we must NOT pop (that
    /// would corrupt the undo stack).
    fn discard_snapshot(&self, expected_before_len: usize) -> bool {
        let mut mgr = self.lock_undo();
        if mgr.undo_stack.len() == expected_before_len + 1 {
            mgr.undo_stack.pop();
            true
        } else {
            false
        }
    }

    fn with_active_sketch_mut<T>(&self, f: impl FnOnce(&mut Sketch) -> T) -> Option<T> {
        let mut doc = self.lock_doc();
        let active = *self.lock_sketch();
        let id = active?;
        let feature = doc.get_feature_mut(id)?;
        match &mut feature.kind {
            FeatureKind::Sketch { sketch, .. } | FeatureKind::CustomSketch { sketch, .. } | FeatureKind::SketchModule { sketch, .. } => {
                let result = f(sketch);
                // Sketch data changed — dependent solids are now stale.
                // Regeneration happens lazily on the next mesh read
                // (regen_if_dirty), so point drags don't re-tessellate
                // the whole model per mousemove.
                self.regen_dirty.store(true, Ordering::SeqCst);
                Some(result)
            }
            _ => None,
        }
    }

    /// Regenerate if any sketch mutation marked the result dirty since the
    /// last read. Cheap no-op otherwise. Called at the top of every command
    /// that serves mesh data, so readers always see fresh geometry without
    /// paying for regen on write-only paths.
    fn regen_if_dirty(&self) {
        if self.regen_dirty.swap(false, Ordering::SeqCst) {
            let doc = self.lock_doc();
            regen_locked(&doc, self);
        }
    }

    // ── Lock helpers with expect (design principle F5) ────────────
    //
    // Every Mutex lock uses .expect() with a descriptive message.
    // If a prior command panicked while holding a lock, the mutex is
    // poisoned. Without expect(), the subsequent unwrap() panics with
    // a generic "poisoned" message that is undebuggable.  These helpers
    // turn that into a clear signal so the user/developer can see
    // WHICH lock was poisoned.

    pub(crate) fn lock_doc(&self) -> std::sync::MutexGuard<'_, Document> {
        self.document.lock().expect("document mutex poisoned — a prior command panicked while holding the document lock")
    }
    fn lock_sketch(&self) -> std::sync::MutexGuard<'_, Option<FeatureId>> {
        self.active_sketch.lock().expect("active_sketch mutex poisoned — a prior command panicked while holding the active_sketch lock")
    }
    fn lock_undo(&self) -> std::sync::MutexGuard<'_, UndoManager> {
        self.undo_manager.lock().expect("undo_manager mutex poisoned — a prior command panicked while holding the undo_manager lock")
    }
    fn lock_regen(&self) -> std::sync::MutexGuard<'_, RegenResult> {
        self.regen_result.lock().expect("regen_result mutex poisoned — a prior command panicked while holding the regen_result lock")
    }
}

// ── Render DTOs ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RenderEntity {
    Point { id: EntityId, x: f64, y: f64 },
    Line {
        id: EntityId,
        x1: f64, y1: f64, x2: f64, y2: f64,
        #[serde(default)] construction: bool,
    },
    Circle {
        id: EntityId, cx: f64, cy: f64, radius: f64,
        #[serde(default)] construction: bool,
    },
    Arc {
        id: EntityId, cx: f64, cy: f64, radius: f64,
        start_angle: f64, end_angle: f64,
    },
    Spline { id: EntityId, points: Vec<(f64, f64)> },
    Ellipse { id: EntityId, cx: f64, cy: f64, major_rx: f64, major_ry: f64, ratio: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureNode {
    pub id: FeatureId,
    pub name: String,
    pub feature_type: String,
    pub parameters: Vec<ParameterInfo>,
    pub suppressed: bool,
    pub has_dependents: bool,
    pub errors: Option<String>,
    pub plane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plane_base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plane_distance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<FeatureId>,
    /// Parent feature ID for hierarchical tree display.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<FeatureId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInfo {
    pub id: ParameterId,
    pub name: String,
    pub value: f64,
    #[serde(default)] pub readonly: bool,
}

// ── Regeneration helpers ─────────────────────────────────────────

/// Bridge between the plugin registry and the regen pipeline's
/// [`SolidGenerator`] trait. `SolidGenerator` lives in `echi-render` and
/// `PluginRegistry` lives in `echi-plugin` — neither is local to this crate,
/// so implementing one for the other directly violates the orphan rule.
/// A local newtype wrapper keeps the impl on the right side of that rule
/// while still keeping `echi-render` free of a direct `echi-plugin` dep.
struct PluginSolidGen<'a>(&'a PluginRegistry);

impl SolidGenerator for PluginSolidGen<'_> {
    fn generate_solid(
        &self,
        plugin_id: &str,
        generator_id: &str,
        params: &HashMap<String, f64>,
    ) -> Result<echi_geom::Mesh, String> {
        PluginRegistry::generate_solid(self.0, plugin_id, generator_id, params)
            .map_err(|e| e.to_string())
    }
}

fn brep_kernel_for_state(state: &AppState) -> Option<&'static dyn BrepKernel> {
    #[cfg(feature = "brep")]
    {
        if !state.use_brep.load(Ordering::Relaxed) {
            return None;
        }
        #[cfg(feature = "occt")]
        {
            use echi_render::OcctBrepKernel;
            return Some(&OcctBrepKernel);
        }
        #[cfg(not(feature = "occt"))]
        {
            use echi_render::MockBrepKernel;
            return Some(&MockBrepKernel);
        }
    }
    #[cfg(not(feature = "brep"))]
    {
        let _ = state;
        None
    }
}

/// Re-inject STEP-imported meshes into a fresh regen result. Imported solids
/// aren't backed by document features, so the pipeline can't produce them;
/// without this they would vanish on the next regen.
fn reinject_imported(result: &mut RegenResult, state: &AppState) {
    let imported = state.imported_meshes.lock()
        .expect("imported_meshes mutex poisoned");
    for (fid, mesh) in imported.iter() {
        result.solids.insert(*fid, mesh.clone());
    }
}

/// Stage the active sketch's linework (polyline-ized) into the renderer.
/// Called after every sketch mutation command.
fn sync_sketch(state: &AppState) {
    let Some(manager) = &state.render_manager else { return };
    let doc = state.lock_doc();
    let active = *state.lock_sketch();
    let positions = sketch_linework(&doc, active);
    manager.sync_sketch(positions);
}

/// Convert the active sketch's entities into world-space line segments
/// (LineList positions). Points are skipped; curves are polyline-ized.
fn sketch_linework(doc: &Document, active_sketch: Option<FeatureId>) -> Vec<f32> {
    use echi_core::sketch::{SketchEntity};
    use std::f64::consts::TAU;

    let mut out: Vec<f32> = Vec::new();
    let Some(sid) = active_sketch else { return out };
    let Some(feature) = doc.get_feature(sid) else { return out };
    let (sketch, plane) = match &feature.kind {
        echi_core::feature::FeatureKind::Sketch { sketch, plane }
        | echi_core::feature::FeatureKind::CustomSketch { sketch, plane, .. } => (sketch, plane),
        _ => return out,
    };
    let (origin, u, v, _n) = plane.frame();
    let to_world = |p: (f64, f64)| -> [f32; 3] {
        [
            (origin[0] + p.0 * u[0] + p.1 * v[0]) as f32,
            (origin[1] + p.0 * u[1] + p.1 * v[1]) as f32,
            (origin[2] + p.0 * u[2] + p.1 * v[2]) as f32,
        ]
    };
    let pos = |id: echi_core::sketch::EntityId| -> Option<(f64, f64)> {
        match sketch.entities.get(&id) {
            Some(SketchEntity::Point(p)) => Some((p.x, p.y)),
            _ => None,
        }
    };

    fn seg(out: &mut Vec<f32>, to_world: &dyn Fn((f64, f64)) -> [f32; 3], a: (f64, f64), b: (f64, f64)) {
        out.extend_from_slice(&to_world(a));
        out.extend_from_slice(&to_world(b));
    }
    fn polyline(out: &mut Vec<f32>, to_world: &dyn Fn((f64, f64)) -> [f32; 3], pts: &[(f64, f64)]) {
        for w in pts.windows(2) {
            seg(out, to_world, w[0], w[1]);
        }
    }
    fn arc_points(out: &mut Vec<f32>, to_world: &dyn Fn((f64, f64)) -> [f32; 3], center: (f64, f64), radius: f64, a0: f64, a1: f64, n: usize) {
        let mut pts = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let t = a0 + (a1 - a0) * (i as f64 / n as f64);
            pts.push((center.0 + radius * t.cos(), center.1 + radius * t.sin()));
        }
        polyline(out, to_world, &pts);
    }
    let to_world_ref: &dyn Fn((f64, f64)) -> [f32; 3] = &to_world;

    for (_, ent) in &sketch.entities {
        match ent {
            SketchEntity::Line { start, end, .. } => {
                if let (Some(a), Some(b)) = (pos(*start), pos(*end)) {
                    seg(&mut out, to_world_ref, a, b);
                }
            }
            SketchEntity::Circle { center, radius, .. } => {
                if let Some(c) = pos(*center) {
                    arc_points(&mut out, to_world_ref, c, *radius, 0.0, TAU, 64);
                }
            }
            SketchEntity::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                ..
            } => {
                if let Some(c) = pos(*center) {
                    arc_points(&mut out, to_world_ref, c, *radius, *start_angle, *end_angle, 64);
                }
            }
            SketchEntity::Spline {
                control_points, ..
            } => {
                let pts: Vec<(f64, f64)> = control_points
                    .iter()
                    .filter_map(|id| pos(*id))
                    .collect();
                if pts.len() >= 2 {
                    polyline(&mut out, to_world_ref, &pts);
                }
            }
            SketchEntity::Ellipse {
                center,
                major_axis_end,
                ratio,
                ..
            } => {
                if let (Some(c), Some(m)) = (pos(*center), pos(*major_axis_end)) {
                    let (dx, dy) = (m.0 - c.0, m.1 - c.1);
                    let major = dx.hypot(dy).max(1e-9);
                    let ang = dy.atan2(dx);
                    let minor = major * *ratio;
                    let mut pts = Vec::with_capacity(65);
                    for i in 0..=64 {
                        let t = TAU * (i as f64 / 64.0);
                        pts.push((
                            c.0 + major * t.cos() * ang.cos() - minor * t.sin() * ang.sin(),
                            c.1 + major * t.cos() * ang.sin() + minor * t.sin() * ang.cos(),
                        ));
                    }
                    polyline(&mut out, to_world_ref, &pts);
                }
            }
            SketchEntity::Point(_) => {}
        }
    }
    out
}

/// Stage the fresh regen result into the native renderer (wgpu mode) and
/// notify the frontend with the new revision. Mesh bytes never cross IPC.
fn sync_renderer(state: &AppState, full_regen: bool, changed_ids: &[FeatureId]) {
    let Some(manager) = &state.render_manager else { return };
    log::debug!("sync_renderer: full={full_regen} solids={}", state.lock_regen().solids.len());
    if full_regen {
        let solids = state.lock_regen().solids.clone();
        let mut scene = std::collections::HashMap::new();
        for (id, mesh) in solids {
            scene.insert(
                id.0 as u32,
                echi_wgpu::SceneMesh {
                    positions: mesh.positions.clone(),
                    normals: mesh.normals.clone(),
                    indices: mesh.indices.clone(),
                    label: String::new(),
                    suppressed: false,
                    error: None,
                },
            );
        }
        manager.sync_from_regen(&scene, true);
    } else {
        manager.mark_dirty(changed_ids.iter().map(|id| id.0 as u32));
        let solids = state.lock_regen().solids.clone();
        let mut scene = std::collections::HashMap::new();
        for (id, mesh) in solids {
            scene.insert(
                id.0 as u32,
                echi_wgpu::SceneMesh {
                    positions: mesh.positions.clone(),
                    normals: mesh.normals.clone(),
                    indices: mesh.indices.clone(),
                    label: String::new(),
                    suppressed: false,
                    error: None,
                },
            );
        }
        manager.sync_from_regen(&scene, false);
    }
    // Notify the frontend (panel UIs refresh their state from the revision).
    if let Some(handle) = state.app_handle.lock().unwrap().as_ref() {
        use tauri::Emitter;
        let _ = handle.emit(
            "viewport_updated",
            serde_json::json!({ "rev": manager.revision() }),
        );
    }
}

fn regenerate_state(state: &AppState) {
    let doc = state.lock_doc();
    let solid_gen: &dyn SolidGenerator = &PluginSolidGen(&state.plugin_registry);
    let brep_kernel = brep_kernel_for_state(state);
    let mut result = regenerate_with(&doc, Some(solid_gen), brep_kernel, None);
    reinject_imported(&mut result, state);
    *state.lock_regen() = result;
    // We just regenerated everything — any pending dirty flag is satisfied.
    state.regen_dirty.store(false, Ordering::SeqCst);
    sync_renderer(state, true, &[]);
}

/// Full regeneration from an already-locked document reference.
fn regen_locked(doc: &Document, state: &AppState) {
    let solid_gen: &dyn SolidGenerator = &PluginSolidGen(&state.plugin_registry);
    let brep_kernel = brep_kernel_for_state(state);
    let mut result = regenerate_with(doc, Some(solid_gen), brep_kernel, None);
    reinject_imported(&mut result, state);
    *state.lock_regen() = result;
    state.regen_dirty.store(false, Ordering::SeqCst);
    sync_renderer(state, true, &[]);
}

/// Incremental regeneration: only re-evaluate the changed feature and its
/// transitive dependents. Falls back to full regen if the previous result
/// is missing or fails.
fn incremental_regen_locked(doc: &Document, state: &AppState, changed_id: FeatureId) {
    let solid_gen: &dyn SolidGenerator = &PluginSolidGen(&state.plugin_registry);
    let brep_kernel = brep_kernel_for_state(state);
    let mut prev = state.lock_regen().clone();
    prev.dirty = RegenResult::compute_dirty(doc, changed_id);
    let mut result = regenerate_with(doc, Some(solid_gen), brep_kernel, Some(&prev));
    reinject_imported(&mut result, state);
    *state.lock_regen() = result;
    // NOTE: regen_dirty is intentionally NOT cleared here — an incremental
    // regen only covers one feature's dirty set, while the flag may also
    // reflect sketch mutations whose dependents weren't re-evaluated. The
    // next mesh read (regen_if_dirty) will do the full regen if needed.
    sync_renderer(state, false, &[changed_id]);
}

// ── Feature node projection ─────────────────────────────────────

fn parameter_info(doc: &Document, id: ParameterId) -> Option<ParameterInfo> {
    doc.get_parameter(id).map(|p| ParameterInfo {
        id: p.id,
        name: p.name.clone(),
        value: p.value,
        readonly: false,
    })
}

fn feature_to_node(f: &Feature, doc: &Document, errors: &HashMap<FeatureId, String>) -> FeatureNode {
    let (ft, params) = match &f.kind {
        FeatureKind::Sketch { .. } => ("Sketch".to_string(), Vec::new()),
        FeatureKind::SketchModule { source, .. } => {
            if let Some(custom) = source {
                let params: Vec<ParameterInfo> = custom.params.iter()
                    .map(|(k, v)| ParameterInfo {
                        id: ParameterId(0),
                        name: k.clone(),
                        value: *v,
                        readonly: true,
                    })
                    .collect();
                (format!("Module:{}:{}", custom.plugin_id, custom.generator_id), params)
            } else {
                ("SketchModule".to_string(), Vec::new())
            }
        }
        FeatureKind::CustomSketch { custom, .. } => {
            let params: Vec<ParameterInfo> = custom.params.iter()
                .map(|(k, v)| ParameterInfo {
                    id: ParameterId(0),
                    name: k.clone(),
                    value: *v,
                    readonly: true,
                })
                .collect();
            (format!("Custom:{}:{}", custom.plugin_id, custom.generator_id), params)
        }
        FeatureKind::Extrude { distance, .. } => {
            ("Extrude".to_string(), parameter_info(doc, *distance).into_iter().collect())
        }
        FeatureKind::Revolve { angle, .. } => {
            ("Revolve".to_string(), parameter_info(doc, *angle).into_iter().collect())
        }
        FeatureKind::Fillet { radius, .. } => {
            ("Fillet".to_string(), parameter_info(doc, *radius).into_iter().collect())
        }
        FeatureKind::Chamfer { distance, .. } => {
            ("Chamfer".to_string(), parameter_info(doc, *distance).into_iter().collect())
        }
        FeatureKind::Shell { thickness, .. } => {
            ("Shell".to_string(), parameter_info(doc, *thickness).into_iter().collect())
        }
        FeatureKind::LinearPattern { dir_x, dir_y, dir_z, count, spacing, .. } => {
            let mut params = Vec::new();
            params.push(ParameterInfo { id: ParameterId(0), name: "count".into(), value: *count as f64, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "spacing".into(), value: *spacing, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "dir_x".into(), value: *dir_x, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "dir_y".into(), value: *dir_y, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "dir_z".into(), value: *dir_z, readonly: true });
            ("LinearPattern".to_string(), params)
        }
        FeatureKind::CircularPattern { count, total_angle_deg, axis_x, axis_y, axis_z, axis_dx, axis_dy, axis_dz, .. } => {
            let mut params = Vec::new();
            params.push(ParameterInfo { id: ParameterId(0), name: "count".into(), value: *count as f64, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "total_angle_deg".into(), value: *total_angle_deg, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "axis_x".into(), value: *axis_x, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "axis_y".into(), value: *axis_y, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "axis_z".into(), value: *axis_z, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "axis_dx".into(), value: *axis_dx, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "axis_dy".into(), value: *axis_dy, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "axis_dz".into(), value: *axis_dz, readonly: true });
            ("CircularPattern".to_string(), params)
        }
        FeatureKind::Mirror { plane_nx, plane_ny, plane_nz, plane_px, plane_py, plane_pz, .. } => {
            let mut params = Vec::new();
            params.push(ParameterInfo { id: ParameterId(0), name: "normal_x".into(), value: *plane_nx, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "normal_y".into(), value: *plane_ny, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "normal_z".into(), value: *plane_nz, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "point_x".into(), value: *plane_px, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "point_y".into(), value: *plane_py, readonly: true });
            params.push(ParameterInfo { id: ParameterId(0), name: "point_z".into(), value: *plane_pz, readonly: true });
            ("Mirror".to_string(), params)
        }
        FeatureKind::Sweep { .. } => ("Sweep".to_string(), Vec::new()),
        FeatureKind::Boolean { .. } => ("Boolean".to_string(), Vec::new()),
        FeatureKind::CustomSolid { distance, .. } => {
            ("CustomSolid".to_string(), parameter_info(doc, *distance).into_iter().collect())
        }
    };

    let plane: Option<String>;
    let plane_base: Option<String>;
    let plane_distance: Option<f64>;
    if let Some(p) = f.plane() {
        match p {
            PlaneDefinition::YZ => { plane = Some("yz".into()); plane_base = None; plane_distance = None; }
            PlaneDefinition::ZX => { plane = Some("zx".into()); plane_base = None; plane_distance = None; }
            PlaneDefinition::Offset { base, distance } => {
                plane = Some("offset".into());
                plane_base = Some(match **base {
                    PlaneDefinition::YZ => "yz",
                    PlaneDefinition::ZX => "zx",
                    PlaneDefinition::XY => "xy",
                    PlaneDefinition::Offset { .. } => "xy",
                }.to_string());
                plane_distance = Some(*distance);
            }
            PlaneDefinition::XY => { plane = Some("xy".into()); plane_base = None; plane_distance = None; }
        }
    } else {
        plane = None; plane_base = None; plane_distance = None;
    }

    let target_id = match &f.kind {
        FeatureKind::Extrude { sketch_id, .. } | FeatureKind::Revolve { sketch_id, .. } => Some(*sketch_id),
        FeatureKind::Fillet { target_id, .. } | FeatureKind::Chamfer { target_id, .. }
        | FeatureKind::Shell { target_id, .. } => Some(*target_id),
        FeatureKind::LinearPattern { target_id, .. } | FeatureKind::CircularPattern { target_id, .. }
        | FeatureKind::Mirror { target_id, .. } => Some(*target_id),
        FeatureKind::Sweep { profile_sketch_id, .. } => Some(*profile_sketch_id),
        _ => None,
    };

    FeatureNode {
        id: f.id(),
        name: f.name.clone(),
        feature_type: ft,
        parameters: params,
        suppressed: f.suppressed,
        has_dependents: doc.has_dependents(f.id()),
        errors: errors.get(&f.id()).cloned(),
        plane,
        plane_base,
        plane_distance,
        color: f.color.clone(),
        target_id,
        parent_id: f.parent_id,
    }
}

// ── Helper: append feature with auto-named parameter ──────────────

/// Locks the doc, snapshots the undo state, allocates a parameter,
/// creates the feature with the given kind, and regenerates.
/// Validates dependencies BEFORE mutating: a failed add leaves no orphan
/// parameter and no bogus undo entry.
fn add_feature_with_param(
    state: &AppState,
    param_name: &str,
    param_value: f64,
    kind_fn: impl FnOnce(FeatureId, ParameterId) -> FeatureKind,
) -> Result<FeatureId, String> {
    // Build the kind with speculative ids (param = next, feature = next+1,
    // matching the allocation order below) and validate first.
    let (kind, expected_param_id, expected_id) = {
        let doc = state.lock_doc();
        let next = doc.peek_next_id();
        let kind = kind_fn(FeatureId(next + 1), ParameterId(next));
        let probe = Feature::new(FeatureId(next + 1), "probe", kind.clone());
        for dep in probe.dependencies() {
            if doc.get_feature(dep).is_none() {
                return Err(format!("Dependency {:?} does not exist", dep));
            }
        }
        (kind, ParameterId(next), FeatureId(next + 1))
    };

    state.snapshot();
    let mut doc = state.lock_doc();
    let param_id = doc.add_parameter(format!("{}{}", param_name, expected_id.0), param_value);
    debug_assert_eq!(param_id, expected_param_id);
    let id = doc.new_feature_id();
    debug_assert_eq!(id, expected_id);

    let name = default_feature_name(&kind, id);
    doc.add_feature(Feature::new(id, name, kind));
    regen_locked(&doc, &state);
    Ok(id)
}

fn default_feature_name(kind: &FeatureKind, id: FeatureId) -> String {
    let base = match kind {
        FeatureKind::Sketch { .. } => "Sketch",
        FeatureKind::CustomSketch { .. } => "Custom",
        FeatureKind::SketchModule { .. } => "SketchModule",
        FeatureKind::Extrude { .. } => "Extrude",
        FeatureKind::Revolve { .. } => "Revolve",
        FeatureKind::Fillet { .. } => "Fillet",
        FeatureKind::Chamfer { .. } => "Chamfer",
        FeatureKind::Shell { .. } => "Shell",
        FeatureKind::Sweep { .. } => "Sweep",
        FeatureKind::Boolean { .. } => "Boolean",
        FeatureKind::LinearPattern { .. } => "Pattern",
        FeatureKind::CircularPattern { .. } => "CircPattern",
        FeatureKind::Mirror { .. } => "Mirror",
        FeatureKind::CustomSolid { .. } => "CustomSolid",
    };
    format!("{}{}", base, id.0)
}

/// Locks the doc, snapshots, creates the feature (no parameter), regenerates.
fn add_feature_kind(state: &AppState, kind: FeatureKind) -> Result<FeatureId, String> {
    // Validate dependencies BEFORE snapshot/mutation: a failed add must not
    // grow the undo stack (validate-then-mutate, principle #3).
    {
        let doc = state.lock_doc();
        let probe = Feature::new(FeatureId(doc.peek_next_id()), "probe", kind.clone());
        for dep in probe.dependencies() {
            if doc.get_feature(dep).is_none() {
                return Err(format!("Dependency {:?} does not exist", dep));
            }
        }
    }

    state.snapshot();
    let mut doc = state.lock_doc();
    let id = doc.new_feature_id();
    let name = default_feature_name(&kind, id);
    doc.add_feature(Feature::new(id, name, kind));
    regen_locked(&doc, &state);
    Ok(id)
}

// ── Feature queries ──────────────────────────────────────────────

#[tauri::command]
pub fn get_features(state: tauri::State<AppState>) -> Vec<FeatureNode> {
    state.regen_if_dirty();
    let doc = state.lock_doc();
    let errors = &state.lock_regen().errors;
    doc.features.iter().map(|f| feature_to_node(f, &doc, errors)).collect()
}

// ── Feature creation commands ────────────────────────────────────

#[tauri::command]
pub fn add_sketch_feature(plane: String, state: tauri::State<AppState>) -> FeatureId {
    state.snapshot();
    let mut doc = state.lock_doc();
    let id = doc.new_feature_id();
    let p = match plane.as_str() {
        "yz" => PlaneDefinition::YZ,
        "zx" => PlaneDefinition::ZX,
        _ => PlaneDefinition::XY,
    };
    let kind = FeatureKind::Sketch { plane: p, sketch: Sketch::new() };
    doc.add_feature(Feature::new(
        id,
        default_feature_name(&kind, id),
        kind,
    ));
    *state.lock_sketch() = Some(id);
    regen_locked(&doc, &state);
    id
}

#[tauri::command]
pub fn set_active_sketch(id: FeatureId, state: tauri::State<AppState>) {
    set_active_sketch_inner(id, state.inner());
}

fn set_active_sketch_inner(id: FeatureId, state: &AppState) {
    // Note: no snapshot. Switching the active sketch is a UI state change,
    // not a document mutation — it should not pollute the undo history.
    {
        let doc = state.lock_doc();
        if let Some(f) = doc.get_feature(id) {
            if f.is_sketch() {
                *state.lock_sketch() = Some(id);
            }
        }
    } // doc guard dropped BEFORE sync_sketch — sync_sketch locks the
      // document itself, and std::sync::Mutex is non-reentrant, so calling
      // it with the guard held deadlocks the command thread.
    sync_sketch(state);
}

#[tauri::command]
pub fn add_extrude_feature(
    sketch_id: FeatureId,
    direction: String,
    dist2: f64,
    draft_angle_deg: f64,
    depth: f64,
    selected_regions: Option<Vec<usize>>,
    state: tauri::State<AppState>,
) -> Result<FeatureId, String> {
    let dir = match direction.as_str() {
        "midplane" => ExtrudeDirection::Midplane,
        "two_sides" => ExtrudeDirection::TwoSides {
            dist1: depth.max(0.1),
            dist2: dist2.max(0.1),
        },
        _ => ExtrudeDirection::OneSide,
    };
    let d = if depth > 0.0 { depth } else { 1.0 };
    add_feature_with_param(&state, "Extrude", d, |_id, param_id| {
        FeatureKind::Extrude {
            sketch_id,
            distance: param_id,
            direction: dir,
            draft_angle_deg,
            selected_regions: selected_regions.clone(),
        }
    })
}

#[tauri::command]
pub fn add_revolve_feature(
    sketch_id: FeatureId,
    axis_entity_id: Option<EntityId>,
    state: tauri::State<AppState>,
) -> Result<FeatureId, String> {
    add_feature_with_param(&state, "Revolve", 360.0, move |_id, param_id| {
        FeatureKind::Revolve { sketch_id, angle: param_id, axis_entity_id }
    })
}

#[tauri::command]
pub fn add_fillet_feature(target_id: FeatureId, radius: f64, state: tauri::State<AppState>) -> Result<FeatureId, String> {
    // No edge list → legacy "fillet every sharp edge" behaviour.
    add_feature_with_param(&state, "Fillet", radius, move |_id, param_id| {
        FeatureKind::Fillet { target_id, radius: param_id, edges: Vec::new() }
    })
}

/// Fillet a specific selection of edges. Each edge is a `(vertex_a, vertex_b)`
/// index pair into the target solid's mesh — the UI produces these from the
/// user's edge picks. An empty list falls back to "all sharp edges".
#[tauri::command]
pub fn add_fillet_edges_feature(
    target_id: FeatureId,
    radius: f64,
    edges: Vec<(u32, u32)>,
    state: tauri::State<AppState>,
) -> Result<FeatureId, String> {
    add_feature_with_param(&state, "Fillet", radius, move |_id, param_id| {
        FeatureKind::Fillet { target_id, radius: param_id, edges }
    })
}

#[tauri::command]
pub fn add_chamfer_feature(target_id: FeatureId, distance: f64, state: tauri::State<AppState>) -> Result<FeatureId, String> {
    add_feature_with_param(&state, "Chamfer", distance, move |_id, param_id| {
        FeatureKind::Chamfer { target_id, distance: param_id, edges: Vec::new() }
    })
}

/// Chamfer a specific selection of edges. See [`add_fillet_edges_feature`].
#[tauri::command]
pub fn add_chamfer_edges_feature(
    target_id: FeatureId,
    distance: f64,
    edges: Vec<(u32, u32)>,
    state: tauri::State<AppState>,
) -> Result<FeatureId, String> {
    add_feature_with_param(&state, "Chamfer", distance, move |_id, param_id| {
        FeatureKind::Chamfer { target_id, distance: param_id, edges }
    })
}

/// Create a new sketch on a plane offset from one of the three base planes.
/// `base_plane_tag` is `"xy"` | `"yz"` | `"zx"`; `distance` is signed offset
/// along the base plane's normal. The sketch becomes the active sketch.
#[tauri::command]
pub fn create_offset_plane(
    base_plane_tag: String,
    distance: f64,
    state: tauri::State<AppState>,
) -> Result<FeatureId, String> {
    let base = match base_plane_tag.as_str() {
        "yz" => PlaneDefinition::YZ,
        "zx" => PlaneDefinition::ZX,
        "xy" => PlaneDefinition::XY,
        other => return Err(format!("unknown base plane '{}' (expected xy|yz|zx)", other)),
    };
    state.snapshot();
    let mut doc = state.lock_doc();
    let id = doc.new_feature_id();
    let plane = PlaneDefinition::Offset { base: Box::new(base), distance };
    let kind = FeatureKind::Sketch { plane, sketch: Sketch::new() };
    doc.add_feature(Feature::new(
        id,
        default_feature_name(&kind, id),
        kind,
    ));
    *state.lock_sketch() = Some(id);
    regen_locked(&doc, &state);
    Ok(id)
}

#[tauri::command]
pub fn add_linear_pattern(
    target_id: FeatureId, dir_x: f64, dir_y: f64, dir_z: f64,
    count: u32, spacing: f64, state: tauri::State<AppState>,
) -> Result<FeatureId, String> {
    add_feature_kind(&state, FeatureKind::LinearPattern {
        target_id, dir_x, dir_y, dir_z, count, spacing,
    })
}

#[tauri::command]
pub fn add_circular_pattern(
    target_id: FeatureId,
    axis_x: f64, axis_y: f64, axis_z: f64,
    axis_dx: f64, axis_dy: f64, axis_dz: f64,
    count: u32, total_angle_deg: f64,
    state: tauri::State<AppState>,
) -> Result<FeatureId, String> {
    add_feature_kind(&state, FeatureKind::CircularPattern {
        target_id, axis_x, axis_y, axis_z,
        axis_dx, axis_dy, axis_dz,
        count, total_angle_deg,
    })
}

#[tauri::command]
pub fn add_mirror_feature(
    target_id: FeatureId,
    plane_nx: f64, plane_ny: f64, plane_nz: f64,
    plane_px: f64, plane_py: f64, plane_pz: f64,
    state: tauri::State<AppState>,
) -> Result<FeatureId, String> {
    add_feature_kind(&state, FeatureKind::Mirror {
        target_id,
        plane_nx, plane_ny, plane_nz,
        plane_px, plane_py, plane_pz,
    })
}

#[tauri::command]
pub fn add_sweep_feature(profile_id: FeatureId, path_id: FeatureId, state: tauri::State<AppState>) -> Result<FeatureId, String> {
    add_feature_kind(&state, FeatureKind::Sweep {
        profile_sketch_id: profile_id,
        path_sketch_id: path_id,
    })
}

#[tauri::command]
pub fn add_shell_feature(target_id: FeatureId, thickness: f64, state: tauri::State<AppState>) -> Result<FeatureId, String> {
    add_feature_with_param(&state, "Shell", thickness, move |_id, param_id| {
        FeatureKind::Shell { target_id, thickness: param_id }
    })
}

#[tauri::command]
pub fn add_boolean_feature(
    target_a: FeatureId, target_b: FeatureId, op: String,
    state: tauri::State<AppState>,
) -> Result<FeatureId, String> {
    let bool_op = match op.as_str() {
        "subtract" => BoolOp::Subtract,
        "intersect" => BoolOp::Intersect,
        _ => BoolOp::Union,
    };
    add_feature_kind(&state, FeatureKind::Boolean { op: bool_op, target_a, target_b })
}

#[tauri::command]
pub fn update_parameter(id: ParameterId, value: f64, state: tauri::State<AppState>) {
    // Only snapshot when the parameter actually exists — otherwise this is
    // a silent no-op that must not grow the undo stack (principle #3).
    {
        let doc = state.lock_doc();
        if doc.get_parameter(id).is_none() {
            return;
        }
    }
    state.snapshot();
    let mut doc = state.lock_doc();
    if let Some(p) = doc.get_parameter_mut(id) {
        p.value = value;
    }
    // Incremental regen: find the owning feature and only re-evaluate it
    let owner = doc.features.iter()
        .find(|f| parameter_belongs_to(&f.kind, id))
        .map(|f| f.id());
    if let Some(fid) = owner {
        incremental_regen_locked(&doc, &state, fid);
    } else {
        regen_locked(&doc, &state);
    }
}

/// Check if a feature kind references a parameter (by ParameterId).
fn parameter_belongs_to(kind: &FeatureKind, pid: ParameterId) -> bool {
    match kind {
        FeatureKind::Extrude { distance, .. } => *distance == pid,
        FeatureKind::Revolve { angle, .. } => *angle == pid,
        FeatureKind::Fillet { radius, .. } => *radius == pid,
        FeatureKind::Chamfer { distance, .. } => *distance == pid,
        FeatureKind::Shell { thickness, .. } => *thickness == pid,
        _ => false,
    }
}

#[tauri::command]
pub fn rename_feature(id: FeatureId, name: String, state: tauri::State<AppState>) {
    {
        let doc = state.lock_doc();
        if doc.get_feature(id).is_none() {
            return; // no-op for nonexistent id — don't grow the undo stack
        }
    }
    state.snapshot();
    let mut doc = state.lock_doc();
    if let Some(f) = doc.get_feature_mut(id) {
        f.set_name(name);
    }
    regen_locked(&doc, &state);
}

#[tauri::command]
pub fn set_feature_suppressed(id: FeatureId, suppressed: bool, state: tauri::State<AppState>) {
    {
        let doc = state.lock_doc();
        if doc.get_feature(id).is_none() {
            return;
        }
    }
    state.snapshot();
    let mut doc = state.lock_doc();
    if let Some(f) = doc.get_feature_mut(id) {
        f.suppressed = suppressed;
    }
    regen_locked(&doc, &state);
}

#[tauri::command]
pub fn set_feature_color(feature_id: FeatureId, color: Option<String>, state: tauri::State<AppState>) -> Result<(), String> {
    // No snapshot — color is cosmetic, not a structural mutation (design principle #3).
    let mut doc = state.lock_doc();
    let feature = doc.get_feature_mut(feature_id).ok_or("feature not found")?;
    feature.color = color.clone();
    regen_locked(&doc, &state);
    Ok(())
}

#[tauri::command]
/// Delete a feature. Returns Err with a list of dependent IDs if the
/// feature has downstream dependencies — the UI can then ask the user
/// to confirm cascading delete.
pub fn delete_feature(
    id: FeatureId,
    cascade: bool,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    // U1: use snapshot_with_token so we can safely discard only THIS
    // snapshot if the operation is cancelled. A bare snapshot()+pop()
    // could pop a snapshot that was pushed by a concurrent command
    // between our snapshot() and the pop, corrupting the undo stack.
    let snap_token = state.snapshot_with_token();
    let mut doc = state.lock_doc();
    let dependents = doc.dependents_of(id);
    if !dependents.is_empty() && !cascade {
        // Discard the snapshot we just took — we don't want to record
        // an undo point for a no-op. The token ensures we only pop our
        // own snapshot.
        state.discard_snapshot(snap_token);
        let names: Vec<String> = dependents.iter()
            .filter_map(|d| doc.get_feature(*d).map(|f| f.name().to_string()))
            .collect();
        return Err(serde_json::json!({
            "kind": "dependents",
            "names": names
        }).to_string());
    }

    // Cascade: repeatedly remove features that depend on this one,
    // plus all child features (parent-child hierarchy).
    if cascade {
        let mut to_remove: Vec<FeatureId> = vec![id];
        let mut removed: std::collections::HashSet<FeatureId> = std::collections::HashSet::new();
        while let Some(rid) = to_remove.pop() {
            if removed.contains(&rid) { continue; }
            // Dependents (Extrude depends on Sketch)
            for dep in doc.dependents_of(rid) {
                if !removed.contains(&dep) { to_remove.push(dep); }
            }
            // Children (SketchModule parented under Sketch)
            for child in doc.children_of(rid) {
                if !removed.contains(&child) { to_remove.push(child); }
            }
            removed.insert(rid);
        }
        for rid in &removed {
            doc.remove_feature(*rid);
        }
    } else {
        // Also remove children when deleting without cascade prompt
        let children = doc.children_of(id);
        for child in &children {
            doc.remove_feature(*child);
        }
        doc.remove_feature(id);
    }

    let mut active = state.lock_sketch();
    if *active == Some(id) || (cascade && active.map(|a| doc.get_feature(a).is_none()).unwrap_or(false)) {
        *active = doc.features.iter().find_map(|f| if f.is_sketch() { Some(f.id()) } else { None });
    }
    regen_locked(&doc, &state);
    Ok(())
}

#[tauri::command]
pub fn clear_document(app: tauri::AppHandle, state: tauri::State<'_, AppState>) {
    state.snapshot();
    let mut doc = state.lock_doc();
    *doc = Document::new("Part1");
    let id = doc.new_feature_id();
    doc.add_feature(Feature::new(
        id,
        "Sketch1",
        FeatureKind::Sketch { sketch: Sketch::new(), plane: PlaneDefinition::default() },
    ));
    *state.lock_sketch() = Some(id);
    // Imported meshes belong to the old document — drop them too.
    state.imported_meshes.lock().expect("imported_meshes poisoned").clear();
    regen_locked(&doc, &state);
    clear_autosave_file(&app);
}

// ── Sketch entity operations ─────────────────────────────────────

#[tauri::command]
pub fn get_sketch_entities(state: tauri::State<AppState>) -> Vec<RenderEntity> {
    let doc = state.lock_doc();
    let id = match *state.lock_sketch() {
        Some(id) => id,
        None => return vec![],
    };
    let sketch = match doc.get_feature(id).and_then(|f| f.sketch()) {
        Some(s) => s,
        None => return vec![],
    };
    let origin = SketchPoint::new(0.0, 0.0);
    sketch.entities.iter().map(|(&eid, entity)| match entity {
        SketchEntity::Point(p) => RenderEntity::Point { id: eid, x: p.x, y: p.y },
        SketchEntity::Line { start, end, construction } => {
            let ps = sketch.get_point(*start).unwrap_or(&origin);
            let pe = sketch.get_point(*end).unwrap_or(&origin);
            RenderEntity::Line { id: eid, x1: ps.x, y1: ps.y, x2: pe.x, y2: pe.y, construction: *construction }
        }
        SketchEntity::Circle { center, radius, construction } => {
            let pc = sketch.get_point(*center).unwrap_or(&origin);
            RenderEntity::Circle { id: eid, cx: pc.x, cy: pc.y, radius: *radius, construction: *construction }
        }
        SketchEntity::Arc { center, radius, start_angle, end_angle, .. } => {
            let pc = sketch.get_point(*center).unwrap_or(&origin);
            RenderEntity::Arc { id: eid, cx: pc.x, cy: pc.y, radius: *radius, start_angle: *start_angle, end_angle: *end_angle }
        }
        SketchEntity::Spline { control_points, .. } => {
            let pts: Vec<(f64, f64)> = control_points.iter()
                .map(|cid| { let p = sketch.get_point(*cid).unwrap_or(&origin); (p.x, p.y) })
                .collect();
            RenderEntity::Spline { id: eid, points: pts }
        }
        SketchEntity::Ellipse { center, major_axis_end, ratio, .. } => {
            let pc = sketch.get_point(*center).unwrap_or(&origin);
            let pm = sketch.get_point(*major_axis_end).unwrap_or(&origin);
            RenderEntity::Ellipse { id: eid, cx: pc.x, cy: pc.y, major_rx: pm.x - pc.x, major_ry: pm.y - pc.y, ratio: *ratio }
        }
    }).collect()
}

#[tauri::command]
pub fn get_sketch_constraints(state: tauri::State<AppState>) -> Vec<Constraint> {
    let doc = state.lock_doc();
    let id = match *state.lock_sketch() {
        Some(id) => id,
        None => return vec![],
    };
    let sketch = match doc.get_feature(id).and_then(|f| f.sketch()) {
        Some(s) => s,
        None => return vec![],
    };
    sketch.constraints.clone()
}

#[tauri::command]
pub fn remove_constraint(index: usize, state: tauri::State<AppState>) -> bool {
    state.snapshot_if_sketch();
    state.with_active_sketch_mut(|s| {
        if index >= s.constraints.len() {
            return false;
        }
        s.constraints.remove(index);
        true
    }).unwrap_or(false)
}

#[tauri::command]
pub fn add_point(x: f64, y: f64, state: tauri::State<AppState>) -> Option<EntityId> {
    state.snapshot_if_sketch();
    let __result = state.with_active_sketch_mut(|s| s.add_point(x, y));
    sync_sketch(state.inner());
    __result
}
#[tauri::command]
pub fn add_line(start: EntityId, end: EntityId, state: tauri::State<AppState>) -> Option<EntityId> {
    state.snapshot_if_sketch();
    let __result = state.with_active_sketch_mut(|s| s.add_line(start, end));
    sync_sketch(state.inner());
    __result
}
#[tauri::command]
pub fn add_circle(center: EntityId, radius: f64, state: tauri::State<AppState>) -> Option<EntityId> {
    state.snapshot_if_sketch();
    state.with_active_sketch_mut(|s| s.add_circle(center, radius))
}
#[tauri::command]
pub fn add_arc(center: EntityId, radius: f64, start_angle: f64, end_angle: f64, state: tauri::State<AppState>) -> Option<EntityId> {
    state.snapshot_if_sketch();
    state.with_active_sketch_mut(|s| s.add_arc(center, radius, start_angle, end_angle))
}
#[tauri::command]
pub fn add_spline(control_points: Vec<EntityId>, state: tauri::State<AppState>) -> Option<EntityId> {
    state.snapshot_if_sketch();
    state.with_active_sketch_mut(|s| s.add_spline(control_points))
}
#[tauri::command]
pub fn add_ellipse(center: EntityId, major_axis_end: EntityId, ratio: f64, state: tauri::State<AppState>) -> Option<EntityId> {
    state.snapshot_if_sketch();
    state.with_active_sketch_mut(|s| s.add_ellipse(center, major_axis_end, ratio))
}
#[tauri::command]
pub fn add_constraint(constraint: Constraint, state: tauri::State<AppState>) {
    state.snapshot_if_sketch();
    state.with_active_sketch_mut(|s| s.add_constraint(constraint));
}

/// Update the numeric value of an existing constraint that matches
/// (variant, target entities). Used by the dimension overlay to edit a
/// driving dimension in place — avoids accumulating duplicate constraints
/// that fight each other in the solver.
#[tauri::command]
pub fn update_constraint_value(constraint: Constraint, state: tauri::State<AppState>) -> bool {
    state.snapshot_if_sketch();
    state.with_active_sketch_mut(|s| {
        // Find an existing constraint with the same variant and target
        // entities, then replace its value in place.
        let matches = |existing: &Constraint, incoming: &Constraint| -> bool {
            use Constraint::*;
            match (existing, incoming) {
                (Distance { a: a1, b: b1, .. }, Distance { a: a2, b: b2, .. }) => a1 == a2 && b1 == b2,
                (Radius { circle: c1, .. }, Radius { circle: c2, .. }) => c1 == c2,
                (Diameter { circle: c1, .. }, Diameter { circle: c2, .. }) => c1 == c2,
                (Angle { line_a: a1, line_b: b1, .. }, Angle { line_a: a2, line_b: b2, .. }) => a1 == a2 && b1 == b2,
                _ => false,
            }
        };
        for existing in s.constraints.iter_mut() {
            if matches(existing, &constraint) {
                *existing = constraint.clone();
                return true;
            }
        }
        // No matching constraint — add it as new (fresh dimension).
        s.constraints.push(constraint);
        true
    }).unwrap_or(false)
}
#[tauri::command]
pub fn solve_sketch(state: tauri::State<AppState>) -> Vec<String> {
    // No snapshot — the command that triggered the solve (addConstraint,
    // movePoint, etc.) already took a snapshot. solve_sketch is purely
    // computational (resolving constraint positions); snapshotting here
    // would create a duplicate undo entry for every constraint operation
    // (设计原则 §3).
    let mut diagnostics = Vec::new();
    state.with_active_sketch_mut(|s| {
        let iterations = echi_geom::solve(s, 100, 1e-6);
        if iterations.is_none() {
            diagnostics.push("警告：约束求解未收敛（100 次迭代后仍未达到容差）".to_string());
        }
        diagnostics.extend(
            echi_geom::check_overconstrained(s)
                .into_iter()
                .map(|(_, msg)| msg),
        );
    });
    diagnostics
}

#[tauri::command]
pub fn move_point(id: EntityId, x: f64, y: f64, state: tauri::State<AppState>) {
    move_point_inner(id, x, y, false, state)
}

#[tauri::command]
pub fn move_point_no_snapshot(id: EntityId, x: f64, y: f64, state: tauri::State<AppState>) {
    move_point_inner(id, x, y, true, state)
}

fn move_point_inner(id: EntityId, x: f64, y: f64, skip_snapshot: bool, state: tauri::State<AppState>) {
    if !skip_snapshot {
        state.snapshot_if_sketch();
    }
    state.with_active_sketch_mut(|s| {
        if let Some(p) = s.get_point_mut(id) {
            p.x = x;
            p.y = y;
        }
    });
}

#[tauri::command]
pub fn update_entity_prop(id: EntityId, prop: String, value: f64, state: tauri::State<AppState>) -> bool {
    state.snapshot_if_sketch();
    state.with_active_sketch_mut(|sketch| {
        match sketch.entities.get_mut(&id) {
            Some(SketchEntity::Point(p)) => match prop.as_str() {
                "x" => p.x = value,
                "y" => p.y = value,
                _ => return false,
            },
            Some(SketchEntity::Circle { radius, construction, .. }) => match prop.as_str() {
                "radius" => *radius = value,
                "construction" => *construction = value != 0.0,
                _ => return false,
            },
            Some(SketchEntity::Line { construction, .. }) => {
                if prop == "construction" { *construction = value != 0.0; } else { return false; }
            }
            Some(SketchEntity::Arc { radius, start_angle, end_angle, construction, .. }) => match prop.as_str() {
                "radius" => *radius = value,
                "start_angle" => *start_angle = value,
                "end_angle" => *end_angle = value,
                "construction" => *construction = value != 0.0,
                _ => return false,
            },
            Some(SketchEntity::Spline { construction, .. }) => {
                if prop == "construction" { *construction = value != 0.0; } else { return false; }
            }
            Some(SketchEntity::Ellipse { ratio, construction, .. }) => match prop.as_str() {
                "ratio" => *ratio = value,
                "construction" => *construction = value != 0.0,
                _ => return false,
            },
            _ => return false,
        };
        true
    }).unwrap_or(false)
}

#[tauri::command]
/// Delete a sketch entity. Cascades to dependent entities and constraints
/// via `Sketch::delete_entity_cascade` — otherwise deleting a circle's center
/// point would leave the circle stranded at (0,0).
pub fn delete_entity(id: EntityId, state: tauri::State<AppState>) -> bool {
    state.snapshot_if_sketch();
    let __result = state.with_active_sketch_mut(|s| !s.delete_entity_cascade(id).is_empty()).unwrap_or(false);
    sync_sketch(state.inner());
    __result
}

/// Delete an entity WITHOUT an undo snapshot. Used by the draw-cancel
/// cleanup path: the pending points were created moments ago by the same
/// gesture, and undoing that gesture as a whole already covers them —
/// N extra snapshots would spam the stack and (worse) clear the redo
/// stack mid-undo (原则3).
#[tauri::command]
pub fn delete_entity_no_snapshot(id: EntityId, state: tauri::State<AppState>) -> bool {
    state.with_active_sketch_mut(|s| !s.delete_entity_cascade(id).is_empty()).unwrap_or(false)
}

/// Move a feature under a new parent in the tree hierarchy.
/// The feature is re-ordered to sit just after its new parent.
#[tauri::command]
pub fn move_feature_under(
    child_id: FeatureId,
    new_parent_id: FeatureId,
    state: tauri::State<AppState>,
) -> Result<bool, String> {
    state.snapshot();
    let mut doc = state.lock_doc();
    Ok(doc.reparent_to(child_id, new_parent_id))
}

/// Detach a feature from its parent, making it a top-level feature.
#[tauri::command]
pub fn detach_feature_child(
    id: FeatureId,
    state: tauri::State<AppState>,
) -> Result<bool, String> {
    state.snapshot();
    let mut doc = state.lock_doc();
    Ok(doc.detach_from_parent(id))
}

/// Helper: check if a SketchEntity has a construction flag set to true.
fn entity_is_construction(entity: &SketchEntity) -> bool {
    match entity {
        SketchEntity::Line { construction, .. }
        | SketchEntity::Circle { construction, .. }
        | SketchEntity::Arc { construction, .. }
        | SketchEntity::Spline { construction, .. }
        | SketchEntity::Ellipse { construction, .. } => *construction,
        SketchEntity::Point(_) => false,
    }
}

/// Trim an entity at the nearest intersection with another non-construction entity.
/// The portion nearest to (click_x, click_y) is kept.
#[tauri::command]
pub fn trim_entity(
    entity_id: EntityId,
    click_x: f64,
    click_y: f64,
    state: tauri::State<AppState>,
) -> Result<bool, String> {
    use echi_geom::sketch_geom;
    state.snapshot_if_sketch();
    state.with_active_sketch_mut(|sketch| {
        let entity = sketch.entities.get(&entity_id).ok_or("entity not found")?.clone();
        // Collect intersection points with ALL other non-construction entities
        struct Hit { x: f64, y: f64, dist_to_click: f64 }
        let mut hits: Vec<Hit> = Vec::new();

        for (&other_id, other) in &sketch.entities {
            if other_id == entity_id { continue; }
            if entity_is_construction(other) { continue; }

            match &entity {
                SketchEntity::Line { start, end, .. } => {
                    let p1 = sketch.get_point(*start).ok_or("start point missing")?;
                    let p2 = sketch.get_point(*end).ok_or("end point missing")?;
                    match other {
                        SketchEntity::Line { start: os, end: oe, .. } => {
                            let op1 = match sketch.get_point(*os) { Some(p) => p, None => continue };
                            let op2 = match sketch.get_point(*oe) { Some(p) => p, None => continue };
                            if let Some((ix, iy, _, _)) = sketch_geom::segment_intersection(
                                p1.x, p1.y, p2.x, p2.y, op1.x, op1.y, op2.x, op2.y, 1e-6,
                            ) {
                                let d = ((click_x - ix).powi(2) + (click_y - iy).powi(2)).sqrt();
                                hits.push(Hit { x: ix, y: iy, dist_to_click: d });
                            }
                        }
                        SketchEntity::Circle { center, radius, .. } => {
                            let cp = match sketch.get_point(*center) { Some(p) => p, None => continue };
                            for (ix, iy) in sketch_geom::segment_circle_intersection(
                                p1.x, p1.y, p2.x, p2.y, cp.x, cp.y, *radius,
                            ) {
                                let d = ((click_x - ix).powi(2) + (click_y - iy).powi(2)).sqrt();
                                hits.push(Hit { x: ix, y: iy, dist_to_click: d });
                            }
                        }
                        _ => {}
                    }
                }
                SketchEntity::Circle { center, radius, .. } => {
                    let cp = match sketch.get_point(*center) { Some(p) => p, None => continue };
                    match other {
                        SketchEntity::Line { start: os, end: oe, .. } => {
                            let op1 = match sketch.get_point(*os) { Some(p) => p, None => continue };
                            let op2 = match sketch.get_point(*oe) { Some(p) => p, None => continue };
                            for (ix, iy) in sketch_geom::segment_circle_intersection(
                                op1.x, op1.y, op2.x, op2.y, cp.x, cp.y, *radius,
                            ) {
                                let d = ((click_x - ix).powi(2) + (click_y - iy).powi(2)).sqrt();
                                hits.push(Hit { x: ix, y: iy, dist_to_click: d });
                            }
                        }
                        _ => {}
                    }
                }
                _ => return Err("trim only supports Line and Circle entities".into()),
            }
        }

        if hits.is_empty() {
            return Ok(false);
        }

        // Select the nearest intersection to the click
        hits.sort_by(|a, b| a.dist_to_click.partial_cmp(&b.dist_to_click).unwrap());
        let cut = &hits[0];

        // Split the entity and keep the portion nearest the click
        match &entity {
            SketchEntity::Line { start, end, construction } => {
                let p1 = sketch.get_point(*start).unwrap();
                let p2 = sketch.get_point(*end).unwrap();
                let d1 = ((click_x - p1.x).powi(2) + (click_y - p1.y).powi(2)).sqrt();
                let d2 = ((click_x - p2.x).powi(2) + (click_y - p2.y).powi(2)).sqrt();

                // Create intersection point
                let cut_pt_id = sketch.add_point(cut.x, cut.y);

                let kept = if d1 < d2 {
                    // Keep from start to cut
                    sketch.add_line(*start, cut_pt_id)
                } else {
                    // Keep from cut to end
                    sketch.add_line(cut_pt_id, *end)
                };
                // Copy construction flag by updating entity
                if *construction {
                    if let Some(kept_entity) = sketch.entities.get_mut(&kept) {
                        match kept_entity {
                            SketchEntity::Line { construction: c, .. } => *c = true,
                            _ => {}
                        }
                    }
                }
                // Delete original entity (cascade removes its constraints)
                sketch.delete_entity_cascade(entity_id);
            }
            SketchEntity::Circle { center, radius, construction: _ } => {
                // Trim a circle by replacing it with an arc
                // Compute the two intersection points to determine the arc angles
                // For now: keep the full circle but add the intersection point
                // (Circle-to-arc conversion is complex; implementation deferred)
                // Create intersection point at the cut
                sketch.add_point(cut.x, cut.y);
                // For simplicity: replace circle with arc spanning the kept side
                // Angle from center to cut point
                let cp = sketch.get_point(*center).unwrap();
                let cut_angle = (cut.y - cp.y).atan2(cut.x - cp.x);
                // Keep the larger portion (opposite from click, i.e., the complement)
                // Create a short arc - this is a simplified approach
                let arc_start = cut_angle;
                let arc_end = cut_angle + std::f64::consts::PI; // half circle approximation
                sketch.add_arc(*center, *radius, arc_start, arc_end);
                sketch.delete_entity_cascade(entity_id);
            }
            _ => return Err("trim only supports Line and Circle entities".into()),
        }
        Ok(true)
    }).unwrap_or(Err("no active sketch".to_string()))
}

/// Extend an entity along its natural direction until it hits a boundary entity.
/// The endpoint nearest to (click_x, click_y) is extended.
#[tauri::command]
pub fn extend_entity(
    entity_id: EntityId,
    click_x: f64,
    click_y: f64,
    state: tauri::State<AppState>,
) -> Result<bool, String> {
    use echi_geom::sketch_geom;
    state.snapshot_if_sketch();
    state.with_active_sketch_mut(|sketch| {
        let entity = sketch.entities.get(&entity_id).ok_or("entity not found")?.clone();

        match &entity {
            SketchEntity::Line { start, end, .. } => {
                let p1 = sketch.get_point(*start).ok_or("start point missing")?;
                let p2 = sketch.get_point(*end).ok_or("end point missing")?;

                // Determine which end to extend: the one closer to the click
                let d1 = ((click_x - p1.x).powi(2) + (click_y - p1.y).powi(2)).sqrt();
                let d2 = ((click_x - p2.x).powi(2) + (click_y - p2.y).powi(2)).sqrt();

                // Extend from the far endpoint through the near endpoint
                let (anchor, ext_from) = if d1 < d2 {
                    ((p2.x, p2.y), (p1.x, p1.y))
                } else {
                    ((p1.x, p1.y), (p2.x, p2.y))
                };

                // Direction from anchor through ext_from (extending outward)
                let dx = ext_from.0 - anchor.0;
                let dy = ext_from.1 - anchor.1;
                let len = (dx * dx + dy * dy).sqrt();
                if len < 1e-9 { return Err("zero-length line".into()); }
                let dir_x = dx / len;
                let dir_y = dy / len;

                // Find the nearest intersection along the extension direction
                // We search a long ray from ext_from in the extension direction
                let ray_len = 10000.0; // large search distance
                let rx1 = ext_from.0;
                let ry1 = ext_from.1;
                let rx2 = ext_from.0 + dir_x * ray_len;
                let ry2 = ext_from.1 + dir_y * ray_len;

                struct Hit { x: f64, y: f64, dist: f64 }
                let mut hits: Vec<Hit> = Vec::new();

                for (&other_id, other) in &sketch.entities {
                    if other_id == entity_id { continue; }
                    if entity_is_construction(other) { continue; }

                    match other {
                        SketchEntity::Line { start: os, end: oe, .. } => {
                            let op1 = match sketch.get_point(*os) { Some(p) => p, None => continue };
                            let op2 = match sketch.get_point(*oe) { Some(p) => p, None => continue };
                            if let Some((ix, iy, _, _)) = sketch_geom::segment_intersection(
                                rx1, ry1, rx2, ry2, op1.x, op1.y, op2.x, op2.y, 1e-6,
                            ) {
                                let d = ((ix - ext_from.0).powi(2) + (iy - ext_from.1).powi(2)).sqrt();
                                if d > 1e-6 {
                                    hits.push(Hit { x: ix, y: iy, dist: d });
                                }
                            }
                        }
                        SketchEntity::Circle { center, radius, .. } => {
                            let cp = match sketch.get_point(*center) { Some(p) => p, None => continue };
                            for (ix, iy) in sketch_geom::line_circle_intersection(
                                rx1, ry1, rx2, ry2, cp.x, cp.y, *radius,
                            ) {
                                // Only keep intersections in the extension direction
                                let t = (ix - ext_from.0) * dir_x + (iy - ext_from.1) * dir_y;
                                if t > 1e-6 {
                                    let d = ((ix - ext_from.0).powi(2) + (iy - ext_from.1).powi(2)).sqrt();
                                    hits.push(Hit { x: ix, y: iy, dist: d });
                                }
                            }
                        }
                        _ => {}
                    }
                }

                if hits.is_empty() {
                    return Ok(false);
                }

                // Choose the nearest intersection
                hits.sort_by(|a, b| a.dist.partial_cmp(&b.dist).unwrap());
                let hit = &hits[0];

                // Create intersection point and move the extended endpoint
                let new_point_id = sketch.add_point(hit.x, hit.y);

                // Replace the entity: delete old, create new with extended endpoint
                if d1 < d2 {
                    // Extended start point
                    sketch.add_line(new_point_id, *end);
                } else {
                    // Extended end point
                    sketch.add_line(*start, new_point_id);
                }
                sketch.delete_entity_cascade(entity_id);

                Ok(true)
            }
            _ => Err("extend only supports Line entities".into()),
        }
    }).unwrap_or(Err("no active sketch".to_string()))
}

#[tauri::command]
pub fn clear_sketch(state: tauri::State<AppState>) {
    state.snapshot_if_sketch();
    state.with_active_sketch_mut(|s| *s = Sketch::new());
    regenerate_state(&state);
    sync_sketch(state.inner());
}

// ── Solid mesh queries ───────────────────────────────────────────

#[tauri::command]
pub fn get_solid_mesh(state: tauri::State<AppState>) -> Option<RenderMesh> {
    state.regen_if_dirty();
    let result = state.lock_regen();
    result.current_solid.and_then(|id| result.solids.get(&id)).map(RenderMesh::from)
}

/// Generate a preview extrude mesh without adding it to the document.
#[tauri::command]
pub fn preview_extrude(
    sketch_id: FeatureId,
    direction: String,
    dist2: f64,
    draft_angle_deg: f64,
    depth: f64,
    selected_regions: Option<Vec<usize>>,
    state: tauri::State<AppState>,
) -> Option<RenderMesh> {
    let doc = state.lock_doc();
    let feature = doc.get_feature(sketch_id)?;
    let sketch = feature.sketch()?;
    let plane = feature.plane().cloned().unwrap_or_default();

    let dir = match direction.as_str() {
        "midplane" => ExtrudeDirection::Midplane,
        "two_sides" => ExtrudeDirection::TwoSides { dist1: depth.max(0.1), dist2: dist2.max(0.1) },
        _ => ExtrudeDirection::OneSide,
    };

    let h = if depth > 0.0 { depth } else { 1.0 };
    let mesh = if let Some(ref indices) = selected_regions {
        echi_geom::extrude::extrude_selected(sketch, h, dir, draft_angle_deg, &plane, indices)?
    } else {
        echi_geom::extrude(sketch, h, dir, draft_angle_deg, &plane)?
    };
    Some(RenderMesh::from(&mesh))
}

/// Serializable region info returned to the frontend for the extrude region picker.
/// Each region corresponds to a CLOSED LOOP (polygon) in the sketch — not a
/// pre-classified outer+holes group. When multiple loops are selected, the
/// extrude pipeline automatically classifies which are outers and which are
/// holes based on containment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtrudeRegionInfo {
    pub index: usize,
    /// Polygon points forming the loop contour (in sketch 2D coords).
    pub outer: Vec<(f64, f64)>,
    /// Absolute area of the loop.
    pub area: f64,
}

/// Return available extrude regions (closed loops) from the active sketch.
/// Each loop is independently selectable — the extrude engine will determine
/// outer/hole relationships at extrude time based on which loops are selected.
///
/// Takes an explicit `sketch_id` so the region list matches the sketch the
/// extrude will actually target (the tree-selected sketch), NOT whatever
/// happens to be backend-active (原则1/14 — the previous implicit-active
/// behavior listed one sketch's loops while extruding another's).
#[tauri::command]
pub fn get_extrude_regions(sketch_id: Option<FeatureId>, state: tauri::State<AppState>) -> Vec<ExtrudeRegionInfo> {
    use echi_geom::extrude::{self, Point2D};

    let doc = state.lock_doc();
    let sketch_id = match sketch_id.or(*state.lock_sketch()) {
        Some(id) => id,
        None => return vec![],
    };
    let feature = match doc.get_feature(sketch_id) {
        Some(f) => f,
        None => return vec![],
    };
    let sketch = match feature.sketch() {
        Some(s) => s,
        None => return vec![],
    };

    let loops = match extrude::extract_loops(sketch) {
        Some(l) => l,
        None => return vec![],
    };

    // Sort by area (largest first) for consistent ordering
    let mut with_area: Vec<(usize, Vec<Point2D>, f64)> = loops
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let area = extrude::polygon_area(&l).abs();
            (i, l, area)
        })
        .collect();
    with_area.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    with_area.into_iter().enumerate().map(|(new_idx, (_orig_idx, loop_pts, area))| {
        ExtrudeRegionInfo {
            index: new_idx,
            outer: loop_pts.iter().map(|p| (p.x, p.y)).collect(),
            area,
        }
    }).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolidMeshEntry {
    pub feature_id: FeatureId,
    pub name: String,
    pub mesh: RenderMesh,
    pub suppressed: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub fn get_all_solid_meshes(state: tauri::State<AppState>) -> Vec<SolidMeshEntry> {
    state.regen_if_dirty();
    let doc = state.lock_doc();
    let result = state.lock_regen();
    result.solids.iter().map(|(id, mesh)| {
        let feature = doc.get_feature(*id);
        let name = feature.map(|f| f.name().to_string()).unwrap_or_else(|| format!("Solid{}", id.0));
        let suppressed = feature.map(|f| f.suppressed).unwrap_or(false);
        let error = result.errors.get(id).cloned();
        SolidMeshEntry { feature_id: *id, name, mesh: RenderMesh::from(mesh), suppressed, error }
    }).collect()
}

#[tauri::command]
pub fn get_regen_errors(state: tauri::State<AppState>) -> Vec<(FeatureId, String)> {
    state.regen_if_dirty();
    let result = state.lock_regen();
    let mut errors: Vec<(FeatureId, String)> = result.errors.iter().map(|(k, v)| (*k, v.clone())).collect();
    errors.sort_by_key(|(id, _)| id.0);
    errors
}

// ── B-rep toggle ────────────────────────────────────────────────

// B-rep pipeline is now always-on by default.
// `use_brep` is initialised to `true` in AppState and no longer toggled at runtime.

// ── Viewport capture ────────────────────────────────────────────

/// Capture the current 3D viewport as a base64-encoded PNG image.
/// Emits a `capture-viewport` event to the frontend, which renders
/// the current frame and returns the image via `viewport-image` event.
#[tauri::command]
pub fn capture_viewport(window: tauri::Window) -> Result<String, String> {
    use std::sync::mpsc;
    use tauri::{Emitter, Listener};
    let (tx, rx) = mpsc::channel();

    // Listen for the response
    let window_clone = window.clone();
    let unlisten = window_clone.listen("viewport-image", move |event| {
        let data = serde_json::from_str::<serde_json::Value>(event.payload())
            .unwrap_or_default()
            .get("data")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let _ = tx.send(data);
    });

    // Request capture from frontend
    window.emit("capture-viewport", ()).map_err(|e| e.to_string())?;

    // Wait for response (with timeout)
    let result = rx.recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| "viewport capture timed out".to_string());

    // Clean up listener
    window.unlisten(unlisten);

    result
}

// ── File IO ──────────────────────────────────────────────────────

fn file_path_to_path(path: tauri_plugin_dialog::FilePath) -> Option<std::path::PathBuf> {
    match path {
        tauri_plugin_dialog::FilePath::Path(p) => Some(p),
        _ => None,
    }
}

#[tauri::command]
pub fn save_project_cmd(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let path = app.dialog().file().add_filter("EchoCAD Project", &["echi"]).blocking_save_file();
    if let Some(path) = path {
        let path = file_path_to_path(path).ok_or("Invalid file path")?;
        let doc = state.lock_doc();
        echi_io::save_project(&*doc, &path).map_err(|e| e.to_string())?;
        record_recent_file(&app, &path);
        clear_autosave_file(&app);
        Ok(path.to_string_lossy().to_string())
    } else {
        Err("No file selected".into())
    }
}

/// Save the document to a specific path (no dialog). Used for "Save" when
/// the project is already on disk, or by tests.
#[tauri::command]
pub fn save_project_to(app: tauri::AppHandle, state: tauri::State<'_, AppState>, path: String) -> Result<(), String> {
    let p: std::path::PathBuf = path.into();
    let doc = state.lock_doc();
    echi_io::save_project(&*doc, &p).map_err(|e| e.to_string())?;
    clear_autosave_file(&app);
    Ok(())
}

#[tauri::command]
pub fn load_project_cmd(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    use tauri_plugin_dialog::DialogExt;
    let path = app.dialog().file().add_filter("EchoCAD Project", &["echi"]).blocking_pick_file();
    if let Some(path) = path {
        let path = file_path_to_path(path).ok_or("Invalid file path")?;
        let doc = echi_io::load_project(&path).map_err(|e| e.to_string())?;
        // D1: compute the next active sketch while we hold the document lock,
        // then drop it BEFORE locking active_sketch. This keeps the lock
        // ordering consistent with snapshot() (doc → active_sketch) and
        // prevents a deadlock with any concurrent caller that holds
        // active_sketch first.
        let active = {
            let mut state_doc = state.lock_doc();
            *state_doc = doc;
            state_doc.features.iter()
                .find_map(|f| if f.is_sketch() { Some(f.id()) } else { None })
        }; // doc lock dropped here
        *state.lock_sketch() = active;
        // Loading replaces the entire document — undo history for the
        // previous document would restore the wrong state. Clear it.
        state.lock_undo().clear();
        // Imported meshes belong to the previous document — drop them.
        state.imported_meshes.lock().map_err(|e| e.to_string())?.clear();
        regenerate_state(&state);
        record_recent_file(&app, &path);
        clear_autosave_file(&app);
        Ok(())
    } else {
        Err("No file selected".into())
    }
}

#[tauri::command]
pub fn load_project_from(app: tauri::AppHandle, state: tauri::State<'_, AppState>, path: String) -> Result<(), String> {
    let p: std::path::PathBuf = path.into();
    let doc = echi_io::load_project(&p).map_err(|e| e.to_string())?;
    // D1: same lock-ordering fix as load_project_cmd.
    let active = {
        let mut state_doc = state.lock_doc();
        *state_doc = doc;
        state_doc.features.iter()
            .find_map(|f| if f.is_sketch() { Some(f.id()) } else { None })
    }; // doc lock dropped here
    *state.lock_sketch() = active;
    // Loading replaces the entire document — clear stale undo history.
    state.lock_undo().clear();
    // Imported meshes belong to the previous document — drop them.
    state.imported_meshes.lock().map_err(|e| e.to_string())?.clear();
    regenerate_state(&state);
    clear_autosave_file(&app);
    Ok(())
}

// ── Recent files (persisted via tauri's app-data dir) ───────────

const RECENT_FILES_FILE: &str = "recent_files.json";
const MAX_RECENT: usize = 10;

fn recent_files_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    let dir = app.path().app_config_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(RECENT_FILES_FILE))
}

fn record_recent_file(app: &tauri::AppHandle, path: &std::path::Path) {
    let Some(store_path) = recent_files_path(app) else { return };
    let mut files = read_recent_files_internal(&store_path);
    let s = path.to_string_lossy().to_string();
    files.retain(|p| p != &s);
    files.insert(0, s);
    if files.len() > MAX_RECENT {
        files.truncate(MAX_RECENT);
    }
    if let Ok(data) = serde_json::to_vec_pretty(&files) {
        let _ = std::fs::write(&store_path, data);
    }
}

fn read_recent_files_internal(path: &std::path::Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

#[tauri::command]
pub fn get_recent_files(app: tauri::AppHandle) -> Vec<String> {
    let Some(p) = recent_files_path(&app) else { return vec![] };
    read_recent_files_internal(&p)
}

#[tauri::command]
pub fn clear_recent_files(app: tauri::AppHandle) -> bool {
    let Some(p) = recent_files_path(&app) else { return false };
    std::fs::write(&p, "[]").is_ok()
}

/// Merge all solid meshes in the regen result into a single combined mesh.
/// (Internal helper — not a registered Tauri command.)
fn merge_all_export_meshes(result: &RegenResult) -> Option<echi_geom::Mesh> {
    if result.solids.is_empty() {
        return result.current_solid.and_then(|id| result.solids.get(&id)).cloned();
    }
    let mut combined = echi_geom::Mesh::default();
    for mesh in result.solids.values() {
        let voff = combined.vertex_count() as u32;
        combined.positions.extend_from_slice(&mesh.positions);
        combined.normals.extend_from_slice(&mesh.normals);
        for &idx in &mesh.indices {
            combined.indices.push(voff + idx);
        }
    }
    Some(combined)
}

#[tauri::command]
pub fn export_stl(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let path = app.dialog().file().add_filter("STL", &["stl"]).blocking_save_file();
    if let Some(path) = path {
        let path = file_path_to_path(path).ok_or("Invalid file path")?;
        state.regen_if_dirty();
        let mesh = {
            let result = state.lock_regen();
            merge_all_export_meshes(&result).ok_or("No solid to export")?
        };
        let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        echi_io::export_stl_ascii(&mesh, &mut file).map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().to_string())
    } else {
        Err("No file selected".into())
    }
}

#[tauri::command]
pub fn export_obj(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let path = app.dialog().file().add_filter("OBJ", &["obj"]).blocking_save_file();
    if let Some(path) = path {
        let path = file_path_to_path(path).ok_or("Invalid file path")?;
        state.regen_if_dirty();
        let mesh = {
            let result = state.lock_regen();
            merge_all_export_meshes(&result).ok_or("No solid to export")?
        };
        let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        echi_io::export_obj(&mesh, &mut file).map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().to_string())
    } else {
        Err("No file selected".into())
    }
}

#[tauri::command]
pub fn export_gltf_cmd(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let path = app.dialog().file().add_filter("glTF", &["gltf"]).blocking_save_file();
    if let Some(path) = path {
        let path = file_path_to_path(path).ok_or("Invalid file path")?;
        state.regen_if_dirty();
        let mesh = {
            let result = state.lock_regen();
            merge_all_export_meshes(&result).ok_or("No solid to export")?
        };
        let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        echi_io::export_gltf(&mesh, &mut file).map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().to_string())
    } else {
        Err("No file selected".into())
    }
}

// ── STEP export (OCCT-powered) ───────────────────────────────────

#[cfg(feature = "occt")]
#[tauri::command]
pub fn export_step(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;

    let path = app.dialog().file().add_filter("STEP", &["step", "stp"]).blocking_save_file();
    if let Some(path) = path {
        let path = file_path_to_path(path).ok_or("Invalid file path")?;
        let doc = state.lock_doc();

        // Delegate OCCT solid collection to the domain helper (§9: thin commands).
        let solids = crate::step_io::collect_occt_solids(&doc)?;
        let count = solids.len();

        let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        cadrum::Solid::write_step(&solids, &mut file)
            .map_err(|e| format!("STEP export failed: {e}"))?;
        log::info!("Exported {count} solids to STEP: {}", path.display());
        Ok(path.to_string_lossy().to_string())
    } else {
        Err("No file selected".into())
    }
}

#[cfg(feature = "occt")]
#[tauri::command]
pub fn import_step(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<Vec<RenderMesh>, String> {
    use cadrum::Solid as OcctSolid;
    use tauri_plugin_dialog::DialogExt;

    let path = app.dialog().file().add_filter("STEP", &["step", "stp"]).blocking_pick_file();
    if let Some(path) = path {
        let path = file_path_to_path(path).ok_or("Invalid file path")?;
        let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let solids = OcctSolid::read_step(&mut file)
            .map_err(|e| format!("STEP import failed: {e}"))?;

        let count = solids.len();
        log::info!("Imported {count} solids from STEP: {}", path.display());

        // Delegate tessellation to the domain helper (§9: thin commands).
        let render_meshes = crate::step_io::tessellate_occt_solids(&solids)?;

        // Store in imported_meshes so every future regen re-injects them
        // (regen pipeline can't produce them — no backing document feature).
        // Synthetic FeatureIds live in [2^52 - 1000, 2^52): safely above any
        // real document id yet below 2^53, so they survive the JSON round
        // trip to JavaScript exactly (f64 can't represent ids near u64::MAX).
        {
            let mut imported = state.imported_meshes.lock()
                .map_err(|e| format!("lock error: {e}"))?;
            let mut result = state.lock_regen();
            let base = FeatureId(1u64 << 52);
            for (i, rm) in render_meshes.iter().enumerate() {
                let fid = FeatureId(base.0 - 1 - i as u64);
                let mesh = echi_geom::Mesh {
                    positions: rm.positions.clone(),
                    normals: rm.normals.clone(),
                    indices: rm.indices.iter().map(|&x| x).collect(),
                };
                imported.insert(fid, mesh.clone());
                result.solids.insert(fid, mesh);
            }
        }

        Ok(render_meshes)
    } else {
        Err("No file selected".into())
    }
}

// ── Plugin commands ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfoDto {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub generators: Vec<GeneratorInfoDto>,
    pub tools: Vec<ToolDefDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorInfoDto {
    pub plugin_id: String,
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub parameters: Vec<ParamDefDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDefDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefDto {
    pub plugin_id: String,
    pub id: String,
    pub name: String,
    pub tool_type: String,
}

#[tauri::command]
pub fn list_plugins(state: tauri::State<AppState>) -> Vec<PluginInfoDto> {
    state.plugin_registry.list_plugins().into_iter().map(|p| {
        let pid = p.id.clone();
        PluginInfoDto {
            id: p.id,
            name: p.name,
            version: p.version,
            description: p.description,
            generators: p.generators.into_iter().map(|g| {
                let plugin_id = pid.clone();
                GeneratorInfoDto {
                    plugin_id,
                    id: g.id,
                    name: g.name,
                    description: g.description,
                    icon: g.icon,
                    parameters: g.parameters.into_iter().map(|param| ParamDefDto {
                        id: param.id,
                        name: param.name,
                        description: param.description,
                        default_value: param.default_value,
                        min: param.min,
                        max: param.max,
                        step: param.step,
                    }).collect(),
                }
            }).collect(),
            tools: p.tools.into_iter().map(|t| {
                let plugin_id = pid.clone();
                ToolDefDto {
                    plugin_id,
                    id: t.id,
                    name: t.name,
                    tool_type: match t.tool_type {
                        echi_plugin::ToolType::Sketch => "sketch".into(),
                        echi_plugin::ToolType::Solid => "solid".into(),
                    },
                }
            }).collect(),
        }
    }).collect()
}

#[tauri::command]
pub fn list_generators(state: tauri::State<AppState>) -> Vec<GeneratorInfoDto> {
    state.plugin_registry.list_generators().into_iter().map(|(pid, g)| {
        GeneratorInfoDto {
            plugin_id: pid,
            id: g.id,
            name: g.name,
            description: g.description,
            icon: g.icon,
            parameters: g.parameters.into_iter().map(|param| ParamDefDto {
                id: param.id,
                name: param.name,
                description: param.description,
                default_value: param.default_value,
                min: param.min,
                max: param.max,
                step: param.step,
            }).collect(),
        }
    }).collect()
}

#[tauri::command]
pub fn generate_plugin_feature(
    plugin_id: String,
    generator_id: String,
    params: HashMap<String, f64>,
    state: tauri::State<AppState>,
) -> Result<FeatureId, String> {
    let sketch = state.plugin_registry.generate_sketch(&plugin_id, &generator_id, &params)
        .map_err(|e| e.to_string())?;
    state.snapshot();
    let mut doc = state.lock_doc();
    let id = doc.new_feature_id();
    let gen_name = state.plugin_registry.find_generator(&plugin_id, &generator_id)
        .map(|g| g.name)
        .unwrap_or_else(|| generator_id.clone());
    doc.add_feature(Feature::new(
        id,
        format!("{}_{}", gen_name, id.0),
        FeatureKind::SketchModule {
            sketch,
            plane: PlaneDefinition::default(),
            source: Some(CustomFeatureData { plugin_id, generator_id, params }),
        },
    ));
    *state.lock_sketch() = Some(id);
    regen_locked(&doc, &state);
    Ok(id)
}

// ── Undo / Redo ──────────────────────────────────────────────────

#[tauri::command]
pub fn undo(state: tauri::State<AppState>) -> Result<bool, String> {
    undo_inner(state.inner())
}

fn undo_inner(state: &AppState) -> Result<bool, String> {
    // All guards (doc + undo manager) are scoped to this block: sync_sketch
    // re-locks the document, and std::sync::Mutex is non-reentrant, so it
    // must run only after every guard is dropped.
    let did_undo = {
        let mut doc = state.lock_doc();
        let active = *state.lock_sketch();
        let mut mgr = state.lock_undo();
        if !mgr.can_undo() {
            false
        } else if let Some((restored, ra)) = mgr.undo(&doc, active) {
            *doc = restored;
            // Clear imported meshes: undo restores a snapshot whose id space may be
            // unrelated to the current document's. Stale entries alias onto new features
            // (phantom geometry, §16).
            if let Ok(mut imported) = state.imported_meshes.lock() {
                imported.clear();
            }
            let valid = ra
                .filter(|&id| doc.get_feature(id).map(|f| f.is_sketch()).unwrap_or(false))
                .or_else(|| doc.features.iter().find_map(|f| if f.is_sketch() { Some(f.id()) } else { None }));
            *state.lock_sketch() = valid;
            regen_locked(&doc, state);
            true
        } else {
            false
        }
    };
    if did_undo {
        sync_sketch(state);
    }
    Ok(did_undo)
}

#[tauri::command]
pub fn redo(state: tauri::State<AppState>) -> Result<bool, String> {
    redo_inner(state.inner())
}

fn redo_inner(state: &AppState) -> Result<bool, String> {
    // Same guard-scoping as undo: sync_sketch re-locks the document and
    // must run only after doc + undo guards are dropped.
    let did_redo = {
        let mut doc = state.lock_doc();
        let active = *state.lock_sketch();
        let mut mgr = state.lock_undo();
        if !mgr.can_redo() {
            false
        } else if let Some((restored, ra)) = mgr.redo(&doc, active) {
            *doc = restored;
            // Clear imported meshes (same reasoning as undo, §16).
            if let Ok(mut imported) = state.imported_meshes.lock() {
                imported.clear();
            }
            let valid = ra
                .filter(|&id| doc.get_feature(id).map(|f| f.is_sketch()).unwrap_or(false))
                .or_else(|| doc.features.iter().find_map(|f| if f.is_sketch() { Some(f.id()) } else { None }));
            *state.lock_sketch() = valid;
            regen_locked(&doc, state);
            true
        } else {
            false
        }
    };
    if did_redo {
        sync_sketch(state);
    }
    Ok(did_redo)
}

#[tauri::command]
pub fn can_undo_redo(state: tauri::State<AppState>) -> (bool, bool) {
    let mgr = state.lock_undo();
    (mgr.can_undo(), mgr.can_redo())
}

// ── Auto-save / recovery ─────────────────────────────────────────

/// Remove the autosave file so stale snapshots don't survive a save/load/clear.
fn clear_autosave_file(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Ok(dir) = app.path().app_config_dir() {
        let path = dir.join("autosave.echi");
        let _ = std::fs::remove_file(&path);
    }
}

#[tauri::command]
pub fn check_recovery(state: tauri::State<AppState>) -> bool {
    state.has_recovery_file.load(Ordering::Relaxed)
}

// ── Mass properties DTO ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassPropertiesDto {
    pub volume: f64,
    pub surface_area: f64,
    pub centroid: [f64; 3],
}

// ── Measurement commands ──────────────────────────────────────────

/// Compute the Euclidean distance between two 3D world-space points.
/// Idempotent — no document mutation, no snapshot, no regen.
#[tauri::command]
pub fn measure_distance(
    ax: f64, ay: f64, az: f64,
    bx: f64, by: f64, bz: f64,
) -> f64 {
    let dx = ax - bx;
    let dy = ay - by;
    let dz = az - bz;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Compute the angle <p1-p2-p3 in degrees.
/// Idempotent — no document mutation, no snapshot, no regen.
#[tauri::command]
pub fn measure_angle(
    p1x: f64, p1y: f64, p1z: f64,
    p2x: f64, p2y: f64, p2z: f64,
    p3x: f64, p3y: f64, p3z: f64,
) -> f64 {
    let v1x = p1x - p2x;
    let v1y = p1y - p2y;
    let v1z = p1z - p2z;
    let v2x = p3x - p2x;
    let v2y = p3y - p2y;
    let v2z = p3z - p2z;
    let dot = v1x * v2x + v1y * v2y + v1z * v2z;
    let mag1 = (v1x * v1x + v1y * v1y + v1z * v1z).sqrt();
    let mag2 = (v2x * v2x + v2y * v2y + v2z * v2z).sqrt();
    if mag1 < 1e-12 || mag2 < 1e-12 {
        return 0.0;
    }
    let cos_theta = (dot / (mag1 * mag2)).clamp(-1.0, 1.0);
    cos_theta.acos().to_degrees()
}

/// Compute mass properties (volume, surface area, centroid) for a
/// feature's solid mesh.  Idempotent — no snapshot, no mutation.
#[tauri::command]
pub fn get_mass_properties(
    feature_id: FeatureId,
    state: tauri::State<AppState>,
) -> Result<MassPropertiesDto, String> {
    #[cfg(feature = "occt")]
    {
        // Try OCCT exact mass properties first
        let doc = state.lock_doc();
        if let Some(solid) = echi_render::build_occt_solid_for_mass(&doc, feature_id) {
            let centroid = solid.center();
            return Ok(MassPropertiesDto {
                volume: solid.volume(),
                surface_area: solid.area(),
                centroid: [centroid.x, centroid.y, centroid.z],
            });
        }
    }

    state.regen_if_dirty();
    let result = state.lock_regen();
    let mesh = result.solids.get(&feature_id).ok_or("feature has no solid mesh")?;
    let props = echi_geom::compute_mass_properties(mesh).ok_or("empty mesh")?;
    Ok(MassPropertiesDto {
        volume: props.volume,
        surface_area: props.surface_area,
        centroid: props.centroid,
    })
}

// ── Pattern / Mirror parameter update commands ────────────────────

/// Update the parameters of an existing LinearPattern feature.
/// This is a document mutation — snapshot + regen.
#[tauri::command]
pub fn update_linear_pattern(
    feature_id: FeatureId,
    count: u32,
    spacing: f64,
    dir_x: f64,
    dir_y: f64,
    dir_z: f64,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    // Validate before snapshot (§15): wrong id/kind must not grow the undo stack.
    {
        let doc = state.lock_doc();
        let f = doc.get_feature(feature_id).ok_or("feature not found")?;
        if !matches!(f.kind, FeatureKind::LinearPattern { .. }) {
            return Err("feature is not a LinearPattern".into());
        }
    }
    state.snapshot();
    let mut doc = state.lock_doc();
    let feature = doc.get_feature_mut(feature_id).ok_or("feature not found")?;
    match &mut feature.kind {
        FeatureKind::LinearPattern {
            count: c, spacing: s, dir_x: dx, dir_y: dy, dir_z: dz, ..
        } => {
            *c = count;
            *s = spacing;
            *dx = dir_x;
            *dy = dir_y;
            *dz = dir_z;
        }
        _ => return Err("feature is not a LinearPattern".into()),
    }
    incremental_regen_locked(&doc, &state, feature_id);
    Ok(())
}

/// Update the parameters of an existing CircularPattern feature.
#[tauri::command]
pub fn update_circular_pattern(
    feature_id: FeatureId,
    count: u32,
    total_angle_deg: f64,
    axis_x: f64,
    axis_y: f64,
    axis_z: f64,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    // Validate before snapshot (§15): wrong id/kind must not grow the undo stack.
    {
        let doc = state.lock_doc();
        let f = doc.get_feature(feature_id).ok_or("feature not found")?;
        if !matches!(f.kind, FeatureKind::CircularPattern { .. }) {
            return Err("feature is not a CircularPattern".into());
        }
    }
    state.snapshot();
    let mut doc = state.lock_doc();
    let feature = doc.get_feature_mut(feature_id).ok_or("feature not found")?;
    match &mut feature.kind {
        FeatureKind::CircularPattern {
            count: c, total_angle_deg: a,
            axis_x: ax, axis_y: ay, axis_z: az, ..
        } => {
            *c = count;
            *a = total_angle_deg;
            *ax = axis_x;
            *ay = axis_y;
            *az = axis_z;
        }
        _ => return Err("feature is not a CircularPattern".into()),
    }
    incremental_regen_locked(&doc, &state, feature_id);
    Ok(())
}

/// Update the mirror plane parameters of an existing Mirror feature.
#[tauri::command]
pub fn update_mirror_params(
    feature_id: FeatureId,
    plane_nx: f64, plane_ny: f64, plane_nz: f64,
    plane_px: f64, plane_py: f64, plane_pz: f64,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    // Validate before snapshot (§15): wrong id/kind must not grow the undo stack.
    {
        let doc = state.lock_doc();
        let f = doc.get_feature(feature_id).ok_or("feature not found")?;
        if !matches!(f.kind, FeatureKind::Mirror { .. }) {
            return Err("feature is not a Mirror".into());
        }
    }
    state.snapshot();
    let mut doc = state.lock_doc();
    let feature = doc.get_feature_mut(feature_id).ok_or("feature not found")?;
    match &mut feature.kind {
        FeatureKind::Mirror {
            plane_nx: nx, plane_ny: ny, plane_nz: nz,
            plane_px: px, plane_py: py, plane_pz: pz, ..
        } => {
            *nx = plane_nx;
            *ny = plane_ny;
            *nz = plane_nz;
            *px = plane_px;
            *py = plane_py;
            *pz = plane_pz;
        }
        _ => return Err("feature is not a Mirror".into()),
    }
    incremental_regen_locked(&doc, &state, feature_id);
    Ok(())
}

// ── Integration tests ─────────────────────────────────────────────
// Design principle §11: every critical path must have at least one
// automated test. These cover the validate-then-mutate pattern (§15),
// cascade delete (§7), naming conventions (§10), and snapshot hygiene (§3).

#[cfg(test)]
mod tests {
    use super::*;
    use echi_core::sketch::Sketch;
    use echi_core::feature::{Feature, FeatureKind, PlaneDefinition, ExtrudeDirection};
    use echi_core::document::Document;
    use echi_core::{FeatureId, ParameterId};

    fn make_state() -> AppState {
        AppState::new()
    }

    // ── §15: Validate-then-mutate ──────────────────────────────

    #[test]
    fn add_feature_with_param_fails_cleanly_on_missing_dependency() {
        let state = make_state();
        // Try to add an extrude referencing a non-existent sketch (id 999).
        let result = add_feature_with_param(
            &state,
            "Extrude",
            10.0,
            |_id, param_id| FeatureKind::Extrude {
                sketch_id: FeatureId(999),
                distance: param_id,
                direction: ExtrudeDirection::OneSide,
                draft_angle_deg: 0.0,
                selected_regions: None,
            },
        );
        assert!(result.is_err(), "should fail on missing dependency");

        // Verify no orphaned parameter was added to the document.
        let doc = state.lock_doc();
        // The initial AppState has 1 feature (Sketch1) with no params.
        assert_eq!(doc.features.len(), 1,
            "document should still only have the initial sketch");
        // Verify the undo stack is NOT polluted (validate-then-mutate, §15).
        let mgr = state.lock_undo();
        assert!(!mgr.can_undo(), "failed add must not push an undo snapshot");
    }

    #[test]
    fn add_feature_kind_fails_cleanly_on_missing_dependency() {
        let state = make_state();
        let kind = FeatureKind::Extrude {
            sketch_id: FeatureId(999),
            distance: ParameterId(1),
            direction: ExtrudeDirection::OneSide,
            draft_angle_deg: 0.0,
            selected_regions: None,
        };
        let result = add_feature_kind(&state, kind);
        assert!(result.is_err(), "should fail on missing dependency");

        let mgr = state.lock_undo();
        assert!(!mgr.can_undo(), "failed add must not pollute undo stack");
    }

    // ── §7: Document-level cascade logic ───────────────────────

    #[test]
    fn dependents_of_tracks_downstream_features() {
        let mut doc = Document::new("test");
        doc.add_feature(Feature::new(
            FeatureId(1), "Sketch1",
            FeatureKind::Sketch { sketch: Sketch::new(), plane: PlaneDefinition::XY },
        ));
        doc.add_parameter("depth", 5.0);
        doc.add_feature(Feature::new(
            FeatureId(2), "Extrude2",
            FeatureKind::Extrude {
                sketch_id: FeatureId(1), distance: ParameterId(1),
                direction: ExtrudeDirection::OneSide, draft_angle_deg: 0.0,
                selected_regions: None,
            },
        ));
        // Extrude(2) depends on Sketch(1).
        let deps = doc.dependents_of(FeatureId(1));
        assert_eq!(deps, vec![FeatureId(2)]);
        assert!(doc.has_dependents(FeatureId(1)));
        assert!(!doc.has_dependents(FeatureId(2)));
    }

    #[test]
    fn cascade_remove_clears_transitive_dependents() {
        let mut doc = Document::new("test");
        doc.add_feature(Feature::new(
            FeatureId(1), "Sketch1",
            FeatureKind::Sketch { sketch: Sketch::new(), plane: PlaneDefinition::XY },
        ));
        doc.add_parameter("p1", 5.0);
        doc.add_feature(Feature::new(
            FeatureId(2), "Extrude2",
            FeatureKind::Extrude {
                sketch_id: FeatureId(1), distance: ParameterId(1),
                direction: ExtrudeDirection::OneSide, draft_angle_deg: 0.0,
                selected_regions: None,
            },
        ));
        doc.add_parameter("p2", 0.5);
        doc.add_feature(Feature::new(
            FeatureId(3), "Fillet3",
            FeatureKind::Fillet { target_id: FeatureId(2), radius: ParameterId(2), edges: vec![] },
        ));

        // Remove Sketch(1): dependents include [2, 3] transitively.
        assert!(doc.dependents_of(FeatureId(1)).contains(&FeatureId(2)));
        assert!(doc.dependents_of(FeatureId(2)).contains(&FeatureId(3)));
    }

    // ── §10: Naming convention ─────────────────────────────────

    #[test]
    fn default_feature_name_follows_convention() {
        let kind = FeatureKind::Extrude {
            sketch_id: FeatureId(1), distance: ParameterId(1),
            direction: ExtrudeDirection::OneSide, draft_angle_deg: 0.0,
            selected_regions: None,
        };
        assert_eq!(default_feature_name(&kind, FeatureId(3)), "Extrude3");

        let sketch_kind = FeatureKind::Sketch {
            sketch: Sketch::new(), plane: PlaneDefinition::XY,
        };
        assert_eq!(default_feature_name(&sketch_kind, FeatureId(5)), "Sketch5");

        let offset_kind = FeatureKind::Sketch {
            sketch: Sketch::new(),
            plane: PlaneDefinition::Offset {
                base: Box::new(PlaneDefinition::XY),
                distance: 10.0,
            },
        };
        assert_eq!(default_feature_name(&offset_kind, FeatureId(1)), "Sketch1");
    }

    // ── §3: Snapshot hygiene ───────────────────────────────────

    #[test]
    fn snapshot_if_sketch_noop_when_no_active_sketch() {
        let state = AppState::new();
        *state.lock_sketch() = None;
        let mgr = state.lock_undo();
        let can_undo_before = mgr.can_undo();
        drop(mgr);

        state.snapshot_if_sketch();
        let mgr = state.lock_undo();
        assert_eq!(mgr.can_undo(), can_undo_before,
            "snapshot_if_sketch with no active sketch should be no-op");
    }

    #[test]
    fn autosave_flag_defaults() {
        let state = make_state();
        assert!(state.autosave_enabled.load(std::sync::atomic::Ordering::Relaxed));
        assert!(!state.has_recovery_file.load(std::sync::atomic::Ordering::Relaxed));
    }

    // ── Deadlock regression: non-reentrant document mutex ──────
    //
    // `sync_sketch` early-returns when there is no render manager, which is
    // why unit tests never caught it: in production the manager is always
    // Some, and any command that called sync_sketch while still holding the
    // document guard re-locked the same std::sync::Mutex on the same thread
    // → the whole app froze (observed: "create sketch" hang via
    // set_active_sketch; undo had the same bug).
    //
    // These tests attach a real (GPU-less) RenderManager and run the
    // commands on a worker thread: a deadlock blocks the thread forever,
    // so the channel recv_timeout fires and fails the test with a clear
    // message instead of hanging the suite.

    /// Run `f` on a worker thread; panic if it doesn't finish in 5s.
    fn run_with_deadlock_timeout<F>(name: &str, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            f();
            let _ = tx.send(());
        });
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap_or_else(|_| {
            panic!("{name} deadlocked — a command re-locked the document mutex while holding it")
        });
    }

    fn make_render_state() -> AppState {
        AppState::new_with_render_manager(Some(std::sync::Arc::new(
            echi_wgpu::RenderManager::new(),
        )))
    }

    #[test]
    fn set_active_sketch_no_deadlock_with_render_manager() {
        let state = std::sync::Arc::new(make_render_state());
        let sketch_id = state.lock_doc().features[0].id();
        let s = state.clone();
        run_with_deadlock_timeout("set_active_sketch", move || {
            set_active_sketch_inner(sketch_id, &s);
        });
        assert_eq!(*state.lock_sketch(), Some(sketch_id));
    }

    #[test]
    fn undo_redo_no_deadlock_with_render_manager() {
        let state = std::sync::Arc::new(make_render_state());
        // Put a snapshot on the stack and mutate so undo has an effect.
        state.snapshot();
        state.lock_doc().features[0].set_name("Changed");

        let s = state.clone();
        run_with_deadlock_timeout("undo", move || {
            undo_inner(&s).expect("undo should succeed");
        });
        assert_eq!(state.lock_doc().features[0].name(), "Sketch1");

        let s = state.clone();
        run_with_deadlock_timeout("redo", move || {
            redo_inner(&s).expect("redo should succeed");
        });
        assert_eq!(state.lock_doc().features[0].name(), "Changed");
    }
}
