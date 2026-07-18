//! Document regeneration: execute feature tree to produce renderable geometry.

use echi_core::{Document, ExtrudeDirection, Feature, FeatureId, FeatureKind};
use echi_geom::extrude::{Mesh, extrude};
use echi_geom::{apply_chamfer, apply_fillet, boolean_op, circular_pattern, linear_pattern, mirror_across_plane, revolve, sweep_mesh, shell_mesh};
use std::collections::HashMap;
use std::f64::consts::PI;

/// Result of regenerating a document.
#[derive(Debug, Clone, Default)]
pub struct RegenResult {
    /// Meshes produced by solid features (extrude, revolve, fillet, ...).
    pub solids: HashMap<FeatureId, Mesh>,
    /// The current active solid (last successful solid feature), if any.
    pub current_solid: Option<FeatureId>,
    /// Per-feature diagnostics produced during the last regen.
    pub errors: HashMap<FeatureId, String>,
}

impl RegenResult {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Regenerate the document by executing all features in tree order.
/// Suppressed features are skipped; features whose dependencies failed
/// are also skipped (with an error recorded).
pub fn regenerate(doc: &Document) -> RegenResult {
    let mut result = RegenResult::default();

    for feature in &doc.features {
        if feature.suppressed {
            continue;
        }
        if let Err(msg) = regenerate_feature(feature, doc, &mut result) {
            result.errors.insert(feature.id(), msg);
        }
    }

    result
}

fn regenerate_feature(
    feature: &Feature,
    doc: &Document,
    result: &mut RegenResult,
) -> Result<(), String> {
    // Validate dependencies first — if any input is missing or failed, skip.
    for dep in feature.dependencies() {
        if doc.get_feature(dep).is_none() {
            return Err(format!("dependency {:?} not found", dep));
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
        FeatureKind::Extrude { sketch_id, distance, direction, draft_angle_deg, .. } => {
            let height = doc.get_parameter(*distance).map(|p| p.value).unwrap_or(1.0);
            let plane = doc.get_feature(*sketch_id)
                .and_then(|f| f.plane())
                .cloned()
                .unwrap_or_default();
            let sketch = doc.get_feature(*sketch_id)
                .and_then(|f| f.sketch())
                .ok_or_else(|| "extrude source is not a sketch".to_string())?;

            match extrude(sketch, height, *direction, *draft_angle_deg, &plane) {
                Some(mesh) => {
                    result.solids.insert(feature.id(), mesh);
                    result.current_solid = Some(feature.id());
                    Ok(())
                }
                None => Err("extrude produced no mesh (open profile or zero depth?)".into()),
            }
        }
        FeatureKind::CustomSolid { sketch_id, distance, .. } => {
            let height = doc.get_parameter(*distance).map(|p| p.value).unwrap_or(1.0);
            let plane = doc.get_feature(*sketch_id)
                .and_then(|f| f.plane())
                .cloned()
                .unwrap_or_default();
            let sketch = doc.get_feature(*sketch_id)
                .and_then(|f| f.sketch())
                .ok_or_else(|| "custom solid source is not a sketch".to_string())?;

            match extrude(sketch, height, ExtrudeDirection::OneSide, 0.0, &plane) {
                Some(mesh) => {
                    result.solids.insert(feature.id(), mesh);
                    result.current_solid = Some(feature.id());
                    Ok(())
                }
                None => Err("custom solid produced no mesh".into()),
            }
        }
        FeatureKind::Fillet { target_id, radius, .. } => {
            let r = doc.get_parameter(*radius).map(|p| p.value).unwrap_or(0.5);
            let target_mesh = result.solids.get(target_id)
                .ok_or_else(|| "fillet target not yet evaluated".to_string())?;
            let mesh = apply_fillet(target_mesh, r as f32);
            result.solids.insert(feature.id(), mesh);
            result.current_solid = Some(feature.id());
            Ok(())
        }
        FeatureKind::Chamfer { target_id, distance, .. } => {
            let d = doc.get_parameter(*distance).map(|p| p.value).unwrap_or(0.5);
            let target_mesh = result.solids.get(target_id)
                .ok_or_else(|| "chamfer target not yet evaluated".to_string())?;
            let mesh = apply_chamfer(target_mesh, d as f32);
            result.solids.insert(feature.id(), mesh);
            result.current_solid = Some(feature.id());
            Ok(())
        }
        FeatureKind::LinearPattern { target_id, dir_x, dir_y, dir_z, count, spacing, .. } => {
            let target = result.solids.get(target_id)
                .ok_or_else(|| "pattern target not yet evaluated".to_string())?;
            let mesh = linear_pattern(target, *dir_x, *dir_y, *dir_z, *count, *spacing);
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
            let mesh = circular_pattern(
                target, *axis_x, *axis_y, *axis_z,
                *axis_dx, *axis_dy, *axis_dz,
                *count, *total_angle_deg,
            );
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
            let mesh = mirror_across_plane(
                target, *plane_nx, *plane_ny, *plane_nz,
                *plane_px, *plane_py, *plane_pz,
            );
            result.solids.insert(feature.id(), mesh);
            result.current_solid = Some(feature.id());
            Ok(())
        }
        FeatureKind::Sweep { profile_sketch_id, path_sketch_id, .. } => {
            let profile = doc.get_feature(*profile_sketch_id).and_then(|f| f.sketch())
                .ok_or_else(|| "sweep profile is not a sketch".to_string())?;
            let path = doc.get_feature(*path_sketch_id).and_then(|f| f.sketch())
                .ok_or_else(|| "sweep path is not a sketch".to_string())?;
            match sweep_mesh(profile, path) {
                Some(mesh) => {
                    result.solids.insert(feature.id(), mesh);
                    result.current_solid = Some(feature.id());
                    Ok(())
                }
                None => Err("sweep produced no mesh".into()),
            }
        }
        FeatureKind::Shell { target_id, thickness, .. } => {
            let t = doc.get_parameter(*thickness).map(|p| p.value).unwrap_or(0.5);
            let target = result.solids.get(target_id)
                .ok_or_else(|| "shell target not yet evaluated".to_string())?;
            let mesh = shell_mesh(target, t);
            result.solids.insert(feature.id(), mesh);
            result.current_solid = Some(feature.id());
            Ok(())
        }
        FeatureKind::Boolean { op, target_a, target_b, .. } => {
            let a = result.solids.get(target_a)
                .ok_or_else(|| "boolean target_a not yet evaluated".to_string())?;
            let b = result.solids.get(target_b)
                .ok_or_else(|| "boolean target_b not yet evaluated".to_string())?;
            let op_str = match op {
                echi_core::feature::BoolOp::Union => "union",
                echi_core::feature::BoolOp::Subtract => "subtract",
                echi_core::feature::BoolOp::Intersect => "intersect",
            };
            let mesh = boolean_op(a, b, op_str);
            result.solids.insert(feature.id(), mesh);
            result.current_solid = Some(feature.id());
            Ok(())
        }
        FeatureKind::Revolve { sketch_id, angle, axis_entity_id, .. } => {
            let angle_rad = doc.get_parameter(*angle).map(|p| p.value).unwrap_or(360.0)
                * PI / 180.0;
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

            match revolve(sketch, angle_rad, 48, axis_start, axis_end) {
                Some(mesh) => {
                    result.solids.insert(feature.id(), mesh);
                    result.current_solid = Some(feature.id());
                    Ok(())
                }
                None => Err("revolve produced no mesh".into()),
            }
        }
    }
}
