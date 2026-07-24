//! Extrude a 2D sketch profile into a 3D triangle mesh.

use echi_core::feature::{ExtrudeDirection, PlaneDefinition};
use echi_core::sketch::{EntityId, Sketch, SketchEntity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 3D mesh data for a solid.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Mesh {
    pub positions: Vec<f32>, // xyz per vertex
    pub normals: Vec<f32>,   // nx ny nz per vertex
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }

    pub fn add_vertex(&mut self, x: f64, y: f64, z: f64, nx: f64, ny: f64, nz: f64) -> u32 {
        let idx = self.vertex_count() as u32;
        self.positions.extend([x as f32, y as f32, z as f32]);
        self.normals.extend([nx as f32, ny as f32, nz as f32]);
        idx
    }

    pub fn add_triangle(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend([a, b, c]);
    }
}

/// Extrude a closed sketch profile with direction, draft angle, and plane support.
///
/// Handles multiple disjoint closed loops (outer + holes). The outer loop is
/// detected as the loop with the largest absolute area; other loops are
/// treated as holes and triangulated using the even-odd rule.
pub fn extrude(
    sketch: &Sketch,
    height: f64,
    direction: ExtrudeDirection,
    draft_angle_deg: f64,
    plane: &PlaneDefinition,
) -> Option<Mesh> {
    extrude_inner(sketch, height, direction, draft_angle_deg, plane, None)
}

/// Like [`extrude`] but only extrudes the regions at the given indices.
/// Indices correspond to those returned by the frontend region picker.
/// `None` for `selected_regions` extrudes all regions.
pub fn extrude_selected(
    sketch: &Sketch,
    height: f64,
    direction: ExtrudeDirection,
    draft_angle_deg: f64,
    plane: &PlaneDefinition,
    selected_regions: &[usize],
) -> Option<Mesh> {
    extrude_inner(sketch, height, direction, draft_angle_deg, plane, Some(selected_regions))
}

fn extrude_inner(
    sketch: &Sketch,
    height: f64,
    direction: ExtrudeDirection,
    draft_angle_deg: f64,
    plane: &PlaneDefinition,
    selected_regions: Option<&[usize]>,
) -> Option<Mesh> {
    let mut loops = extract_loops(sketch)?;
    if loops.is_empty() || height.abs() < 1e-10 {
        return None;
    }

    // When specific loop indices are selected, filter to just those loops.
    // Indices correspond to the sorted-by-area order returned by get_extrude_regions.
    // extrude_loops will classify them as outer/hole at extrude time.
    if let Some(indices) = selected_regions {
        let mut indexed: Vec<(Vec<Point2D>, f64)> = loops
            .into_iter()
            .map(|l| {
                let area = polygon_area(&l).abs();
                (l, area)
            })
            .collect();
        // Sort by area descending (largest first) — must match get_extrude_regions
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        loops = indices.iter()
            .filter_map(|&idx| indexed.get(idx).map(|(pts, _)| pts.clone()))
            .collect();

        if loops.is_empty() {
            return None;
        }
    }

    let mut mesh = match direction {
        ExtrudeDirection::OneSide => extrude_loops(&loops, height, 0.0, draft_angle_deg)?,
        ExtrudeDirection::Midplane => {
            let half = height / 2.0;
            let lower = extrude_loops(&loops, half, -half, draft_angle_deg)?;
            let upper = extrude_loops(&loops, half, 0.0, -draft_angle_deg)?;
            merge_mesh_pair(lower, upper)
        }
        ExtrudeDirection::TwoSides { dist1, dist2 } => {
            let lower = extrude_loops(&loops, dist1, -dist1, draft_angle_deg)?;
            let upper = extrude_loops(&loops, dist2, 0.0, -draft_angle_deg)?;
            merge_mesh_pair(lower, upper)
        }
    };
    transform_mesh_to_world(&mut mesh, plane);
    Some(mesh)
}

/// Extrude a list of loops (one outer + zero or more holes) into a single mesh.
/// `z_min` is the bottom-Z of the extrusion; height is added to it.
pub fn extrude_loops(loops: &[Vec<Point2D>], height: f64, z_min: f64, draft_angle_deg: f64) -> Option<Mesh> {
    if loops.is_empty() || height.abs() < 1e-10 {
        return None;
    }

    // Classify loops: largest-area loops are candidate outers; a smaller loop
    // is a hole if its centroid lies inside a larger loop. Loops that are not
    // contained in any larger loop are their own outers. This correctly handles
    // concentric circles (outer + hole), separate circles (two outers), and
    // mixed geometry like a rectangle with circular holes.
    //
    // We enforce outer = CCW and holes = CW for the triangulator.
    let mut classified: Vec<(Vec<Point2D>, f64, Point2D)> = loops
        .iter()
        .map(|l| {
            let area = polygon_area(l);
            let ccw = area > 0.0;
            let mut l = l.clone();
            if !ccw {
                l.reverse();
            }
            let centroid = polygon_centroid(&l);
            (l, area.abs(), centroid)
        })
        .collect();
    classified.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Group loops: each outer gets its contained holes.
    let mut outers: Vec<Vec<Point2D>> = Vec::new();
    let mut hole_groups: Vec<Vec<Vec<Point2D>>> = Vec::new();

    for (i, (_loop_pts, _area, centroid)) in classified.iter().enumerate() {
        // Check if this loop is contained inside any already-registered outer.
        let mut is_hole_of: Option<usize> = None;
        for (j, outer_pts) in outers.iter().enumerate() {
            if point_in_polygon(centroid, outer_pts) {
                is_hole_of = Some(j);
                break;
            }
        }
        if let Some(outer_idx) = is_hole_of {
            // Reverse to CW for hole convention
            let mut h = classified[i].0.clone();
            h.reverse();
            hole_groups[outer_idx].push(h);
        } else {
            outers.push(classified[i].0.clone());
            hole_groups.push(Vec::new());
        }
    }

    // Extrude each outer+holes group and combine into a single mesh.
    let mut combined = Mesh::default();
    for (i, outer) in outers.iter().enumerate() {
        let holes = &hole_groups[i];
        if let Some(mesh) = extrude_with_holes(outer, holes, height, z_min, draft_angle_deg) {
            let voff = combined.vertex_count() as u32;
            combined.positions.extend_from_slice(&mesh.positions);
            combined.normals.extend_from_slice(&mesh.normals);
            for &idx in &mesh.indices {
                combined.indices.push(voff + idx);
            }
        }
    }

    if combined.vertex_count() == 0 {
        None
    } else {
        Some(combined)
    }
}

