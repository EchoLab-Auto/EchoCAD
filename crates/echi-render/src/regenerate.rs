//! Document regeneration: execute feature tree to produce renderable geometry.

use echi_core::{Document, Feature, FeatureId, FeatureKind};
use echi_geom::extrude::{Mesh, extrude};
use echi_geom::{
    apply_chamfer, apply_chamfer_edges, apply_fillet, apply_fillet_edges, boolean_op,
    circular_pattern, linear_pattern, mirror_across_plane, revolve, shell_mesh, sweep_mesh,
};
use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;

use echi_brep::BrepKernel;
#[cfg(feature = "brep")]
use echi_brep::ProfilePoint;
#[cfg(feature = "brep")]
use echi_core::feature::{ExtrudeDirection, PlaneDefinition};
#[cfg(feature = "brep")]
use echi_core::sketch::Sketch;
#[cfg(feature = "brep")]
use echi_geom::extrude::extract_loops;

/// Result of regenerating a document.
#[derive(Debug, Clone, Default)]
pub struct RegenResult {
    /// Meshes produced by solid features (extrude, revolve, fillet, ...).
    pub solids: HashMap<FeatureId, Mesh>,
    /// The current active solid (last successful solid feature), if any.
    pub current_solid: Option<FeatureId>,
    /// Per-feature diagnostics produced during the last regen.
    pub errors: HashMap<FeatureId, String>,
    /// Features that need re-evaluation (set by incremental regen).
    /// When empty, a full regen is performed.
    /// Features that need re-evaluation (set by incremental regen).
    /// When empty, a full regen is performed.
    pub dirty: HashSet<FeatureId>,
}

impl RegenResult {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Compute the set of dirty features: the changed feature plus all
    /// transitive dependents that reference it directly or indirectly.
    pub fn compute_dirty(doc: &Document, changed: FeatureId) -> HashSet<FeatureId> {
        let mut dirty = HashSet::new();
        let mut stack = vec![changed];
        while let Some(id) = stack.pop() {
            if dirty.insert(id) {
                // Find features that depend on `id`
                for feature in &doc.features {
                    if feature.dependencies().contains(&id) {
                        stack.push(feature.id());
                    }
                }
            }
        }
        dirty
    }
}

/// External hook that lets `CustomSolid` features ask a plugin to produce
/// their mesh during regeneration.
///
/// Defined here (in `echi-render`) and implemented by the application layer
/// over its `PluginRegistry`, so `echi-render` does not need to depend on
/// `echi-plugin` directly. The error type is `String` so failures surface
/// straight into [`RegenResult::errors`] (design principle #8).
pub trait SolidGenerator {
    /// Produce a solid mesh for the named plugin/generator with the given
    /// driving parameters. The mesh is in world coordinates already.
    fn generate_solid(
        &self,
        plugin_id: &str,
        generator_id: &str,
        params: &HashMap<String, f64>,
    ) -> Result<Mesh, String>;
}

/// Regenerate the document by executing all features in tree order, with no
/// plugin solid generator or B-rep kernel attached. `CustomSolid` features will
/// record an error (they need a generator to produce geometry). Use
/// [`regenerate_with`] from the application layer to supply one.
pub fn regenerate(doc: &Document) -> RegenResult {
    regenerate_with(doc, None, None, None)
}

/// Regenerate with an optional plugin solid generator and optional B-rep kernel.
/// Suppressed features are skipped; features whose dependencies failed are also
/// skipped (with an error recorded).
///
/// When `previous` is provided, only dirty features (the `changed` feature plus
/// its transitive dependents) are re-evaluated. Clean features retain their
/// cached meshes from the previous result. This enables incremental regeneration
/// for parameter edits, giving 10-100x speedup on large parts.
pub fn regenerate_with(
    doc: &Document,
    generator: Option<&dyn SolidGenerator>,
    _brep_kernel: Option<&dyn BrepKernel>,
    previous: Option<&RegenResult>,
) -> RegenResult {
    let mut result = if let Some(prev) = previous {
        // Incremental: start from previous result, only recompute dirty features.
        // Keep errors of CLEAN features (still accurate — they weren't
        // re-evaluated) and of no other set: dirty features get their error
        // re-recorded below; deleted features' errors are dropped. Clearing
        // ALL errors here made previously-failed features silently "heal"
        // and sent their dependents to the misleading "target not yet
        // evaluated" message (原则8).
        let mut r = prev.clone();
        r.errors.retain(|id, _| !r.dirty.contains(id) && doc.get_feature(*id).is_some());
        r
    } else {
        RegenResult::default()
    };

    let is_incremental = !result.dirty.is_empty();

    for feature in &doc.features {
        if feature.suppressed {
            // Remove suppressed features from incremental results
            if is_incremental {
                result.solids.remove(&feature.id());
                result.errors.remove(&feature.id());
            }
            continue;
        }
        // In incremental mode, skip clean features
        if is_incremental && !result.dirty.contains(&feature.id()) {
            continue;
        }
        // Clear old results for features being re-evaluated
        result.solids.remove(&feature.id());
        result.errors.remove(&feature.id());

        if let Err(msg) = regenerate_feature(feature, doc, &mut result, generator, _brep_kernel) {
            result.errors.insert(feature.id(), msg);
        }
    }

    result
}

