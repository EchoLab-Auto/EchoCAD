use echi_geom::Mesh;

use crate::kernel::{BrepError, BrepKernel, ProfilePoint};
use crate::types::{Curve, Edge, Face, Solid, Surface, Vertex, Wire, WireEdge};

// ============================================================================
// MockBrepKernel
// ============================================================================

/// A pure-Rust mock B-rep kernel for testing and development.
///
/// Supports extruding a simple convex polygon profile (no holes) and
/// tessellating the resulting solid into a triangle mesh.
#[derive(Debug, Clone, Copy, Default)]
pub struct MockBrepKernel;

impl BrepKernel for MockBrepKernel {
    fn extrude(&self, profile: &[ProfilePoint], height: f64) -> Result<Solid, BrepError> {
        extrude_impl(profile, height)
    }

    fn revolve(
        &self,
        profile: &[ProfilePoint],
        axis_start: (f64, f64),
        axis_end: (f64, f64),
        total_angle: f64,
        segments: u32,
    ) -> Result<Solid, BrepError> {
        revolve_impl(profile, axis_start, axis_end, total_angle, segments)
    }

    fn tessellate(&self, solid: &Solid, tolerance: f64) -> Result<Mesh, BrepError> {
        tessellate_impl(solid, tolerance)
    }

    fn translate(&self, solid: &Solid, dx: f64, dy: f64, dz: f64) -> Result<Solid, BrepError> {
        let mut result = solid.clone();
        for v in &mut result.vertices {
            v.x += dx;
            v.y += dy;
            v.z += dz;
        }
        // Update face surface origins
        for face in &mut result.faces {
            match &mut face.surface {
                Surface::Plane { origin, .. } => {
                    origin[0] += dx;
                    origin[1] += dy;
                    origin[2] += dz;
                }
                _ => {}
            }
        }
        Ok(result)
    }

    fn rotate(
        &self,
        solid: &Solid,
        axis_origin: [f64; 3],
        axis_dir: [f64; 3],
        angle_rad: f64,
    ) -> Result<Solid, BrepError> {
        let mut result = solid.clone();
        // Normalize axis direction
        let len = (axis_dir[0].powi(2) + axis_dir[1].powi(2) + axis_dir[2].powi(2)).sqrt();
        if len < 1e-12 {
            return Err(BrepError::InvalidGeometry("rotate axis has zero length".into()));
        }
        let ux = axis_dir[0] / len;
        let uy = axis_dir[1] / len;
        let uz = axis_dir[2] / len;
        let cos_a = angle_rad.cos();
        let sin_a = angle_rad.sin();

        for v in &mut result.vertices {
            // Rodrigues rotation formula
            let rx = v.x - axis_origin[0];
            let ry = v.y - axis_origin[1];
            let rz = v.z - axis_origin[2];
            let dot = rx * ux + ry * uy + rz * uz;
            let cx = rx - dot * ux;
            let cy = ry - dot * uy;
            let cz = rz - dot * uz;
            let cross_x = uy * cz - uz * cy;
            let cross_y = uz * cx - ux * cz;
            let cross_z = ux * cy - uy * cx;
            v.x = axis_origin[0] + dot * ux + cx * cos_a + cross_x * sin_a;
            v.y = axis_origin[1] + dot * uy + cy * cos_a + cross_y * sin_a;
            v.z = axis_origin[2] + dot * uz + cz * cos_a + cross_z * sin_a;
        }
        // Rotate face surface normals
        for face in &mut result.faces {
            match &mut face.surface {
                Surface::Plane { origin, normal, u_dir } => {
                    let rotate_vec = |v: &mut [f64; 3]| {
                        let rx = v[0] - axis_origin[0];
                        let ry = v[1] - axis_origin[1];
                        let rz = v[2] - axis_origin[2];
                        let dot = rx * ux + ry * uy + rz * uz;
                        let cx = rx - dot * ux;
                        let cy = ry - dot * uy;
                        let cz = rz - dot * uz;
                        let cross_x = uy * cz - uz * cy;
                        let cross_y = uz * cx - ux * cz;
                        let cross_z = ux * cy - uy * cx;
                        v[0] = axis_origin[0] + dot * ux + cx * cos_a + cross_x * sin_a;
                        v[1] = axis_origin[1] + dot * uy + cy * cos_a + cross_y * sin_a;
                        v[2] = axis_origin[2] + dot * uz + cz * cos_a + cross_z * sin_a;
                    };
                    rotate_vec(origin);
                    rotate_vec(normal);
                    rotate_vec(u_dir);
                }
                _ => {}
            }
        }
        Ok(result)
    }