/// Extrude an outer polygon with optional holes. Uses even-odd rule during
/// triangulation so holes are punched correctly in both caps. Side walls are
/// generated for both the outer and inner contours.
fn extrude_with_holes(
    outer: &[Point2D],
    holes: &[Vec<Point2D>],
    height: f64,
    z_min: f64,
    draft_angle_deg: f64,
) -> Option<Mesh> {
    if outer.len() < 3 || height.abs() < 1e-10 {
        return None;
    }

    let top_z = z_min + height;
    let draft = draft_angle_deg.to_radians();
    let n_pts = outer.len();

    let n = n_pts as f64;
    let (cx, cy) = outer.iter().fold((0.0, 0.0), |(ax, ay), p| (ax + p.x, ay + p.y));
    let cx = cx / n;
    let cy = cy / n;

    let max_dist = outer.iter()
        .map(|p| ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt())
        .fold(0.0f64, f64::max)
        .max(1e-6);

    let draft_scale = if draft.abs() > 1e-6 {
        (1.0 - height.abs() * draft.tan() / max_dist).max(0.01)
    } else {
        1.0
    };

    let mut mesh = Mesh::default();

    // Cap triangulation (used for both bottom and top)
    let triangles = triangulate_with_holes(outer, holes);

    // Outer bottom/top vertices
    let bottom_outer: Vec<u32> = outer.iter()
        .map(|p| mesh.add_vertex(p.x, p.y, z_min, 0.0, 0.0, -1.0))
        .collect();
    let top_outer: Vec<u32> = outer.iter()
        .map(|p| {
            let tx = cx + (p.x - cx) * draft_scale;
            let ty = cy + (p.y - cy) * draft_scale;
            mesh.add_vertex(tx, ty, top_z, 0.0, 0.0, 1.0)
        })
        .collect();

    // Hole bottom/top vertices (no draft; holes would otherwise self-intersect)
    let mut bottom_holes: Vec<Vec<u32>> = Vec::new();
    let mut top_holes: Vec<Vec<u32>> = Vec::new();
    for hole in holes {
        let b: Vec<u32> = hole.iter()
            .map(|p| mesh.add_vertex(p.x, p.y, z_min, 0.0, 0.0, -1.0))
            .collect();
        let t: Vec<u32> = hole.iter()
            .map(|p| {
                let tx = cx + (p.x - cx) * draft_scale;
                let ty = cy + (p.y - cy) * draft_scale;
                mesh.add_vertex(tx, ty, top_z, 0.0, 0.0, 1.0)
            })
            .collect();
        bottom_holes.push(b);
        top_holes.push(t);
    }

    // Emit caps using the triangulation indices.
    // We need to know which loop each index refers to so we can pick the
    // right vertex buffer. triangulate_with_holes returns indices in a
    // unified numbering: outer vertices 0..outer.len(), then concatenated
    // hole vertices.
    let offset_outer = 0usize;
    let mut hole_offsets: Vec<usize> = Vec::new();
    let mut acc = outer.len();
    for hole in holes {
        hole_offsets.push(acc);
        acc += hole.len();
    }
    let _ = offset_outer;

    let bottom_lookup = |global_idx: usize| -> u32 {
        if global_idx < outer.len() {
            return bottom_outer[global_idx];
        }
        for (hi, off) in hole_offsets.iter().enumerate() {
            let next = off + holes[hi].len();
            if global_idx < next {
                return bottom_holes[hi][global_idx - off];
            }
        }
        0
    };
    let top_lookup = |global_idx: usize| -> u32 {
        if global_idx < outer.len() {
            return top_outer[global_idx];
        }
        for (hi, off) in hole_offsets.iter().enumerate() {
            let next = off + holes[hi].len();
            if global_idx < next {
                return top_holes[hi][global_idx - off];
            }
        }
        0
    };

    for [i, j, k] in &triangles {
        let bi = bottom_lookup(*i);
        let bj = bottom_lookup(*j);
        let bk = bottom_lookup(*k);
        mesh.add_triangle(bi, bk, bj);
        let ti = top_lookup(*i);
        let tj = top_lookup(*j);
        let tk = top_lookup(*k);
        mesh.add_triangle(ti, tj, tk);
    }

    // Outer side walls
    add_side_walls(&mut mesh, outer, &bottom_outer, &top_outer, z_min, top_z, cx, cy, draft_scale);
    // Hole side walls (note: normals point inward into the hole)
    for (h_idx, hole) in holes.iter().enumerate() {
        let b = &bottom_holes[h_idx];
        let t = &top_holes[h_idx];
        add_side_walls(&mut mesh, hole, b, t, z_min, top_z, cx, cy, draft_scale);
    }

    Some(mesh)
}

