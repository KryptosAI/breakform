use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use exl_core::geom::Mesh;
use exl_core::{Document, EntityStatus, FidelityReport, GeometryPayload, Part};

use crate::{doc_meshes, fresh_doc, FmtError};

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamed")
        .to_string()
}

fn cross_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let nx = u[1] * v[2] - u[2] * v[1];
    let ny = u[2] * v[0] - u[0] * v[2];
    let nz = u[0] * v[1] - u[1] * v[0];
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    if len > 1e-12 {
        [nx / len, ny / len, nz / len]
    } else {
        [0.0, 0.0, 0.0]
    }
}

fn dedup_vertices(verts: &[[f32; 3]], faces: &[[u32; 3]]) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let mut map: HashMap<[u32; 3], u32> = HashMap::new();
    let mut unique: Vec<[f32; 3]> = Vec::new();
    let mut new_faces: Vec<[u32; 3]> = faces.to_vec();
    for face in new_faces.iter_mut() {
        for vi in face.iter_mut() {
            let v = verts[*vi as usize];
            let key = [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()];
            let idx = map.entry(key).or_insert_with(|| {
                let idx = unique.len() as u32;
                unique.push(v);
                idx
            });
            *vi = *idx;
        }
    }
    (unique, new_faces)
}

fn read_le_f32(buf: &[u8], off: usize) -> f32 {
    let arr: [u8; 4] = [buf[off], buf[off + 1], buf[off + 2], buf[off + 3]];
    f32::from_le_bytes(arr)
}

fn read_le_u32(buf: &[u8], off: usize) -> u32 {
    let arr: [u8; 4] = [buf[off], buf[off + 1], buf[off + 2], buf[off + 3]];
    u32::from_le_bytes(arr)
}

fn parse_stl_binary(data: &[u8]) -> Result<(Vec<[f32; 3]>, Vec<[u32; 3]>, String), FmtError> {
    if data.len() < 84 {
        return Err(FmtError::Parse("binary STL too short".into()));
    }
    let tri_count = read_le_u32(data, 80) as usize;
    let expected = 84 + tri_count * 50;
    if data.len() < expected {
        return Err(FmtError::Parse(format!(
            "binary STL truncated: expected {} bytes, got {}",
            expected,
            data.len()
        )));
    }
    let mut vertices: Vec<[f32; 3]> = Vec::with_capacity(tri_count * 3);
    let mut faces: Vec<[u32; 3]> = Vec::with_capacity(tri_count);
    for i in 0..tri_count {
        let off = 84 + i * 50 + 12;
        let v0 = [
            read_le_f32(data, off),
            read_le_f32(data, off + 4),
            read_le_f32(data, off + 8),
        ];
        let v1 = [
            read_le_f32(data, off + 12),
            read_le_f32(data, off + 16),
            read_le_f32(data, off + 20),
        ];
        let v2 = [
            read_le_f32(data, off + 24),
            read_le_f32(data, off + 28),
            read_le_f32(data, off + 32),
        ];
        let base = vertices.len() as u32;
        vertices.push(v0);
        vertices.push(v1);
        vertices.push(v2);
        faces.push([base, base + 1, base + 2]);
    }
    Ok((vertices, faces, "binary_import".into()))
}

fn parse_stl_ascii(data: &[u8]) -> Result<(Vec<[f32; 3]>, Vec<[u32; 3]>, String), FmtError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| FmtError::Parse("STL file is not valid UTF-8".into()))?;
    let lines: Vec<&str> = text.lines().collect();
    let mut solid_name = String::from("ascii_import");
    let mut vertices: Vec<[f32; 3]> = Vec::new();
    let mut faces: Vec<[u32; 3]> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        let lower = line.to_lowercase();
        if lower.starts_with("solid") && i == 0 {
            let rest = line[5..].trim();
            if !rest.is_empty() {
                solid_name = rest.to_string();
            }
            i += 1;
            continue;
        }
        if lower == "endsolid" || lower.starts_with("endsolid ") {
            break;
        }
        if lower.starts_with("facet normal") {
            let mut tri_verts: [[f32; 3]; 3] = [[0.0; 3]; 3];
            let mut vi = 0;
            i += 1;
            while i < lines.len() {
                let inner = lines[i].trim().to_lowercase();
                if inner == "outer loop" || inner.is_empty() {
                    i += 1;
                    continue;
                }
                if inner.starts_with("vertex") {
                    let parts: Vec<&str> = lines[i].trim().split_whitespace().collect();
                    if parts.len() < 4 {
                        return Err(FmtError::Parse(format!(
                            "invalid vertex line: {}",
                            lines[i]
                        )));
                    }
                    let x: f32 = parts[1]
                        .parse()
                        .map_err(|_| FmtError::Parse(format!("bad vertex x: {}", parts[1])))?;
                    let y: f32 = parts[2]
                        .parse()
                        .map_err(|_| FmtError::Parse(format!("bad vertex y: {}", parts[2])))?;
                    let z: f32 = parts[3]
                        .parse()
                        .map_err(|_| FmtError::Parse(format!("bad vertex z: {}", parts[3])))?;
                    tri_verts[vi] = [x, y, z];
                    vi += 1;
                    if vi == 3 {
                        i += 1;
                        break;
                    }
                }
                i += 1;
            }
            if vi != 3 {
                return Err(FmtError::Parse(
                    "facet missing vertices before endloop/endfacet".into(),
                ));
            }
            while i < lines.len() {
                let inner = lines[i].trim().to_lowercase();
                if inner == "endloop" || inner.is_empty() {
                    i += 1;
                    continue;
                }
                if inner == "endfacet" {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let base = vertices.len() as u32;
            vertices.push(tri_verts[0]);
            vertices.push(tri_verts[1]);
            vertices.push(tri_verts[2]);
            faces.push([base, base + 1, base + 2]);
            continue;
        }
        i += 1;
    }
    Ok((vertices, faces, solid_name))
}

