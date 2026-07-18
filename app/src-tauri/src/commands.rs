//! EchoCAD application state and Tauri commands.

use echi_core::{
    BoolOp, CustomFeatureData, Document, ExtrudeDirection, Feature, FeatureId, FeatureKind,
    ParameterId, PlaneDefinition, Sketch,
    sketch::{Constraint, EntityId, SketchEntity, SketchPoint},
};
use echi_plugin::PluginRegistry;
use echi_render::{RegenResult, RenderMesh, regenerate};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

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
}

impl AppState {
    pub fn new() -> Self {
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
        }
    }

    fn snapshot(&self) {
        let doc = self.document.lock().unwrap();
        let active = *self.active_sketch.lock().unwrap();
        self.undo_manager.lock().unwrap().push_snapshot(&doc, active);
    }

    fn with_active_sketch_mut<T>(&self, f: impl FnOnce(&mut Sketch) -> T) -> Option<T> {
        let mut doc = self.document.lock().unwrap();
        let active = *self.active_sketch.lock().unwrap();
        let id = active?;
        let feature = doc.get_feature_mut(id)?;
        match &mut feature.kind {
            FeatureKind::Sketch { sketch, .. } | FeatureKind::CustomSketch { sketch, .. } => Some(f(sketch)),
            _ => None,
        }
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
    /// For sketch features, the plane the sketch lives on ("xy" / "yz" / "zx").
    pub plane: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInfo {
    pub id: ParameterId,
    pub name: String,
    pub value: f64,
    #[serde(default)] pub readonly: bool,
}

// ── Regeneration helpers ─────────────────────────────────────────

fn regenerate_state(state: &AppState) {
    let doc = state.document.lock().unwrap();
    let result = regenerate(&doc);
    *state.regen_result.lock().unwrap() = result;
}

/// Regenerate from an already-locked document reference. Use this
/// whenever the caller already holds the document Mutex to avoid deadlock.
fn regen_locked(doc: &Document, state: &AppState) {
    let result = regenerate(doc);
    *state.regen_result.lock().unwrap() = result;
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
            params.push(ParameterInfo { id: ParameterId(0), name: "count".into(), value: *count as f64, readonly: false });
            params.push(ParameterInfo { id: ParameterId(0), name: "spacing".into(), value: *spacing, readonly: false });
            params.push(ParameterInfo { id: ParameterId(0), name: "dir_x".into(), value: *dir_x, readonly: false });
            params.push(ParameterInfo { id: ParameterId(0), name: "dir_y".into(), value: *dir_y, readonly: false });
            params.push(ParameterInfo { id: ParameterId(0), name: "dir_z".into(), value: *dir_z, readonly: false });
            ("LinearPattern".to_string(), params)
        }
        FeatureKind::CircularPattern { count, total_angle_deg, .. } => {
            let mut params = Vec::new();
            params.push(ParameterInfo { id: ParameterId(0), name: "count".into(), value: *count as f64, readonly: false });
            params.push(ParameterInfo { id: ParameterId(0), name: "total_angle_deg".into(), value: *total_angle_deg, readonly: false });
            ("CircularPattern".to_string(), params)
        }
        FeatureKind::Mirror { .. } => ("Mirror".to_string(), Vec::new()),
        FeatureKind::Sweep { .. } => ("Sweep".to_string(), Vec::new()),
        FeatureKind::Boolean { .. } => ("Boolean".to_string(), Vec::new()),
        FeatureKind::CustomSolid { distance, .. } => {
            ("CustomSolid".to_string(), parameter_info(doc, *distance).into_iter().collect())
        }
    };

    let plane = f.plane().map(|p| match p {
        PlaneDefinition::YZ => "yz".to_string(),
        PlaneDefinition::ZX => "zx".to_string(),
        PlaneDefinition::Offset { .. } => "offset".to_string(),
        PlaneDefinition::XY => "xy".to_string(),
    });

    FeatureNode {
        id: f.id(),
        name: f.name.clone(),
        feature_type: ft,
        parameters: params,
        suppressed: f.suppressed,
        has_dependents: doc.has_dependents(f.id()),
        errors: errors.get(&f.id()).cloned(),
        plane,
    }
}

// ── Helper: append feature with auto-named parameter ──────────────