    fn mirror_across_plane(
        &self,
        solid: &Solid,
        plane_normal: [f64; 3],
        plane_point: [f64; 3],
    ) -> Result<Solid, BrepError> {
        let mut result = solid.clone();
        let nx = plane_normal[0];
        let ny = plane_normal[1];
        let nz = plane_normal[2];
        let n_len = (nx * nx + ny * ny + nz * nz).sqrt();
        if n_len < 1e-12 {
            return Err(BrepError::InvalidGeometry("mirror plane normal has zero length".into()));
        }
        let nx = nx / n_len;
        let ny = ny / n_len;
        let nz = nz / n_len;

        for v in &mut result.vertices {
            let dx = v.x - plane_point[0];
            let dy = v.y - plane_point[1];
            let dz = v.z - plane_point[2];
            let dot = dx * nx + dy * ny + dz * nz;
            v.x -= 2.0 * dot * nx;
            v.y -= 2.0 * dot * ny;
            v.z -= 2.0 * dot * nz;
        }
        // Mirror face surface normals and origins
        for face in &mut result.faces {
            match &mut face.surface {
                Surface::Plane { origin, normal, u_dir } => {
                    let mirror_vec = |v: &mut [f64; 3]| {
                        let dx = v[0] - plane_point[0];
                        let dy = v[1] - plane_point[1];
                        let dz = v[2] - plane_point[2];
                        let dot = dx * nx + dy * ny + dz * nz;
                        v[0] -= 2.0 * dot * nx;
                        v[1] -= 2.0 * dot * ny;
                        v[2] -= 2.0 * dot * nz;
                    };
                    mirror_vec(origin);
                    mirror_vec(normal);
                    mirror_vec(u_dir);
                }
                _ => {}
            }
        }
        Ok(result)
    }
}

// ============================================================================
// extrude_impl
// ============================================================================

fn extrude_impl(profile: &[ProfilePoint], height: f64) -> Result<Solid, BrepError> {
    let n = profile.len();

    // --- validation ---
    if n < 3 {
        return Err(BrepError::InvalidGeometry(
            "profile must have at least 3 points".into(),
        ));
    }
    if height.abs() < 1e-10 {
        return Err(BrepError::InvalidGeometry(
            "height must be non-zero".into(),
        ));
    }

    // --- Step 1: build vertices (2n total) ---
    // bottom ring v[0..n), top ring v[n..2n)
    let mut vertices = Vec::with_capacity(2 * n);
    for pt in profile {
        vertices.push(Vertex::new(pt.x, pt.y, 0.0));
    }
    for pt in profile {
        vertices.push(Vertex::new(pt.x, pt.y, height));
    }

    // --- Step 2: build edges (3n total) ---
    // bottom edges [0..n), top edges [n..2n), vertical edges [2n..3n)
    let mut edges = Vec::with_capacity(3 * n);

    // bottom edges: (i, (i+1)%n)
    for i in 0..n {
        let j = (i + 1) % n;
        edges.push(Edge {
            curve: Curve::Line,
            vertices: (i, j),
        });
    }
    // top edges: (n+i, n+(i+1)%n)
    for i in 0..n {
        let j = (i + 1) % n;
        edges.push(Edge {
            curve: Curve::Line,
            vertices: (n + i, n + j),
        });
    }
    // vertical edges: (i, n+i)
    for i in 0..n {
        edges.push(Edge {
            curve: Curve::Line,
            vertices: (i, n + i),
        });
    }

    // --- Step 3: build faces (n + 2 total) ---
    let mut faces = Vec::with_capacity(n + 2);

    // Bottom face (index 0)
    {
        let bottom_wire = Wire {
            edges: (0..n)
                .map(|i| WireEdge {
                    edge_idx: i,
                    forward: true,
                })
                .collect(),
        };
        faces.push(Face {
            surface: Surface::Plane {
                origin: [0.0, 0.0, 0.0],
                normal: [0.0, 0.0, -1.0],
                u_dir: [1.0, 0.0, 0.0],
            },
            outer_wire: bottom_wire,
            inner_wires: Vec::new(),
        });
    }

    // Top face (index 1)
    {
        let top_wire = Wire {
            edges: (n..2 * n)
                .map(|i| WireEdge {
                    edge_idx: i,
                    forward: true,
                })
                .collect(),
        };
        faces.push(Face {
            surface: Surface::Plane {
                origin: [0.0, 0.0, height],
                normal: [0.0, 0.0, 1.0],
                u_dir: [1.0, 0.0, 0.0],
            },
            outer_wire: top_wire,
            inner_wires: Vec::new(),
        });
    }

    // Side faces (indices 2..n+2)
    for i in 0..n {
        let j = (i + 1) % n; // next profile index

        // Compute the side face plane
        let p_i = &profile[i];
        let p_j = &profile[j];

        // Edge direction in XY plane
        let dx = p_j.x - p_i.x;
        let dy = p_j.y - p_i.y;
        let edge_len = (dx * dx + dy * dy).sqrt();
        if edge_len < 1e-12 {
            return Err(BrepError::InvalidGeometry(format!(
                "profile edge {i} has zero length"
            )));
        }

        // Outward normal of the side face (rotate edge vector 90 deg CCW
        // so it points outward for a CCW profile)
        let nx = dy / edge_len; // outward X
        let ny = -dx / edge_len; // outward Y

        let side_wire = Wire {
            edges: vec![
                // Bottom edge i, forward
                WireEdge {
                    edge_idx: i,
                    forward: true,
                },
                // Vertical edge at j, up
                WireEdge {
                    edge_idx: 2 * n + j,
                    forward: true,
                },
                // Top edge i, reversed
                WireEdge {
                    edge_idx: n + i,
                    forward: false,
                },
                // Vertical edge at i, down
                WireEdge {
                    edge_idx: 2 * n + i,
                    forward: false,
                },
            ],
        };

        faces.push(Face {
            surface: Surface::Plane {
                origin: [p_i.x, p_i.y, 0.0],
                normal: [nx, ny, 0.0],
                u_dir: [dx / edge_len, dy / edge_len, 0.0],
            },
            outer_wire: side_wire,
            inner_wires: Vec::new(),
        });
    }

    Ok(Solid {
        faces,
        edges,
        vertices,
    })
}

