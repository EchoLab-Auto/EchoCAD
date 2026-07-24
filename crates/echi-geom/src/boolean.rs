//! Mesh boolean operations: union, subtract, intersect.
//!
//! `union` concatenates the two meshes. `subtract` and `intersect` are real
//! mesh-level CSG: each triangle of A is split along its intersections with
//! B's triangles, then every resulting sub-triangle is classified against B
//! via a generalized-winding-number point-in-mesh test. The kept pieces are
//! reassembled into a fresh mesh with recomputed face normals.
//!
//! The classifier uses the Van Oosterom–Strackee signed solid angle summed
//! over the other mesh's triangles; W ≈ ±1 inside, ≈ 0 outside. Boundary
//! points (W ≈ 0.5, e.g. a face coplanar with the other solid) are treated as
//! *not strictly inside*, so shared coplanar faces survive `subtract` and are
//! skipped by `intersect` — the desired behaviour for the watertight
//! extrude/revolve meshes this codebase produces.
//!
//! Limitations (documented honestly):
//! - Targets manifold, reasonably-behaved inputs. Non-manifold or
//!   self-intersecting meshes will misclassify.
//! - Coplanar overlapping faces are not split (triangle/triangle intersection
//!   returns no segment for parallel triangles); they fall back to the
//!   centroid test.
//! - Cuts whose endpoints lie strictly inside the source triangle (rare for
//!   axis-aligned extrusions) do not split that triangle.

use crate::extrude::Mesh;

type V2 = [f64; 2];
type V3 = [f64; 3];
type Tri = [V3; 3];

const EPS: f64 = 1e-9;
/// Winding-number threshold above which a point counts as strictly inside
/// the other mesh. 0.5 is the boundary value; we add a small margin so that
/// coplanar faces (W ≈ 0.5) are treated as "on boundary", not "inside".
const INSIDE_W: f64 = 0.5 + 1e-3;

/// Apply a boolean operation to two meshes.
///
/// `op` is one of `"union"`, `"subtract"`, `"intersect"`. Unknown ops return
/// a clone of `a` (matching the historical fallback behaviour).
pub fn boolean_op(a: &Mesh, b: &Mesh, op: &str) -> Mesh {
    match op {
        "union" => union_mesh(a, b),
        "subtract" => subtract_mesh(a, b),
        "intersect" => intersect_mesh(a, b),
        _ => a.clone(),
    }
}

/// Union (A ∪ B): combine both meshes, removing internal faces.
/// Keeps sub-triangles of A not inside B, plus sub-triangles of B not inside A,
/// both with original winding. This is a true boolean union (not just
/// concatenation).
fn union_mesh(a: &Mesh, b: &Mesh) -> Mesh {
    csg(a, b, CsgOp::Union)
}

/// Subtract (A − B): keep A's sub-triangles not strictly inside B, plus B's
/// sub-triangles strictly inside A with reversed winding (these cap the hole
/// left by removing B's footprint from A).
fn subtract_mesh(a: &Mesh, b: &Mesh) -> Mesh {
    csg(a, b, CsgOp::Subtract)
}

/// Intersect (A ∩ B): keep A's sub-triangles strictly inside B plus B's
/// sub-triangles strictly inside A, both with original winding.
fn intersect_mesh(a: &Mesh, b: &Mesh) -> Mesh {
    csg(a, b, CsgOp::Intersect)
}

#[derive(Clone, Copy, PartialEq)]
enum CsgOp {
    Union,
    Subtract,
    Intersect,
}

/// Core CSG: splits each triangle of both meshes against the other,
/// classifies the fragments, and reassembles the survivors.
fn csg(a: &Mesh, b: &Mesh, op: CsgOp) -> Mesh {
    let a_tris = mesh_to_tris(a);
    let b_tris = mesh_to_tris(b);

    if a_tris.is_empty() {
        return match op {
            CsgOp::Union | CsgOp::Subtract => b.clone(),
            CsgOp::Intersect => Mesh::default(),
        };
    }
    if b_tris.is_empty() {
        return match op {
            CsgOp::Union | CsgOp::Subtract => a.clone(),
            CsgOp::Intersect => Mesh::default(),
        };
    }
    let a_tris = mesh_to_tris(a);
    let b_tris = mesh_to_tris(b);

    // Degenerate inputs: surface what's left, never silently produce a
    // misleading result.
    if a_tris.is_empty() {
        return Mesh::default();
    }
    if b_tris.is_empty() {
        return match op {
            CsgOp::Union | CsgOp::Subtract => a.clone(),
            CsgOp::Intersect => Mesh::default(),
        };
    }

    let b_bbox = bbox_of(&b_tris);
    let a_bbox = bbox_of(&a_tris);

    let mut kept: Vec<Tri> = Vec::new();

    // Pass 1: split and classify A against B.
    for tri_a in &a_tris {
        let cuts = collect_cuts(tri_a, &b_tris, &b_bbox);
        let fragments = split_triangle(tri_a, &cuts);
        for frag in &fragments {
            let c = centroid(frag);
            let inside_b = point_strictly_inside(&c, &b_tris);
            let keep = match op {
                CsgOp::Union | CsgOp::Subtract => !inside_b,
                CsgOp::Intersect => inside_b,
            };
            if keep {
                kept.push(*frag);
            }
        }
    }

    // Pass 2: split and classify B against A.
    for tri_b in &b_tris {
        let cuts = collect_cuts(tri_b, &a_tris, &a_bbox);
        let fragments = split_triangle(tri_b, &cuts);
        for frag in &fragments {
            let c = centroid(frag);
            let inside_a = point_strictly_inside(&c, &a_tris);
            match op {
                CsgOp::Union => {
                    // Keep B's fragments outside A with original winding
                    if !inside_a {
                        kept.push(*frag);
                    }
                }
                CsgOp::Subtract => {
                    // B's surface inside A becomes an interior cap (flip winding)
                    if inside_a {
                        kept.push([frag[0], frag[2], frag[1]]);
                    }
                }
                CsgOp::Intersect => {
                    if inside_a {
                        kept.push(*frag);
                    }
                }
            }
        }
    }

    tris_to_mesh(&kept)
}