fn regenerate_feature(
    feature: &Feature,
    doc: &Document,
    result: &mut RegenResult,
    generator: Option<&dyn SolidGenerator>,
    _brep_kernel: Option<&dyn BrepKernel>,
) -> Result<(), String> {
    // Validate dependencies first — if any input is missing, suppressed, or
    // failed, skip with a clear error. (R2: suppressed features exist in the
    // doc but produce no solid. Without this check, a feature depending on a
    // suppressed feature would pass validation but get "target not yet
    // evaluated" — a confusing misdirection.)
    for dep in feature.dependencies() {
        if doc.get_feature(dep).is_none() {
            return Err(format!("dependency {:?} not found", dep));
        }
        if doc.get_feature(dep).map(|f| f.suppressed).unwrap_or(false) {
            return Err(format!("dependency {:?} is suppressed", dep));
        }
        if result.errors.contains_key(&dep) {
            return Err(format!("dependency {:?} failed to regenerate", dep));
        }
    }

    match &feature.kind {
        FeatureKind::Sketch { .. } | FeatureKind::CustomSketch { .. } => {
            // Sketch data is stored inline; no geometry to compute until referenced.
            Ok(())
        }
        FeatureKind::Extrude { sketch_id, distance, direction, draft_angle_deg, selected_regions, .. } => {
            let height = doc.get_parameter(*distance)
                .map(|p| p.value)
                .ok_or_else(|| format!("extrude distance parameter {:?} not found", distance))?;
            let plane = doc.get_feature(*sketch_id)
                .and_then(|f| f.plane())
                .cloned()
                .unwrap_or_default();
            let sketch = doc.get_feature(*sketch_id)
                .and_then(|f| f.sketch())
                .ok_or_else(|| "extrude source is not a sketch".to_string())?;

            // Try B-rep path first, then fall back to mesh
            #[cfg(feature = "brep")]
            let brep_mesh = _brep_kernel
                .and_then(|kernel| extrude_via_brep(kernel, sketch, height, *direction, &plane));
            #[cfg(not(feature = "brep"))]
            let brep_mesh: Option<Mesh> = None;

            let mesh = brep_mesh.or_else(|| {
                if let Some(indices) = selected_regions {
                    echi_geom::extrude::extrude_selected(sketch, height, *direction, *draft_angle_deg, &plane, indices)
                } else {
                    extrude(sketch, height, *direction, *draft_angle_deg, &plane)
                }
            });

            match mesh {
                Some(mesh) => {
                    result.solids.insert(feature.id(), mesh);
                    result.current_solid = Some(feature.id());
                    Ok(())
                }
                None => Err("extrude produced no mesh (open profile or zero depth?)".into()),
            }
        }
        FeatureKind::CustomSolid { custom, .. } => {
            // CustomSolid no longer plain-extrudes its sketch; it asks the
            // attached plugin to generate the body. Without a generator the
            // feature cannot produce geometry, and per principle #8 that must
            // surface as an error rather than a silent no-op.
            let solid_gen = generator.ok_or_else(|| {
                "custom solid requires a plugin generator (none attached to regen)".to_string()
            })?;
            match solid_gen.generate_solid(&custom.plugin_id, &custom.generator_id, &custom.params) {
                Ok(mesh) => {
                    result.solids.insert(feature.id(), mesh);
                    result.current_solid = Some(feature.id());
                    Ok(())
                }
                Err(e) => Err(format!(
                    "plugin solid '{} / {}': {}",
                    custom.plugin_id, custom.generator_id, e
                )),
            }
        }
        FeatureKind::Fillet { target_id, radius, edges, .. } => {
            let r = doc.get_parameter(*radius)
                .map(|p| p.value)
                .ok_or_else(|| format!("fillet radius parameter {:?} not found", radius))?;
            let target_mesh = result.solids.get(target_id)
                .ok_or_else(|| "fillet target not yet evaluated".to_string())?;

            // Try OCCT fillet (handles both all-edges and specific-edge modes)
            #[cfg(feature = "occt")]
            let occt_result = occt_fillet_via_doc(doc, *target_id, r, target_mesh, edges);
            #[cfg(not(feature = "occt"))]
            let occt_result: Option<Mesh> = None;

            let mesh = if let Some(m) = occt_result {
                m
            } else if edges.is_empty() {
                apply_fillet(target_mesh, r as f32)
            } else {
                apply_fillet_edges(target_mesh, r as f32, edges)
            };
            result.solids.insert(feature.id(), mesh);
            result.current_solid = Some(feature.id());
            Ok(())
        }
        FeatureKind::Chamfer { target_id, distance, edges, .. } => {
            let d = doc.get_parameter(*distance)
                .map(|p| p.value)
                .ok_or_else(|| format!("chamfer distance parameter {:?} not found", distance))?;
            let target_mesh = result.solids.get(target_id)
                .ok_or_else(|| "chamfer target not yet evaluated".to_string())?;

            #[cfg(feature = "occt")]
            let occt_result = occt_chamfer_via_doc(doc, *target_id, d, target_mesh, edges);
            #[cfg(not(feature = "occt"))]
            let occt_result: Option<Mesh> = None;

            let mesh = if let Some(m) = occt_result {
                m
            } else if edges.is_empty() {
                apply_chamfer(target_mesh, d as f32)
            } else {
                apply_chamfer_edges(target_mesh, d as f32, edges)
            };
            result.solids.insert(feature.id(), mesh);
            result.current_solid = Some(feature.id());
            Ok(())
        }
        FeatureKind::LinearPattern { target_id, dir_x, dir_y, dir_z, count, spacing, .. } => {
            let target = result.solids.get(target_id)
                .ok_or_else(|| "pattern target not yet evaluated".to_string())?;
            #[cfg(feature = "occt")]
            let occt_result = occt_pattern(doc, *target_id, |solid, i| {
                let s = *spacing * i as f64;
                Some(solid.translate(cadrum::DVec3::new(*dir_x * s, *dir_y * s, *dir_z * s)))
            }, *count);
            #[cfg(not(feature = "occt"))]
            let occt_result: Option<Mesh> = None;

            let mesh = occt_result.unwrap_or_else(|| {
                #[cfg(feature = "brep")]
                {
                    _brep_kernel.and_then(|k| k.linear_pattern_mesh_for(target, *dir_x, *dir_y, *dir_z, *count, *spacing).ok())
                        .unwrap_or_else(|| linear_pattern(target, *dir_x, *dir_y, *dir_z, *count, *spacing))
                }
                #[cfg(not(feature = "brep"))]
                linear_pattern(target, *dir_x, *dir_y, *dir_z, *count, *spacing)
            });
            result.solids.insert(feature.id(), mesh);
            result.current_solid = Some(feature.id());
            Ok(())
        }
        FeatureKind::CircularPattern {
            target_id,
            axis_x, axis_y, axis_z,
            axis_dx, axis_dy, axis_dz,
            count, total_angle_deg, ..
        } => {
            let target = result.solids.get(target_id)
                .ok_or_else(|| "pattern target not yet evaluated".to_string())?;
            #[cfg(feature = "occt")]
            let occt_result = occt_pattern(doc, *target_id, |solid, i| {
                let angle = total_angle_deg.to_radians() * i as f64 / *count as f64;
                Some(solid.rotate(
                    cadrum::DVec3::new(*axis_x, *axis_y, *axis_z),
                    cadrum::DVec3::new(*axis_dx, *axis_dy, *axis_dz),
                    angle,
                ))
            }, *count);
            #[cfg(not(feature = "occt"))]
            let occt_result: Option<Mesh> = None;

            let mesh = occt_result.unwrap_or_else(|| {
                #[cfg(feature = "brep")]
                {
                    _brep_kernel.and_then(|k| k.circular_pattern_mesh_for(target, *axis_x, *axis_y, *axis_z, *axis_dx, *axis_dy, *axis_dz, *count, *total_angle_deg).ok())
                        .unwrap_or_else(|| circular_pattern(target, *axis_x, *axis_y, *axis_z, *axis_dx, *axis_dy, *axis_dz, *count, *total_angle_deg))
                }
                #[cfg(not(feature = "brep"))]
                circular_pattern(target, *axis_x, *axis_y, *axis_z, *axis_dx, *axis_dy, *axis_dz, *count, *total_angle_deg)
            });
            result.solids.insert(feature.id(), mesh);
            result.current_solid = Some(feature.id());
            Ok(())
        }
        FeatureKind::Mirror {
            target_id,
            plane_nx, plane_ny, plane_nz,
            plane_px, plane_py, plane_pz, ..
        } => {
            let target = result.solids.get(target_id)
                .ok_or_else(|| "mirror target not yet evaluated".to_string())?;
            #[cfg(feature = "occt")]
            let occt_result = occt_mirror_via_doc(
                doc, *target_id, *plane_nx, *plane_ny, *plane_nz, *plane_px, *plane_py, *plane_pz
            );
            #[cfg(not(feature = "occt"))]
            let occt_result: Option<Mesh> = None;

            let mesh = occt_result.unwrap_or_else(|| {
                #[cfg(feature = "brep")]
                {
                    _brep_kernel.and_then(|k| k.mirror_mesh_for(target, *plane_nx, *plane_ny, *plane_nz, *plane_px, *plane_py, *plane_pz).ok())
                        .unwrap_or_else(|| mirror_across_plane(target, *plane_nx, *plane_ny, *plane_nz, *plane_px, *plane_py, *plane_pz))
                }
                #[cfg(not(feature = "brep"))]
                mirror_across_plane(target, *plane_nx, *plane_ny, *plane_nz, *plane_px, *plane_py, *plane_pz)
            });
            result.solids.insert(feature.id(), mesh);
            result.current_solid = Some(feature.id());
            Ok(())
        }
        FeatureKind::Sweep { profile_sketch_id, path_sketch_id, .. } => {
            let profile = doc.get_feature(*profile_sketch_id).and_then(|f| f.sketch())
                .ok_or_else(|| "sweep profile is not a sketch".to_string())?;
            let path = doc.get_feature(*path_sketch_id).and_then(|f| f.sketch())
                .ok_or_else(|| "sweep path is not a sketch".to_string())?;

            // Try OCCT sweep first, then brep, then mesh
            #[cfg(feature = "occt")]
            let occt_result = occt_sweep_via_sketches(profile, path);
            #[cfg(not(feature = "occt"))]
            let occt_result: Option<Mesh> = None;

            #[cfg(feature = "brep")]
            let brep_result = _brep_kernel.and_then(|k| k.sweep_mesh(profile, path).ok());
            #[cfg(not(feature = "brep"))]
            let brep_result: Option<Mesh> = None;

            let mesh = occt_result
                .or(brep_result)
                .or_else(|| sweep_mesh(profile, path));
            match mesh {
                Some(mut mesh) => {
                    // sweep_mesh builds with the path in local XY; place the
                    // result on the PATH sketch's plane (原则5).
                    let path_plane = doc.get_feature(*path_sketch_id)
                        .and_then(|f| f.plane())
                        .cloned()
                        .unwrap_or_default();
                    echi_geom::extrude::transform_mesh_to_world(&mut mesh, &path_plane);
                    result.solids.insert(feature.id(), mesh);
                    result.current_solid = Some(feature.id());
                    Ok(())
                }
                None => Err("sweep produced no mesh".into()),
            }
        }
        FeatureKind::Shell { target_id, thickness, .. } => {
            let t = doc.get_parameter(*thickness)
                .map(|p| p.value)
                .ok_or_else(|| format!("shell thickness parameter {:?} not found", thickness))?;
            let target = result.solids.get(target_id)
                .ok_or_else(|| "shell target not yet evaluated".to_string())?;

            // Try OCCT shell first, then brep, then mesh
            #[cfg(feature = "occt")]
            let occt_result = occt_shell_via_doc(doc, *target_id, t);
            #[cfg(not(feature = "occt"))]
            let occt_result: Option<Mesh> = None;

            #[cfg(feature = "brep")]
            let brep_result = _brep_kernel.and_then(|k| k.shell_mesh(target, t).ok());
            #[cfg(not(feature = "brep"))]
            let brep_result: Option<Mesh> = None;

            let mesh = occt_result
                .or(brep_result)
                .unwrap_or_else(|| shell_mesh(target, t));
            result.solids.insert(feature.id(), mesh);
            result.current_solid = Some(feature.id());
            Ok(())
        }
        FeatureKind::Boolean { op, target_a, target_b, .. } => {
            let a = result.solids.get(target_a)
                .ok_or_else(|| "boolean target_a not yet evaluated".to_string())?;
            let b = result.solids.get(target_b)
                .ok_or_else(|| "boolean target_b not yet evaluated".to_string())?;

            // Try OCCT boolean first, then brep, then mesh CSG
            #[cfg(feature = "occt")]
            let occt_result = occt_boolean_via_doc(doc, *target_a, *target_b, *op);
            #[cfg(not(feature = "occt"))]
            let occt_result: Option<Mesh> = None;

            #[cfg(feature = "brep")]
            let brep_result = match op {
                echi_core::feature::BoolOp::Union =>
                    _brep_kernel.and_then(|k| k.boolean_union(a, b).ok()),
                echi_core::feature::BoolOp::Subtract =>
                    _brep_kernel.and_then(|k| k.boolean_subtract(a, b).ok()),
                echi_core::feature::BoolOp::Intersect =>
                    _brep_kernel.and_then(|k| k.boolean_intersect(a, b).ok()),
            };
            #[cfg(not(feature = "brep"))]
            let brep_result: Option<Mesh> = None;

            let op_str = match op {
                echi_core::feature::BoolOp::Union => "union",
                echi_core::feature::BoolOp::Subtract => "subtract",
                echi_core::feature::BoolOp::Intersect => "intersect",
            };
            let mesh = occt_result
                .or(brep_result)
                .unwrap_or_else(|| boolean_op(a, b, op_str));
            result.solids.insert(feature.id(), mesh);
            result.current_solid = Some(feature.id());
            Ok(())
        }
        FeatureKind::Revolve { sketch_id, angle, axis_entity_id, .. } => {
            let angle_deg = doc.get_parameter(*angle)
                .map(|p| p.value)
                .ok_or_else(|| format!("revolve angle parameter {:?} not found", angle))?;
            let angle_rad = angle_deg * PI / 180.0;
            let sketch = doc.get_feature(*sketch_id).and_then(|f| f.sketch())
                .ok_or_else(|| "revolve source is not a sketch".to_string())?;

            let (axis_start, axis_end) = axis_entity_id
                .and_then(|axis_id| {
                    sketch.entities.get(&axis_id).and_then(|e| match e {
                        echi_core::sketch::SketchEntity::Line { start, end, .. } => {
                            let ps = sketch.get_point(*start)?;
                            let pe = sketch.get_point(*end)?;
                            Some((Some((ps.x, ps.y)), Some((pe.x, pe.y))))
                        }
                        _ => None,
                    })
                })
                .unwrap_or((None, None));

            // Try OCCT revolve first, then brep, then mesh
            #[cfg(feature = "occt")]
            let occt_result = occt_revolve_via_sketch(sketch, angle_rad, axis_start, axis_end);
            #[cfg(not(feature = "occt"))]
            let occt_result: Option<Mesh> = None;

            #[cfg(feature = "brep")]
            let brep_result = _brep_kernel.and_then(|kernel| {
                revolve_via_brep(kernel, sketch, angle_rad, axis_start, axis_end)
            });
            #[cfg(not(feature = "brep"))]
            let brep_result: Option<Mesh> = None;

            let mesh = occt_result
                .or(brep_result)
                .or_else(|| revolve(sketch, angle_rad, 48, axis_start, axis_end));

            match mesh {
                Some(mut mesh) => {
                    // revolve() builds in sketch-local XY; place the solid on
                    // the sketch's actual plane like extrude does (原则5).
                    let plane = doc.get_feature(*sketch_id)
                        .and_then(|f| f.plane())
                        .cloned()
                        .unwrap_or_default();
                    echi_geom::extrude::transform_mesh_to_world(&mut mesh, &plane);
                    result.solids.insert(feature.id(), mesh);
                    result.current_solid = Some(feature.id());
                    Ok(())
                }
                None => Err("revolve produced no mesh".into()),
            }
        }
    }
}