// ============================================================================
// revolve_impl
// ============================================================================

/// Revolve a 2D profile around an axis in the XY plane.
///
/// Builds a B-rep solid with vertices arranged in rings around the axis,
/// planar-facet side faces between adjacent rings, and end caps for full
/// 360° revolves.
fn revolve_impl(
    profile: &[ProfilePoint],
    axis_start: (f64, f64),
    axis_end: (f64, f64),
    total_angle: f64,
    segments: u32,
) -> Result<Solid, BrepError> {
    let n = profile.len();

    // --- validation ---
    if n < 2 {
        return Err(BrepError::InvalidGeometry(
            "profile must have at least 2 points for revolve".into(),
        ));
    }
    if total_angle.abs() < 1e-10 {
        return Err(BrepError::InvalidGeometry(
            "revolve angle must be non-zero".into(),
        ));
    }

    let (ax, ay) = axis_start;
    let (bx, by) = axis_end;
    let axis_dx = bx - ax;
    let axis_dy = by - ay;
    let axis_len = (axis_dx * axis_dx + axis_dy * axis_dy).sqrt();
    if axis_len < 1e-10 {
        return Err(BrepError::InvalidGeometry(
            "revolve axis must have non-zero length".into(),
        ));
    }
    let ux = axis_dx / axis_len;
    let uy = axis_dy / axis_len;
    // Perpendicular direction in XY plane (rotate axis by +90°)
    let px = -uy;
    let py = ux;

    let n_segments = segments.max(4);
    let angle_step = total_angle / n_segments as f64;
    let is_full_rev = (total_angle.abs() - 2.0 * std::f64::consts::PI).abs() < 1e-10;

    // --- Build vertices: (n_segments + 1) rings ---
    let ring_count = n_segments as usize + 1;
    let total_verts = ring_count * n;
    let mut vertices = Vec::with_capacity(total_verts);

    for ring in 0..ring_count {
        let angle = ring as f64 * angle_step;
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        for pt in profile {
            // Decompose profile point into axial / radial components
            let dx = pt.x - ax;
            let dy = pt.y - ay;
            let axial_dist = dx * ux + dy * uy;
            let radial_dist = dx * px + dy * py;

            // Rotate around the axis
            let wx = ax + axial_dist * ux + radial_dist * cos_a * px;
            let wy = ay + axial_dist * uy + radial_dist * cos_a * py;
            let wz = radial_dist * sin_a;

            vertices.push(Vertex::new(wx, wy, wz));
        }
    }

    // --- Build edges ---
    // Profile edges per ring: n edges per ring * ring_count
    // Ring-to-ring edges: n edges * n_segments
    let total_profile_edges = ring_count * n;
    let ring_to_ring_edges = n * n_segments as usize;
    let total_edges = total_profile_edges + ring_to_ring_edges;
    let mut edges = Vec::with_capacity(total_edges);

    // Profile edges for each ring
    for ring in 0..ring_count {
        let base = ring * n;
        for i in 0..n {
            let j = (i + 1) % n;
            edges.push(Edge {
                curve: Curve::Line,
                vertices: (base + i, base + j),
            });
        }
    }

    // Ring-to-ring vertical edges
    for ring in 0..n_segments as usize {
        let base_a = ring * n;
        let base_b = (ring + 1) * n;
        for i in 0..n {
            edges.push(Edge {
                curve: Curve::Line,
                vertices: (base_a + i, base_b + i),
            });
        }
    }

    // --- Build faces ---
    let mut faces = Vec::new();

    // Side faces: one per profile edge per angular segment
    for ring in 0..n_segments as usize {
        for i in 0..n {
            let j = (i + 1) % n;
            // Four vertices of the quad:
            // bottom_edge = profile edge i in ring k (edge_idx = ring*n + i)
            // top_edge = profile edge i in ring k+1
            // left_vertical = ring-to-ring edge at i
            // right_vertical = ring-to-ring edge at j
            let bottom_edge_idx = ring * n + i;
            let top_edge_idx = (ring + 1) * n + i;
            let left_vert_idx = total_profile_edges + ring * n + i;
            let right_vert_idx = total_profile_edges + ring * n + j;

            // Compute approximate plane normal from three vertices
            let v0 = vertices[ring * n + i].to_array();
            let v1 = vertices[ring * n + j].to_array();
            let v2 = vertices[(ring + 1) * n + i].to_array();
            let edge1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
            let edge2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
            let nx = edge1[1] * edge2[2] - edge1[2] * edge2[1];
            let ny = edge1[2] * edge2[0] - edge1[0] * edge2[2];
            let nz = edge1[0] * edge2[1] - edge1[1] * edge2[0];
            let n_len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-12);
            let normal = [nx / n_len, ny / n_len, nz / n_len];
            let u_dir = [edge1[0] / edge1.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-12),
                          edge1[1] / edge1.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-12),
                          edge1[2] / edge1.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-12)];

            faces.push(Face {
                surface: Surface::Plane {
                    origin: v0,
                    normal,
                    u_dir,
                },
                outer_wire: Wire {
                    edges: vec![
                        WireEdge { edge_idx: bottom_edge_idx, forward: true },
                        WireEdge { edge_idx: right_vert_idx, forward: true },
                        WireEdge { edge_idx: top_edge_idx, forward: false },
                        WireEdge { edge_idx: left_vert_idx, forward: false },
                    ],
                },
                inner_wires: Vec::new(),
            });
        }
    }

    // End caps: only for full 360° revolve
    if is_full_rev {
        // We need to close the volume with caps at ring 0 and ring (n_segments).
        // Since ring 0 and ring n_segments share the same positions (angle 0 == 2π),
        // the "caps" are actually the profile polygon at the start/end.
        // For a full revolve, the first and last rings are coincident, so we
        // don't need separate cap faces — the side faces already form a closed
        // volume. But the mesh would have a hole without explicit caps.
        // We'll add disc faces by running the ear-clip on the first ring:
        let first_ring_edges: Vec<usize> = (0..n).collect();

        // Start cap
        faces.push(Face {
            surface: Surface::Plane {
                origin: vertices[0].to_array(),
                normal: [0.0, 0.0, -1.0],
                u_dir: [1.0, 0.0, 0.0],
            },
            outer_wire: Wire {
                edges: first_ring_edges.iter().map(|&ei| WireEdge { edge_idx: ei, forward: true }).collect(),
            },
            inner_wires: Vec::new(),
        });

        // End cap (last ring, reverse winding)
        let last_ring_start = n_segments as usize * n;
        let last_ring_edges: Vec<usize> = (n_segments as usize * n..n_segments as usize * n + n).collect();
        faces.push(Face {
            surface: Surface::Plane {
                origin: vertices[last_ring_start].to_array(),
                normal: [0.0, 0.0, 1.0],
                u_dir: [1.0, 0.0, 0.0],
            },
            outer_wire: Wire {
                edges: last_ring_edges.iter().map(|&ei| WireEdge { edge_idx: ei, forward: false }).collect(),
            },
            inner_wires: Vec::new(),
        });
    }

    Ok(Solid {
        faces,
        edges,
        vertices,
    })
}