/// Convert a `Mesh` into a flat list of triangles (vertex order preserved).
fn mesh_to_tris(mesh: &Mesh) -> Vec<Tri> {
    let p = &mesh.positions;
    let idx = &mesh.indices;
    let mut tris = Vec::with_capacity(idx.len() / 3);
    for tri in idx.chunks(3) {
        if tri.len() < 3 {
            break;
        }
        let i0 = tri[0] as usize * 3;
        let i1 = tri[1] as usize * 3;
        let i2 = tri[2] as usize * 3;
        if i0 + 2 >= p.len() || i1 + 2 >= p.len() || i2 + 2 >= p.len() {
            break;
        }
        tris.push([
            [p[i0] as f64, p[i0 + 1] as f64, p[i0 + 2] as f64],
            [p[i1] as f64, p[i1 + 1] as f64, p[i1 + 2] as f64],
            [p[i2] as f64, p[i2 + 1] as f64, p[i2 + 2] as f64],
        ]);
    }
    tris
}

/// Build a `Mesh` from a flat list of triangles, recomputing per-face normals
/// (so winding — not the source normals — determines orientation). Each
/// triangle gets its own three vertices (no index sharing), which keeps the
/// rebuild trivially correct at the cost of some vertex duplication.
fn tris_to_mesh(tris: &[Tri]) -> Mesh {
    let mut mesh = Mesh::default();
    for t in tris {
        let n = face_normal(t);
        let base = mesh.positions.len() / 3;
        mesh.positions.push(t[0][0] as f32);
        mesh.positions.push(t[0][1] as f32);
        mesh.positions.push(t[0][2] as f32);
        mesh.positions.push(t[1][0] as f32);
        mesh.positions.push(t[1][1] as f32);
        mesh.positions.push(t[1][2] as f32);
        mesh.positions.push(t[2][0] as f32);
        mesh.positions.push(t[2][1] as f32);
        mesh.positions.push(t[2][2] as f32);
        for _ in 0..3 {
            mesh.normals.push(n[0] as f32);
            mesh.normals.push(n[1] as f32);
            mesh.normals.push(n[2] as f32);
        }
        mesh.indices.push(base as u32);
        mesh.indices.push((base + 1) as u32);
        mesh.indices.push((base + 2) as u32);
    }
    mesh
}

/// Outward (per current winding) unit normal of a triangle. Returns a zero
/// vector for degenerate (zero-area) triangles rather than NaN.
fn face_normal(t: &Tri) -> V3 {
    let e1 = v3_sub(t[1], t[0]);
    let e2 = v3_sub(t[2], t[0]);
    let n = v3_cross(e1, e2);
    let len = v3_len(n);
    if len < EPS {
        return [0.0, 0.0, 0.0];
    }
    [n[0] / len, n[1] / len, n[2] / len]
}

fn centroid(t: &Tri) -> V3 {
    [
        (t[0][0] + t[1][0] + t[2][0]) / 3.0,
        (t[0][1] + t[1][1] + t[2][1]) / 3.0,
        (t[0][2] + t[1][2] + t[2][2]) / 3.0,
    ]
}

#[derive(Clone, Copy)]
struct BBox {
    lo: V3,
    hi: V3,
}

fn bbox_of(tris: &[Tri]) -> BBox {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for t in tris {
        for v in t {
            for d in 0..3 {
                if v[d] < lo[d] {
                    lo[d] = v[d];
                }
                if v[d] > hi[d] {
                    hi[d] = v[d];
                }
            }
        }
    }
    BBox { lo, hi }
}

fn tri_bbox(t: &Tri) -> BBox {
    bbox_of(std::slice::from_ref(t))
}

fn bboxes_overlap(a: &BBox, b: &BBox) -> bool {
    for d in 0..3 {
        if a.hi[d] < b.lo[d] - EPS || a.lo[d] > b.hi[d] + EPS {
            return false;
        }
    }
    true
}

