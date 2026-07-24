use crate::parameter::ParameterId;
use crate::sketch::{EntityId, Sketch};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FeatureId(pub u64);

/// Extrude direction mode.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtrudeDirection {
    /// One-sided (positive direction only).
    #[default]
    OneSide,
    /// Symmetric about the sketch plane.
    Midplane,
    /// Different distances on each side.
    TwoSides { dist1: f64, dist2: f64 },
}

/// Definition of a sketch/work plane in 3D space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaneDefinition {
    /// Default: XY plane (Z=0).
    XY,
    /// YZ plane (X=0).
    YZ,
    /// ZX plane (Y=0).
    ZX,
    /// Offset from another plane by a distance along its normal.
    Offset {
        base: Box<PlaneDefinition>,
        distance: f64,
    },
}

impl Default for PlaneDefinition {
    fn default() -> Self {
        PlaneDefinition::XY
    }
}

impl PlaneDefinition {
    /// Resolve this plane definition to (origin, u_dir, v_dir, normal) in 3D.
    /// u_dir and v_dir are orthonormal in-plane axes; normal is their cross product.
    pub fn frame(&self) -> ([f64; 3], [f64; 3], [f64; 3], [f64; 3]) {
        match self {
            PlaneDefinition::XY => (
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ),
            PlaneDefinition::YZ => (
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
            ),
            PlaneDefinition::ZX => (
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
            ),
            PlaneDefinition::Offset { base, distance } => {
                let (origin, u, v, n) = base.frame();
                (
                    [origin[0] + n[0] * distance, origin[1] + n[1] * distance, origin[2] + n[2] * distance],
                    u,
                    v,
                    n,
                )
            }
        }
    }

    /// Backwards-compatible resolve: returns (origin_x, origin_y, origin_z, nx, ny, nz).
    pub fn resolve(&self) -> (f64, f64, f64, f64, f64, f64) {
        let (o, _, _, n) = self.frame();
        (o[0], o[1], o[2], n[0], n[1], n[2])
    }

    /// Map a 2D sketch point (sx, sy) to 3D world coordinates on this plane.
    pub fn map_to_3d(&self, sx: f64, sy: f64) -> (f64, f64, f64) {
        let (o, u, v, _) = self.frame();
        (
            o[0] + u[0] * sx + v[0] * sy,
            o[1] + u[1] * sx + v[1] * sy,
            o[2] + u[2] * sx + v[2] * sy,
        )
    }

    /// Get the extrusion direction (normal) from the plane into 3D.
    pub fn extrude_dir(&self) -> (f64, f64, f64) {
        let (_, _, _, n) = self.frame();
        (n[0], n[1], n[2])
    }

    /// In-plane u direction (the "X" axis of the sketch).
    pub fn u_dir(&self) -> [f64; 3] {
        self.frame().1
    }

    /// In-plane v direction (the "Y" axis of the sketch).
    pub fn v_dir(&self) -> [f64; 3] {
        self.frame().2
    }
}

/// A feature defined by a plugin rather than built-in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomFeatureData {
    /// The plugin that owns this feature type.
    pub plugin_id: String,
    /// The specific generator within the plugin.
    pub generator_id: String,
    /// Parameters used to generate this feature.
    pub params: HashMap<String, f64>,
}

/// Common metadata shared by every feature, regardless of kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    /// When true, the feature is skipped during regeneration and does
    /// not contribute geometry. Dependent features fall back to their
    /// next-most-recent valid input.
    #[serde(default)]
    pub suppressed: bool,
    /// Optional per-feature color as a hex string (e.g. "#ff8800").
    /// `None` means the renderer should use the default palette-based color.
    /// Currently UI-only (not yet exposed via a Tauri command); the store
    /// persists it for project serialization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Kind-specific data (sketch, extrude, fillet, ...).
    #[serde(flatten)]
    pub kind: FeatureKind,
}