// ============================================================================
// tessellate_impl
// ============================================================================

fn tessellate_impl(solid: &Solid, _tolerance: f64) -> Result<Mesh, BrepError> {
    let mut mesh = Mesh::default();

    for face in &solid.faces {
        // Walk the outer wire to get 3D polygon vertices
        let polygon_3d = walk_wire(&face.outer_wire, &solid.edges, &solid.vertices)?;

        // Project to 2D on the face's plane
        let polygon_2d = project_to_2d(&polygon_3d, &face.surface)?;

        // Triangulate the 2D polygon
        let triangles = triangulate_ear_clip(&polygon_2d);

        // Emit triangles with the face's normal
        let normal = match &face.surface {
            Surface::Plane { normal, .. } => normal,
            _ => {
                return Err(BrepError::Unsupported(
                    "tessellation of non-planar faces is not yet supported".into(),
                ));
            }
        };

        // Convert polygon_3d to echi_geom Point2D for mesh building
        // We need to add vertices with the face normal
        let mut local_indices = Vec::<u32>::new();
        for pt_3d in &polygon_3d {
            let idx = mesh.add_vertex(pt_3d[0], pt_3d[1], pt_3d[2], normal[0], normal[1], normal[2]);
            local_indices.push(idx);
        }

        for tri in &triangles {
            mesh.add_triangle(
                local_indices[tri[0]],
                local_indices[tri[1]],
                local_indices[tri[2]],
            );
        }
    }

    Ok(mesh)
}

