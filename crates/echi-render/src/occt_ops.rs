//! OpenCASCADE-powered geometry operations.
//!
//! These functions rebuild OCCT solids from the feature tree and apply
//! geometry operations (extrude, boolean, shell, fillet, sweep, revolve,
//! pattern, mirror). All functions return `Option<echi_geom::Mesh>` and
//! delegate to `cadrum` for the actual OCCT calls.

use echi_core::{Document, FeatureId, FeatureKind};
use echi_geom::Mesh;
use echi_geom::extrude::extract_loops;

#[cfg(feature = "brep")]
use {
    echi_brep::{BrepKernel, ProfilePoint},
    echi_core::feature::{ExtrudeDirection, PlaneDefinition},
    echi_core::sketch::Sketch,
};

// copy_mesh and transform_mesh_to_world are now in crate::mesh
#[cfg(feature = "brep")]
use crate::mesh::{copy_mesh, transform_mesh_to_world};

/// Helper: convert cadrum Mesh to echi_geom Mesh.
#[cfg(feature = "occt")]
pub fn cadrum_mesh_to_echi(occt_mesh: &cadrum::Mesh) -> Mesh {
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
    mesh
}

/// Recursively build an OCCT solid from a feature in the document.
#[cfg(feature = "occt")]
pub fn build_occt_solid(doc: &Document, feature_id: FeatureId) -> Option<cadrum::Solid> {
    use cadrum::{DVec3, Solid as OcctSolid};

    let feature = doc.get_feature(feature_id)?;
    if feature.suppressed {
        return None;
    }

    match &feature.kind {
        FeatureKind::Extrude { sketch_id, distance, .. } => {
            let height = doc.get_parameter(*distance).map(|p| p.value).unwrap_or(1.0);
            let sketch = doc.get_feature(*sketch_id).and_then(|f| f.sketch())?;
            let loops = extract_loops(sketch)?;
            if loops.is_empty() {
                return None;
            }
            let outer_pts: Vec<DVec3> = loops[0].iter().map(|p| DVec3::new(p.x, p.y, 0.0)).collect();
            let outer_edges = cadrum::Edge::polygon(&outer_pts).ok()?;
            let mut solid = OcctSolid::extrude(&outer_edges, DVec3::new(0.0, 0.0, height)).ok()?;
            for hole_pts in loops.iter().skip(1) {
                let pts: Vec<DVec3> = hole_pts.iter().map(|p| DVec3::new(p.x, p.y, -1.0)).collect();
                if let Ok(edges) = cadrum::Edge::polygon(&pts) {
                    if let Ok(hole) = OcctSolid::extrude(&edges, DVec3::new(0.0, 0.0, height + 2.0)) {
                        if let Ok(result) = (cadrum::Boolean::from(&solid) - &hole).build() {
                            solid = result;
                        }
                    }
                }
            }
            Some(solid)
        }
        FeatureKind::Boolean { op, target_a, target_b, .. } => {
            let a = build_occt_solid(doc, *target_a)?;
            let b = build_occt_solid(doc, *target_b)?;
            let result = match op {
                echi_core::feature::BoolOp::Union =>
                    (cadrum::Boolean::from(&a) + &b).build().ok()?,
                echi_core::feature::BoolOp::Subtract =>
                    (cadrum::Boolean::from(&a) - &b).build().ok()?,
                echi_core::feature::BoolOp::Intersect =>
                    (cadrum::Boolean::from(&a) * &b).build().ok()?,
            };
            Some(result)
        }
        FeatureKind::Revolve { sketch_id, angle, .. } => {
            let angle_rad = doc.get_parameter(*angle).map(|p| p.value).unwrap_or(2.0 * std::f64::consts::PI);
            let sketch = doc.get_feature(*sketch_id).and_then(|f| f.sketch())?;
            let loops = extract_loops(sketch)?;
            if loops.is_empty() { return None; }
            // Revolve via sweep around a circular spine (same approach as occt_revolve_via_sketch)
            let profile_pts: Vec<DVec3> = loops[0].iter().map(|p| DVec3::new(p.x, p.y, 0.0)).collect();
            let profile_edges = cadrum::Edge::polygon(&profile_pts).ok()?;
            // Build a circular spine for the revolve around Y axis
            let n_seg = 64usize;
            let radius = profile_pts.iter().map(|p| (p.x.powi(2) + p.y.powi(2)).sqrt()).fold(0.0f64, f64::max).max(1.0);
            let spine_pts: Vec<DVec3> = (0..=n_seg).map(|i| {
                let a = angle_rad * i as f64 / n_seg as f64;
                DVec3::new(radius * a.cos(), 0.0, radius * a.sin())
            }).collect();
            let spine_edges: Vec<_> = spine_pts.windows(2)
                .filter_map(|w| cadrum::Edge::polygon(&[w[0], w[1]]).ok()).flatten().collect();
            if spine_edges.is_empty() { return None; }
            use cadrum::ProfileOrient;
            OcctSolid::sweep(&profile_edges, &spine_edges, ProfileOrient::Fixed).ok()
        }
        FeatureKind::Sweep { profile_sketch_id, path_sketch_id } => {
            let profile_sketch = doc.get_feature(*profile_sketch_id).and_then(|f| f.sketch())?;
            let path_sketch = doc.get_feature(*path_sketch_id).and_then(|f| f.sketch())?;
            let profile_loops = extract_loops(profile_sketch)?;
            let path_loops = extract_loops(path_sketch)?;
            if profile_loops.is_empty() || path_loops.is_empty() { return None; }
            let profile_pts: Vec<DVec3> = profile_loops[0].iter().map(|p| DVec3::new(p.x, p.y, 0.0)).collect();
            let path_pts: Vec<DVec3> = path_loops[0].iter().map(|p| DVec3::new(p.x, p.y, 0.0)).collect();
            let profile_edges = cadrum::Edge::polygon(&profile_pts).ok()?;
            if path_pts.len() < 2 { return None; }
            let spine_edges: Vec<_> = path_pts.windows(2)
                .filter_map(|w| cadrum::Edge::polygon(&[w[0], w[1]]).ok()).flatten().collect();
            if spine_edges.is_empty() { return None; }
            use cadrum::ProfileOrient;
            OcctSolid::sweep(&profile_edges, &spine_edges, ProfileOrient::Fixed).ok()
        }
        FeatureKind::Fillet { target_id, radius, .. } => {
            let r = doc.get_parameter(*radius).map(|p| p.value).unwrap_or(0.5);
            let solid = build_occt_solid(doc, *target_id)?;
            let edges: Vec<&cadrum::Edge> = solid.iter_edge().collect();
            if edges.is_empty() { return Some(solid); }
            solid.fillet_edges(r, edges).ok()
        }
        FeatureKind::Chamfer { target_id, distance, .. } => {
            let d = doc.get_parameter(*distance).map(|p| p.value).unwrap_or(0.5);
            let solid = build_occt_solid(doc, *target_id)?;
            let edges: Vec<&cadrum::Edge> = solid.iter_edge().collect();
            if edges.is_empty() { return Some(solid); }
            solid.chamfer_edges(d, edges).ok()
        }
        FeatureKind::Shell { target_id, thickness } => {
            let t = doc.get_parameter(*thickness).map(|p| p.value).unwrap_or(0.5);
            let solid = build_occt_solid(doc, *target_id)?;
            solid.shell(t, std::iter::empty()).ok()
        }
        FeatureKind::LinearPattern { target_id, dir_x, dir_y, dir_z, count, spacing } => {
            if *count < 2 { return build_occt_solid(doc, *target_id); }
            let base = build_occt_solid(doc, *target_id)?;
            let dir = DVec3::new(*dir_x, *dir_y, *dir_z);
            let len = (dir.x.powi(2) + dir.y.powi(2) + dir.z.powi(2)).sqrt();
            if len < 1e-10 { return Some(base); }
            let step = DVec3::new(dir.x / len * spacing, dir.y / len * spacing, dir.z / len * spacing);
            let mut combined: Option<OcctSolid> = None;
            for i in 0..*count {
                let offset = DVec3::new(step.x * i as f64, step.y * i as f64, step.z * i as f64);
                let instance = base.clone().translate(offset);
                combined = match combined {
                    None => Some(instance),
                    Some(c) => (cadrum::Boolean::from(&c) + &instance).build().ok(),
                };
            }
            combined
        }
        FeatureKind::CircularPattern { target_id, axis_x, axis_y, axis_z, axis_dx, axis_dy, axis_dz, count, total_angle_deg } => {
            if *count < 2 { return build_occt_solid(doc, *target_id); }
            let base = build_occt_solid(doc, *target_id)?;
            let axis_origin = DVec3::new(*axis_x, *axis_y, *axis_z);
            let axis_dir = DVec3::new(*axis_dx, *axis_dy, *axis_dz);
            let angle_step = total_angle_deg.to_radians() / (*count - 1) as f64;
            let mut combined: Option<OcctSolid> = None;
            for i in 0..*count {
                let angle = angle_step * i as f64;
                let instance = base.clone().rotate(axis_origin, axis_dir, angle);
                combined = match combined {
                    None => Some(instance),
                    Some(c) => (cadrum::Boolean::from(&c) + &instance).build().ok(),
                };
            }
            combined
        }
        FeatureKind::Mirror { target_id, plane_nx, plane_ny, plane_nz, plane_px, plane_py, plane_pz } => {
            let base = build_occt_solid(doc, *target_id)?;
            let mirrored = base.clone().mirror(
                DVec3::new(*plane_px, *plane_py, *plane_pz),
                DVec3::new(*plane_nx, *plane_ny, *plane_nz),
            );
            (cadrum::Boolean::from(&base) + &mirrored).build().ok()
        }
        _ => None,
    }
}