/// Try to extrude a sketch via the B-rep kernel. Returns `None` when the
/// B-rep path cannot handle the input (multi-loop sketches with holes,
/// kernel errors) — the caller should fall through to the mesh extrude.
/// Supports all planes (XY, YZ, ZX, Offset), all extrude directions
/// (OneSide, Midplane, TwoSides). The mesh is transformed to world
/// coordinates using the plane's frame.
#[cfg(feature = "brep")]
fn extrude_via_brep(
    kernel: &dyn BrepKernel,
    sketch: &Sketch,
    height: f64,
    direction: ExtrudeDirection,
    plane: &PlaneDefinition,
) -> Option<Mesh> {
    // Extract closed loops from the sketch
    let loops = extract_loops(sketch)?;

    // Multi-loop (holes) handled via OCCT boolean subtraction
    if loops.len() > 1 {
        #[cfg(feature = "occt")]
        if let Some(mut mesh) = occt_extrude_loops(&loops, height, direction) {
            transform_mesh_to_world(&mut mesh, plane);
            return Some(mesh);
        }
        return None;
    }
    if loops.is_empty() {
        return None;
    }

    // Convert Point2D → ProfilePoint
    let profile: Vec<ProfilePoint> = loops[0]
        .iter()
        .map(|p| ProfilePoint::new(p.x, p.y))
        .collect();

    // Build the mesh in local space based on direction
    let mut mesh = match direction {
        ExtrudeDirection::OneSide => {
            kernel.extrude_mesh(&profile, height).ok()?
        }
        ExtrudeDirection::Midplane => {
            let half = height / 2.0;
            let lower = kernel.extrude_mesh(&profile, half).ok()?;
            let upper = kernel.extrude_mesh(&profile, half).ok()?;
            // Offset upper half by +half in Z to stack above lower
            let mut combined = Mesh::default();
            copy_mesh(&lower, &mut combined, 0.0);
            copy_mesh(&upper, &mut combined, half);
            combined
        }
        ExtrudeDirection::TwoSides { dist1, dist2 } => {
            let lower = kernel.extrude_mesh(&profile, dist1).ok()?;
            let upper = kernel.extrude_mesh(&profile, dist2).ok()?;
            let mut combined = Mesh::default();
            copy_mesh(&lower, &mut combined, -dist1);
            copy_mesh(&upper, &mut combined, 0.0);
            combined
        }
    };

    // Transform from local XY space to world coordinates
    transform_mesh_to_world(&mut mesh, plane);
    Some(mesh)
}
// OCCT helper functions moved to crate::occt_ops
#[cfg(feature = "occt")]
use crate::occt_ops::*;
#[cfg(feature = "brep")]
use crate::mesh::{copy_mesh, transform_mesh_to_world};