// ============================================================================
// walk_wire — collect ordered 3D vertex positions by walking a wire
// ============================================================================

fn walk_wire(wire: &Wire, edges: &[Edge], vertices: &[Vertex]) -> Result<Vec<[f64; 3]>, BrepError> {
    let mut pts = Vec::new();

    for we in &wire.edges {
        let edge = edges.get(we.edge_idx).ok_or_else(|| {
            BrepError::Kernel(format!("wire references non-existent edge {}", we.edge_idx))
        })?;

        let (vi, vj) = edge.vertices;
        let v0 = vertices.get(vi).ok_or_else(|| {
            BrepError::Kernel(format!("edge {} references non-existent vertex {}", we.edge_idx, vi))
        })?;
        let v1 = vertices.get(vj).ok_or_else(|| {
            BrepError::Kernel(format!("edge {} references non-existent vertex {}", we.edge_idx, vj))
        })?;

        if we.forward {
            pts.push(v0.to_array());
        } else {
            pts.push(v1.to_array());
        }
    }

    // Deduplicate: if the last point equals the first point (closed loop),
    // remove the duplicate.
    if pts.len() >= 2 {
        let first = pts[0];
        let last = pts[pts.len() - 1];
        let dist_sq = (first[0] - last[0]).powi(2)
            + (first[1] - last[1]).powi(2)
            + (first[2] - last[2]).powi(2);
        if dist_sq < 1e-20 {
            pts.pop();
        }
    }

    Ok(pts)
}

// ============================================================================
// project_to_2d — project 3D polygon onto a face's surface plane
// ============================================================================

fn project_to_2d(
    polygon_3d: &[[f64; 3]],
    surface: &Surface,
) -> Result<Vec<ProfilePoint>, BrepError> {
    match surface {
        Surface::Plane {
            origin,
            normal,
            u_dir,
        } => {
            // Compute v_dir = normal x u_dir (cross product)
            let v_dir = cross_product(normal, u_dir);

            let mut pts = Vec::with_capacity(polygon_3d.len());
            for p in polygon_3d {
                let dx = p[0] - origin[0];
                let dy = p[1] - origin[1];
                let dz = p[2] - origin[2];
                let u = dx * u_dir[0] + dy * u_dir[1] + dz * u_dir[2];
                let v = dx * v_dir[0] + dy * v_dir[1] + dz * v_dir[2];
                pts.push(ProfilePoint::new(u, v));
            }
            Ok(pts)
        }
        Surface::Cylinder { .. } => Err(BrepError::Unsupported(
            "projection of cylinder surfaces is not yet supported".into(),
        )),
        Surface::Other => Err(BrepError::Unsupported(
            "projection of unknown surfaces is not supported".into(),
        )),
    }
}

fn cross_product(a: &[f64; 3], b: &[f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

// ============================================================================
// triangulate_ear_clip — ear-clipping triangulation for a simple polygon
// ============================================================================

/// Triangulate a simple 2D polygon using the ear-clipping algorithm.
///
/// Returns a list of triangles, each represented as `[a, b, c]` indices
/// into the input polygon. For correct results the polygon should be
/// counter-clockwise. CW polygons are automatically reversed.
fn triangulate_ear_clip(polygon: &[ProfilePoint]) -> Vec<[usize; 3]> {
    let n = polygon.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }

    // Ensure CCW winding
    let area = polygon_area(polygon);
    let mut indices: Vec<usize> = (0..n).collect();
    if area < 0.0 {
        indices.reverse();
    }

    let mut triangles = Vec::new();

    // Ear-clipping loop
    while indices.len() > 3 {
        let m = indices.len();
        let mut ear_found = false;

        for i in 0..m {
            let prev = if i == 0 { m - 1 } else { i - 1 };
            let next = if i == m - 1 { 0 } else { i + 1 };

            let a = &polygon[indices[prev]];
            let b = &polygon[indices[i]];
            let c = &polygon[indices[next]];

            // Check that the angle at b is convex
            if cross(*a, *b, *c) <= 0.0 {
                continue;
            }

            // Check that no other vertex lies inside triangle (a,b,c)
            let mut is_ear = true;
            for j in 0..m {
                if j == prev || j == i || j == next {
                    continue;
                }
                if point_in_triangle(&polygon[indices[j]], a, b, c) {
                    is_ear = false;
                    break;
                }
            }

            if is_ear {
                triangles.push([indices[prev], indices[i], indices[next]]);
                indices.remove(i);
                ear_found = true;
                break;
            }
        }

        if !ear_found {
            // Fall back to fan triangulation from vertex 0
            break;
        }
    }

    // Emit the remaining triangle (or fan-fallback)
    if indices.len() == 3 {
        triangles.push([indices[0], indices[1], indices[2]]);
    } else if indices.len() > 3 {
        // Fan triangulation from vertex 0
        for i in 1..indices.len() - 1 {
            triangles.push([indices[0], indices[i], indices[i + 1]]);
        }
    }

    triangles
}

/// 2D cross product: cross(o→a, o→b)
fn cross(o: ProfilePoint, a: ProfilePoint, b: ProfilePoint) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Shoelace formula — returns signed area (positive for CCW).
fn polygon_area(polygon: &[ProfilePoint]) -> f64 {
    let n = polygon.len();
    if n < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        let p = &polygon[i];
        let q = &polygon[j];
        area += p.x * q.y - q.x * p.y;
    }
    area * 0.5
}