/// Public entry point for OCCT mass properties (used by commands.rs).
#[cfg(feature = "occt")]
pub fn build_occt_solid_for_mass(doc: &Document, feature_id: FeatureId) -> Option<cadrum::Solid> {
    build_occt_solid(doc, feature_id)
}

#[cfg(feature = "occt")]
pub fn occt_boolean_via_doc(
    doc: &Document,
    target_a: FeatureId,
    target_b: FeatureId,
    op: echi_core::feature::BoolOp,
) -> Option<Mesh> {
    use cadrum::{Boolean, Solid as OcctSolid};

    let solid_a = build_occt_solid(doc, target_a)?;
    let solid_b = build_occt_solid(doc, target_b)?;

    let boolean_result = match op {
        echi_core::feature::BoolOp::Union =>
            (Boolean::from(&solid_a) + &solid_b).build().ok()?,
        echi_core::feature::BoolOp::Subtract =>
            (Boolean::from(&solid_a) - &solid_b).build().ok()?,
        echi_core::feature::BoolOp::Intersect =>
            (Boolean::from(&solid_a) * &solid_b).build().ok()?,
    };

    let occt_mesh = OcctSolid::mesh(
        std::iter::once(&boolean_result),
        cadrum::Tessellation { deflection_linear: 0.1, relative_linear: false, ..Default::default() },
    ).ok()?;
    Some(cadrum_mesh_to_echi(&occt_mesh))
}

