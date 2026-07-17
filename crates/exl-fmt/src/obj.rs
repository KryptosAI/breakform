use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use exl_core::geom::Mesh;
use exl_core::{Document, EntityStatus, FidelityReport, GeometryPayload, Part};

use crate::{doc_meshes, fresh_doc, FmtError};

fn parse_face_element(
    token: &str,
    num_verts: usize,
    num_normals: usize,
    num_uvs: usize,
) -> Result<(u32, Option<u32>, Option<u32>), FmtError> {
    let resolve = |s: &str, max: usize| -> Result<u32, FmtError> {
        let i: i32 = s
            .parse()
            .map_err(|_| FmtError::Parse(format!("bad index: {}", s)))?;
        if i == 0 {
            return Err(FmtError::Parse("OBJ indices are 1-based, got 0".into()));
        }
        if i > 0 {
            if i as usize > max {
                return Err(FmtError::Parse(format!(
                    "index {} out of range (max {})",
                    i, max
                )));
            }
            Ok((i - 1) as u32)
        } else {
            let resolved = (max as i32 + i) as u32;
            Ok(resolved)
        }
    };
    let parts: Vec<&str> = token.split('/').collect();
    match parts.len() {
        1 => {
            let vi = resolve(parts[0], num_verts)?;
            Ok((vi, None, None))
        }
        2 => {
            let vi = resolve(parts[0], num_verts)?;
            let ti = resolve(parts[1], num_uvs)?;
            Ok((vi, Some(ti), None))
        }
        3 => {
            let vi = resolve(parts[0], num_verts)?;
            let ti = if parts[1].is_empty() {
                None
            } else {
                Some(resolve(parts[1], num_uvs)?)
            };
            let ni = if parts[2].is_empty() {
                None
            } else {
                Some(resolve(parts[2], num_normals)?)
            };
            Ok((vi, ti, ni))
        }
        _ => Err(FmtError::Parse(format!("invalid face element: {}", token))),
    }
}

fn triangulate_fan(indices: &[u32]) -> Vec<[u32; 3]> {
    if indices.len() < 3 {
        return Vec::new();
    }
    let mut tris = Vec::with_capacity(indices.len() - 2);
    for k in 1..(indices.len() - 1) {
        tris.push([indices[0], indices[k], indices[k + 1]]);
    }
    tris
}

pub fn import_obj(path: &Path) -> Result<(Document, FidelityReport), FmtError> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut faces: Vec<[u32; 3]> = Vec::new();
    let mut face_groups: Vec<u32> = Vec::new();
    let mut group_names: Vec<String> = Vec::new();
    let mut current_group: Option<u32> = None;

    let mut obj_name = String::from("obj_import");
    let mut poly_count = 0usize;
    let mut dropped_materials = false;

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut tokens = trimmed.split_whitespace();
        let cmd = tokens.next().unwrap_or("");
        match cmd {
            "v" => {
                let mut coords = [0.0f32; 3];
                for (i, tok) in tokens.enumerate() {
                    if i >= 3 {
                        break;
                    }
                    coords[i] = tok
                        .parse()
                        .map_err(|_| FmtError::Parse(format!("bad vertex: {}", tok)))?;
                }
                positions.push(coords);
            }
            "vn" => {
                let mut coords = [0.0f32; 3];
                for (i, tok) in tokens.enumerate() {
                    if i >= 3 {
                        break;
                    }
                    coords[i] = tok
                        .parse()
                        .map_err(|_| FmtError::Parse(format!("bad normal: {}", tok)))?;
                }
                normals.push(coords);
            }
            "vt" => {
                let mut coords = [0.0f32; 2];
                for (i, tok) in tokens.enumerate() {
                    if i >= 2 {
                        break;
                    }
                    coords[i] = tok
                        .parse()
                        .map_err(|_| FmtError::Parse(format!("bad uv: {}", tok)))?;
                }
                uvs.push(coords);
            }
            "f" => {
                let raw_tokens: Vec<&str> = tokens.collect();
                if raw_tokens.len() < 3 {
                    return Err(FmtError::Parse(format!(
                        "face needs at least 3 vertices, got {}",
                        raw_tokens.len()
                    )));
                }
                let mut vert_indices: Vec<u32> = Vec::with_capacity(raw_tokens.len());
                for tok in &raw_tokens {
                    let (vi, _ti, _ni) =
                        parse_face_element(tok, positions.len(), normals.len(), uvs.len())?;
                    vert_indices.push(vi);
                }
                let tris = triangulate_fan(&vert_indices);
                for tri in tris {
                    faces.push(tri);
                    if let Some(g) = current_group {
                        face_groups.push(g);
                    }
                }
                poly_count += 1;
            }
            "g" | "o" => {
                let rest: Vec<&str> = tokens.collect();
                let gname = if rest.is_empty() {
                    "default".to_string()
                } else {
                    rest.join(" ")
                };
                let gid: u32 = group_names
                    .iter()
                    .position(|n| n == &gname)
                    .map(|p| p as u32)
                    .unwrap_or_else(|| {
                        let id = group_names.len() as u32;
                        group_names.push(gname);
                        id
                    });
                current_group = Some(gid);
            }
            "usemtl" | "mtllib" => {
                dropped_materials = true;
            }
            _ => {}
        }
    }

    if obj_name == "obj_import" && !group_names.is_empty() {
        obj_name = group_names[0].clone();
    }

    let mut report = FidelityReport::new("obj", "exl");
    report.record("vertices", positions.len(), EntityStatus::Lossless, None);
    report.record(
        "triangles",
        faces.len(),
        EntityStatus::Lossless,
        Some(format!("from {} polygons", poly_count)),
    );

    if dropped_materials {
        report.record(
            "materials",
            0,
            EntityStatus::Dropped,
            Some("mtllib/usemtl not resolved at v0".into()),
        );
    }

    let mesh_normals = if normals.is_empty() {
        None
    } else {
        Some(normals)
    };
    let mesh_uvs = if uvs.is_empty() { None } else { Some(uvs) };
    let mesh_face_groups = if face_groups.is_empty() {
        None
    } else {
        Some(face_groups)
    };

    let mesh = Mesh {
        vertices: positions,
        faces,
        normals: mesh_normals,
        uvs: mesh_uvs,
        face_groups: mesh_face_groups,
        group_names,
    };
    let part = Part::new(&obj_name, GeometryPayload::Mesh(mesh));
    let mut doc = fresh_doc(vec![part], "exl-fmt OBJ importer");
    doc.refresh_content_hash();
    Ok((doc, report))
}