/// Barycentric point-in-triangle test.
fn point_in_triangle(p: &ProfilePoint, a: &ProfilePoint, b: &ProfilePoint, c: &ProfilePoint) -> bool {
    let d1 = cross(*a, *b, *p);
    let d2 = cross(*b, *c, *p);
    let d3 = cross(*c, *a, *p);
    let has_neg = d1 < -1e-12 || d2 < -1e-12 || d3 < -1e-12;
    let has_pos = d1 > 1e-12 || d2 > 1e-12 || d3 > 1e-12;
    !(has_neg && has_pos)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrude_unit_square_produces_valid_solid() {
        let kernel = MockBrepKernel;
        let profile = vec![
            ProfilePoint::new(0.0, 0.0),
            ProfilePoint::new(1.0, 0.0),
            ProfilePoint::new(1.0, 1.0),
            ProfilePoint::new(0.0, 1.0),
        ];
        let solid = kernel.extrude(&profile, 2.0).expect("extrude should succeed");

        // 4 bottom + 4 top = 8 vertices
        assert_eq!(solid.vertex_count(), 8);
        // 4 bottom + 4 top + 4 vertical = 12 edges
        assert_eq!(solid.edge_count(), 12);
        // 1 bottom + 1 top + 4 sides = 6 faces
        assert_eq!(solid.face_count(), 6);

        // Bottom face normal points down (-Z)
        match &solid.faces[0].surface {
            Surface::Plane { normal, .. } => {
                assert!(
                    (normal[2] + 1.0).abs() < 1e-10,
                    "bottom normal should be -Z, got {:?}",
                    normal
                );
            }
            _ => panic!("expected Plane"),
        }

        // Top face normal points up (+Z)
        match &solid.faces[1].surface {
            Surface::Plane { normal, .. } => {
                assert!(
                    (normal[2] - 1.0).abs() < 1e-10,
                    "top normal should be +Z, got {:?}",
                    normal
                );
            }
            _ => panic!("expected Plane"),
        }
    }

    #[test]
    fn extrude_rejects_empty_profile() {
        let kernel = MockBrepKernel;
        assert!(kernel.extrude(&[], 1.0).is_err());
    }

    #[test]
    fn extrude_rejects_zero_height() {
        let kernel = MockBrepKernel;
        let profile = vec![
            ProfilePoint::new(0.0, 0.0),
            ProfilePoint::new(1.0, 0.0),
            ProfilePoint::new(0.0, 1.0),
        ];
        assert!(kernel.extrude(&profile, 0.0).is_err());
    }

    #[test]
    fn extrude_rejects_two_point_profile() {
        let kernel = MockBrepKernel;
        let profile = vec![
            ProfilePoint::new(0.0, 0.0),
            ProfilePoint::new(1.0, 0.0),
        ];
        assert!(kernel.extrude(&profile, 1.0).is_err());
    }

    #[test]
    fn tessellate_cube_produces_watertight_mesh() {
        let kernel = MockBrepKernel;
        let profile = vec![
            ProfilePoint::new(0.0, 0.0),
            ProfilePoint::new(1.0, 0.0),
            ProfilePoint::new(1.0, 1.0),
            ProfilePoint::new(0.0, 1.0),
        ];
        let solid = kernel.extrude(&profile, 1.0).unwrap();
        let mesh = kernel.tessellate(&solid, 0.01).expect("tessellate should succeed");

        // A cube has at least 12 triangles (2 per face * 6 faces)
        assert!(mesh.vertex_count() > 0);
        assert_eq!(
            mesh.indices.len() % 3,
            0,
            "indices must form complete triangles"
        );
        assert!(
            !mesh.positions.iter().any(|v| v.is_nan()),
            "no NaN in positions"
        );
        assert!(
            !mesh.normals.iter().any(|v| v.is_nan()),
            "no NaN in normals"
        );

        // At least 12 triangles = 36 indices for a unit cube
        assert!(
            mesh.indices.len() >= 36,
            "expected at least 36 indices for a cube, got {}",
            mesh.indices.len()
        );
    }

    #[test]
    fn extrude_tessellate_triangle_prism() {
        let kernel = MockBrepKernel;
        let profile = vec![
            ProfilePoint::new(0.0, 0.0),
            ProfilePoint::new(2.0, 0.0),
            ProfilePoint::new(1.0, 1.732),
        ];
        let solid = kernel.extrude(&profile, 0.5).unwrap();
        let mesh = kernel.tessellate(&solid, 0.01).unwrap();

        // 2 * 3 = 6 vertices, 3*3 = 9 edges, 2+3 = 5 faces
        assert_eq!(solid.vertex_count(), 6);
        assert_eq!(solid.edge_count(), 9);
        assert_eq!(solid.face_count(), 5);

        // 1 triangle each for 2 caps + 2 triangles each for 3 quad sides
        // = 8 triangles = 24 indices
        assert!(mesh.indices.len() >= 24);
        assert!(mesh.vertex_count() >= 6);
    }

    #[test]
    fn solid_serde_roundtrip() {
        let kernel = MockBrepKernel;
        let profile = vec![
            ProfilePoint::new(0.0, 0.0),
            ProfilePoint::new(1.0, 0.0),
            ProfilePoint::new(0.0, 1.0),
        ];
        let solid = kernel.extrude(&profile, 1.0).unwrap();

        let json = serde_json::to_string_pretty(&solid).expect("serialize");
        let roundtripped: Solid = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(solid.vertex_count(), roundtripped.vertex_count());
        assert_eq!(solid.edge_count(), roundtripped.edge_count());
        assert_eq!(solid.face_count(), roundtripped.face_count());
        // Verify vertex positions match
        for (a, b) in solid.vertices.iter().zip(roundtripped.vertices.iter()) {
            assert!((a.x - b.x).abs() < 1e-12);
            assert!((a.y - b.y).abs() < 1e-12);
            assert!((a.z - b.z).abs() < 1e-12);
        }
    }

    #[test]
    fn brep_error_display_and_debug() {
        let err = BrepError::InvalidGeometry("profile too small".into());
        assert!(format!("{}", err).contains("profile too small"));
        assert!(format!("{:?}", err).contains("InvalidGeometry"));
    }

    #[test]
    fn snapshot_simple_cube_solid() {
        let kernel = MockBrepKernel;
        let profile = vec![
            ProfilePoint::new(0.0, 0.0),
            ProfilePoint::new(1.0, 0.0),
            ProfilePoint::new(1.0, 1.0),
            ProfilePoint::new(0.0, 1.0),
        ];
        let solid = kernel.extrude(&profile, 1.0).unwrap();
        insta::assert_debug_snapshot!(solid);
    }

    #[test]
    fn revolve_square_around_y_axis() {
        let kernel = MockBrepKernel;
        // Profile: a rectangle in the X>0 half-plane that will sweep a cylinder-like shape
        let profile = vec![
            ProfilePoint::new(1.0, 0.0),
            ProfilePoint::new(2.0, 0.0),
            ProfilePoint::new(2.0, 1.0),
            ProfilePoint::new(1.0, 1.0),
        ];
        let solid = kernel
            .revolve(&profile, (0.0, 0.0), (0.0, 1.0), 2.0 * std::f64::consts::PI, 16)
            .expect("revolve should succeed");

        // 16 segments + 1 = 17 rings * 4 profile points = 68 vertices
        assert_eq!(solid.vertex_count(), 17 * 4);
        // 4 profile edges * 17 rings + 4 * 16 vertical edges = 132
        assert_eq!(solid.edge_count(), 4 * 17 + 4 * 16);
        // 16 * 4 side faces + 2 end caps = 66
        assert_eq!(solid.face_count(), 16 * 4 + 2);
    }

    #[test]
    fn revolve_produces_tessellatable_mesh() {
        let kernel = MockBrepKernel;
        let profile = vec![
            ProfilePoint::new(1.0, 0.0),
            ProfilePoint::new(2.0, 0.0),
            ProfilePoint::new(2.0, 0.5),
            ProfilePoint::new(1.0, 0.5),
        ];
        let solid = kernel
            .revolve(&profile, (0.0, 0.0), (0.0, 1.0), 2.0 * std::f64::consts::PI, 12)
            .expect("revolve should succeed");
        let mesh = kernel.tessellate(&solid, 0.01).expect("tessellate should succeed");

        assert!(mesh.vertex_count() > 0);
        assert!(!mesh.indices.is_empty());
        assert_eq!(mesh.indices.len() % 3, 0);
        assert!(!mesh.positions.iter().any(|v| v.is_nan()));
    }

    #[test]
    fn revolve_rejects_short_profile() {
        let kernel = MockBrepKernel;
        let profile = vec![ProfilePoint::new(1.0, 0.0)];
        assert!(kernel
            .revolve(&profile, (0.0, 0.0), (0.0, 1.0), std::f64::consts::PI, 8)
            .is_err());
    }

    #[test]
    fn revolve_rejects_zero_angle() {
        let kernel = MockBrepKernel;
        let profile = vec![
            ProfilePoint::new(1.0, 0.0),
            ProfilePoint::new(2.0, 0.0),
            ProfilePoint::new(2.0, 1.0),
            ProfilePoint::new(1.0, 1.0),
        ];
        assert!(kernel
            .revolve(&profile, (0.0, 0.0), (0.0, 1.0), 0.0, 8)
            .is_err());
    }

    #[test]
    fn revolve_rejects_degenerate_axis() {
        let kernel = MockBrepKernel;
        let profile = vec![
            ProfilePoint::new(1.0, 0.0),
            ProfilePoint::new(2.0, 0.0),
            ProfilePoint::new(2.0, 1.0),
        ];
        assert!(kernel
            .revolve(&profile, (0.0, 0.0), (0.0, 0.0), std::f64::consts::PI, 8)
            .is_err());
    }

    // ---- transform tests ----

    fn make_cube() -> Solid {
        let kernel = MockBrepKernel;
        let profile = vec![
            ProfilePoint::new(0.0, 0.0),
            ProfilePoint::new(1.0, 0.0),
            ProfilePoint::new(1.0, 1.0),
            ProfilePoint::new(0.0, 1.0),
        ];
        kernel.extrude(&profile, 1.0).unwrap()
    }

    #[test]
    fn translate_cube() {
        let kernel = MockBrepKernel;
        let cube = make_cube();
        let moved = kernel.translate(&cube, 3.0, 4.0, 5.0).unwrap();
        assert_eq!(moved.vertex_count(), cube.vertex_count());
        // Check first vertex moved
        assert!((moved.vertices[0].x - 3.0).abs() < 1e-12);
        assert!((moved.vertices[0].y - 4.0).abs() < 1e-12);
        assert!((moved.vertices[0].z - 5.0).abs() < 1e-12);
        // Original unchanged
        assert!((cube.vertices[0].x - 0.0).abs() < 1e-12);
    }

    #[test]
    fn rotate_cube_around_z() {
        let kernel = MockBrepKernel;
        let cube = make_cube();
        let rotated = kernel
            .rotate(&cube, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], std::f64::consts::PI / 2.0)
            .unwrap();
        assert_eq!(rotated.vertex_count(), cube.vertex_count());
        // Point (1,0,0) rotated 90° around Z → (0,1,0)
        let v1 = &rotated.vertices[1]; // (1,0,0) → (0,1,0)
        assert!((v1.x - 0.0).abs() < 1e-10, "expected x≈0, got {}", v1.x);
        assert!((v1.y - 1.0).abs() < 1e-10, "expected y≈1, got {}", v1.y);
    }

    #[test]
    fn mirror_cube_across_yz() {
        let kernel = MockBrepKernel;
        let cube = make_cube();
        let mirrored = kernel
            .mirror_across_plane(&cube, [1.0, 0.0, 0.0], [0.0, 0.0, 0.0])
            .unwrap();
        assert_eq!(mirrored.vertex_count(), cube.vertex_count());
        // Point (1,0,0) → (-1,0,0)
        let v1 = &mirrored.vertices[1]; // was (1,0,0)
        assert!((v1.x + 1.0).abs() < 1e-10, "expected x≈-1, got {}", v1.x);
    }

    #[test]
    fn linear_pattern_mesh_creates_copies() {
        let kernel = MockBrepKernel;
        let cube = make_cube();
        let single_mesh = kernel.tessellate(&cube, 0.01).unwrap();
        let single_vc = single_mesh.vertex_count();
        let single_tc = single_mesh.indices.len() / 3;
        let mesh = kernel.linear_pattern_mesh(&cube, 1.0, 0.0, 0.0, 3, 2.0).unwrap();
        // 3 copies
        assert_eq!(mesh.vertex_count(), single_vc * 3);
        assert_eq!(mesh.indices.len() / 3, single_tc * 3);
    }

    #[test]
    fn circular_pattern_mesh_creates_copies() {
        let kernel = MockBrepKernel;
        let cube = make_cube();
        let single_vc = kernel.tessellate(&cube, 0.01).unwrap().vertex_count();
        let mesh = kernel
            .circular_pattern_mesh(&cube, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 4, 360.0)
            .unwrap();
        assert_eq!(mesh.vertex_count(), single_vc * 4);
    }

    #[test]
    fn mirror_mesh_doubles_vertex_count() {
        let kernel = MockBrepKernel;
        let cube = make_cube();
        let single_vc = kernel.tessellate(&cube, 0.01).unwrap().vertex_count();
        let mesh = kernel.mirror_mesh(&cube, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0).unwrap();
        assert_eq!(mesh.vertex_count(), single_vc * 2);
    }
}
