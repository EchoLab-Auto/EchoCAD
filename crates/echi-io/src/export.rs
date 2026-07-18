//! Export geometry to common mesh formats.

use echi_geom::extrude::Mesh;
use std::io::Write;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
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
}