/// Kind-specific data of a feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeatureKind {
    Sketch {
        sketch: Sketch,
        #[serde(default)]
        plane: PlaneDefinition,
    },
    Extrude {
        sketch_id: FeatureId,
        distance: ParameterId,
        #[serde(default)]
        direction: ExtrudeDirection,
        #[serde(default)]
        draft_angle_deg: f64,
        /// Optional region indices to extrude. `None` extrudes all regions.
        #[serde(default)]
        selected_regions: Option<Vec<usize>>,
    },
    Revolve {
        sketch_id: FeatureId,
        angle: ParameterId,
        /// Optional axis line entity in the sketch. If None, defaults to Y axis.
        axis_entity_id: Option<EntityId>,
    },
    CustomSketch {
        sketch: Sketch,
        custom: CustomFeatureData,
        #[serde(default)]
        plane: PlaneDefinition,
    },
    CustomSolid {
        sketch_id: FeatureId,
        distance: ParameterId,
        custom: CustomFeatureData,
    },
    /// Blend edges with a radius.
    Fillet {
        target_id: FeatureId,
        radius: ParameterId,
        /// Specific edges to fillet, as (vertex_a, vertex_b) index pairs into
        /// the target mesh. An empty vec means "all sharp edges" (the legacy
        /// behaviour before per-edge selection existed).
        #[serde(default)]
        edges: Vec<(u32, u32)>,
    },
    /// Bevel edges at a 45° angle.
    Chamfer {
        target_id: FeatureId,
        distance: ParameterId,
        /// Specific edges to chamfer, as (vertex_a, vertex_b) index pairs into
        /// the target mesh. An empty vec means "all sharp edges".
        #[serde(default)]
        edges: Vec<(u32, u32)>,
    },
    /// Linear pattern: repeat a solid along a direction.
    LinearPattern {
        target_id: FeatureId,
        dir_x: f64,
        dir_y: f64,
        dir_z: f64,
        /// Number of instances (including original).
        count: u32,
        /// Spacing between instances.
        spacing: f64,
    },
    /// Circular pattern: repeat a solid around an axis.
    CircularPattern {
        target_id: FeatureId,
        axis_x: f64,
        axis_y: f64,
        axis_z: f64,
        axis_dx: f64,
        axis_dy: f64,
        axis_dz: f64,
        count: u32,
        total_angle_deg: f64,
    },
    /// Mirror a solid across a plane.
    Mirror {
        target_id: FeatureId,
        plane_nx: f64,
        plane_ny: f64,
        plane_nz: f64,
        plane_px: f64,
        plane_py: f64,
        plane_pz: f64,
    },
    /// Sweep a profile sketch along a path sketch.
    Sweep {
        profile_sketch_id: FeatureId,
        path_sketch_id: FeatureId,
    },
    /// Hollow a solid with a given wall thickness.
    Shell {
        target_id: FeatureId,
        thickness: ParameterId,
    },
    Boolean {
        op: BoolOp,
        target_a: FeatureId,
        target_b: FeatureId,
    },
}

/// Boolean operation type.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
}

impl Feature {
    pub fn new(id: FeatureId, name: impl Into<String>, kind: FeatureKind) -> Self {
        Self { id, name: name.into(), suppressed: false, color: None, kind }
    }

