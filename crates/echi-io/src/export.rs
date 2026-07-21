//! Export geometry to common mesh formats.

use echi_geom::extrude::Mesh;
use std::io::Write;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("no mesh data to export")]
    NoMesh,
}

/// Export a mesh as ASCII STL.
pub fn export_stl_ascii(mesh: &Mesh, writer: &mut impl Write) -> Result<(), ExportError> {
    writeln!(writer, "solid EchoCAD")?;

    for chunk in mesh.indices.chunks(3) {
        let i0 = chunk[0] as usize * 3;
        let i1 = chunk[1] as usize * 3;
        let i2 = chunk[2] as usize * 3;

        let p0 = [
            mesh.positions[i0],
            mesh.positions[i0 + 1],
            mesh.positions[i0 + 2],
        ];
        let p1 = [
            mesh.positions[i1],
            mesh.positions[i1 + 1],
            mesh.positions[i1 + 2],
        ];
        let p2 = [
            mesh.positions[i2],
            mesh.positions[i2 + 1],
            mesh.positions[i2 + 2],
        ];

        let normal = compute_normal(p0, p1, p2);

        writeln!(writer, "  facet normal {} {} {}", normal[0], normal[1], normal[2])?;
        writeln!(writer, "    outer loop")?;
        writeln!(writer, "      vertex {} {} {}", p0[0], p0[1], p0[2])?;
        writeln!(writer, "      vertex {} {} {}", p1[0], p1[1], p1[2])?;
        writeln!(writer, "      vertex {} {} {}", p2[0], p2[1], p2[2])?;
        writeln!(writer, "    endloop")?;
        writeln!(writer, "  endfacet")?;
    }

    writeln!(writer, "endsolid EchoCAD")?;
    Ok(())
}

/// Export a mesh as binary STL.
///
/// Standard format: 80-byte header (arbitrary content), a little-endian
/// `u32` triangle count, then 50 bytes per triangle (3 × f32 normal,
/// 3 × 3 × f32 vertices, and a trailing `u16` attribute byte count of 0).
/// Face normals are recomputed from vertex positions, matching the ASCII path.
pub fn export_stl_binary(mesh: &Mesh, writer: &mut impl Write) -> Result<(), ExportError> {
    // 80-byte header. Content is arbitrary per the spec; we embed a readable
    // label and zero-pad the remainder (some strict readers reject headers
    // that begin with "solid", so we avoid that prefix).
    let mut header = [0u8; 80];
    let label = b"EchoCAD binary STL";
    let n = label.len().min(header.len());
    header[..n].copy_from_slice(&label[..n]);
    writer.write_all(&header)?;

    // Triangle count is derived from complete index triples; a trailing
    // partial triple (malformed input) is dropped instead of panicking.
    let triangle_count = (mesh.indices.len() / 3) as u32;
    writer.write_all(&triangle_count.to_le_bytes())?;

    for chunk in mesh.indices.chunks_exact(3) {
        let i0 = chunk[0] as usize * 3;
        let i1 = chunk[1] as usize * 3;
        let i2 = chunk[2] as usize * 3;
        let p0 = [
            mesh.positions[i0],
            mesh.positions[i0 + 1],
            mesh.positions[i0 + 2],
        ];
        let p1 = [
            mesh.positions[i1],
            mesh.positions[i1 + 1],
            mesh.positions[i1 + 2],
        ];
        let p2 = [
            mesh.positions[i2],
            mesh.positions[i2 + 1],
            mesh.positions[i2 + 2],
        ];

        let normal = compute_normal(p0, p1, p2);
        writer.write_all(&normal[0].to_le_bytes())?;
        writer.write_all(&normal[1].to_le_bytes())?;
        writer.write_all(&normal[2].to_le_bytes())?;

        for p in [p0, p1, p2] {
            writer.write_all(&p[0].to_le_bytes())?;
            writer.write_all(&p[1].to_le_bytes())?;
            writer.write_all(&p[2].to_le_bytes())?;
        }

        writer.write_all(&0u16.to_le_bytes())?; // attribute byte count
    }

    Ok(())
}