/// Generate side-wall quads between `bottom` and `top` vertex rings following
/// the order of `polygon`.
fn add_side_walls(
    mesh: &mut Mesh,
    polygon: &[Point2D],
    bottom: &[u32],
    top: &[u32],
    _z_min: f64,
    _top_z: f64,
    _cx: f64,
    _cy: f64,
    _draft_scale: f64,
) {
    let n = polygon.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let p0 = &polygon[i];
        let p1 = &polygon[j];
        let dx = p1.x - p0.x;
        let dy = p1.y - p0.y;
        let len = (dx * dx + dy * dy).sqrt();
        let (nx, ny) = if len > 0.0 { (dy / len, -dx / len) } else { (0.0, 0.0) };

        // Snapshot the positions of the four cap vertices we'll bridge.
        let v_pos = |mesh: &Mesh, v: u32| -> [f64; 3] {
            let i = (v as usize) * 3;
            [
                mesh.positions[i] as f64,
                mesh.positions[i + 1] as f64,
                mesh.positions[i + 2] as f64,
            ]
        };
        let p0b = v_pos(mesh, bottom[i]);
        let p1b = v_pos(mesh, bottom[j]);
        let p0t = v_pos(mesh, top[i]);
        let p1t = v_pos(mesh, top[j]);

        let b0 = mesh.add_vertex(p0b[0], p0b[1], p0b[2], nx, ny, 0.0);
        let b1 = mesh.add_vertex(p1b[0], p1b[1], p1b[2], nx, ny, 0.0);
        let t0 = mesh.add_vertex(p0t[0], p0t[1], p0t[2], nx, ny, 0.0);
        let t1 = mesh.add_vertex(p1t[0], p1t[1], p1t[2], nx, ny, 0.0);

        mesh.add_triangle(b0, b1, t1);
        mesh.add_triangle(b0, t1, t0);
    }
}

/// Triangulate a polygon with holes using ear clipping with the even-odd rule.
/// Returns triangles as indices into a flat array whose first `outer.len()`
/// entries are the outer vertices and subsequent entries are the holes'
/// vertices in the order given.
fn triangulate_with_holes(outer: &[Point2D], holes: &[Vec<Point2D>]) -> Vec<[usize; 3]> {
    // If no holes, fall back to the simple ear clip.
    if holes.is_empty() {
        return triangulate_ear_clip(outer);
    }

    // Build a single indexed polygon: outer first, then each hole.
    // We use the bridge-edge approach: connect each hole to the outer by
    // finding the closest pair of vertices, then split the resulting
    // degenerate polygon and ear-clip it.

    // Build unified vertex list and the offset of each hole.
    let mut all: Vec<Point2D> = outer.to_vec();
    let mut hole_starts: Vec<usize> = Vec::new();
    for hole in holes {
        hole_starts.push(all.len());
        all.extend_from_slice(hole);
    }

    // Bridge each hole into the outer ring using duplicate vertices at the
    // bridge endpoints. Start from the last hole and work backwards so the
    // earlier hole_starts indices stay valid.
    let mut polygon: Vec<usize> = (0..all.len()).collect();

    for (hi, hole_start) in hole_starts.iter().enumerate().rev() {
        let hole = &holes[hi];
        // Find closest pair (outer_or_existing_polygon_vertex, hole_vertex)
        let mut best: Option<(usize, usize, f64)> = None;
        for (i, &pi) in polygon.iter().enumerate() {
            let pi_inside_hole = hole.iter().position(|p| (p.x - all[pi].x).abs() < 1e-12 && (p.y - all[pi].y).abs() < 1e-12).is_some();
            if pi_inside_hole {
                continue;
            }
            for (hj, hp) in hole.iter().enumerate() {
                let global_hj = hole_start + hj;
                let d = (all[pi].x - hp.x).powi(2) + (all[pi].y - hp.y).powi(2);
                if best.map(|(_, _, bd)| d < bd).unwrap_or(true) {
                    best = Some((i, global_hj, d));
                }
            }
        }
        let Some((poly_idx, hole_global, _)) = best else { continue };
        // Insert bridge: duplicate vertex at poly_idx, then jump to hole, walk
        // around the hole, back to hole_global, back to poly_idx.
        // Concretely, replace polygon[poly_idx] with:
        //   [polygon[poly_idx], hole_global, hole_global+1, ..., hole_global + hole.len()-1, hole_global, polygon[poly_idx]]
        let hole_len = hole.len();
        let _bridge_start_global = hole_global;
        let mut inserted: Vec<usize> = Vec::with_capacity(hole_len + 3);
        inserted.push(polygon[poly_idx]); // outer vertex (start of bridge)
        // Walk the hole starting from `hole_global`
        for k in 0..hole_len {
            inserted.push(hole_start + (k + (hole_global - hole_start)) % hole_len);
        }
        inserted.push(hole_global); // close hole loop
        // Back to outer: push polygon[poly_idx] again
        inserted.push(polygon[poly_idx]);
        // Replace polygon[poly_idx] with inserted
        polygon.splice(poly_idx..=poly_idx, inserted);
    }

    // Now ear-clip `polygon` whose vertices are indices into `all`.
    // Use the same algorithm as triangulate_ear_clip but on the index list.
    ear_clip_indexed(&all, &polygon)
}