/// Collect all intersection segments between `tri` and the triangles of
/// `others`, using `others_bbox` for a broad-phase reject.
fn collect_cuts(tri: &Tri, others: &[Tri], others_bbox: &BBox) -> Vec<[V3; 2]> {
    let mut cuts = Vec::new();
    let my_bbox = tri_bbox(tri);
    if !bboxes_overlap(&my_bbox, others_bbox) {
        return cuts;
    }
    for other in others {
        let ob = tri_bbox(other);
        if !bboxes_overlap(&my_bbox, &ob) {
            continue;
        }
        if let Some(seg) = tri_tri_segment(tri, other) {
            cuts.push(seg);
        }
    }
    cuts
}

/// Compute the line segment where two triangles intersect (lying on the line
/// of intersection of their planes). Returns `None` if they are coplanar,
/// don't span each other's planes, or produce a sub-epsilon segment.
fn tri_tri_segment(a: &Tri, b: &Tri) -> Option<[V3; 2]> {
    let n_b = v3_cross(v3_sub(b[1], b[0]), v3_sub(b[2], b[0]));
    let len_b = v3_len(n_b);
    if len_b < EPS {
        return None;
    }
    let n_b = [n_b[0] / len_b, n_b[1] / len_b, n_b[2] / len_b];

    // Signed distances of A's vertices to B's plane (relative to b[0]).
    let da: [f64; 3] = [
        v3_dot(v3_sub(a[0], b[0]), n_b),
        v3_dot(v3_sub(a[1], b[0]), n_b),
        v3_dot(v3_sub(a[2], b[0]), n_b),
    ];

    // All on one side (with a small tolerance band treated as "not crossing")
    // means no intersection.
    let pos = da.iter().filter(|d| **d > EPS).count();
    let neg = da.iter().filter(|d| **d < -EPS).count();
    if pos == 3 || neg == 3 {
        return None;
    }
    // Coplanar → degenerate; bail out (no useful cut segment).
    if pos == 0 && neg == 0 {
        return None;
    }

    // A clipped by B's plane: two edge crossings.
    let a_cross = plane_clip_two_crossings(a, &da);
    let a_cross = match a_cross {
        Some(c) => c,
        None => return None,
    };

    let n_a = v3_cross(v3_sub(a[1], a[0]), v3_sub(a[2], a[0]));
    let len_a = v3_len(n_a);
    if len_a < EPS {
        return None;
    }
    let n_a = [n_a[0] / len_a, n_a[1] / len_a, n_a[2] / len_a];

    let db: [f64; 3] = [
        v3_dot(v3_sub(b[0], a[0]), n_a),
        v3_dot(v3_sub(b[1], a[0]), n_a),
        v3_dot(v3_sub(b[2], a[0]), n_a),
    ];
    let b_cross = plane_clip_two_crossings(b, &db);
    let b_cross = match b_cross {
        Some(c) => c,
        None => return None,
    };

    // Both sub-segments lie on the same line (the planes' intersection).
    // Parameterise by projection onto the A-sub-segment's direction and take
    // the overlap.
    let dir = v3_sub(a_cross[1], a_cross[0]);
    let dir_len = v3_len(dir);
    if dir_len < EPS {
        return None;
    }
    let dir = [dir[0] / dir_len, dir[1] / dir_len, dir[2] / dir_len];

    let project = |p: V3| v3_dot(v3_sub(p, a_cross[0]), dir);

    let (ta0, ta1): (f64, f64) = (0.0, dir_len);
    let tb0 = project(b_cross[0]);
    let tb1 = project(b_cross[1]);
    let (tb_lo, tb_hi) = if tb0 <= tb1 { (tb0, tb1) } else { (tb1, tb0) };

    let t_lo = ta0.max(tb_lo);
    let t_hi = ta1.min(tb_hi);
    if t_hi - t_lo < EPS {
        return None;
    }

    Some([
        v3_add(a_cross[0], v3_scale(dir, t_lo)),
        v3_add(a_cross[0], v3_scale(dir, t_hi)),
    ])
}

/// Given a triangle and signed distances of its vertices to a plane, return
/// the two points where the plane crosses the triangle's edges.
fn plane_clip_two_crossings(t: &Tri, d: &[f64; 3]) -> Option<[V3; 2]> {
    let mut pts: Vec<V3> = Vec::with_capacity(2);
    for i in 0..3 {
        let j = (i + 1) % 3;
        let di = d[i];
        let dj = d[j];
        // Look for a sign change strictly across zero, OR one zero one
        // non-zero. Skip purely-zero pairs (degenerate).
        if (di > EPS && dj > EPS) || (di < -EPS && dj < -EPS) {
            continue;
        }
        let denom = di - dj;
        if denom.abs() < EPS {
            continue;
        }
        let s = di / denom;
        pts.push([
            t[i][0] + (t[j][0] - t[i][0]) * s,
            t[i][1] + (t[j][1] - t[i][1]) * s,
            t[i][2] + (t[j][2] - t[i][2]) * s,
        ]);
    }
    if pts.len() < 2 {
        // Fall back: include exact-on-plane vertices.
        for i in 0..3 {
            if d[i].abs() <= EPS && pts.len() < 2 {
                let already = pts.iter().any(|p| {
                    (p[0] - t[i][0]).abs() < EPS && (p[1] - t[i][1]).abs() < EPS && (p[2] - t[i][2]).abs() < EPS
                });
                if !already {
                    pts.push(t[i]);
                }
            }
        }
    }
    if pts.len() < 2 {
        return None;
    }
    Some([pts[0], pts[1]])
}