#[cfg(feature = "occt")]
pub fn occt_fillet_via_doc(
    doc: &Document, target_id: FeatureId, radius: f64,
    mesh: &Mesh, edges: &[(u32, u32)],
) -> Option<Mesh> {
    occt_bevel_via_doc(doc, target_id, radius, mesh, edges, false)
}

#[cfg(feature = "occt")]
pub fn occt_chamfer_via_doc(
    doc: &Document, target_id: FeatureId, distance: f64,
    mesh: &Mesh, edges: &[(u32, u32)],
) -> Option<Mesh> {
    occt_bevel_via_doc(doc, target_id, distance, mesh, edges, true)
}

#[cfg(feature = "occt")]
fn occt_bevel_via_doc(
    doc: &Document, target_id: FeatureId, amount: f64,
    target_mesh: &Mesh, edge_pairs: &[(u32, u32)], is_chamfer: bool,
) -> Option<Mesh> {
    use cadrum::{DVec3, Solid as OcctSolid, Tessellation};

    let solid = build_occt_solid(doc, target_id)?;
    let occt_edges: Vec<_> = solid.iter_edge().collect();
    if occt_edges.is_empty() { return None; }

    let all_edges: Vec<&cadrum::Edge> = occt_edges.iter().copied().collect();
    let selected: Vec<&cadrum::Edge> = if edge_pairs.is_empty() {
        all_edges
    } else {
        edge_pairs.iter().filter_map(|&(vi, vj)| {
            let pi = vi as usize * 3; let pj = vj as usize * 3;
            if pi + 2 >= target_mesh.positions.len() || pj + 2 >= target_mesh.positions.len() { return None; }
            let mx = (target_mesh.positions[pi] + target_mesh.positions[pj]) / 2.0;
            let my = (target_mesh.positions[pi + 1] + target_mesh.positions[pj + 1]) / 2.0;
            let mz = (target_mesh.positions[pi + 2] + target_mesh.positions[pj + 2]) / 2.0;
            let midpoint = DVec3::new(mx as f64, my as f64, mz as f64);
            occt_edges.iter()
                .min_by(|a, b| {
                    let em_a = { let s = a.start_point(); let e = a.end_point(); DVec3::new((s.x+e.x)/2.0, (s.y+e.y)/2.0, (s.z+e.z)/2.0) };
                    let em_b = { let s = b.start_point(); let e = b.end_point(); DVec3::new((s.x+e.x)/2.0, (s.y+e.y)/2.0, (s.z+e.z)/2.0) };
                    (em_a - midpoint).length_squared().partial_cmp(&(em_b - midpoint).length_squared()).unwrap_or(std::cmp::Ordering::Equal)
                }).copied()
        }).collect()
    };
    if selected.is_empty() { return None; }

    let result = if is_chamfer {
        solid.chamfer_edges(amount, selected).ok()?
    } else {
        solid.fillet_edges(amount, selected).ok()?
    };
    let occt_mesh = OcctSolid::mesh(std::iter::once(&result),
        Tessellation { deflection_linear: 0.1, relative_linear: false, ..Default::default() }).ok()?;
    Some(cadrum_mesh_to_echi(&occt_mesh))
}