fn ear_clip_indexed(points: &[Point2D], polygon: &[usize]) -> Vec<[usize; 3]> {
    let mut indices: Vec<usize> = polygon.to_vec();
    let mut triangles = Vec::new();
    let max_iter = indices.len() * 10;

    for _ in 0..max_iter {
        let m = indices.len();
        if m < 3 {
            break;
        }
        if m == 3 {
            triangles.push([indices[0], indices[1], indices[2]]);
            break;
        }

        let mut ear_found = false;
        for i in 0..m {
            let prev_idx = indices[(i + m - 1) % m];
            let curr_idx = indices[i];
            let next_idx = indices[(i + 1) % m];

            let prev = points[prev_idx];
            let curr = points[curr_idx];
            let next = points[next_idx];

            if cross(&prev, &curr, &next) < 1e-12 {
                continue;
            }

            let mut contains_point = false;
            for &idx in &indices {
                if idx == prev_idx || idx == curr_idx || idx == next_idx {
                    continue;
                }
                if point_in_triangle(&points[idx], &prev, &curr, &next) {
                    contains_point = true;
                    break;
                }
            }
            if contains_point {
                continue;
            }

            triangles.push([prev_idx, curr_idx, next_idx]);
            indices.remove(i);
            ear_found = true;
            break;
        }
        if !ear_found {
            if indices.len() >= 3 {
                let prev_idx = indices[0];
                let curr_idx = indices[1];
                let next_idx = indices[2];
                triangles.push([prev_idx, curr_idx, next_idx]);
                indices.remove(1);
            } else {
                break;
            }
        }
    }
    triangles
}

/// Transform a mesh from local sketch coordinates to world coordinates.
pub fn transform_mesh_to_world(mesh: &mut Mesh, plane: &PlaneDefinition) {
    if matches!(plane, PlaneDefinition::XY) {
        return;
    }

    let (ox, oy, oz, _, _, _) = plane.resolve();
    let u = plane.u_dir();
    let v = plane.v_dir();

    for i in (0..mesh.positions.len()).step_by(3) {
        let sx = mesh.positions[i] as f64;
        let sy = mesh.positions[i + 1] as f64;
        let sz = mesh.positions[i + 2] as f64;

        // Local coordinates are (sketch_u, sketch_v, extrude_axis) — i.e. sx/sy
        // lie in the sketch plane and sz is along the plane normal.
        let n = plane.extrude_dir();
        let wx = ox + u[0] * sx + v[0] * sy + n.0 * sz;
        let wy = oy + u[1] * sx + v[1] * sy + n.1 * sz;
        let wz = oz + u[2] * sx + v[2] * sy + n.2 * sz;

        mesh.positions[i] = wx as f32;
        mesh.positions[i + 1] = wy as f32;
        mesh.positions[i + 2] = wz as f32;
    }

    // Transform normals using the same frame.
    for i in (0..mesh.normals.len()).step_by(3) {
        let nx = mesh.normals[i] as f64;
        let ny = mesh.normals[i + 1] as f64;
        let nz = mesh.normals[i + 2] as f64;
        let n = plane.extrude_dir();
        let wx = u[0] * nx + v[0] * ny + n.0 * nz;
        let wy = u[1] * nx + v[1] * ny + n.1 * nz;
        let wz = u[2] * nx + v[2] * ny + n.2 * nz;
        let wlen = (wx * wx + wy * wy + wz * wz).sqrt().max(1e-12);
        mesh.normals[i] = (wx / wlen) as f32;
        mesh.normals[i + 1] = (wy / wlen) as f32;
        mesh.normals[i + 2] = (wz / wlen) as f32;
    }
}

fn merge_mesh_pair(a: Mesh, b: Mesh) -> Mesh {
    let mut m = Mesh::default();
    m.positions = a.positions;
    m.normals = a.normals;
    m.indices = a.indices;

    let b_off = (m.positions.len() / 3) as u32;
    m.positions.extend_from_slice(&b.positions);
    m.normals.extend_from_slice(&b.normals);
    for &idx in &b.indices {
        m.indices.push(b_off + idx);
    }
    m
}

/// Compute polygon area using the shoelace formula.
/// Positive => CCW, negative => CW.
pub fn polygon_area(polygon: &[Point2D]) -> f64 {
    let n = polygon.len();
    let mut area = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        area += polygon[i].x * polygon[j].y;
        area -= polygon[j].x * polygon[i].y;
    }
    area / 2.0
}