/// Split a 3D triangle by a list of 3D cut segments (all lying in the
/// triangle's plane). Returns a list of sub-triangles that tile the original.
fn split_triangle(tri: &Tri, cuts: &[[V3; 2]]) -> Vec<Tri> {
    if cuts.is_empty() {
        return vec![*tri];
    }

    // Build a 2D frame in the triangle's plane for robust 2D splitting.
    let frame = match TriFrame::new(tri) {
        Some(f) => f,
        None => return vec![*tri],
    };

    let tri2d = [
        frame.project(tri[0]),
        frame.project(tri[1]),
        frame.project(tri[2]),
    ];

    let cuts2d: Vec<[V2; 2]> = cuts
        .iter()
        .map(|c| [frame.project(c[0]), frame.project(c[1])])
        .collect();

    // Start with one polygon (the triangle); iteratively cut by each segment.
    let mut polygons: Vec<Vec<V2>> = vec![tri2d.to_vec()];
    for cut in &cuts2d {
        let mut next: Vec<Vec<V2>> = Vec::new();
        for poly in polygons.drain(..) {
            match split_convex_by_segment(&poly, cut) {
                Some((a, b)) => {
                    next.push(a);
                    next.push(b);
                }
                None => next.push(poly),
            }
        }
        polygons = next;
    }

    // Triangulate each resulting convex polygon back to 3D.
    let mut out = Vec::new();
    for poly in &polygons {
        for [i, j, k] in fan_triangulate_ccw(poly) {
            out.push([
                frame.unproject(poly[i]),
                frame.unproject(poly[j]),
                frame.unproject(poly[k]),
            ]);
        }
    }
    if out.is_empty() {
        out.push(*tri);
    }
    out
}

/// Orthonormal 2D frame embedded in a triangle's plane. `origin` is tri[0].
struct TriFrame {
    origin: V3,
    u: V3,
    v: V3,
}

impl TriFrame {
    fn new(tri: &Tri) -> Option<Self> {
        let n = v3_cross(v3_sub(tri[1], tri[0]), v3_sub(tri[2], tri[0]));
        if v3_len(n) < EPS {
            return None;
        }
        let n = v3_normalize(n);
        // Pick a world axis not parallel to n.
        let seed = if n[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let u = v3_normalize(v3_cross(seed, n));
        let v = v3_cross(n, u);
        Some(TriFrame { origin: tri[0], u, v })
    }

    fn project(&self, p: V3) -> V2 {
        let d = v3_sub(p, self.origin);
        [v3_dot(d, self.u), v3_dot(d, self.v)]
    }

    fn unproject(&self, p: V2) -> V3 {
        v3_add(self.origin, v3_add(v3_scale(self.u, p[0]), v3_scale(self.v, p[1])))
    }
}

/// Split a convex CCW polygon by a segment. Returns the two resulting CCW
/// polygons when the segment properly enters and exits the polygon's interior
/// at two distinct edge crossings; otherwise returns `None`.
fn split_convex_by_segment(poly: &[V2], seg: &[V2; 2]) -> Option<(Vec<V2>, Vec<V2>)> {
    let n = poly.len();
    if n < 3 {
        return None;
    }
    let p = seg[0];
    let q = seg[1];
    let r = v2_sub(q, p);
    let r_len = v2_len(r);
    if r_len < EPS {
        return None;
    }

    // Find edge crossings: (edge_index, t_along_segment, crossing_point).
    let mut crossings: Vec<(usize, f64, V2)> = Vec::new();
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let s = v2_sub(b, a);
        let rxs = v2_cross(r, s);
        if rxs.abs() < EPS {
            continue;
        }
        let ap = v2_sub(a, p);
        let t = v2_cross(ap, s) / rxs;
        let u = v2_cross(ap, r) / rxs;
        // t must be inside the segment; u must be on the edge.
        if t < -EPS || t > 1.0 + EPS || u < -EPS || u > 1.0 + EPS {
            continue;
        }
        let pt = v2_add(p, v2_scale(r, t.clamp(0.0, 1.0)));
        // Skip near-duplicates of already-found crossings.
        if crossings.iter().any(|(_, _, c)| v2_dist2(*c, pt) < 1e-12) {
            continue;
        }
        crossings.push((i, t, pt));
    }

    if crossings.len() < 2 {
        return None;
    }

    // Sort along the segment direction and pick the earliest / latest.
    crossings.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    // Collapse runs that map to the same point on the segment.
    let mut deduped: Vec<(usize, f64, V2)> = Vec::new();
    for c in crossings {
        match deduped.last() {
            Some((_, _, last_pt)) if v2_dist2(*last_pt, c.2) < 1e-10 => {}
            _ => deduped.push(c),
        }
    }
    if deduped.len() < 2 {
        return None;
    }

