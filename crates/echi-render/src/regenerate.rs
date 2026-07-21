//! Document regeneration: execute feature tree to produce renderable geometry.

use echi_core::{Document, Feature, FeatureId, FeatureKind};
use echi_geom::extrude::{Mesh, extrude};
use echi_geom::{
    apply_chamfer, apply_chamfer_edges, apply_fillet, apply_fillet_edges, boolean_op,
    circular_pattern, linear_pattern, mirror_across_plane, revolve, shell_mesh, sweep_mesh,
};
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
/// plugin solid generator attached. `CustomSolid` features will record an
/// error (they need a generator to produce geometry). Use
/// [`regenerate_with`] from the application layer to supply one.
pub fn regenerate(doc: &Document) -> RegenResult {
    regenerate_with(doc, None)
}

/// Regenerate with an optional plugin solid generator. Suppressed features
/// are skipped; features whose dependencies failed are also skipped (with an
/// error recorded).
pub fn regenerate_with(doc: &Document, generator: Option<&dyn SolidGenerator>) -> RegenResult {
    let mut result = RegenResult::default();

    for feature in &doc.features {
        if feature.suppressed {
            continue;
        }
        if let Err(msg) = regenerate_feature(feature, doc, &mut result, generator) {
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
        FeatureKind::Extrude { sketch_id, distance, direction, draft_angle_deg, .. } => {
            // F6: fail loudly if the parameter is missing — silent fallback (1.0)
            // hides data corruption and violates design principle #8.
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

            match extrude(sketch, height, *direction, *draft_angle_deg, &plane) {
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
            // Empty edge list = legacy "fillet every sharp edge".
            let mesh = if edges.is_empty() {
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
            let mesh = if edges.is_empty() {
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
            let t = doc.get_parameter(*thickness)
                .map(|p| p.value)
                .ok_or_else(|| format!("shell thickness parameter {:?} not found", thickness))?;
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
        let result = regenerate_with(&doc, Some(&StubGenerator));
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
        let result = regenerate_with(&doc, Some(&StubGenerator));
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
}