/// Compute the centroid of a polygon using the shoelace-based formula.
pub fn polygon_centroid(polygon: &[Point2D]) -> Point2D {
    let n = polygon.len();
    if n == 0 {
        return Point2D { x: 0.0, y: 0.0 };
    }
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut area = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        let cross = polygon[i].x * polygon[j].y - polygon[j].x * polygon[i].y;
        area += cross;
        cx += (polygon[i].x + polygon[j].x) * cross;
        cy += (polygon[i].y + polygon[j].y) * cross;
    }
    area *= 0.5;
    if area.abs() < 1e-12 {
        // Degenerate polygon — fall back to average of vertices
        let sum_x: f64 = polygon.iter().map(|p| p.x).sum();
        let sum_y: f64 = polygon.iter().map(|p| p.y).sum();
        return Point2D { x: sum_x / n as f64, y: sum_y / n as f64 };
    }
    Point2D {
        x: cx / (6.0 * area),
        y: cy / (6.0 * area),
    }
}

/// Ray-casting point-in-polygon test. Returns true if the point is inside
/// or on the boundary of the polygon.
pub fn point_in_polygon(point: &Point2D, polygon: &[Point2D]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let yi = polygon[i].y;
        let yj = polygon[j].y;
        if (yi > point.y) != (yj > point.y) {
            let intersect_x = polygon[i].x
                + (point.y - yi) * (polygon[j].x - polygon[i].x) / (yj - yi);
            if point.x < intersect_x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

#[derive(Debug, Clone, Copy)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

fn cross(o: &Point2D, a: &Point2D, b: &Point2D) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn point_in_triangle(p: &Point2D, a: &Point2D, b: &Point2D, c: &Point2D) -> bool {
    let d1 = cross(a, b, p);
    let d2 = cross(b, c, p);
    let d3 = cross(c, a, p);
    let has_neg = d1 < -1e-12 || d2 < -1e-12 || d3 < -1e-12;
    let has_pos = d1 > 1e-12 || d2 > 1e-12 || d3 > 1e-12;
    !(has_neg && has_pos)
}

/// Ear-clipping triangulation with improved robustness.
fn triangulate_ear_clip(polygon: &[Point2D]) -> Vec<[usize; 3]> {
    let n = polygon.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }

    let mut indices: Vec<usize> = (0..n).collect();
    let mut triangles = Vec::new();
    let max_iter = n * 10;

    for _iter in 0..max_iter {
        let m = indices.len();
        if m < 3 {
            break;
        }
        if m == 3 {
            triangles.push([indices[0], indices[1], indices[2]]);
            break;
        }

        let mut ear_found = false;

        for i in 0..m {
            let prev_idx = indices[(i + m - 1) % m];
            let curr_idx = indices[i];
            let next_idx = indices[(i + 1) % m];

            let prev = polygon[prev_idx];
            let curr = polygon[curr_idx];
            let next = polygon[next_idx];

            // Must be a convex vertex (CCW polygon => cross > 0)
            if cross(&prev, &curr, &next) < 1e-12 {
                continue;
            }

            // Check no other vertex lies inside this ear triangle
            let mut contains_point = false;
            for &idx in &indices {
                if idx == prev_idx || idx == curr_idx || idx == next_idx {
                    continue;
                }
                if point_in_triangle(&polygon[idx], &prev, &curr, &next) {
                    contains_point = true;
                    break;
                }
            }

            if contains_point {
                continue;
            }

            triangles.push([prev_idx, curr_idx, next_idx]);
            indices.remove(i);
            ear_found = true;
            break;
        }

        if !ear_found {
            // Fallback: try to remove any triangle even if not strictly convex.
            if indices.len() >= 3 {
                let prev_idx = indices[(0 + m - 1) % m];
                let curr_idx = indices[0];
                let next_idx = indices[(0 + 1) % m];
                triangles.push([prev_idx, curr_idx, next_idx]);
                indices.remove(0);
            } else {
                break;
            }
        }
    }

    triangles
}

/// Tessellate a standalone circle entity into a polygon.
/// Returns Some(polygon) if the sketch contains a single circle with no lines.
fn tessellate_standalone_circle(sketch: &Sketch) -> Option<Vec<Point2D>> {
    let mut circle: Option<(f64, f64, f64)> = None;

    for entity in sketch.entities.values() {
        match entity {
            SketchEntity::Line { construction, .. } | SketchEntity::Spline { construction, .. } => {
                if !*construction {
                    return None;
                }
            }
            SketchEntity::Circle { center, radius, construction } => {
                if *construction {
                    continue;
                }
                if circle.is_some() {
                    return None;
                }
                if let Some(cp) = sketch.get_point(*center) {
                    circle = Some((cp.x, cp.y, *radius));
                }
            }
            _ => {}
        }
    }

    let (cx, cy, r) = circle?;
    if r <= 1e-10 {
        return None;
    }

    let n_seg = 64;
    let polygon: Vec<Point2D> = (0..n_seg)
        .map(|i| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n_seg as f64;
            Point2D { x: cx + r * a.cos(), y: cy + r * a.sin() }
        })
        .collect();

    Some(polygon)
}