#[cfg(feature = "occt")]
pub fn occt_shell_via_doc(doc: &Document, target_id: FeatureId, thickness: f64) -> Option<Mesh> {
    use cadrum::{Solid as OcctSolid, Tessellation};
    let solid = build_occt_solid(doc, target_id)?;
    let shelled = solid.shell(thickness, std::iter::empty()).ok()?;
    let occt_mesh = OcctSolid::mesh(std::iter::once(&shelled),
        Tessellation { deflection_linear: 0.1, relative_linear: false, ..Default::default() }).ok()?;
    Some(cadrum_mesh_to_echi(&occt_mesh))
}

#[cfg(feature = "occt")]
pub fn occt_sweep_via_sketches(profile_sketch: &Sketch, path_sketch: &Sketch) -> Option<Mesh> {
    use cadrum::{DVec3, ProfileOrient, Solid as OcctSolid};
    let profile_loops = extract_loops(profile_sketch)?;
    let path_loops = extract_loops(path_sketch)?;
    let profile_pts = profile_loops.first()?;
    let path_pts = path_loops.first()?;
    let pts: Vec<DVec3> = profile_pts.iter().map(|p| DVec3::new(p.x, p.y, 0.0)).collect();
    let profile_edges = cadrum::Edge::polygon(&pts).ok()?;
    let spine_pts: Vec<DVec3> = path_pts.iter().map(|p| DVec3::new(p.x, p.y, 0.0)).collect();
    if spine_pts.len() < 2 { return None; }
    let spine_edges: Vec<_> = spine_pts.windows(2)
        .filter_map(|w| cadrum::Edge::polygon(&[w[0], w[1]]).ok()).flatten().collect();
    if spine_edges.is_empty() { return None; }
    let solid = OcctSolid::sweep(&profile_edges, &spine_edges, ProfileOrient::Fixed).ok()?;
    let occt_mesh = OcctSolid::mesh(std::iter::once(&solid),
        cadrum::Tessellation { deflection_linear: 0.1, relative_linear: false, ..Default::default() }).ok()?;
    Some(cadrum_mesh_to_echi(&occt_mesh))
}