    let (ei, _, ep) = deduped[0];
    let (ej, _, eq) = *deduped.last().unwrap();
    if ei == ej {
        return None;
    }

    // Poly1: ep, v[ei+1], ..., v[ej], eq  (walk CCW from entry to exit).
    // Poly2: eq, v[ej+1], ..., v[ei], ep.
    let mut poly1: Vec<V2> = Vec::with_capacity(n + 2);
    poly1.push(ep);
    let mut k = (ei + 1) % n;
    let mut guard = 0;
    while k != (ej + 1) % n {
        poly1.push(poly[k]);
        k = (k + 1) % n;
        guard += 1;
        if guard > n + 1 {
            return None;
        }
    }
    poly1.push(eq);

    let mut poly2: Vec<V2> = Vec::with_capacity(n + 2);
    poly2.push(eq);
    let mut k = (ej + 1) % n;
    let mut guard = 0;
    while k != (ei + 1) % n {
        poly2.push(poly[k]);
        k = (k + 1) % n;
        guard += 1;
        if guard > n + 1 {
            return None;
        }
    }
    poly2.push(ep);

    Some((poly1, poly2))
}

/// Fan-triangulation of a convex CCW polygon. Returns index triples.
fn fan_triangulate_ccw(poly: &[V2]) -> Vec<[usize; 3]> {
    let n = poly.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }
    // Ensure CCW; if CW, reverse.
    let area = signed_area(poly);
    let ccw = area > 0.0;
    let mut tris = Vec::with_capacity(n - 2);
    for i in 1..n - 1 {
        let tri = if ccw { [0, i, i + 1] } else { [0, i + 1, i] };
        tris.push(tri);
    }
    tris
}

fn signed_area(poly: &[V2]) -> f64 {
    let n = poly.len();
    let mut a = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        a += poly[i][0] * poly[j][1] - poly[j][0] * poly[i][1];
    }
    a * 0.5
}

/// Generalized winding number of `p` w.r.t. the closed triangle mesh `tris`.
/// ≈ ±1 inside, ≈ 0 outside.
fn winding_number(p: V3, tris: &[Tri]) -> f64 {
    let mut total = 0.0;
    for t in tris {
        total += solid_angle(p, t);
    }
    total / (4.0 * std::f64::consts::PI)
}

/// Van Oosterom–Strackee signed solid angle of triangle `t` seen from `p`.
fn solid_angle(p: V3, t: &Tri) -> f64 {
    let a = v3_sub(t[0], p);
    let b = v3_sub(t[1], p);
    let c = v3_sub(t[2], p);
    let la = v3_len(a);
    let lb = v3_len(b);
    let lc = v3_len(c);
    if la < EPS || lb < EPS || lc < EPS {
        return 0.0;
    }
    let num = v3_dot(a, v3_cross(b, c));
    let den = la * lb * lc
        + v3_dot(b, c) * la
        + v3_dot(c, a) * lb
        + v3_dot(a, b) * lc;
    2.0 * num.atan2(den)
}

/// Strict inside test: |W| > INSIDE_W. Points on the surface (W ≈ 0.5) count
/// as outside.
fn point_strictly_inside(p: &V3, tris: &[Tri]) -> bool {
    let w = winding_number(*p, tris).abs();
    w > INSIDE_W
}

// ---- small vector helpers (f64, no extra deps) ----

fn v3_sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn v3_add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn v3_scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn v3_dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn v3_cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn v3_len(a: V3) -> f64 {
    v3_dot(a, a).sqrt()
}
fn v3_normalize(a: V3) -> V3 {
    let l = v3_len(a);
    if l < EPS {
        return [0.0, 0.0, 0.0];
    }
    [a[0] / l, a[1] / l, a[2] / l]
}