fn detect_and_parse_stl(data: &[u8]) -> Result<(Vec<[f32; 3]>, Vec<[u32; 3]>, String), FmtError> {
    if data.len() >= 5 && &data[..5] == b"solid" {
        match parse_stl_ascii(data) {
            Ok(result) => return Ok(result),
            Err(_) => {}
        }
    }
    parse_stl_binary(data)
}

pub fn import_stl(path: &Path) -> Result<(Document, FidelityReport), FmtError> {
    let data = std::fs::read(path)?;
    let stem = file_stem(path);
    let (verts, faces, solid_name) = detect_and_parse_stl(&data)?;
    let name = if solid_name.is_empty() {
        stem
    } else {
        solid_name
    };
    let input_tri_count = faces.len();
    let (unique_verts, remapped_faces) = dedup_vertices(&verts, &faces);
    let unique_count = unique_verts.len();
    let dedup_note = if verts.len() != unique_count {
        Some(format!(
            "deduplicated {} raw vertices to {} unique",
            verts.len(),
            unique_count
        ))
    } else {
        None
    };
    let mesh = Mesh {
        vertices: unique_verts,
        faces: remapped_faces,
        ..Default::default()
    };
    let part = Part::new(&name, GeometryPayload::Mesh(mesh));
    let doc = fresh_doc(vec![part], "exl-fmt STL importer");
    let mut report = FidelityReport::new("stl", "exl");
    report.record("triangles", input_tri_count, EntityStatus::Lossless, None);
    if let Some(note) = dedup_note {
        report.record("vertex_dedup", 0, EntityStatus::Lossless, Some(note));
    }
    report.record("vertices", unique_count, EntityStatus::Lossless, None);
    Ok((doc, report))
}