#[cfg(feature = "occt")]
pub fn occt_revolve_via_sketch(
    sketch: &Sketch, angle_rad: f64,
    axis_start: Option<(f64, f64)>, axis_end: Option<(f64, f64)>,
) -> Option<Mesh> {
    use cadrum::{DVec3, ProfileOrient, Solid as OcctSolid};
    let loops = extract_loops(sketch)?; let profile_pts = loops.first()?;
    let (ax, ay, bx, by) = match (axis_start, axis_end) {
        (Some(s), Some(e)) => (s.0, s.1, e.0, e.1), _ => (0.0, 0.0, 0.0, 1.0),
    };
    let ux = bx - ax; let uy = by - ay;
    let axis_len = (ux*ux + uy*uy).sqrt();
    if axis_len < 1e-10 { return None; }
    let max_radius = profile_pts.iter().map(|p| {
        let dx = p.x - ax; let dy = p.y - ay;
        let axial = (dx*ux + dy*uy)/axis_len;
        let radial = (dx*(-uy/axis_len) + dy*(ux/axis_len)).abs();
        (axial*axial + radial*radial).sqrt()
    }).fold(0.0_f64, f64::max).max(0.1);
    let n = 32; let step = angle_rad / n as f64;
    let spine_pts: Vec<DVec3> = (0..=n).map(|i| {
        let a = i as f64 * step;
        DVec3::new(ax + max_radius*a.cos()*(-uy/axis_len), ay + max_radius*a.cos()*(ux/axis_len), max_radius*a.sin())
    }).collect();
    let spine_edges: Vec<_> = spine_pts.windows(2)
        .filter_map(|w| cadrum::Edge::polygon(&[w[0], w[1]]).ok()).flatten().collect();
    if spine_edges.is_empty() { return None; }
    let pts: Vec<DVec3> = profile_pts.iter().map(|p| DVec3::new(p.x, p.y, 0.0)).collect();
    let profile_edges = cadrum::Edge::polygon(&pts).ok()?;
    let solid = OcctSolid::sweep(&profile_edges, &spine_edges, ProfileOrient::Fixed).ok()?;
    let occt_mesh = OcctSolid::mesh(std::iter::once(&solid),
        cadrum::Tessellation { deflection_linear: 0.1, relative_linear: false, ..Default::default() }).ok()?;
    Some(cadrum_mesh_to_echi(&occt_mesh))
}