/// Description of a connected edge in the profile walk.
#[derive(Debug, Clone, Copy)]
enum EdgeKind {
    Line,
    Arc {
        cx: f64,
        cy: f64,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    Ellipse {
        cx: f64,
        cy: f64,
        major_rx: f64,
        major_ry: f64,
        ratio: f64,
    },
}

/// An edge between two sketch points, possibly curved.
struct EdgeRef {
    #[allow(dead_code)]
    entity_id: EntityId,
    /// The two endpoints of the edge (unordered).
    a: EntityId,
    b: EntityId,
    kind: EdgeKind,
}

/// Extract all closed loops from a sketch. Each loop is a list of points
/// (tessellated along the way). The outer loop is whatever the user drew
/// first; the rest are treated as holes during extrude.
pub fn extract_loops(sketch: &Sketch) -> Option<Vec<Vec<Point2D>>> {
    if let Some(polygon) = tessellate_standalone_circle(sketch) {
        return Some(vec![polygon]);
    }

    // Collect all non-construction curve entities with their endpoints.
    // Iterate in deterministic (ascending EntityId) order: `sketch.entities`
    // is a HashMap whose iteration order is randomized per-process by the
    // SipHash seed. That randomness propagates into `edges` ordering, which
    // picks the loop-walk start point and traversal order — making loop
    // extraction (and therefore triangulation) nondeterministic across runs.
    // Sorting by EntityId makes extraction reproducible.
    let mut entity_ids: Vec<EntityId> = sketch.entities.keys().copied().collect();
    entity_ids.sort_unstable_by_key(|e| e.0);
    let mut edges: Vec<EdgeRef> = Vec::new();
    // Standalone circles are tessellated directly into completed polygon loops.
    let mut loops: Vec<Vec<Point2D>> = Vec::new();
    for id in entity_ids {
        let entity = match sketch.entities.get(&id) {
            Some(e) => e,
            None => continue,
        };
        match entity {
            SketchEntity::Line { start, end, construction } if !*construction => {
                edges.push(EdgeRef { entity_id: id, a: *start, b: *end, kind: EdgeKind::Line });
            }
            SketchEntity::Arc { center, radius, start_angle, end_angle, construction }
                if !*construction =>
            {
                if let Some(c) = sketch.get_point(*center) {
                    let sx = c.x + radius * start_angle.cos();
                    let sy = c.y + radius * start_angle.sin();
                    let ex = c.x + radius * end_angle.cos();
                    let ey = c.y + radius * end_angle.sin();
                    let s_id = find_or_synthesize_point(sketch, sx, sy);
                    let e_id = find_or_synthesize_point(sketch, ex, ey);
                    edges.push(EdgeRef {
                        entity_id: id,
                        a: s_id,
                        b: e_id,
                        kind: EdgeKind::Arc {
                            cx: c.x,
                            cy: c.y,
                            radius: *radius,
                            start_angle: *start_angle,
                            end_angle: *end_angle,
                        },
                    });
                }
            }
            SketchEntity::Ellipse { center, major_axis_end, ratio, construction }
                if !*construction =>
            {
                if let (Some(c), Some(m)) =
                    (sketch.get_point(*center), sketch.get_point(*major_axis_end))
                {
                    let major_rx = m.x - c.x;
                    let major_ry = m.y - c.y;
                    let start_id = find_or_synthesize_point(sketch, c.x + major_rx, c.y + major_ry);
                    let end_id = start_id;
                    edges.push(EdgeRef {
                        entity_id: id,
                        a: start_id,
                        b: end_id,
                        kind: EdgeKind::Ellipse {
                            cx: c.x,
                            cy: c.y,
                            major_rx,
                            major_ry,
                            ratio: *ratio,
                        },
                    });
                }
            }
            SketchEntity::Circle { center, radius, construction } if !*construction => {
                if let Some(cp) = sketch.get_point(*center) {
                    let n_seg = 64;
                    let mut polygon: Vec<Point2D> = Vec::with_capacity(n_seg + 1);
                    for i in 0..=n_seg {
                        let a = 2.0 * std::f64::consts::PI * i as f64 / n_seg as f64;
                        polygon.push(Point2D {
                            x: cp.x + radius * a.cos(),
                            y: cp.y + radius * a.sin(),
                        });
                    }
                    loops.push(polygon);
                }
            }
            SketchEntity::Spline { control_points, construction } if !*construction => {
                for w in control_points.windows(2) {
                    edges.push(EdgeRef {
                        entity_id: id,
                        a: w[0],
                        b: w[1],
                        kind: EdgeKind::Line,
                    });
                }
            }
            _ => {}
        }
    }

    // If we have directly-tessellated circles but no edges, we already have
    // complete loops and can skip the edge-walking phase.
    if edges.is_empty() {
        return if loops.is_empty() { None } else { Some(loops) };
    }

    // Build adjacency: point_id → list of (other_point, edge_index)
    let mut adjacency: HashMap<EntityId, Vec<(EntityId, usize)>> = HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        adjacency.entry(e.a).or_default().push((e.b, i));
        adjacency.entry(e.b).or_default().push((e.a, i));
    }

    // Find every closed loop by repeatedly walking until we exhaust edges.
    let mut used_edges = vec![false; edges.len()];