pub fn export_obj(doc: &Document, path: &Path) -> Result<FidelityReport, FmtError> {
    let meshes = doc_meshes(doc)?;
    let mut file = std::fs::File::create(path)?;
    let mut report = FidelityReport::new("exl", "obj");

    for (part, mesh) in meshes {
        let group_lines: Vec<String> = if part.name.is_empty() {
            Vec::new()
        } else {
            vec![format!("o {}", part.name)]
        };
        for gl in &group_lines {
            writeln!(file, "{}", gl)?;
        }

        for v in &mesh.vertices {
            writeln!(file, "v {:.6} {:.6} {:.6}", v[0], v[1], v[2])?;
        }

        let has_normals = mesh.normals.is_some();
        if let Some(ref ns) = mesh.normals {
            for n in ns {
                writeln!(file, "vn {:.6} {:.6} {:.6}", n[0], n[1], n[2])?;
            }
        }

        if mesh.uvs.is_some() {
            report.record(
                "uvs",
                0,
                EntityStatus::Dropped,
                Some("uvs not exported (v0)".into()),
            );
        }

        let mut total_tris = 0usize;

        if let Some(ref fg) = mesh.face_groups {
            if !mesh.group_names.is_empty() {
                let mut groups: Vec<(u32, &str)> = Vec::new();
                for (fi, &gid) in fg.iter().enumerate() {
                    let name = mesh
                        .group_names
                        .get(gid as usize)
                        .map(|s| s.as_str())
                        .unwrap_or("default");
                    if groups.last().map(|g| g.0) != Some(gid) {
                        groups.push((gid, name));
                    }
                    writeln!(file, "g {}", name)?;
                    let tri = mesh.faces[fi];
                    if has_normals {
                        writeln!(
                            file,
                            "f {}//{} {}//{} {}//{}",
                            tri[0] + 1,
                            tri[0] + 1,
                            tri[1] + 1,
                            tri[1] + 1,
                            tri[2] + 1,
                            tri[2] + 1
                        )?;
                    } else {
                        writeln!(file, "f {} {} {}", tri[0] + 1, tri[1] + 1, tri[2] + 1)?;
                    }
                    total_tris += 1;
                }
            } else {
                for (fi, &gid) in fg.iter().enumerate() {
                    if fi == 0 || fg[fi - 1] != gid {
                        writeln!(file, "g group_{}", gid)?;
                    }
                    let tri = mesh.faces[fi];
                    if has_normals {
                        writeln!(
                            file,
                            "f {}//{} {}//{} {}//{}",
                            tri[0] + 1,
                            tri[0] + 1,
                            tri[1] + 1,
                            tri[1] + 1,
                            tri[2] + 1,
                            tri[2] + 1
                        )?;
                    } else {
                        writeln!(file, "f {} {} {}", tri[0] + 1, tri[1] + 1, tri[2] + 1)?;
                    }
                    total_tris += 1;
                }
            }
        } else {
            for tri in &mesh.faces {
                if has_normals {
                    writeln!(
                        file,
                        "f {}//{} {}//{} {}//{}",
                        tri[0] + 1,
                        tri[0] + 1,
                        tri[1] + 1,
                        tri[1] + 1,
                        tri[2] + 1,
                        tri[2] + 1
                    )?;
                } else {
                    writeln!(file, "f {} {} {}", tri[0] + 1, tri[1] + 1, tri[2] + 1)?;
                }
                total_tris += 1;
            }
        }

        report.record("triangles", total_tris, EntityStatus::Lossless, None);
        report.record(
            "vertices",
            mesh.vertices.len(),
            EntityStatus::Lossless,
            None,
        );
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("exl_fmt_test_{}", name));
        p
    }

    #[test]
    fn obj_quad_triangulation() {
        let p = temp_path("quad.obj");
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "o Quad").unwrap();
        writeln!(f, "v 0 0 0").unwrap();
        writeln!(f, "v 1 0 0").unwrap();
        writeln!(f, "v 1 1 0").unwrap();
        writeln!(f, "v 0 1 0").unwrap();
        writeln!(f, "f 1 2 3 4").unwrap();
        drop(f);

        let (doc, report) = import_obj(&p).unwrap();
        assert_eq!(report.source_format, "obj");
        let mesh = match &doc.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };
        assert_eq!(mesh.faces.len(), 2);
        assert_eq!(mesh.vertices.len(), 4);
    }

    #[test]
    fn obj_negative_indices() {
        let p = temp_path("negidx.obj");
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "v 0 0 0").unwrap();
        writeln!(f, "v 1 0 0").unwrap();
        writeln!(f, "v 0 1 0").unwrap();
        writeln!(f, "v 1 1 0").unwrap();
        writeln!(f, "f -4 -3 -2 -1").unwrap();
        drop(f);

        let (doc, _) = import_obj(&p).unwrap();
        let mesh = match &doc.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };
        assert_eq!(mesh.faces.len(), 2);
        assert_eq!(mesh.faces[0], [0, 1, 2]);
    }

    #[test]
    fn obj_groups_preserved_roundtrip() {
        let p = temp_path("groups.obj");
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "v 0 0 0").unwrap();
        writeln!(f, "v 1 0 0").unwrap();
        writeln!(f, "v 0 1 0").unwrap();
        writeln!(f, "v 1 1 0").unwrap();
        writeln!(f, "v 0 0 1").unwrap();
        writeln!(f, "v 1 0 1").unwrap();
        writeln!(f, "g group_a").unwrap();
        writeln!(f, "f 1 2 3").unwrap();
        writeln!(f, "g group_b").unwrap();
        writeln!(f, "f 4 5 6").unwrap();
        drop(f);

        let (doc, _) = import_obj(&p).unwrap();
        let mesh = match &doc.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };
        assert!(mesh.face_groups.is_some());
        let fg = mesh.face_groups.as_ref().unwrap();
        assert_eq!(fg.len(), 2);
        assert_eq!(fg[0], 0);
        assert_eq!(fg[1], 1);
        assert_eq!(mesh.group_names.len(), 2);
        assert_eq!(mesh.group_names[0], "group_a");
        assert_eq!(mesh.group_names[1], "group_b");

        let p2 = temp_path("groups_out.obj");
        export_obj(&doc, &p2).unwrap();

        let content = std::fs::read_to_string(&p2).unwrap();
        assert!(content.contains("g group_a"));
        assert!(content.contains("g group_b"));
    }

    #[test]
    fn obj_v_vn_formats() {
        let p = temp_path("formats.obj");
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "v 0 0 0").unwrap();
        writeln!(f, "v 1 0 0").unwrap();
        writeln!(f, "v 0 1 0").unwrap();
        writeln!(f, "vn 0 0 1").unwrap();
        writeln!(f, "f 1//1 2//1 3//1").unwrap();
        drop(f);

        let (doc, _) = import_obj(&p).unwrap();
        let mesh = match &doc.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };
        assert_eq!(mesh.faces.len(), 1);
        assert_eq!(mesh.faces[0], [0, 1, 2]);
        assert!(mesh.normals.is_some());
    }
}
