use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Vertex
// ---------------------------------------------------------------------------

/// A vertex — a point in 3D space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vertex {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Returns this vertex as an array `[x, y, z]`.
    pub fn to_array(&self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
}

// ---------------------------------------------------------------------------
// Curve
// ---------------------------------------------------------------------------

/// The 3D curve geometry of an edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Curve {
    /// Straight line segment between the edge's two vertices.
    Line,
    /// Circular arc. `center` and `radius` are in 3D world coordinates.
    /// `start_angle` and `end_angle` are in radians measured in the plane
    /// perpendicular to the arc's axis.
    Arc {
        center: [f64; 3],
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    /// Placeholder for future NURBS / general parametric curves.
    /// Not serializable — skipped during serde round-trips.
    #[serde(skip)]
    Other,
}

// ---------------------------------------------------------------------------
// Edge
// ---------------------------------------------------------------------------

/// An edge — a curve segment bounded by two vertices.
///
/// `vertices` are indices into `Solid::vertices`. The edge connects
/// `vertices.0` to `vertices.1` in the direction defined by the curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub curve: Curve,
    /// Indices into the owning Solid's vertex array: (start, end).
    pub vertices: (usize, usize),
}

// ---------------------------------------------------------------------------
// WireEdge / Wire
// ---------------------------------------------------------------------------

/// A reference to an edge within a wire, carrying orientation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WireEdge {
    /// Index into `Solid::edges`.
    pub edge_idx: usize,
    /// When true, the edge is traversed from `vertices.0` to `vertices.1`.
    /// When false, traversed in reverse.
    pub forward: bool,
}

/// An ordered loop of wire edges forming a face boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub edges: Vec<WireEdge>,
}

// ---------------------------------------------------------------------------
// Surface
// ---------------------------------------------------------------------------

/// The surface geometry of a face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Surface {
    /// Infinite plane. `origin` is a point on the plane, `normal` is the
    /// outward-facing unit normal, and `u_dir` is a reference direction in
    /// the plane (for parameterization purposes).
    Plane {
        origin: [f64; 3],
        normal: [f64; 3],
        u_dir: [f64; 3],
    },
    /// Cylindrical surface. `center` is a point on the axis, `axis` is the
    /// axis direction, `radius` is the cylinder radius.
    Cylinder {
        center: [f64; 3],
        axis: [f64; 3],
        radius: f64,
    },
    /// Placeholder for future NURBS / general parametric surfaces.
    /// Not serializable — skipped during serde round-trips.
    #[serde(skip)]
    Other,
}

// ---------------------------------------------------------------------------
// Face
// ---------------------------------------------------------------------------

/// A face — a bounded surface patch.
///
/// Bounded by exactly one outer wire and zero or more inner wires (holes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub surface: Surface,
    pub outer_wire: Wire,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inner_wires: Vec<Wire>,
}

// ---------------------------------------------------------------------------
// Solid
// ---------------------------------------------------------------------------

/// A boundary-representation solid body.
///
/// All topological indices (vertex indices in edges, edge indices in wires)
/// refer into the `vertices` and `edges` vectors of this struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    pub faces: Vec<Face>,
    pub edges: Vec<Edge>,
    pub vertices: Vec<Vertex>,
}

impl Solid {
    /// Create an empty solid.
    pub fn new() -> Self {
        Self {
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: Vec::new(),
        }
    }

    /// Total number of vertices in this solid.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Total number of edges in this solid.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Total number of faces in this solid.
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }
}

impl Default for Solid {
    fn default() -> Self {
        Self::new()
    }
}