    while let Some(start_edge_idx) = used_edges.iter().position(|&u| !u) {
        let start_point = edges[start_edge_idx].a;
        let mut polygon: Vec<Point2D> = Vec::new();
        let mut current = start_point;
        let mut prev_point: Option<EntityId> = None;
        let mut iterations = 0;
        let max_iterations = edges.len() * 2 + 4;

        if let Some(p) = sketch.get_point(current) {
            polygon.push(Point2D { x: p.x, y: p.y });
        } else {
            // Synthetic point; mark all its edges used and skip
            used_edges[start_edge_idx] = true;
            continue;
        }

        loop {
            iterations += 1;
            if iterations > max_iterations {
                break;
            }
            let neighbors = match adjacency.get(&current) {
                Some(n) => n,
                None => break,
            };
            let next = neighbors.iter().find_map(|(n, ei)| {
                if used_edges[*ei] {
                    return None;
                }
                if prev_point == Some(*n) && neighbors.len() > 1 {
                    return None;
                }
                Some((*n, *ei))
            });

            match next {
                Some((n, ei)) => {
                    if n == start_point {
                        used_edges[ei] = true;
                        break;
                    }
                    used_edges[ei] = true;
                    let edge = &edges[ei];
                    tessellate_edge_into(edge, current, n, sketch, &mut polygon);
                    prev_point = Some(current);
                    current = n;
                }
                None => break,
            }
        }

        if polygon.len() >= 3 {
            loops.push(polygon);
        }
    }

    if loops.is_empty() {
        None
    } else {
        Some(loops)
    }
}

/// Find an existing point at (x, y) within tolerance, or synthesize a virtual
/// ID for arc/ellipse endpoints that aren't stored as separate points.
/// Returns a sentinel that will never collide with real EntityIds (we use the
/// high bit).
fn find_or_synthesize_point(sketch: &Sketch, x: f64, y: f64) -> EntityId {
    // Look for an existing point near (x, y).
    for (&id, entity) in &sketch.entities {
        if let SketchEntity::Point(p) = entity {
            if (p.x - x).abs() < 1e-6 && (p.y - y).abs() < 1e-6 {
                return id;
            }
        }
    }
    // Synthesize — we encode the coordinates into the EntityId so it's stable.
    // Use a high bit + low bits to make a unique sentinel.
    EntityId(((x.to_bits() as u64) ^ (y.to_bits() as u64).rotate_left(1)) | (1u64 << 63))
}