/// Export a mesh as a minimal but valid glTF 2.0 JSON document.
///
/// The geometry (positions + indices, normals when present) is packed into a
/// single little-endian binary buffer and embedded as a base64 data URI on
/// `buffers[0]`. The document wires `scene → node → mesh → primitive` with
/// POSITION (and optional NORMAL) attributes and UNSIGNED_INT indices.
/// No external assets, no extensions, no dependencies.
pub fn export_gltf(mesh: &Mesh, writer: &mut impl Write) -> Result<(), ExportError> {
    // Assemble the binary buffer: positions, indices, normals (in that order).
    // All components are 4 bytes wide, so each buffer view is naturally
    // 4-byte aligned when laid out contiguously starting at offset 0.
    let positions_byte_len = mesh.positions.len() * 4;
    let indices_byte_len = mesh.indices.len() * 4;

    let mut buffer: Vec<u8> = Vec::with_capacity(positions_byte_len + indices_byte_len);
    for &p in &mesh.positions {
        buffer.extend_from_slice(&p.to_le_bytes());
    }
    for &i in &mesh.indices {
        buffer.extend_from_slice(&i.to_le_bytes());
    }

    let vertex_count = mesh.positions.len() / 3;
    let index_count = mesh.indices.len();

    // bufferViews + accessors, built incrementally so indices stay in sync.
    let mut buffer_views: Vec<serde_json::Value> = Vec::new();
    let mut accessors: Vec<serde_json::Value> = Vec::new();
    let mut next_bv = 0usize;
    let mut next_acc = 0usize;

    let mut attributes = serde_json::Map::new();

    // POSITION (VEC3 FLOAT). glTF requires min/max on position accessors.
    buffer_views.push(serde_json::json!({
        "buffer": 0,
        "byteOffset": 0,
        "byteLength": positions_byte_len,
        "target": 34962,
    }));
    let mut pos_accessor = serde_json::json!({
        "bufferView": next_bv,
        "componentType": 5126,
        "count": vertex_count,
        "type": "VEC3",
    });
    if vertex_count > 0 {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for c in 0..3 {
            let mut i = c;
            while i < mesh.positions.len() {
                let v = mesh.positions[i];
                if v < min[c] {
                    min[c] = v;
                }
                if v > max[c] {
                    max[c] = v;
                }
                i += 3;
            }
        }
        // Guard against non-finite positions; serde_json renders Inf/NaN as null.
        if min.iter().all(|v| v.is_finite()) && max.iter().all(|v| v.is_finite()) {
            pos_accessor["min"] = serde_json::json!([min[0], min[1], min[2]]);
            pos_accessor["max"] = serde_json::json!([max[0], max[1], max[2]]);
        }
    }
    accessors.push(pos_accessor);
    attributes.insert("POSITION".to_string(), serde_json::json!(next_acc));
    next_bv += 1;
    next_acc += 1;

    // INDICES (SCALAR UNSIGNED_INT). glTF 2.0 supports 32-bit indices natively.
    let mut primitive = serde_json::Map::new();
    if index_count > 0 {
        buffer_views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": positions_byte_len,
            "byteLength": indices_byte_len,
            "target": 34963,
        }));
        accessors.push(serde_json::json!({
            "bufferView": next_bv,
            "componentType": 5125,
            "count": index_count,
            "type": "SCALAR",
        }));
        primitive.insert("indices".to_string(), serde_json::json!(next_acc));
        next_bv += 1;
        next_acc += 1;
    }

    // NORMAL (VEC3 FLOAT) — optional, only when a full normal stream is present.
    let has_normals = mesh.normals.len() == mesh.positions.len() && mesh.normals.len() >= 3;
    if has_normals {
        let normals_byte_len = mesh.normals.len() * 4;
        let normal_offset = positions_byte_len + indices_byte_len;
        buffer_views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": normal_offset,
            "byteLength": normals_byte_len,
            "target": 34962,
        }));
        accessors.push(serde_json::json!({
            "bufferView": next_bv,
            "componentType": 5126,
            "count": vertex_count,
            "type": "VEC3",
        }));
        attributes.insert("NORMAL".to_string(), serde_json::json!(next_acc));
        for &nrm in &mesh.normals {
            buffer.extend_from_slice(&nrm.to_le_bytes());
        }
    }

    primitive.insert("attributes".to_string(), serde_json::Value::Object(attributes));

    let uri = format!(
        "data:application/octet-stream;base64,{}",
        base64_encode(&buffer)
    );

    let gltf = serde_json::json!({
        "asset": { "version": "2.0", "generator": "EchoCAD" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0 }],
        "meshes": [{ "primitives": [serde_json::Value::Object(primitive)] }],
        "buffers": [{ "uri": uri, "byteLength": buffer.len() }],
        "bufferViews": buffer_views,
        "accessors": accessors,
    });

    serde_json::to_writer_pretty(writer, &gltf)?;
    Ok(())
}