/// Locks the doc, snapshots the undo state, allocates a parameter,
/// creates the feature with the given kind, and regenerates.
/// Validates that all dependencies exist before committing.
fn add_feature_with_param(
    state: &AppState,
    param_name: &str,
    param_value: f64,
    kind_fn: impl FnOnce(FeatureId, ParameterId) -> FeatureKind,
) -> Result<FeatureId, String> {
    state.snapshot();
    let mut doc = state.document.lock().unwrap();
    let next = doc.next_id();
    let param_id = doc.add_parameter(format!("{}{}", param_name, next), param_value);
    let id = doc.new_feature_id();
    let kind = kind_fn(id, param_id);

    // Validate dependencies before committing
    let probe = Feature::new(id, "probe", kind.clone());
    for dep in probe.dependencies() {
        if doc.get_feature(dep).is_none() {
            return Err(format!("Dependency {:?} does not exist", dep));
        }
    }

    let name = default_feature_name(&kind, id);
    doc.add_feature(Feature::new(id, name, kind));
    regen_locked(&doc, &state);
    Ok(id)
}

fn default_feature_name(kind: &FeatureKind, id: FeatureId) -> String {
    let base = match kind {
        FeatureKind::Sketch { .. } => "Sketch",
        FeatureKind::CustomSketch { .. } => "Custom",
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
    state.snapshot();
    let mut doc = state.document.lock().unwrap();
    let id = doc.new_feature_id();

    // Validate dependencies before committing
    let probe = Feature::new(id, "probe", kind.clone());
    for dep in probe.dependencies() {
        if doc.get_feature(dep).is_none() {
            return Err(format!("Dependency {:?} does not exist", dep));
        }
    }

    let name = default_feature_name(&kind, id);
    doc.add_feature(Feature::new(id, name, kind));
    regen_locked(&doc, &state);
    Ok(id)
}

// ── Feature queries ──────────────────────────────────────────────

#[tauri::command]
pub fn get_features(state: tauri::State<AppState>) -> Vec<FeatureNode> {
    let doc = state.document.lock().unwrap();
    let errors = &state.regen_result.lock().unwrap().errors;
    doc.features.iter().map(|f| feature_to_node(f, &doc, errors)).collect()
}

// ── Feature creation commands ────────────────────────────────────

#[tauri::command]
pub fn add_sketch_feature(plane: String, state: tauri::State<AppState>) -> FeatureId {
    state.snapshot();
    let mut doc = state.document.lock().unwrap();
    let id = doc.new_feature_id();
    let p = match plane.as_str() {
        "yz" => PlaneDefinition::YZ,
        "zx" => PlaneDefinition::ZX,
        _ => PlaneDefinition::XY,
    };
    doc.add_feature(Feature::new(
        id,
        format!("Sketch{}", id.0),
        FeatureKind::Sketch { plane: p, sketch: Sketch::new() },
    ));
    *state.active_sketch.lock().unwrap() = Some(id);
    regen_locked(&doc, &state);
    id
}

#[tauri::command]
pub fn set_active_sketch(id: FeatureId, state: tauri::State<AppState>) {
    // Note: no snapshot. Switching the active sketch is a UI state change,
    // not a document mutation — it should not pollute the undo history.
    let doc = state.document.lock().unwrap();
    if let Some(f) = doc.get_feature(id) {
        if f.is_sketch() {
            *state.active_sketch.lock().unwrap() = Some(id);
        }
    }
}

#[tauri::command]
pub fn add_extrude_feature(
    sketch_id: FeatureId,
    direction: String,
    dist2: f64,
    draft_angle_deg: f64,
    depth: f64,
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
    add_feature_with_param(&state, "Fillet", radius, move |_id, param_id| {
        FeatureKind::Fillet { target_id, radius: param_id }
    })
}

#[tauri::command]
pub fn add_chamfer_feature(target_id: FeatureId, distance: f64, state: tauri::State<AppState>) -> Result<FeatureId, String> {
    add_feature_with_param(&state, "Chamfer", distance, move |_id, param_id| {
        FeatureKind::Chamfer { target_id, distance: param_id }
    })
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
    state.snapshot();
    let mut doc = state.document.lock().unwrap();
    if let Some(p) = doc.get_parameter_mut(id) {
        p.value = value;
    }
    regen_locked(&doc, &state);
}

#[tauri::command]
pub fn rename_feature(id: FeatureId, name: String, state: tauri::State<AppState>) {
    state.snapshot();
    let mut doc = state.document.lock().unwrap();
    if let Some(f) = doc.get_feature_mut(id) {
        f.set_name(name);
    }
    regen_locked(&doc, &state);
}

#[tauri::command]
pub fn set_feature_suppressed(id: FeatureId, suppressed: bool, state: tauri::State<AppState>) {
    state.snapshot();
    let mut doc = state.document.lock().unwrap();
    if let Some(f) = doc.get_feature_mut(id) {
        f.suppressed = suppressed;
    }
    regen_locked(&doc, &state);
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
    state.snapshot();
    let mut doc = state.document.lock().unwrap();
    let dependents = doc.dependents_of(id);
    if !dependents.is_empty() && !cascade {
        // Roll back the snapshot we just took — we don't want to record
        // an undo point for a no-op.
        state.undo_manager.lock().unwrap().undo_stack.pop();
        let names: Vec<String> = dependents.iter()
            .filter_map(|d| doc.get_feature(*d).map(|f| f.name().to_string()))
            .collect();
        return Err(format!("dependents:{}", names.join(",")));
    }

    // Cascade: repeatedly remove features that depend on this one.
    if cascade {
        let mut to_remove: Vec<FeatureId> = vec![id];
        let mut removed: std::collections::HashSet<FeatureId> = std::collections::HashSet::new();
        while let Some(rid) = to_remove.pop() {
            if removed.contains(&rid) {
                continue;
            }
            for dep in doc.dependents_of(rid) {
                if !removed.contains(&dep) {
                    to_remove.push(dep);
                }
            }
            removed.insert(rid);
        }
        for rid in &removed {
            doc.remove_feature(*rid);
        }
    } else {
        doc.remove_feature(id);
    }

    let mut active = state.active_sketch.lock().unwrap();
    if *active == Some(id) || (cascade && active.map(|a| doc.get_feature(a).is_none()).unwrap_or(false)) {
        *active = doc.features.iter().find_map(|f| if f.is_sketch() { Some(f.id()) } else { None });
    }
    regen_locked(&doc, &state);
    Ok(())
}

#[tauri::command]
pub fn clear_document(state: tauri::State<'_, AppState>) {
    state.snapshot();
    let mut doc = state.document.lock().unwrap();
    *doc = Document::new("Part1");
    let id = doc.new_feature_id();
    doc.add_feature(Feature::new(
        id,
        "Sketch1",
        FeatureKind::Sketch { sketch: Sketch::new(), plane: PlaneDefinition::default() },
    ));
    *state.active_sketch.lock().unwrap() = Some(id);
    regen_locked(&doc, &state);
}

// ── Sketch entity operations ─────────────────────────────────────

#[tauri::command]
pub fn get_sketch_entities(state: tauri::State<AppState>) -> Vec<RenderEntity> {
    let doc = state.document.lock().unwrap();
    let id = match *state.active_sketch.lock().unwrap() {
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
    let doc = state.document.lock().unwrap();
    let id = match *state.active_sketch.lock().unwrap() {
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
    state.snapshot();
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
    state.snapshot();
    state.with_active_sketch_mut(|s| s.add_point(x, y))
}
#[tauri::command]
pub fn add_line(start: EntityId, end: EntityId, state: tauri::State<AppState>) -> Option<EntityId> {
    state.snapshot();
    state.with_active_sketch_mut(|s| s.add_line(start, end))
}
#[tauri::command]
pub fn add_circle(center: EntityId, radius: f64, state: tauri::State<AppState>) -> Option<EntityId> {
    state.snapshot();
    state.with_active_sketch_mut(|s| s.add_circle(center, radius))
}
#[tauri::command]
pub fn add_arc(center: EntityId, radius: f64, start_angle: f64, end_angle: f64, state: tauri::State<AppState>) -> Option<EntityId> {
    state.snapshot();
    state.with_active_sketch_mut(|s| s.add_arc(center, radius, start_angle, end_angle))
}
#[tauri::command]
pub fn add_spline(control_points: Vec<EntityId>, state: tauri::State<AppState>) -> Option<EntityId> {
    state.snapshot();
    state.with_active_sketch_mut(|s| s.add_spline(control_points))
}
#[tauri::command]
pub fn add_ellipse(center: EntityId, major_axis_end: EntityId, ratio: f64, state: tauri::State<AppState>) -> Option<EntityId> {
    state.snapshot();
    state.with_active_sketch_mut(|s| s.add_ellipse(center, major_axis_end, ratio))
}
#[tauri::command]
pub fn add_constraint(constraint: Constraint, state: tauri::State<AppState>) {
    state.snapshot();
    state.with_active_sketch_mut(|s| s.add_constraint(constraint));
}

/// Update the numeric value of an existing constraint that matches
/// (variant, target entities). Used by the dimension overlay to edit a
/// driving dimension in place — avoids accumulating duplicate constraints
/// that fight each other in the solver.
#[tauri::command]
pub fn update_constraint_value(constraint: Constraint, state: tauri::State<AppState>) -> bool {
    state.snapshot();
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
pub fn solve_sketch(state: tauri::State<AppState>) {
    state.snapshot();
    state.with_active_sketch_mut(|s| { echi_geom::solve(s, 100, 1e-6); });
}

#[tauri::command]
pub fn move_point(id: EntityId, x: f64, y: f64, state: tauri::State<AppState>) {
    state.snapshot();
    state.with_active_sketch_mut(|s| {
        if let Some(p) = s.get_point_mut(id) {
            p.x = x;
            p.y = y;
        }
    });
}

#[tauri::command]
pub fn update_entity_prop(id: EntityId, prop: String, value: f64, state: tauri::State<AppState>) -> bool {
    state.snapshot();
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
    state.snapshot();
    state.with_active_sketch_mut(|s| !s.delete_entity_cascade(id).is_empty()).unwrap_or(false)
}

#[tauri::command]
pub fn clear_sketch(state: tauri::State<AppState>) {
    state.snapshot();
    state.with_active_sketch_mut(|s| *s = Sketch::new());
    regenerate_state(&state);
}

// ── Solid mesh queries ───────────────────────────────────────────

#[tauri::command]
pub fn get_solid_mesh(state: tauri::State<AppState>) -> Option<RenderMesh> {
    let result = state.regen_result.lock().unwrap();
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
    state: tauri::State<AppState>,
) -> Option<RenderMesh> {
    let doc = state.document.lock().unwrap();
    let feature = doc.get_feature(sketch_id)?;
    let sketch = feature.sketch()?;
    let plane = feature.plane().cloned().unwrap_or_default();

    let dir = match direction.as_str() {
        "midplane" => ExtrudeDirection::Midplane,
        "two_sides" => ExtrudeDirection::TwoSides { dist1: depth.max(0.1), dist2: dist2.max(0.1) },
        _ => ExtrudeDirection::OneSide,
    };

    let mesh = echi_geom::extrude(sketch, if depth > 0.0 { depth } else { 1.0 }, dir, draft_angle_deg, &plane)?;
    Some(RenderMesh::from(&mesh))
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
    let doc = state.document.lock().unwrap();
    let result = state.regen_result.lock().unwrap();
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
    let result = state.regen_result.lock().unwrap();
    let mut errors: Vec<(FeatureId, String)> = result.errors.iter().map(|(k, v)| (*k, v.clone())).collect();
    errors.sort_by_key(|(id, _)| id.0);
    errors
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
        let doc = state.document.lock().unwrap();
        echi_io::save_project(&*doc, &path).map_err(|e| e.to_string())?;
        record_recent_file(&app, &path);
        Ok(path.to_string_lossy().to_string())
    } else {
        Err("No file selected".into())
    }
}

/// Save the document to a specific path (no dialog). Used for "Save" when
/// the project is already on disk, or by tests.
#[tauri::command]
pub fn save_project_to(state: tauri::State<'_, AppState>, path: String) -> Result<(), String> {
    let p: std::path::PathBuf = path.into();
    let doc = state.document.lock().unwrap();
    echi_io::save_project(&*doc, &p).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_project_cmd(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    use tauri_plugin_dialog::DialogExt;
    let path = app.dialog().file().add_filter("EchoCAD Project", &["echi"]).blocking_pick_file();
    if let Some(path) = path {
        let path = file_path_to_path(path).ok_or("Invalid file path")?;
        let doc = echi_io::load_project(&path).map_err(|e| e.to_string())?;
        {
            let mut state_doc = state.document.lock().unwrap();
            *state_doc = doc;
        }
        let mut active = state.active_sketch.lock().unwrap();
        *active = state.document.lock().unwrap().features.iter()
            .find_map(|f| if f.is_sketch() { Some(f.id()) } else { None });
        regenerate_state(&state);
        record_recent_file(&app, &path);
        Ok(())
    } else {
        Err("No file selected".into())
    }
}

#[tauri::command]
pub fn load_project_from(state: tauri::State<'_, AppState>, path: String) -> Result<(), String> {
    let p: std::path::PathBuf = path.into();
    let doc = echi_io::load_project(&p).map_err(|e| e.to_string())?;
    {
        let mut state_doc = state.document.lock().unwrap();
        *state_doc = doc;
    }
    let mut active = state.active_sketch.lock().unwrap();
    *active = state.document.lock().unwrap().features.iter()
        .find_map(|f| if f.is_sketch() { Some(f.id()) } else { None });
    regenerate_state(&state);
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

#[tauri::command]
pub fn export_stl(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let path = app.dialog().file().add_filter("STL", &["stl"]).blocking_save_file();
    if let Some(path) = path {
        let path = file_path_to_path(path).ok_or("Invalid file path")?;
        let result = state.regen_result.lock().unwrap();
        let mesh = result.current_solid
            .and_then(|id| result.solids.get(&id))
            .ok_or("No solid to export")?;
        let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        echi_io::export_stl_ascii(mesh, &mut file).map_err(|e| e.to_string())?;
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
        let result = state.regen_result.lock().unwrap();
        let mesh = result.current_solid
            .and_then(|id| result.solids.get(&id))
            .ok_or("No solid to export")?;
        let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        echi_io::export_obj(mesh, &mut file).map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().to_string())
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
    let mut doc = state.document.lock().unwrap();
    let id = doc.new_feature_id();
    let gen_name = state.plugin_registry.find_generator(&plugin_id, &generator_id)
        .map(|g| g.name)
        .unwrap_or_else(|| generator_id.clone());
    doc.add_feature(Feature::new(
        id,
        format!("{}_{}", gen_name, id.0),
        FeatureKind::CustomSketch {
            sketch,
            custom: CustomFeatureData { plugin_id, generator_id, params },
            plane: PlaneDefinition::default(),
        },
    ));
    *state.active_sketch.lock().unwrap() = Some(id);
    regen_locked(&doc, &state);
    Ok(id)
}

// ── Undo / Redo ──────────────────────────────────────────────────

#[tauri::command]
pub fn undo(state: tauri::State<AppState>) -> Result<bool, String> {
    let mut doc = state.document.lock().unwrap();
    let active = *state.active_sketch.lock().unwrap();
    let mut mgr = state.undo_manager.lock().unwrap();
    if !mgr.can_undo() {
        return Ok(false);
    }
    if let Some((restored, ra)) = mgr.undo(&doc, active) {
        *doc = restored;
        let valid = ra
            .filter(|&id| doc.get_feature(id).map(|f| f.is_sketch()).unwrap_or(false))
            .or_else(|| doc.features.iter().find_map(|f| if f.is_sketch() { Some(f.id()) } else { None }));
        *state.active_sketch.lock().unwrap() = valid;
        regen_locked(&doc, &state);
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub fn redo(state: tauri::State<AppState>) -> Result<bool, String> {
    let mut doc = state.document.lock().unwrap();
    let active = *state.active_sketch.lock().unwrap();
    let mut mgr = state.undo_manager.lock().unwrap();
    if !mgr.can_redo() {
        return Ok(false);
    }
    if let Some((restored, ra)) = mgr.redo(&doc, active) {
        *doc = restored;
        let valid = ra
            .filter(|&id| doc.get_feature(id).map(|f| f.is_sketch()).unwrap_or(false))
            .or_else(|| doc.features.iter().find_map(|f| if f.is_sketch() { Some(f.id()) } else { None }));
        *state.active_sketch.lock().unwrap() = valid;
        regen_locked(&doc, &state);
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub fn can_undo_redo(state: tauri::State<AppState>) -> (bool, bool) {
    let mgr = state.undo_manager.lock().unwrap();
    (mgr.can_undo(), mgr.can_redo())
}