/// Append tessellated samples (excluding the start point, including the end
/// point) of `edge` to `polygon`.
fn tessellate_edge_into(
    edge: &EdgeRef,
    from: EntityId,
    to: EntityId,
    sketch: &Sketch,
    polygon: &mut Vec<Point2D>,
) {
    // Helper: does `id` correspond to a stored point? If so, get coords.
    let stored = |id: EntityId| -> Option<Point2D> {
        sketch.get_point(id).map(|p| Point2D { x: p.x, y: p.y })
    };

    match edge.kind {
        EdgeKind::Line => {
            if let Some(p_end) = stored(to) {
                polygon.push(p_end);
            }
        }
        EdgeKind::Arc { cx, cy, radius, start_angle, end_angle } => {
            // We need to determine which end of the arc `from` corresponds to,
            // so we walk in the correct direction.
            let (sa, ea) = if let Some(p_from) = stored(from) {
                let from_angle = (p_from.y - cy).atan2(p_from.x - cx);
                let d_start = ((from_angle - start_angle + std::f64::consts::TAU) % std::f64::consts::TAU).abs();
                let d_end = ((from_angle - end_angle + std::f64::consts::TAU) % std::f64::consts::TAU).abs();
                if d_end < d_start {
                    // Walk from end back to start (reversed)
                    (end_angle, start_angle)
                } else {
                    (start_angle, end_angle)
                }
            } else {
                (start_angle, end_angle)
            };

            // Normalize so we always go in the positive direction.
            let (sa, ea) = if ea >= sa { (sa, ea) } else { (sa, ea + std::f64::consts::TAU) };
            let span = ea - sa;
            let n = (span.abs() / (std::f64::consts::PI / 32.0)).ceil() as usize;
            let n = n.clamp(2, 256);
            for i in 1..=n {
                let t = i as f64 / n as f64;
                let a = sa + span * t;
                polygon.push(Point2D { x: cx + radius * a.cos(), y: cy + radius * a.sin() });
            }
        }
        EdgeKind::Ellipse { cx, cy, major_rx, major_ry, ratio } => {
            let angle = major_ry.atan2(major_rx);
            let a_len = (major_rx * major_rx + major_ry * major_ry).sqrt();
            let b_len = a_len * ratio;
            let cos_a = angle.cos();
            let sin_a = angle.sin();
            let n = 48;
            for i in 1..=n {
                let t = i as f64 / n as f64;
                let th = std::f64::consts::TAU * t;
                let lx = a_len * th.cos();
                let ly = b_len * th.sin();
                let wx = lx * cos_a - ly * sin_a;
                let wy = lx * sin_a + ly * cos_a;
                polygon.push(Point2D { x: cx + wx, y: cy + wy });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use echi_core::sketch::Sketch;

    #[test]
    fn extrude_square() {
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(0.0, 0.0);
        let p1 = sketch.add_point(1.0, 0.0);
        let p2 = sketch.add_point(1.0, 1.0);
        let p3 = sketch.add_point(0.0, 1.0);
        sketch.add_line(p0, p1);
        sketch.add_line(p1, p2);
        sketch.add_line(p2, p3);
        sketch.add_line(p3, p0);

        let mesh = extrude(
            &sketch,
            2.0,
            ExtrudeDirection::OneSide,
            0.0,
            &PlaneDefinition::XY,
        )
        .expect("should extrude square");
        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
    }

    #[test]
    fn extrude_ccw_winding() {
        let polygon = vec![
            Point2D { x: 0.0, y: 0.0 },
            Point2D { x: 0.0, y: 1.0 },
            Point2D { x: 1.0, y: 1.0 },
            Point2D { x: 1.0, y: 0.0 },
        ];
        assert!(polygon_area(&polygon) < 0.0);
        // The new extract_loops() function ensures CCW automatically.
        // Verify that an extrusion still works for CW input.
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(polygon[0].x, polygon[0].y);
        let p1 = sketch.add_point(polygon[1].x, polygon[1].y);
        let p2 = sketch.add_point(polygon[2].x, polygon[2].y);
        let p3 = sketch.add_point(polygon[3].x, polygon[3].y);
        sketch.add_line(p0, p1);
        sketch.add_line(p1, p2);
        sketch.add_line(p2, p3);
        sketch.add_line(p3, p0);
        let mesh = extrude(&sketch, 1.0, ExtrudeDirection::OneSide, 0.0, &PlaneDefinition::XY);
        assert!(mesh.is_some(), "CW-drawn polygon should still extrude");
    }

    #[test]
    fn extrude_circle() {
        let mut sketch = Sketch::new();
        let center = sketch.add_point(0.0, 0.0);
        sketch.add_circle(center, 1.0);

        let mesh = extrude(
            &sketch,
            2.0,
            ExtrudeDirection::OneSide,
            0.0,
            &PlaneDefinition::XY,
        )
        .expect("should extrude circle");
        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
        assert!(mesh.vertex_count() > 64);
    }

    #[test]
    fn extrude_with_arc() {
        // Build a rectangle with one rounded corner (top-right).
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(0.0, 0.0); // bottom-left
        let p1 = sketch.add_point(2.0, 0.0); // bottom-right
        let p2 = sketch.add_point(2.0, 1.0); // before arc start
        let p3 = sketch.add_point(1.0, 2.0); // after arc end
        let p4 = sketch.add_point(0.0, 2.0); // top-left
        sketch.add_line(p0, p1);
        sketch.add_line(p1, p2);
        let c = sketch.add_point(1.0, 1.0);
        sketch.add_arc(c, 1.0, 0.0, std::f64::consts::FRAC_PI_2);
        sketch.add_line(p3, p4);
        sketch.add_line(p4, p0);

        // Should produce a non-trivial extrudable polygon mixing lines and arc.
        let polygon = extract_loops(&sketch)
            .and_then(|loops| loops.into_iter().next())
            .expect("arc profile should be extractable");
        assert!(polygon.len() >= 6, "expected at least 6 polygon vertices, got {}", polygon.len());

        // Verify it actually extrudes (no NaN/empty mesh).
        let mesh = extrude(
            &sketch,
            1.0,
            ExtrudeDirection::OneSide,
            0.0,
            &PlaneDefinition::XY,
        )
        .expect("should extrude arc profile");
        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
        assert!(
            !mesh.positions.iter().any(|v| v.is_nan()),
            "no NaN allowed in mesh"
        );
    }

    #[test]
    fn tessellate_circle_polygon() {
        let mut sketch = Sketch::new();
        let center = sketch.add_point(0.0, 0.0);
        sketch.add_circle(center, 1.0);

        let polygon = tessellate_standalone_circle(&sketch).expect("should tessellate circle");
        assert_eq!(polygon.len(), 64);
        for p in &polygon {
            let dist = (p.x * p.x + p.y * p.y).sqrt();
            assert!((dist - 1.0).abs() < 1e-10);
        }
    }

    #[test]
    fn extrude_with_hole() {
        // Outer 4×4 square with a 2×2 hole in the middle.
        let mut sketch = Sketch::new();
        // Outer
        let o0 = sketch.add_point(0.0, 0.0);
        let o1 = sketch.add_point(4.0, 0.0);
        let o2 = sketch.add_point(4.0, 4.0);
        let o3 = sketch.add_point(0.0, 4.0);
        sketch.add_line(o0, o1);
        sketch.add_line(o1, o2);
        sketch.add_line(o2, o3);
        sketch.add_line(o3, o0);
        // Hole
        let h0 = sketch.add_point(1.0, 1.0);
        let h1 = sketch.add_point(3.0, 1.0);
        let h2 = sketch.add_point(3.0, 3.0);
        let h3 = sketch.add_point(1.0, 3.0);
        sketch.add_line(h0, h1);
        sketch.add_line(h1, h2);
        sketch.add_line(h2, h3);
        sketch.add_line(h3, h0);

        let loops = extract_loops(&sketch).expect("two loops should be extracted");
        assert_eq!(loops.len(), 2, "expected outer + hole");

        let mesh = extrude(
            &sketch,
            1.0,
            ExtrudeDirection::OneSide,
            0.0,
            &PlaneDefinition::XY,
        )
        .expect("should extrude plate with hole");
        // Sanity: mesh has geometry and no NaNs.
        assert!(mesh.vertex_count() > 16);
        assert!(!mesh.indices.is_empty());
        assert!(!mesh.positions.iter().any(|v| v.is_nan()));
    }
}