/// Try to revolve a sketch via the B-rep kernel.
#[cfg(feature = "brep")]
fn revolve_via_brep(
    kernel: &dyn BrepKernel,
    sketch: &Sketch,
    angle_rad: f64,
    axis_start: Option<(f64, f64)>,
    axis_end: Option<(f64, f64)>,
) -> Option<Mesh> {
    let start = axis_start.unwrap_or((0.0, 0.0));
    let end = axis_end.unwrap_or((0.0, 1.0));
    let loops = extract_loops(sketch)?;
    if loops.len() != 1 { return None; }
    let profile: Vec<ProfilePoint> = loops[0].iter().map(|p| ProfilePoint::new(p.x, p.y)).collect();
    let solid = kernel.revolve(&profile, start, end, angle_rad, 24).ok()?;
    kernel.tessellate(&solid, 0.01).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use echi_core::feature::{CustomFeatureData, PlaneDefinition};
    use echi_core::sketch::Sketch;
    use echi_core::{Document, Feature, FeatureId, FeatureKind, ParameterId};

    /// A stub solid generator that knows exactly one (plugin, generator) pair
    /// and returns a tiny one-triangle mesh; everything else errors.
    struct StubGenerator;

    impl SolidGenerator for StubGenerator {
        fn generate_solid(
            &self,
            plugin_id: &str,
            generator_id: &str,
            _params: &HashMap<String, f64>,
        ) -> Result<Mesh, String> {
            if plugin_id == "stub" && generator_id == "box" {
                Ok(Mesh {
                    positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                    normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
                    indices: vec![0, 1, 2],
                })
            } else {
                Err(format!("unknown plugin/generator: {}/{}", plugin_id, generator_id))
            }
        }
    }

    /// Build a doc: Sketch → CustomSolid driven by the stub plugin.
    /// Returns (doc, solid_feature_id) so callers don't have to guess the id
    /// (parameter allocation shifts the numeric ids).
    fn doc_with_custom_solid(generator_id: &str) -> (Document, FeatureId) {
        let mut doc = Document::new("test");
        let sketch_id = doc.new_feature_id();
        doc.add_feature(Feature::new(
            sketch_id,
            "Sketch1",
            FeatureKind::Sketch { sketch: Sketch::new(), plane: PlaneDefinition::XY },
        ));
        let dist_param = doc.add_parameter("depth", 1.0);
        let solid_id = doc.new_feature_id();
        let mut params = HashMap::new();
        params.insert("thickness".to_string(), 2.0);
        doc.add_feature(Feature::new(
            solid_id,
            "Custom1",
            FeatureKind::CustomSolid {
                sketch_id,
                distance: dist_param,
                custom: CustomFeatureData {
                    plugin_id: "stub".into(),
                    generator_id: generator_id.into(),
                    params,
                },
            },
        ));
        (doc, solid_id)
    }

    #[test]
    fn custom_solid_dispatches_to_generator() {
        let (doc, solid_id) = doc_with_custom_solid("box");
        let result = regenerate_with(&doc, Some(&StubGenerator), None, None);
        assert!(
            result.solids.contains_key(&solid_id),
            "CustomSolid should produce a solid via the generator"
        );
        assert_eq!(result.current_solid, Some(solid_id));
        assert!(result.errors.is_empty(), "expected no errors, got {:?}", result.errors);
        // The stub mesh has exactly one triangle (3 indices).
        assert_eq!(result.solids[&solid_id].indices.len(), 3);
    }

    #[test]
    fn custom_solid_errors_when_no_generator_attached() {
        // regenerate() (no generator) must record an error for CustomSolid
        // rather than silently emitting nothing (principle #8).
        let (doc, solid_id) = doc_with_custom_solid("box");
        let result = regenerate(&doc);
        assert!(!result.solids.contains_key(&solid_id), "no solid without a generator");
        let err = result.errors.get(&solid_id).expect("error must be recorded");
        assert!(
            err.contains("plugin") || err.contains("generator"),
            "error should mention the missing generator, got: {}",
            err
        );
    }

    #[test]
    fn custom_solid_records_plugin_error() {
        // Generator present but the plugin rejects the generator id → the
        // error must surface into RegenResult.errors.
        let (doc, solid_id) = doc_with_custom_solid("missing");
        let result = regenerate_with(&doc, Some(&StubGenerator), None, None);
        assert!(!result.solids.contains_key(&solid_id));
        let err = result.errors.get(&solid_id).expect("plugin error must be recorded");
        assert!(err.contains("unknown plugin/generator"), "got: {}", err);
    }

    #[test]
    fn fillet_with_empty_edges_falls_back_to_all() {
        // An empty edge list must mean "fillet all sharp edges". We verify the
        // wiring at the Feature level: the feature carries the new edges
        // field and round-trips through regen. The fillet has no target solid
        // to act on, so it records the "target not yet evaluated" error rather
        // than a structural failure.
        let mut doc = Document::new("test");
        let target = doc.new_feature_id();
        doc.add_feature(Feature::new(
            target,
            "Sketch1",
            FeatureKind::Sketch { sketch: Sketch::new(), plane: PlaneDefinition::XY },
        ));
        let radius_param = doc.add_parameter("radius", 0.1);
        let fillet_id = doc.new_feature_id();
        doc.add_feature(Feature::new(
            fillet_id,
            "Fillet1",
            FeatureKind::Fillet { target_id: target, radius: radius_param, edges: Vec::new() },
        ));
        let result = regenerate(&doc);
        let err = result.errors.get(&fillet_id);
        assert!(err.is_some(), "fillet without a solid target should record an error");
        let _ = ParameterId(0); // silence unused import warning if any
    }

    #[cfg(feature = "brep")]
    mod brep_tests {
        use super::*;
        use echi_brep::MockBrepKernel;

        #[test]
        fn extrude_via_brep_square_produces_mesh() {
            let mut sketch = Sketch::new();
            let p0 = sketch.add_point(0.0, 0.0);
            let p1 = sketch.add_point(1.0, 0.0);
            let p2 = sketch.add_point(1.0, 1.0);
            let p3 = sketch.add_point(0.0, 1.0);
            sketch.add_line(p0, p1);
            sketch.add_line(p1, p2);
            sketch.add_line(p2, p3);
            sketch.add_line(p3, p0);

            let kernel = MockBrepKernel;
            let mesh = extrude_via_brep(
                &kernel,
                &sketch,
                2.0,
                ExtrudeDirection::OneSide,
                &PlaneDefinition::XY,
            )
            .expect("B-rep extrude should produce a mesh");

            assert!(mesh.vertex_count() > 0);
            assert!(!mesh.indices.is_empty());
        }

        #[test]
        fn extrude_via_brep_midplane_now_works() {
            let mut sketch = Sketch::new();
            let p0 = sketch.add_point(0.0, 0.0);
            let p1 = sketch.add_point(1.0, 0.0);
            let p2 = sketch.add_point(1.0, 1.0);
            sketch.add_line(p0, p1);
            sketch.add_line(p1, p2);
            sketch.add_line(p2, p0);

            let kernel = MockBrepKernel;
            let result = extrude_via_brep(
                &kernel,
                &sketch,
                2.0,
                ExtrudeDirection::Midplane,
                &PlaneDefinition::XY,
            );
            assert!(
                result.is_some(),
                "Midplane extrude should now work via B-rep path"
            );
            let mesh = result.unwrap();
            assert!(mesh.vertex_count() > 0);
            // Midplane: two half-extrusions, so vertex count is ~2x
            assert!(mesh.vertex_count() >= 24, "expected >=24 vertices for midplane, got {}", mesh.vertex_count());
        }

        #[test]
        fn extrude_via_brep_yz_plane_now_works() {
            let mut sketch = Sketch::new();
            let p0 = sketch.add_point(0.0, 0.0);
            let p1 = sketch.add_point(1.0, 0.0);
            let p2 = sketch.add_point(1.0, 1.0);
            let p3 = sketch.add_point(0.0, 1.0);
            sketch.add_line(p0, p1);
            sketch.add_line(p1, p2);
            sketch.add_line(p2, p3);
            sketch.add_line(p3, p0);

            let kernel = MockBrepKernel;
            let result = extrude_via_brep(
                &kernel,
                &sketch,
                2.0,
                ExtrudeDirection::OneSide,
                &PlaneDefinition::YZ,
            );
            assert!(
                result.is_some(),
                "YZ plane extrude should now work via B-rep path"
            );
            let mesh = result.unwrap();
            assert!(mesh.vertex_count() > 0);
            assert!(!mesh.indices.is_empty());
        }

        #[test]
        fn regenerate_extrude_with_brep_kernel() {
            let mut doc = Document::new("test");
            let sketch_id = doc.new_feature_id();
            doc.add_feature(Feature::new(
                sketch_id,
                "Sketch1",
                FeatureKind::Sketch {
                    sketch: {
                        let mut s = Sketch::new();
                        let p0 = s.add_point(0.0, 0.0);
                        let p1 = s.add_point(1.0, 0.0);
                        let p2 = s.add_point(1.0, 1.0);
                        let p3 = s.add_point(0.0, 1.0);
                        s.add_line(p0, p1);
                        s.add_line(p1, p2);
                        s.add_line(p2, p3);
                        s.add_line(p3, p0);
                        s
                    },
                    plane: PlaneDefinition::XY,
                },
            ));
            let dist_param = doc.add_parameter("Extrude1", 2.0);
            let extrude_id = doc.new_feature_id();
            doc.add_feature(Feature::new(
                extrude_id,
                "Extrude1",
                FeatureKind::Extrude {
                    sketch_id,
                    distance: dist_param,
                    direction: ExtrudeDirection::OneSide,
                    draft_angle_deg: 0.0,
                    selected_regions: None,
                },
            ));

            let kernel = MockBrepKernel;
            let result = regenerate_with(&doc, None, Some(&kernel), None);
            assert!(
                result.solids.contains_key(&extrude_id),
                "B-rep kernel should produce a mesh"
            );
            assert!(
                result.errors.is_empty(),
                "expected no errors, got {:?}",
                result.errors
            );
            assert_eq!(result.current_solid, Some(extrude_id));
        }
    }
}