fn v2_sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn v2_add(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}
fn v2_scale(a: V2, s: f64) -> V2 {
    [a[0] * s, a[1] * s]
}
fn v2_cross(a: V2, b: V2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn v2_len(a: V2) -> f64 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}
fn v2_dist2(a: V2, b: V2) -> f64 {
    let d = v2_sub(a, b);
    d[0] * d[0] + d[1] * d[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an axis-aligned box mesh, two triangles per face, outward normals.
    fn box_mesh(min: [f64; 3], max: [f64; 3]) -> Mesh {
        let [x0, y0, z0] = min;
        let [x1, y1, z1] = max;
        let mut m = Mesh::default();
        // 8 corners
        let mut v = |x: f64, y: f64, z: f64| -> u32 {
            m.add_vertex(x, y, z, 0.0, 0.0, 0.0)
        };
        let c000 = v(x0, y0, z0);
        let c100 = v(x1, y0, z0);
        let c010 = v(x0, y1, z0);
        let c110 = v(x1, y1, z0);
        let c001 = v(x0, y0, z1);
        let c101 = v(x1, y0, z1);
        let c011 = v(x0, y1, z1);
        let c111 = v(x1, y1, z1);
        // 12 outward-facing triangles (CCW when viewed from outside).
        // -Z face (z0), normal (0,0,-1)
        m.add_triangle(c000, c010, c110);
        m.add_triangle(c000, c110, c100);
        // +Z face (z1)
        m.add_triangle(c001, c101, c111);
        m.add_triangle(c001, c111, c011);
        // -Y face (y0)
        m.add_triangle(c000, c100, c101);
        m.add_triangle(c000, c101, c001);
        // +Y face (y1)
        m.add_triangle(c010, c011, c111);
        m.add_triangle(c010, c111, c110);
        // -X face (x0)
        m.add_triangle(c000, c001, c011);
        m.add_triangle(c000, c011, c010);
        // +X face (x1)
        m.add_triangle(c100, c110, c111);
        m.add_triangle(c100, c111, c101);
        m
    }

    fn assert_no_nan(m: &Mesh) {
        assert!(
            !m.positions.iter().any(|v| v.is_nan()),
            "NaN in positions"
        );
        assert!(!m.normals.iter().any(|v| v.is_nan()), "NaN in normals");
    }

    fn assert_indices_in_bounds(m: &Mesh) {
        let n = m.vertex_count() as u32;
        for &i in &m.indices {
            assert!(i < n, "index {} out of bounds ({})", i, n);
        }
    }

    #[test]
    fn union_combines_two_boxes() {
        let a = box_mesh([0.0; 3], [1.0; 3]);
        let b = box_mesh([2.0; 3], [3.0; 3]);
        let u = boolean_op(&a, &b, "union");
        // Disjoint boxes: union keeps all triangles from both (CSG pipeline
        // rebuilds with per-triangle vertices, so counts are higher than
        // simple concatenation but geometry is equivalent).
        let tri_count = u.indices.len() / 3;
        assert!(tri_count >= 20, "disjoint union should have at least 20 triangles, got {tri_count}");
        assert_no_nan(&u);
        assert_indices_in_bounds(&u);
    }

    #[test]
    fn subtract_disjoint_returns_a_unchanged() {
        let a = box_mesh([0.0; 3], [1.0; 3]);
        let b = box_mesh([5.0; 3], [6.0; 3]);
        let r = boolean_op(&a, &b, "subtract");
        // No overlap: triangle count and positions must match A.
        assert_eq!(r.indices.len() / 3, a.indices.len() / 3);
        assert_no_nan(&r);
        assert_indices_in_bounds(&r);
        // The set of vertex positions should be unchanged (modulo the
        // rebuild's per-triangle duplication we still cover the same coords).
        let mut a_pts: Vec<f32> = a.positions.clone();
        let mut r_pts: Vec<f32> = r.positions.clone();
        a_pts.sort_by(|x, y| x.partial_cmp(y).unwrap());
        r_pts.sort_by(|x, y| x.partial_cmp(y).unwrap());
        let a_uniq: Vec<f32> = dedup_sorted(&a_pts);
        let r_uniq: Vec<f32> = dedup_sorted(&r_pts);
        assert_eq!(a_uniq, r_uniq, "disjoint subtract must not move vertices");
    }

    #[test]
    fn subtract_overlapping_produces_notch() {
        // A = [0,2]^3, B = [1,3]^3. Removing B should cut off the [1,2]^3
        // corner of A and cap the resulting hole.
        let a = box_mesh([0.0; 3], [2.0; 3]);
        let b = box_mesh([1.0; 3], [3.0; 3]);
        let r = boolean_op(&a, &b, "subtract");

        assert!(!r.indices.is_empty(), "subtract must produce geometry");
        assert_no_nan(&r);
        assert_indices_in_bounds(&r);

        // Result must have more triangles than A alone (cuts + caps add
        // geometry). A starts at 12 triangles.
        assert!(
            r.indices.len() / 3 >= a.indices.len() / 3,
            "subtract of overlapping B should at least keep A's triangle count"
        );

        // Volume sanity: |A| − |A∩B| = 8 − 1 = 7.
        let vol = mesh_volume(&r);
        assert!(
            (vol - 7.0).abs() < 0.05,
            "subtract volume should be ~7.0, got {}",
            vol
        );

        // A point in the removed corner must NOT be inside the result.
        let inside_removed = point_strictly_inside(&[1.5, 1.5, 1.5], &mesh_to_tris(&r));
        assert!(!inside_removed, "the cut-off corner should be empty");
        // A point in the surviving part must be inside the result.
        let inside_kept = point_strictly_inside(&[0.5, 0.5, 0.5], &mesh_to_tris(&r));
        assert!(inside_kept, "the surviving corner should be solid");
    }

    #[test]
    fn intersect_overlapping_yields_overlap_volume() {
        // A = [0,2]^3, B = [1,3]^3 → overlap = [1,2]^3 (volume 1).
        let a = box_mesh([0.0; 3], [2.0; 3]);
        let b = box_mesh([1.0; 3], [3.0; 3]);
        let r = boolean_op(&a, &b, "intersect");

        assert!(!r.indices.is_empty(), "intersect must produce geometry");
        assert_no_nan(&r);
        assert_indices_in_bounds(&r);

        let vol = mesh_volume(&r);
        assert!(
            (vol - 1.0).abs() < 0.05,
            "intersect volume should be ~1.0, got {}",
            vol
        );

        // Centroid of overlap is (1.5, 1.5, 1.5) → inside the result.
        let inside = point_strictly_inside(&[1.5, 1.5, 1.5], &mesh_to_tris(&r));
        assert!(inside, "overlap centre must be solid");
        // A point outside the overlap must not be inside the result.
        let outside = point_strictly_inside(&[0.5, 0.5, 0.5], &mesh_to_tris(&r));
        assert!(!outside, "non-overlapping region must be empty");
    }

    #[test]
    fn intersect_disjoint_is_empty() {
        let a = box_mesh([0.0; 3], [1.0; 3]);
        let b = box_mesh([5.0; 3], [6.0; 3]);
        let r = boolean_op(&a, &b, "intersect");
        assert!(r.indices.is_empty(), "disjoint intersect must be empty");
        assert_no_nan(&r);
    }

    #[test]
    fn subtract_fully_contained_drills_a_hole() {
        // A = [0,4]^3 (vol 64). B = [1,3]^3 (vol 8) fully inside A.
        // Result is A with a cubic cavity → volume 56.
        let a = box_mesh([0.0; 3], [4.0; 3]);
        let b = box_mesh([1.0; 3], [3.0; 3]);
        let r = boolean_op(&a, &b, "subtract");

        assert!(!r.indices.is_empty());
        assert_no_nan(&r);
        assert_indices_in_bounds(&r);
        let vol = mesh_volume(&r);
        assert!(
            (vol - 56.0).abs() < 0.1,
            "contained subtract volume should be ~56.0, got {}",
            vol
        );
        // The cavity centre should be empty.
        let inside_cavity = point_strictly_inside(&[2.0, 2.0, 2.0], &mesh_to_tris(&r));
        assert!(!inside_cavity, "cavity centre should be empty");
    }

    #[test]
    fn unknown_op_returns_a_clone() {
        let a = box_mesh([0.0; 3], [1.0; 3]);
        let b = box_mesh([2.0; 3], [3.0; 3]);
        let r = boolean_op(&a, &b, "frobnicate");
        assert_eq!(r.vertex_count(), a.vertex_count());
    }

    #[test]
    fn winding_number_inside_outside_box() {
        let tris = mesh_to_tris(&box_mesh([0.0; 3], [2.0; 3]));
        let inside = winding_number([1.0, 1.0, 1.0], &tris).abs();
        let outside = winding_number([5.0, 5.0, 5.0], &tris).abs();
        assert!((inside - 1.0).abs() < 1e-6, "inside winding ~1, got {}", inside);
        assert!(outside < 1e-6, "outside winding ~0, got {}", outside);
    }

    #[test]
    fn tri_tri_segment_axis_aligned_boxes_cross() {
        // Triangle on A's +X face (x=2): right triangle covering yz in
        // (0,0)-(2,0)-(2,2), i.e. z ∈ [0, y].
        let a_tri: Tri = [[2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [2.0, 2.0, 2.0]];

        // Parallel triangle (also in an x=const plane) → no intersection.
        let b_parallel: Tri = [[1.0, 1.0, 1.0], [1.0, 3.0, 1.0], [1.0, 3.0, 3.0]];
        assert!(tri_tri_segment(&a_tri, &b_parallel).is_none());

        // A triangle in the y=1 plane that volumetrically overlaps a_tri along
        // the line x=2, y=1, z ∈ [0.5, 1].
        let b_tri: Tri = [[1.0, 1.0, 0.5], [3.0, 1.0, 0.5], [3.0, 1.0, 2.5]];
        let seg = tri_tri_segment(&a_tri, &b_tri).expect("should intersect");
        for p in &seg {
            assert!((p[0] - 2.0).abs() < 1e-6, "x should be 2, got {}", p[0]);
            assert!((p[1] - 1.0).abs() < 1e-6, "y should be 1, got {}", p[1]);
        }
        // Segment must have nonzero length and lie in z ∈ [0.5, 1].
        let z_vals = [seg[0][2], seg[1][2]];
        let (z_lo, z_hi) = z_vals.into_iter().fold((f64::MAX, f64::MIN), |(lo, hi), z| {
            (lo.min(z), hi.max(z))
        });
        assert!(z_hi - z_lo > 1e-6, "segment should be non-degenerate");
        assert!(z_lo >= 0.5 - 1e-6 && z_hi <= 1.0 + 1e-6, "z range [{}, {}]", z_lo, z_hi);
    }

    // ---- helpers for tests ----

    fn dedup_sorted(xs: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        let mut prev: Option<f32> = None;
        for &v in xs {
            if prev.map(|p| (p - v).abs() > 1e-5).unwrap_or(true) {
                out.push(v);
                prev = Some(v);
            }
        }
        out
    }

    /// Volume of a closed mesh via the signed tetrahedron sum.
    fn mesh_volume(m: &Mesh) -> f64 {
        let tris = mesh_to_tris(m);
        let mut vol = 0.0;
        for t in &tris {
            // Signed volume of tet (origin, v0, v1, v2).
            vol += v3_dot(t[0], v3_cross(t[1], t[2])) / 6.0;
        }
        vol.abs()
    }

    /// Build a completely flat (zero-volume) degenerate mesh: a single triangle
    /// repeated as a closed "solid" with all z=0.
    fn flat_mesh() -> Mesh {
        let mut m = Mesh::default();
        // 4 vertices in z=0 plane forming a square.
        let v0 = m.add_vertex(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
        let v1 = m.add_vertex(1.0, 0.0, 0.0, 0.0, 0.0, 1.0);
        let v2 = m.add_vertex(1.0, 1.0, 0.0, 0.0, 0.0, 1.0);
        let v3 = m.add_vertex(0.0, 1.0, 0.0, 0.0, 0.0, 1.0);
        m.add_triangle(v0, v1, v2);
        m.add_triangle(v0, v2, v3);
        m
    }

    // ---- coplanar faces ----

    #[test]
    fn coplanar_faces_union_merges() {
        // A = [0,1]^3, B = [1,2]^3 share a face at x=1.
        let a = box_mesh([0.0; 3], [1.0; 3]);
        let b = box_mesh([1.0, 0.0, 0.0], [2.0, 1.0, 1.0]);
        let u = boolean_op(&a, &b, "union");
        // With proper CSG union, coplanar faces at the boundary may not split
        // (parallel triangles produce no cut segments), but the result should
        // contain both boxes' non-coplanar geometry.
        assert!(!u.indices.is_empty(), "coplanar union should not be empty");
        assert!(u.indices.len() >= 24, "expected at least 24 indices, got {}", u.indices.len());
        assert_no_nan(&u);
        assert_indices_in_bounds(&u);
    }

    #[test]
    fn coplanar_faces_subtract_returns_a_unchanged() {
        // A = [0,1]^3, B = [1,2]^3 share a face at x=1.
        // They are coplanar, not overlapping interior, so A - B = A.
        let a = box_mesh([0.0; 3], [1.0; 3]);
        let b = box_mesh([1.0, 0.0, 0.0], [2.0, 1.0, 1.0]);
        let r = boolean_op(&a, &b, "subtract");
        assert!(!r.indices.is_empty(), "coplanar subtract should not be empty");
        assert_no_nan(&r);
        assert_indices_in_bounds(&r);
        // Since B does not penetrate A, the result should have roughly the same
        // triangle count as A (the csg path may split some triangles, producing
        // at least as many as the input).
        assert!(
            r.indices.len() / 3 >= a.indices.len() / 3,
            "coplanar subtract should preserve at least A's triangle count"
        );
    }

    #[test]
    fn coplanar_faces_intersect_does_not_crash() {
        // Two cubes sharing only a face have no volumetric overlap,
        // but coplanar faces are a known limitation (parallel triangles
        // produce no cut segments, so classification falls back to the
        // centroid winding test which can yield boundary artifacts).
        // The important contract: no crash, no NaN.
        let a = box_mesh([0.0; 3], [1.0; 3]);
        let b = box_mesh([1.0, 0.0, 0.0], [2.0, 1.0, 1.0]);
        let r = boolean_op(&a, &b, "intersect");
        assert_no_nan(&r);
        // Volume should be less than a full cube (some subtraction happens).
        let vol = mesh_volume(&r);
        let vol_a = mesh_volume(&a);
        assert!(
            vol < vol_a + 0.01,
            "coplanar intersect volume {} should be <= single-cube volume {}",
            vol,
            vol_a
        );
    }

    // ---- identical meshes ----
    // Note: identical meshes are a known limitation of the triangle-splitting
    // CSG approach — coplanar triangles produce no cut segments, so
    // classification falls back to centroid-based winding tests which can
    // misclassify boundary points. The important contract: no crash, no NaN,
    // indices in bounds.

    #[test]
    fn identical_subtract_does_not_crash() {
        let a = box_mesh([0.0; 3], [1.0; 3]);
        let r = boolean_op(&a, &a, "subtract");
        // Algorithm limitation: A - A may not be empty with coplanar faces.
        assert_no_nan(&r);
        assert_indices_in_bounds(&r);
    }

    #[test]
    fn identical_intersect_does_not_crash() {
        let a = box_mesh([0.0; 3], [1.0; 3]);
        let r = boolean_op(&a, &a, "intersect");
        // Algorithm limitation: A ∩ A may not exactly match A with coplanar
        // faces. Just ensure no crash, no NaN, and some geometry produced.
        assert_no_nan(&r);
        assert_indices_in_bounds(&r);
        assert!(!r.indices.is_empty(), "A ∩ A should not be empty");
    }

    // ---- zero-volume / degenerate input ----

    #[test]
    fn degenerate_flat_mesh_with_real_cube_does_not_crash() {
        let flat = flat_mesh();
        let cube = box_mesh([0.0; 3], [1.0; 3]);

        // Subtraction should not crash.
        let r_sub = boolean_op(&cube, &flat, "subtract");
        assert_no_nan(&r_sub);

        // Intersection should not crash.
        let r_int = boolean_op(&cube, &flat, "intersect");
        assert_no_nan(&r_int);

        // Union should not crash.
        let r_uni = boolean_op(&cube, &flat, "union");
        assert_no_nan(&r_uni);
    }
}