/// Base64-encode a byte slice using the standard alphabet with `=` padding.
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    let mut chunks = input.chunks_exact(3);
    for chunk in &mut chunks {
        let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        out.push(TABLE[(n & 0x3F) as usize] as char);
    }
    let rem = chunks.remainder();
    match rem.len() {
        1 => {
            let n = (rem[0] as u32) << 16;
            out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let n = ((rem[0] as u32) << 16) | ((rem[1] as u32) << 8);
            out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
            out.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
            out.push('=');
        }
        _ => {}
    }
    out
}

/// Export a mesh as Wavefront OBJ.
pub fn export_obj(mesh: &Mesh, writer: &mut impl Write) -> Result<(), ExportError> {
    for i in (0..mesh.positions.len()).step_by(3) {
        writeln!(
            writer,
            "v {} {} {}",
            mesh.positions[i],
            mesh.positions[i + 1],
            mesh.positions[i + 2]
        )?;
    }

    for i in (0..mesh.normals.len()).step_by(3) {
        writeln!(
            writer,
            "vn {} {} {}",
            mesh.normals[i],
            mesh.normals[i + 1],
            mesh.normals[i + 2]
        )?;
    }

    // OBJ indices are 1-based
    for chunk in mesh.indices.chunks(3) {
        let i0 = chunk[0] + 1;
        let i1 = chunk[1] + 1;
        let i2 = chunk[2] + 1;
        writeln!(writer, "f {}//{} {}//{} {}//{}", i0, i0, i1, i1, i2, i2)?;
    }

    Ok(())
}