    pub fn id(&self) -> FeatureId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, new_name: impl Into<String>) {
        self.name = new_name.into();
    }

    /// Whether this feature is sketch-based (can be used as active sketch).
    pub fn is_sketch(&self) -> bool {
        matches!(self.kind, FeatureKind::Sketch { .. } | FeatureKind::CustomSketch { .. })
    }

    /// Get the sketch data if this feature contains a sketch.
    pub fn sketch(&self) -> Option<&Sketch> {
        match &self.kind {
            FeatureKind::Sketch { sketch, .. } | FeatureKind::CustomSketch { sketch, .. } => Some(sketch),
            _ => None,
        }
    }

    /// Get mutable sketch data if this feature contains a sketch.
    pub fn sketch_mut(&mut self) -> Option<&mut Sketch> {
        match &mut self.kind {
            FeatureKind::Sketch { sketch, .. } | FeatureKind::CustomSketch { sketch, .. } => Some(sketch),
            _ => None,
        }
    }

    /// Get the plane definition if this feature is a sketch.
    pub fn plane(&self) -> Option<&PlaneDefinition> {
        match &self.kind {
            FeatureKind::Sketch { plane, .. } | FeatureKind::CustomSketch { plane, .. } => Some(plane),
            _ => None,
        }
    }

    /// Get the plane definition if this feature is a sketch (mutable).
    pub fn plane_mut(&mut self) -> Option<&mut PlaneDefinition> {
        match &mut self.kind {
            FeatureKind::Sketch { plane, .. } | FeatureKind::CustomSketch { plane, .. } => Some(plane),
            _ => None,
        }
    }

    /// Short type tag for UI/icons (e.g. "Sketch", "Extrude", "Fillet").
    pub fn type_tag(&self) -> &'static str {
        match &self.kind {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude { .. } => "Extrude",
            FeatureKind::Revolve { .. } => "Revolve",
            FeatureKind::CustomSketch { .. } => "Custom",
            FeatureKind::CustomSolid { .. } => "CustomSolid",
            FeatureKind::Fillet { .. } => "Fillet",
            FeatureKind::Chamfer { .. } => "Chamfer",
            FeatureKind::LinearPattern { .. } => "LinearPattern",
            FeatureKind::CircularPattern { .. } => "CircularPattern",
            FeatureKind::Mirror { .. } => "Mirror",
            FeatureKind::Sweep { .. } => "Sweep",
            FeatureKind::Shell { .. } => "Shell",
            FeatureKind::Boolean { .. } => "Boolean",
        }
    }

    /// Plugin identifier when this is a custom (plugin-defined) feature.
    pub fn custom_plugin(&self) -> Option<&str> {
        match &self.kind {
            FeatureKind::CustomSketch { custom, .. } | FeatureKind::CustomSolid { custom, .. } => {
                Some(&custom.plugin_id)
            }
            _ => None,
        }
    }

    /// Feature IDs this feature directly depends on (used for deletion
    /// safety and regeneration ordering).
    pub fn dependencies(&self) -> Vec<FeatureId> {
        match &self.kind {
            FeatureKind::Extrude { sketch_id, .. } => vec![*sketch_id],
            FeatureKind::Revolve { sketch_id, .. } => vec![*sketch_id],
            // R1: CustomSolid no longer reads `sketch_id` during regeneration
            // (the plugin generator produces the mesh directly from
            // custom.params).  Listing it as a dependency causes a spurious
            // "dependency not found" error when the sketch is deleted, even
            // though CustomSolid never touches it.  The field stays on the
            // variant for backward compatibility with old project files.
            FeatureKind::CustomSolid { .. } => Vec::new(),
            FeatureKind::Fillet { target_id, .. }
            | FeatureKind::Chamfer { target_id, .. }
            | FeatureKind::Shell { target_id, .. }
            | FeatureKind::LinearPattern { target_id, .. }
            | FeatureKind::CircularPattern { target_id, .. }
            | FeatureKind::Mirror { target_id, .. } => vec![*target_id],
            FeatureKind::Sweep { profile_sketch_id, path_sketch_id } => {
                vec![*profile_sketch_id, *path_sketch_id]
            }
            FeatureKind::Boolean { target_a, target_b, .. } => vec![*target_a, *target_b],
            FeatureKind::Sketch { .. } | FeatureKind::CustomSketch { .. } => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fillet(target: FeatureId, radius: ParameterId, edges: Vec<(u32, u32)>) -> Feature {
        Feature::new(FeatureId(2), "Fillet1", FeatureKind::Fillet {
            target_id: target,
            radius,
            edges,
        })
    }

    fn chamfer(target: FeatureId, distance: ParameterId, edges: Vec<(u32, u32)>) -> Feature {
        Feature::new(FeatureId(3), "Chamfer1", FeatureKind::Chamfer {
            target_id: target,
            distance,
            edges,
        })
    }

    #[test]
    fn fillet_dependencies_and_tag_with_edges() {
        let f = fillet(FeatureId(7), ParameterId(1), vec![(0, 1), (2, 3)]);
        // Dependencies ignore the edge list — only the target matters.
        assert_eq!(f.dependencies(), vec![FeatureId(7)]);
        assert_eq!(f.type_tag(), "Fillet");
    }

    #[test]
    fn chamfer_dependencies_and_tag_with_edges() {
        let f = chamfer(FeatureId(9), ParameterId(1), vec![(4, 5)]);
        assert_eq!(f.dependencies(), vec![FeatureId(9)]);
        assert_eq!(f.type_tag(), "Chamfer");
    }

    #[test]
    fn fillet_empty_edges_means_all() {
        // Empty edge list is the legacy "fillet all sharp edges" behaviour;
        // it must still report a dependency on the target.
        let f = fillet(FeatureId(1), ParameterId(1), Vec::new());
        assert_eq!(f.dependencies(), vec![FeatureId(1)]);
    }

    #[test]
    fn offset_plane_frame_translates_origin_along_normal() {
        // XY plane offset by +2 along its normal (Z) lands at (0,0,2).
        let plane = PlaneDefinition::Offset {
            base: Box::new(PlaneDefinition::XY),
            distance: 2.0,
        };
        let (origin, u, v, n) = plane.frame();
        assert!((origin[0] - 0.0).abs() < 1e-12);
        assert!((origin[1] - 0.0).abs() < 1e-12);
        assert!((origin[2] - 2.0).abs() < 1e-12, "origin z should be 2, got {}", origin[2]);
        // Frame axes are unchanged from the base plane (offset is purely a translation).
        assert_eq!(u, [1.0, 0.0, 0.0]);
        assert_eq!(v, [0.0, 1.0, 0.0]);
        assert_eq!(n, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn offset_plane_negative_distance_moves_opposite() {
        let plane = PlaneDefinition::Offset {
            base: Box::new(PlaneDefinition::YZ),
            distance: -1.5,
        };
        let (origin, _, _, n) = plane.frame();
        // YZ plane has normal +X; negative offset moves to x = -1.5.
        assert!((origin[0] - (-1.5)).abs() < 1e-12, "origin x should be -1.5, got {}", origin[0]);
        assert_eq!(n, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn offset_plane_nested_offsets_stack() {
        // Offset-of-offset should stack translations along the same normal.
        let plane = PlaneDefinition::Offset {
            base: Box::new(PlaneDefinition::Offset {
                base: Box::new(PlaneDefinition::XY),
                distance: 1.0,
            }),
            distance: 3.0,
        };
        let (origin, _, _, _) = plane.frame();
        assert!((origin[2] - 4.0).abs() < 1e-12, "stacked offset z should be 4, got {}", origin[2]);
    }

    #[test]
    fn fillet_with_edges_round_trips_serde() {
        // Old project files have no `edges` field; serde(default) must keep
        // them loading as the "all edges" empty vec.
        let json = r#"{"type":"fillet","target_id":5,"radius":9}"#;
        let parsed: Result<FeatureKind, _> = serde_json::from_str(json);
        let kind = parsed.expect("legacy fillet without edges must deserialize");
        match kind {
            FeatureKind::Fillet { target_id, radius, edges } => {
                assert_eq!(target_id, FeatureId(5));
                assert_eq!(radius, ParameterId(9));
                assert!(edges.is_empty(), "missing edges field must default to empty");
            }
            other => panic!("expected Fillet, got {:?}", other),
        }
    }

    #[test]
    fn fillet_with_edges_serde_roundtrip() {
        let kind = FeatureKind::Fillet {
            target_id: FeatureId(10),
            radius: ParameterId(2),
            edges: vec![(0, 1), (2, 3)],
        };
        let json = serde_json::to_string(&kind).expect("should serialize");
        let back: FeatureKind = serde_json::from_str(&json).expect("should deserialize");
        match back {
            FeatureKind::Fillet { target_id, radius, edges } => {
                assert_eq!(target_id, FeatureId(10));
                assert_eq!(radius, ParameterId(2));
                assert_eq!(edges, vec![(0, 1), (2, 3)]);
            }
            other => panic!("expected Fillet, got {:?}", other),
        }
    }

    #[test]
    fn fillet_empty_edges_serde_roundtrip() {
        let kind = FeatureKind::Fillet {
            target_id: FeatureId(1),
            radius: ParameterId(3),
            edges: Vec::new(),
        };
        let json = serde_json::to_string(&kind).expect("should serialize");
        let back: FeatureKind = serde_json::from_str(&json).expect("should deserialize");
        match back {
            FeatureKind::Fillet { edges, .. } => {
                assert!(edges.is_empty(), "empty edges should round-trip as empty");
            }
            other => panic!("expected Fillet, got {:?}", other),
        }
    }

    #[test]
    fn chamfer_with_edges_serde_roundtrip() {
        let kind = FeatureKind::Chamfer {
            target_id: FeatureId(7),
            distance: ParameterId(4),
            edges: vec![(4, 5)],
        };
        let json = serde_json::to_string(&kind).expect("should serialize");
        let back: FeatureKind = serde_json::from_str(&json).expect("should deserialize");
        match back {
            FeatureKind::Chamfer { target_id, distance, edges } => {
                assert_eq!(target_id, FeatureId(7));
                assert_eq!(distance, ParameterId(4));
                assert_eq!(edges, vec![(4, 5)]);
            }
            other => panic!("expected Chamfer, got {:?}", other),
        }
    }

    #[test]
    fn offset_plane_definition_serde_roundtrip() {
        let plane = PlaneDefinition::Offset {
            base: Box::new(PlaneDefinition::XY),
            distance: 1.5,
        };
        let json = serde_json::to_string(&plane).expect("should serialize");
        let back: PlaneDefinition = serde_json::from_str(&json).expect("should deserialize");
        match back {
            PlaneDefinition::Offset { base, distance } => {
                assert!((distance - 1.5).abs() < 1e-12);
                assert_eq!(*base, PlaneDefinition::XY);
            }
            other => panic!("expected Offset, got {:?}", other),
        }
    }
}