pub fn export_stl(doc: &Document, path: &Path, ascii: bool) -> Result<FidelityReport, FmtError> {
    let meshes = doc_meshes(doc)?;
    let mut file = std::fs::File::create(path)?;
    let mut report = FidelityReport::new("exl", "stl");

    for (_part, mesh) in &meshes {
        if mesh.normals.is_some() {
            report.record(
                "normals",
                0,
                EntityStatus::Dropped,
                Some("per-vertex normals not supported in STL".into()),
            );
        }
        if mesh.uvs.is_some() {
            report.record(
                "uvs",
                0,
                EntityStatus::Dropped,
                Some("texture coordinates not supported in STL".into()),
            );
        }
        if mesh.face_groups.is_some() {
            report.record(
                "face_groups",
                0,
                EntityStatus::Dropped,
                Some("face groups not supported in STL".into()),
            );
        }
    }

    let mut all_verts: Vec<[f32; 3]> = Vec::new();
    let mut all_faces: Vec<[u32; 3]> = Vec::new();
    let mut tri_count = 0u32;
    for (_part, mesh) in &meshes {
        let base = all_verts.len() as u32;
        for v in &mesh.vertices {
            all_verts.push(*v);
        }
        for f in &mesh.faces {
            all_faces.push([f[0] + base, f[1] + base, f[2] + base]);
            tri_count += 1;
        }
    }

    for part in &doc.parts {
        if !part.semantics.materials.is_empty() {
            report.record(
                "materials",
                0,
                EntityStatus::Dropped,
                Some("materials not supported in STL".into()),
            );
            break;
        }
    }

    if ascii {
        writeln!(file, "solid exl_export")?;
        for face in &all_faces {
            let a = all_verts[face[0] as usize];
            let b = all_verts[face[1] as usize];
            let c = all_verts[face[2] as usize];
            let n = cross_normal(a, b, c);
            writeln!(file, "  facet normal {:.6} {:.6} {:.6}", n[0], n[1], n[2])?;
            writeln!(file, "    outer loop")?;
            writeln!(file, "      vertex {:.6} {:.6} {:.6}", a[0], a[1], a[2])?;
            writeln!(file, "      vertex {:.6} {:.6} {:.6}", b[0], b[1], b[2])?;
            writeln!(file, "      vertex {:.6} {:.6} {:.6}", c[0], c[1], c[2])?;
            writeln!(file, "    endloop")?;
            writeln!(file, "  endfacet")?;
        }
        writeln!(file, "endsolid exl_export")?;
    } else {
        let mut header = [0u8; 80];
        let hdr_bytes = b"exl-fmt binary STL export";
        let len = hdr_bytes.len().min(80);
        header[..len].copy_from_slice(&hdr_bytes[..len]);
        file.write_all(&header)?;
        file.write_all(&tri_count.to_le_bytes())?;
        for face in &all_faces {
            let a = all_verts[face[0] as usize];
            let b = all_verts[face[1] as usize];
            let c = all_verts[face[2] as usize];
            let n = cross_normal(a, b, c);
            file.write_all(&n[0].to_le_bytes())?;
            file.write_all(&n[1].to_le_bytes())?;
            file.write_all(&n[2].to_le_bytes())?;
            file.write_all(&a[0].to_le_bytes())?;
            file.write_all(&a[1].to_le_bytes())?;
            file.write_all(&a[2].to_le_bytes())?;
            file.write_all(&b[0].to_le_bytes())?;
            file.write_all(&b[1].to_le_bytes())?;
            file.write_all(&b[2].to_le_bytes())?;
            file.write_all(&c[0].to_le_bytes())?;
            file.write_all(&c[1].to_le_bytes())?;
            file.write_all(&c[2].to_le_bytes())?;
            file.write_all(&[0u8, 0u8])?;
        }
    }

    report.record(
        "triangles",
        tri_count as usize,
        EntityStatus::Lossless,
        None,
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fresh_doc;
    use std::io::Write;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("exl_fmt_test_{}", name));
        p
    }

    fn cube_verts() -> Vec<[f32; 3]> {
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ]
    }

    fn cube_faces() -> Vec<[u32; 3]> {
        vec![
            [0, 1, 2],
            [0, 2, 3],
            [1, 5, 6],
            [1, 6, 2],
            [5, 4, 7],
            [5, 7, 6],
            [4, 0, 3],
            [4, 3, 7],
            [3, 2, 6],
            [3, 6, 7],
            [4, 5, 1],
            [4, 1, 0],
        ]
    }

    fn cube_mesh() -> Mesh {
        Mesh {
            vertices: cube_verts(),
            faces: cube_faces(),
            ..Default::default()
        }
    }

    fn write_binary_stl(path: &std::path::Path, verts: &[[f32; 3]], faces: &[[u32; 3]]) {
        let mut f = std::fs::File::create(path).unwrap();
        let header = [0u8; 80];
        f.write_all(&header).unwrap();
        let count = faces.len() as u32;
        f.write_all(&count.to_le_bytes()).unwrap();
        for tri in faces {
            let a = verts[tri[0] as usize];
            let b = verts[tri[1] as usize];
            let c = verts[tri[2] as usize];
            let n = cross_normal(a, b, c);
            f.write_all(&n[0].to_le_bytes()).unwrap();
            f.write_all(&n[1].to_le_bytes()).unwrap();
            f.write_all(&n[2].to_le_bytes()).unwrap();
            f.write_all(&a[0].to_le_bytes()).unwrap();
            f.write_all(&a[1].to_le_bytes()).unwrap();
            f.write_all(&a[2].to_le_bytes()).unwrap();
            f.write_all(&b[0].to_le_bytes()).unwrap();
            f.write_all(&b[1].to_le_bytes()).unwrap();
            f.write_all(&b[2].to_le_bytes()).unwrap();
            f.write_all(&c[0].to_le_bytes()).unwrap();
            f.write_all(&c[1].to_le_bytes()).unwrap();
            f.write_all(&c[2].to_le_bytes()).unwrap();
            f.write_all(&[0u8, 0u8]).unwrap();
        }
    }

    fn write_ascii_stl(path: &std::path::Path, name: &str, verts: &[[f32; 3]], faces: &[[u32; 3]]) {
        let mut f = std::fs::File::create(path).unwrap();
        writeln!(f, "solid {}", name).unwrap();
        for tri in faces {
            let a = verts[tri[0] as usize];
            let b = verts[tri[1] as usize];
            let c = verts[tri[2] as usize];
            let n = cross_normal(a, b, c);
            writeln!(f, "  facet normal {:.6} {:.6} {:.6}", n[0], n[1], n[2]).unwrap();
            writeln!(f, "    outer loop").unwrap();
            writeln!(f, "      vertex {:.6} {:.6} {:.6}", a[0], a[1], a[2]).unwrap();
            writeln!(f, "      vertex {:.6} {:.6} {:.6}", b[0], b[1], b[2]).unwrap();
            writeln!(f, "      vertex {:.6} {:.6} {:.6}", c[0], c[1], c[2]).unwrap();
            writeln!(f, "    endloop").unwrap();
            writeln!(f, "  endfacet").unwrap();
        }
        writeln!(f, "endsolid {}", name).unwrap();
    }

    fn bbox_of(verts: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for v in verts {
            for i in 0..3 {
                if v[i] < min[i] {
                    min[i] = v[i];
                }
                if v[i] > max[i] {
                    max[i] = v[i];
                }
            }
        }
        (min, max)
    }

    #[test]
    fn binary_stl_round_trip() {
        let verts = cube_verts();
        let faces = cube_faces();
        let p = temp_path("binary_roundtrip.stl");
        write_binary_stl(&p, &verts, &faces);

        let (doc, report) = import_stl(&p).unwrap();
        assert_eq!(report.source_format, "stl");
        let mesh = match &doc.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };
        assert_eq!(mesh.faces.len(), 12);
        assert_eq!(mesh.vertices.len(), 8);

        let p2 = temp_path("binary_roundtrip_out.stl");
        let report2 = export_stl(&doc, &p2, false).unwrap();
        assert_eq!(report2.source_format, "exl");

        let (doc2, _) = import_stl(&p2).unwrap();
        let mesh2 = match &doc2.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };
        assert_eq!(mesh2.faces.len(), 12);

        let (bmin, bmax) = bbox_of(&mesh.vertices);
        let (b2min, b2max) = bbox_of(&mesh2.vertices);
        for i in 0..3 {
            assert!((bmin[i] - b2min[i]).abs() < 0.001);
            assert!((bmax[i] - b2max[i]).abs() < 0.001);
        }
    }

    #[test]
    fn ascii_stl_round_trip() {
        let verts = cube_verts();
        let faces = cube_faces();
        let p = temp_path("ascii_roundtrip.stl");
        write_ascii_stl(&p, "TestCube", &verts, &faces);

        let (doc, report) = import_stl(&p).unwrap();
        assert_eq!(report.source_format, "stl");
        let mesh = match &doc.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };
        assert_eq!(mesh.faces.len(), 12);

        let p2 = temp_path("ascii_roundtrip_out.stl");
        export_stl(&doc, &p2, true).unwrap();

        let (doc2, _) = import_stl(&p2).unwrap();
        let mesh2 = match &doc2.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };
        assert_eq!(mesh2.faces.len(), 12);

        let (bmin, bmax) = bbox_of(&mesh.vertices);
        let (b2min, b2max) = bbox_of(&mesh2.vertices);
        for i in 0..3 {
            assert!((bmin[i] - b2min[i]).abs() < 0.001);
            assert!((bmax[i] - b2max[i]).abs() < 0.001);
        }
    }

    #[test]
    fn vertex_dedup_cube() {
        let verts = cube_verts();
        let faces = cube_faces();
        let p = temp_path("dedup_cube.stl");
        write_binary_stl(&p, &verts, &faces);

        let (doc, _) = import_stl(&p).unwrap();
        let mesh = match &doc.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };
        assert_eq!(mesh.vertices.len(), 8);
        assert_eq!(mesh.faces.len(), 12);
    }

    #[test]
    fn stl_export_drops_normals() {
        let mut mesh = cube_mesh();
        mesh.normals = Some(vec![[0.0, 0.0, 1.0]; 8]);
        let part = Part::new("cube", GeometryPayload::Mesh(mesh));
        let doc = fresh_doc(vec![part], "test");
        let p = temp_path("drops.stl");
        let report = export_stl(&doc, &p, false).unwrap();
        let has_normals_note = report
            .entities
            .iter()
            .any(|e| e.entity == "normals" && e.status == EntityStatus::Dropped);
        assert!(has_normals_note);
    }

    #[test]
    fn combine_all_tri_counts() {
        let verts = cube_verts();
        let faces = cube_faces();
        let p = temp_path("combine.stl");
        write_ascii_stl(&p, "TestCube", &verts, &faces);

        let (doc, _) = import_stl(&p).unwrap();
        let mesh = match &doc.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };
        assert_eq!(mesh.faces.len(), 12);
        assert_eq!(mesh.vertices.len(), 8);
    }
}