fn compute_normal(p0: [f32; 3], p1: [f32; 3], p2: [f32; 3]) -> [f32; 3] {
    let ux = p1[0] - p0[0];
    let uy = p1[1] - p0[1];
    let uz = p1[2] - p0[2];
    let vx = p2[0] - p0[0];
    let vy = p2[1] - p0[1];
    let vz = p2[2] - p0[2];

    let nx = uy * vz - uz * vy;
    let ny = uz * vx - ux * vz;
    let nz = ux * vy - uy * vx;

    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    if len > 0.0 {
        [nx / len, ny / len, nz / len]
    } else {
        [0.0, 0.0, 1.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_small_stl() {
        let mesh = Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
        };
        let mut buf = Vec::new();
        export_stl_ascii(&mesh, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("solid EchoCAD"));
        assert!(s.contains("facet normal"));
    }

    fn quad_mesh() -> Mesh {
        Mesh {
            positions: vec![
                0.0, 0.0, 0.0,
                1.0, 0.0, 0.0,
                1.0, 1.0, 0.0,
                0.0, 1.0, 0.0,
            ],
            normals: vec![
                0.0, 0.0, 1.0,
                0.0, 0.0, 1.0,
                0.0, 0.0, 1.0,
                0.0, 0.0, 1.0,
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }

    #[test]
    fn export_obj_counts_two_triangles() {
        let mesh = quad_mesh();
        let mut buf = Vec::new();
        export_obj(&mesh, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let v_count = s.lines().filter(|l| l.starts_with("v ")).count();
        let vn_count = s.lines().filter(|l| l.starts_with("vn ")).count();
        let f_count = s.lines().filter(|l| l.starts_with("f ")).count();
        assert_eq!(v_count, 4);
        assert_eq!(vn_count, 4);
        assert_eq!(f_count, 2);
    }

    #[test]
    fn export_binary_stl_header_and_count() {
        let mesh = quad_mesh();
        let mut buf = Vec::new();
        export_stl_binary(&mesh, &mut buf).unwrap();

        // 80-byte header + 4-byte count + 50 bytes per triangle.
        assert_eq!(buf.len(), 80 + 4 + 2 * 50);

        // Header carries our label and does not start with "solid".
        assert!(!buf.starts_with(b"solid"));
        assert_eq!(&buf[..7], b"EchoCAD");

        // Triangle count is the little-endian u32 immediately after the header.
        let count = u32::from_le_bytes(buf[80..84].try_into().unwrap());
        assert_eq!(count, 2);

        // First triangle's first vertex should read back as (0, 0, 0): the
        // normal (12B) sits at offset 84, then the three vertices (36B).
        let v0x = f32::from_le_bytes(buf[96..100].try_into().unwrap());
        let v0y = f32::from_le_bytes(buf[100..104].try_into().unwrap());
        let v0z = f32::from_le_bytes(buf[104..108].try_into().unwrap());
        assert!(v0x.abs() < 1e-5 && v0y.abs() < 1e-5 && v0z.abs() < 1e-5);
    }

    #[test]
    fn export_binary_stl_empty_mesh_is_valid() {
        let mesh = Mesh::default();
        let mut buf = Vec::new();
        export_stl_binary(&mesh, &mut buf).unwrap();
        // Header + zero triangle count, nothing more.
        assert_eq!(buf.len(), 84);
        let count = u32::from_le_bytes(buf[80..84].try_into().unwrap());
        assert_eq!(count, 0);
    }

    #[test]
    fn export_binary_stl_drops_partial_triple() {
        // Five indices: two full triangles + one stray index. The stray must
        // be ignored, not panic.
        let mut mesh = quad_mesh();
        mesh.indices.push(7);
        let mut buf = Vec::new();
        export_stl_binary(&mesh, &mut buf).unwrap();
        let count = u32::from_le_bytes(buf[80..84].try_into().unwrap());
        assert_eq!(count, 2);
    }

    #[test]
    fn export_gltf_has_asset_meshes_and_base64() {
        let mesh = quad_mesh();
        let mut buf = Vec::new();
        export_gltf(&mesh, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(!s.is_empty());
        assert!(s.contains("\"asset\""));
        assert!(s.contains("\"meshes\""));
        assert!(s.contains("base64"));
        assert!(s.contains("data:application/octet-stream;base64,"));
        // POSITION + NORMAL both present, indices wired up.
        assert!(s.contains("\"POSITION\""));
        assert!(s.contains("\"NORMAL\""));
        assert!(s.contains("\"indices\""));
        assert!(s.contains("5125")); // UNSIGNED_INT component type
        // POSITION min/max must be numeric arrays (no NaN/Inf → no `null`).
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        let pos_acc = &v["accessors"][0];
        assert_eq!(pos_acc["type"], "VEC3");
        assert!(pos_acc["min"].is_array());
        assert!(pos_acc["max"].is_array());
        assert!(pos_acc["min"][0].as_f64().is_some());
    }

    #[test]
    fn export_gltf_without_normals_is_valid() {
        let mut mesh = quad_mesh();
        mesh.normals.clear();
        let mut buf = Vec::new();
        export_gltf(&mesh, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("\"POSITION\""));
        assert!(!s.contains("\"NORMAL\""));
    }

    #[test]
    fn export_gltf_empty_mesh_is_valid_json() {
        let mesh = Mesh::default();
        let mut buf = Vec::new();
        export_gltf(&mesh, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        // Still parseable JSON with the required top-level keys.
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["asset"]["version"], "2.0");
        assert_eq!(v["meshes"][0]["primitives"][0]["attributes"]["POSITION"], 0);
    }

    #[test]
    fn base64_roundtrip_matches_known_vectors() {
        // "Man" → "TWFu", well-known base64 vector.
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_encode(b"M"), "TQ==");
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(&[0u8; 3]), "AAAA");
    }
}
