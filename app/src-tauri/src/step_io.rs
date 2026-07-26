//! STEP import/export utilities (OCCT-powered).
//!
//! Extracted from commands.rs per design principle §9 (thin commands, thick domain).
//! These pure functions handle the OCCT <-> mesh conversions; the Tauri commands
//! only manage dialog, state, and IPC concerns.

#[cfg(feature = "occt")]
use cadrum::{DVec3, Solid as OcctSolid, Tessellation};
#[cfg(feature = "occt")]
use echi_core::{Document, FeatureKind};
#[cfg(feature = "occt")]
use echi_geom::{Mesh, extrude::extract_loops};
#[cfg(feature = "occt")]
use echi_render::RenderMesh;

/// Collect all extrudable solids from the document as OCCT solids,
/// ready for STEP export.
#[cfg(feature = "occt")]
pub fn collect_occt_solids(doc: &Document) -> Result<Vec<OcctSolid>, String> {
    let mut solids: Vec<OcctSolid> = Vec::new();
    for feature in doc.active_features() {
        let (sketch, height) = match &feature.kind {
            FeatureKind::Extrude { sketch_id, distance, .. } => {
                let h = doc.get_parameter(*distance).map(|p| p.value).unwrap_or(1.0);
                let s = doc.get_feature(*sketch_id).and_then(|f| f.sketch());
                (s, h)
            }
            FeatureKind::Revolve { .. } => {
                // Revolve not yet supported in OCCT STEP export
                continue;
            }
            _ => continue,
        };

        if let (Some(sketch), height) = (sketch, height) {
            if let Some(loops) = extract_loops(sketch) {
                for loop_pts in loops {
                    let points: Vec<DVec3> = loop_pts
                        .iter()
                        .map(|p| DVec3::new(p.x, p.y, 0.0))
                        .collect();
                    if let Ok(edges) = cadrum::Edge::polygon(&points) {
                        if let Ok(solid) = OcctSolid::extrude(&edges, DVec3::new(0.0, 0.0, height)) {
                            solids.push(solid);
                        }
                    }
                }
            }
        }
    }
    if solids.is_empty() {
        return Err("No extruded solids to export as STEP".into());
    }
    Ok(solids)
}

/// Convert a collection of OCCT solids to renderable meshes.
#[cfg(feature = "occt")]
pub fn tessellate_occt_solids(solids: &[OcctSolid]) -> Result<Vec<RenderMesh>, String> {
    solids.iter().map(|solid| {
        let occt_mesh = OcctSolid::mesh(
            std::iter::once(solid),
            Tessellation { deflection_linear: 0.1, relative_linear: false, ..Default::default() },
        ).map_err(|e| format!("tessellation failed: {e}"))?;
        let mut mesh = Mesh::default();
        for v in &occt_mesh.vertices {
            mesh.positions.push(v.x as f32);
            mesh.positions.push(v.y as f32);
            mesh.positions.push(v.z as f32);
        }
        for n in &occt_mesh.normals {
            mesh.normals.push(n.x as f32);
            mesh.normals.push(n.y as f32);
            mesh.normals.push(n.z as f32);
        }
        for &idx in &occt_mesh.indices {
            mesh.indices.push(idx as u32);
        }
        Ok(RenderMesh::from(&mesh))
    }).collect()
}
