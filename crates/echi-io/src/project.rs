//! EchoCAD project file IO and serialization.

use echi_core::Document;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("unsupported format version: {0}")]
    UnsupportedVersion(String),
}

/// Current format version written by `save_project`.
pub const CURRENT_FORMAT_VERSION: u32 = 1;

/// Migrate a project JSON string to the current format version.
///
/// Reads `format_version` from the JSON object (defaults to 0 if absent),
/// applies the migration chain, and returns the updated JSON string.
pub fn migrate_project(json: &str) -> Result<String, ProjectError> {
    let mut value: serde_json::Value = serde_json::from_str(json)?;

    let version = match value.get("format_version") {
        Some(v) if v.is_number() => v.as_u64().unwrap_or(0),
        Some(_) => {
            return Err(ProjectError::UnsupportedVersion(
                "unknown (non-numeric)".to_string(),
            ));
        }
        None => 0,
    };

    if version > CURRENT_FORMAT_VERSION as u64 {
        return Err(ProjectError::UnsupportedVersion(version.to_string()));
    }

    // version 0 → 1: inject the format_version field.
    if version == 0 {
        if let Some(obj) = value.as_object_mut() {
            obj.insert(
                "format_version".to_string(),
                serde_json::Value::Number(serde_json::Number::from(1)),
            );
        }
    }

    // version == 1: no migration needed.

    serde_json::to_string(&value).map_err(ProjectError::from)
}