#[cfg(feature = "occt")]
pub fn occt_extrude_loops(
    loops: &[Vec<echi_geom::extrude::Point2D>], height: f64, direction: ExtrudeDirection,
) -> Option<Mesh> {
    use cadrum::{DVec3, Solid as OcctSolid};
    let (h1, h2_offset) = match direction {
        ExtrudeDirection::OneSide => (height, 0.0),
        ExtrudeDirection::Midplane => (height/2.0, height/2.0),
        ExtrudeDirection::TwoSides { dist1, dist2 } => (dist1.max(dist2), 0.0),
    };
    // Sort loops by area (largest first) to reliably identify the outer loop.
    // extract_loops returns loops in EntityId order, which is NOT guaranteed
    // to be outer-first. Without sorting, concentric circles drawn inner-first
    // would produce a thin inner cylinder instead of a hollow tube.
    use echi_geom::extrude::polygon_area;
    let mut sorted: Vec<&Vec<echi_geom::extrude::Point2D>> = loops.iter().collect();
    sorted.sort_by(|a, b| {
        let area_b = polygon_area(b).abs();
        let area_a = polygon_area(a).abs();
        area_b.partial_cmp(&area_a).unwrap_or(std::cmp::Ordering::Equal)
    });

    let outer_pts: Vec<DVec3> = sorted[0].iter().map(|p| DVec3::new(p.x, p.y, 0.0)).collect();
    let outer_edges = cadrum::Edge::polygon(&outer_pts).ok()?;
    let mut solid = OcctSolid::extrude(&outer_edges, DVec3::new(0.0, 0.0, h1)).ok()?;
    for hole_pts in sorted.iter().skip(1) {
        let pts: Vec<DVec3> = hole_pts.iter().map(|p| DVec3::new(p.x, p.y, -1.0)).collect();
        // Boolean failure must NOT be silently skipped — it means the hole
        // wasn't punched, producing a solid cylinder where a tube was expected.
        // Return None to fall through to the mesh extrusion path.
        let edges = cadrum::Edge::polygon(&pts).ok()?;
        let hole = OcctSolid::extrude(&edges, DVec3::new(0.0, 0.0, h1+2.0)).ok()?;
        solid = (cadrum::Boolean::from(&solid) - &hole).build().ok()?;
    }
    let occt_mesh = OcctSolid::mesh(std::iter::once(&solid),
        cadrum::Tessellation { deflection_linear: 0.1, relative_linear: false, ..Default::default() }).ok()?;
    let mut mesh = cadrum_mesh_to_echi(&occt_mesh);
    if h2_offset > 0.0 {
        let pts: Vec<ProfilePoint> = sorted[0].iter().map(|p| ProfilePoint::new(p.x, p.y)).collect();
        if let Ok(upper) = crate::MockBrepKernel.extrude_mesh(&pts, h1) {
            let mut combined = Mesh::default();
            copy_mesh(&mesh, &mut combined, 0.0);
            copy_mesh(&upper, &mut combined, h2_offset);
            mesh = combined;
        }
    }
    Some(mesh)
}

#[cfg(feature = "occt")]
pub fn occt_pattern(
    doc: &Document, target_id: FeatureId,
    transform: impl Fn(cadrum::Solid, usize) -> Option<cadrum::Solid>, count: u32,
) -> Option<Mesh> {
    use cadrum::{Solid as OcctSolid, Tessellation};
    if count < 2 { return None; }
    let base = build_occt_solid(doc, target_id)?;
    let mut combined: Option<OcctSolid> = None;
    for i in 0..count {
        let instance = if i == 0 { base.clone() } else { transform(base.clone(), i as usize)? };
        combined = match combined {
            None => Some(instance),
            Some(c) => (cadrum::Boolean::from(&c) + &instance).build().ok(),
        };
    }
    let result = combined?;
    let occt_mesh = OcctSolid::mesh(std::iter::once(&result),
        Tessellation { deflection_linear: 0.1, relative_linear: false, ..Default::default() }).ok()?;
    Some(cadrum_mesh_to_echi(&occt_mesh))
}

#[cfg(feature = "occt")]
pub fn occt_mirror_via_doc(
    doc: &Document, target_id: FeatureId,
    nx: f64, ny: f64, nz: f64, px: f64, py: f64, pz: f64,
) -> Option<Mesh> {
    use cadrum::{Solid as OcctSolid, Tessellation, DVec3};
    let base = build_occt_solid(doc, target_id)?;
    let mirrored = base.clone().mirror(DVec3::new(px, py, pz), DVec3::new(nx, ny, nz));
    let combined = (cadrum::Boolean::from(&base) + &mirrored).build().ok()?;
    let occt_mesh = OcctSolid::mesh(std::iter::once(&combined),
        Tessellation { deflection_linear: 0.1, relative_linear: false, ..Default::default() }).ok()?;
    Some(cadrum_mesh_to_echi(&occt_mesh))
}
