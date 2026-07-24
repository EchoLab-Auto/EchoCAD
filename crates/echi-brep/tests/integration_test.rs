use echi_brep::{BrepKernel, MockBrepKernel, ProfilePoint};

/// End-to-end: extrude a pentagon, tessellate, verify the mesh properties.
#[test]
fn pentagon_extrude_and_tessellate() {
    let kernel = MockBrepKernel;

    // Regular pentagon of radius 1 centered at origin
    let n = 5;
    let profile: Vec<ProfilePoint> = (0..n)
        .map(|i| {
            let angle = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            ProfilePoint::new(angle.cos(), angle.sin())
        })
        .collect();

    let solid = kernel.extrude(&profile, 2.0).expect("pentagon extrude");
    assert_eq!(solid.vertex_count(), 10);
    assert_eq!(solid.edge_count(), 15);
    assert_eq!(solid.face_count(), 7); // 1 bottom + 1 top + 5 sides

    let mesh = kernel.tessellate(&solid, 0.001).expect("pentagon tessellate");
    // Verify bounds: all Z in [0, 2]
    for i in (0..mesh.positions.len()).step_by(3) {
        let z = mesh.positions[i + 2];
        assert!(
            z >= -1e-6 && z <= 2.0 + 1e-6,
            "vertex Z {} out of bounds [0,2]",
            z
        );
    }
    // Verify we have a reasonable number of triangles
    assert!(mesh.indices.len() >= 3 * 7); // at least 7 triangles (one per face min)
}