/// Save a document to a project file.
pub fn save_project(doc: &Document, path: impl AsRef<Path>) -> Result<(), ProjectError> {
    let json = serde_json::to_string_pretty(doc)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Load a document from a project file, migrating the format if necessary.
pub fn load_project(path: impl AsRef<Path>) -> Result<Document, ProjectError> {
    let raw = std::fs::read_to_string(path)?;
    let migrated = migrate_project(&raw)?;
    let doc = serde_json::from_str(&migrated)?;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use echi_core::feature::{BoolOp, ExtrudeDirection, FeatureKind, PlaneDefinition};
    use echi_core::sketch::Sketch;
    use echi_core::{Document, Feature, FeatureId, ParameterId};

    #[test]
    fn project_roundtrip_preserves_features() {
        let mut doc = Document::new("roundtrip-test");
        doc.add_feature(Feature::new(
            FeatureId(1),
            "Sketch1",
            FeatureKind::Sketch {
                sketch: Sketch::new(),
                plane: PlaneDefinition::XY,
            },
        ));
        doc.add_parameter("depth", 2.5);
        doc.add_feature(Feature::new(
            FeatureId(2),
            "Extrude1",
            FeatureKind::Extrude {
                sketch_id: FeatureId(1),
                distance: ParameterId(1),
                direction: ExtrudeDirection::OneSide,
                draft_angle_deg: 0.0,
            },
        ));

        // Use std::env::temp_dir (not the tempfile crate) and a per-process
        // filename so parallel test runs don't collide.
        let path = std::env::temp_dir()
            .join(format!("echi_io_roundtrip_{}.json", std::process::id()));
        save_project(&doc, &path).expect("save should succeed");
        let loaded = load_project(&path).expect("load should succeed");
        // Clean up before asserting so a failing assertion never leaks the file.
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.format_version, 1, "saved doc must carry format_version 1");
        assert_eq!(loaded.name, "roundtrip-test");
        assert_eq!(loaded.features.len(), 2, "feature count must round-trip");
        assert!(
            matches!(loaded.features[0].kind, FeatureKind::Sketch { .. }),
            "first feature should be a Sketch"
        );
        assert!(
            matches!(loaded.features[1].kind, FeatureKind::Extrude { .. }),
            "second feature should be an Extrude"
        );
        // Parameter survived too.
        assert_eq!(loaded.parameters.len(), 1);
        assert!((loaded.parameters[0].value - 2.5).abs() < 1e-12);
    }

    #[test]
    fn project_roundtrip_with_sketch_entities_fillet_and_boolean() {
        let mut doc = Document::new("extended-roundtrip");

        // Feature 1: Sketch with actual entities (validates the serde_entities fix).
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(0.0, 0.0);
        let p1 = sketch.add_point(10.0, 0.0);
        sketch.add_line(p0, p1);
        doc.add_feature(Feature::new(
            FeatureId(1),
            "Sketch1",
            FeatureKind::Sketch {
                sketch: sketch.clone(),
                plane: PlaneDefinition::XY,
            },
        ));
        doc.add_parameter("depth", 2.5);

        // Feature 2: Fillet with edges.
        doc.add_feature(Feature::new(
            FeatureId(2),
            "Fillet1",
            FeatureKind::Fillet {
                target_id: FeatureId(1),
                radius: ParameterId(1),
                edges: vec![(0, 1), (2, 3)],
            },
        ));

        // Feature 3: Boolean (union).
        doc.add_feature(Feature::new(
            FeatureId(3),
            "Boolean1",
            FeatureKind::Boolean {
                op: BoolOp::Union,
                target_a: FeatureId(1),
                target_b: FeatureId(2),
            },
        ));

        let path = std::env::temp_dir()
            .join(format!("echi_io_extended_roundtrip_{}.json", std::process::id()));
        save_project(&doc, &path).expect("save should succeed");
        let loaded = load_project(&path).expect("load should succeed");
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.name, "extended-roundtrip");
        assert_eq!(loaded.features.len(), 3, "should have 3 features");

        // Feature 1: Sketch with entities that survived the round-trip.
        assert_eq!(loaded.features[0].type_tag(), "Sketch");
        let sketch_kind = match &loaded.features[0].kind {
            FeatureKind::Sketch { sketch, .. } => Some(sketch.clone()),
            _ => None,
        };
        assert!(sketch_kind.is_some(), "first feature should be Sketch");
        let loaded_sketch = sketch_kind.unwrap();
        assert_eq!(
            loaded_sketch.entities.len(),
            3,
            "sketch should still have 3 entities (2 points + 1 line)"
        );

        // Feature 2: Fillet with edges.
        assert_eq!(loaded.features[1].type_tag(), "Fillet");
        match &loaded.features[1].kind {
            FeatureKind::Fillet { target_id, edges, .. } => {
                assert_eq!(*target_id, FeatureId(1));
                assert_eq!(*edges, vec![(0, 1), (2, 3)]);
            }
            other => panic!("expected Fillet, got {:?}", other),
        }

        // Feature 3: Boolean.
        assert_eq!(loaded.features[2].type_tag(), "Boolean");
        match &loaded.features[2].kind {
            FeatureKind::Boolean { op, target_a, target_b } => {
                assert_eq!(*op, BoolOp::Union);
                assert_eq!(*target_a, FeatureId(1));
                assert_eq!(*target_b, FeatureId(2));
            }
            other => panic!("expected Boolean, got {:?}", other),
        }
    }

    // ── format version / migration tests ──────────────────────────────

    #[test]
    fn migrate_v0_to_v1() {
        // JSON without format_version should get format_version: 1.
        let input = r#"{"name":"old-project","features":[],"parameters":[],"next_id":1}"#;
        let migrated = migrate_project(input).expect("migration should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&migrated).expect("migrated output must be valid JSON");
        assert_eq!(
            parsed["format_version"].as_u64(),
            Some(1),
            "v0 JSON should get format_version: 1"
        );
    }

    #[test]
    fn v1_passthrough() {
        // JSON already at version 1 should pass through unchanged.
        let input =
            r#"{"format_version":1,"name":"v1-project","features":[],"parameters":[],"next_id":1}"#;
        let migrated = migrate_project(input).expect("migration should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&migrated).expect("migrated output must be valid JSON");
        assert_eq!(parsed["format_version"].as_u64(), Some(1));
        assert_eq!(parsed["name"].as_str(), Some("v1-project"));
    }

    #[test]
    fn roundtrip_v1_format_version() {
        // save_project writes format_version: 1; load_project preserves it.
        let mut doc = Document::new("roundtrip-v1");
        doc.add_feature(Feature::new(
            FeatureId(1),
            "Sketch1",
            FeatureKind::Sketch {
                sketch: Sketch::new(),
                plane: PlaneDefinition::XY,
            },
        ));
        let path = std::env::temp_dir()
            .join(format!("echi_io_v1_roundtrip_{}.json", std::process::id()));
        save_project(&doc, &path).expect("save should succeed");
        let loaded = load_project(&path).expect("load should succeed");
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.format_version, 1);
    }

    #[test]
    fn backward_compat_stripped_version() {
        // Save a document, strip the format_version field, migrate, then load.
        let mut doc = Document::new("backward-compat");
        doc.add_feature(Feature::new(
            FeatureId(1),
            "Sketch1",
            FeatureKind::Sketch {
                sketch: Sketch::new(),
                plane: PlaneDefinition::XY,
            },
        ));
        let path = std::env::temp_dir()
            .join(format!("echi_io_backcompat_{}.json", std::process::id()));
        save_project(&doc, &path).expect("save should succeed");
        let raw = std::fs::read_to_string(&path).expect("read should succeed");
        let _ = std::fs::remove_file(&path);

        // Strip the format_version line.
        let stripped: String = raw
            .lines()
            .filter(|line| !line.contains("format_version"))
            .collect::<Vec<_>>()
            .join("\n");

        let migrated = migrate_project(&stripped).expect("migration should succeed");
        let loaded: Document =
            serde_json::from_str(&migrated).expect("deserialize after migration should succeed");
        assert_eq!(loaded.format_version, 1);
        assert_eq!(loaded.name, "backward-compat");
        assert_eq!(loaded.features.len(), 1);
    }

    #[test]
    fn future_version_rejected() {
        // format_version 99 should be rejected by migrate_project.
        let input = r#"{"format_version":99,"name":"future-project","features":[],"parameters":[],"next_id":1}"#;
        let err = migrate_project(input).expect_err("future version should be rejected");
        match err {
            ProjectError::UnsupportedVersion(msg) => {
                assert!(msg.contains("99"), "error should mention the version number");
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }
}
