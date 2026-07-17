use exl_core::{Document, EntityStatus, FidelityReport, GeometryPayload, Part, ToolOfOrigin};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, thiserror::Error)]
pub enum OpenfoamError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Unsupported: {0}")]
    Unsupported(String),
}

struct CharReader<'a> {
    chars: &'a [u8],
    pos: usize,
}

impl<'a> CharReader<'a> {
    fn new(input: &'a str) -> Self {
        CharReader {
            chars: input.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_block_comment(&mut self) -> Result<(), OpenfoamError> {
        if self.peek() != Some(b'/') {
            return Ok(());
        }
        if self.chars.get(self.pos + 1).copied() != Some(b'*') {
            return Ok(());
        }
        self.pos += 2;
        while self.pos + 1 < self.chars.len() {
            if self.chars[self.pos] == b'*' && self.chars[self.pos + 1] == b'/' {
                self.pos += 2;
                return Ok(());
            }
            self.pos += 1;
        }
        Err(OpenfoamError::Parse("unclosed block comment".into()))
    }

    fn skip_line_comment(&mut self) {
        if self.peek() == Some(b'/') && self.chars.get(self.pos + 1).copied() == Some(b'/') {
            self.pos += 2;
            while let Some(c) = self.peek() {
                if c == b'\n' {
                    break;
                }
                self.advance();
            }
        }
    }

    fn skip_braced_block(&mut self) -> Result<(), OpenfoamError> {
        if self.peek() != Some(b'{') {
            return Ok(());
        }
        self.advance();
        let mut depth: i32 = 1;
        while depth > 0 {
            match self.peek() {
                Some(b'{') => {
                    depth += 1;
                    self.advance();
                }
                Some(b'}') => {
                    depth -= 1;
                    self.advance();
                }
                Some(b'"') => {
                    self.advance();
                    while let Some(c) = self.peek() {
                        if c == b'"' {
                            self.advance();
                            break;
                        }
                        if c == b'\\' {
                            self.advance();
                        }
                        self.advance();
                    }
                }
                Some(b'/') => match self.chars.get(self.pos + 1).copied() {
                    Some(b'*') => {
                        self.skip_block_comment()?;
                    }
                    Some(b'/') => {
                        self.skip_line_comment();
                    }
                    _ => {
                        self.advance();
                    }
                },
                Some(_) => {
                    self.advance();
                }
                None => {
                    return Err(OpenfoamError::Parse("unclosed brace block".into()));
                }
            }
        }
        Ok(())
    }

    fn skip_foam_header(&mut self) -> Result<(), OpenfoamError> {
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(b'F') => {
                    if self.pos + 7 < self.chars.len()
                        && &self.chars[self.pos..self.pos + 8] == b"FoamFile"
                    {
                        self.pos += 8;
                        self.skip_whitespace();
                        self.skip_braced_block()?;
                    } else {
                        break;
                    }
                }
                Some(b'/') => match self.chars.get(self.pos + 1).copied() {
                    Some(b'*') => {
                        self.skip_block_comment()?;
                    }
                    Some(b'/') => {
                        self.skip_line_comment();
                    }
                    _ => break,
                },
                _ => break,
            }
        }
        Ok(())
    }

    fn read_f64(&mut self) -> Result<f64, OpenfoamError> {
        self.skip_whitespace();
        let start = self.pos;
        if matches!(self.peek(), Some(b'+') | Some(b'-')) {
            self.advance();
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }
        if self.peek() == Some(b'.') {
            self.advance();
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.advance();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.advance();
            }
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        if start == self.pos {
            return Err(OpenfoamError::Parse(format!(
                "expected number at pos {}",
                start
            )));
        }
        let s = std::str::from_utf8(&self.chars[start..self.pos])
            .map_err(|_| OpenfoamError::Parse("invalid utf8 in number".into()))?;
        s.parse::<f64>()
            .map_err(|_| OpenfoamError::Parse(format!("invalid number '{}'", s)))
    }

    fn read_usize(&mut self) -> Result<usize, OpenfoamError> {
        self.skip_whitespace();
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }
        if start == self.pos {
            return Err(OpenfoamError::Parse(format!(
                "expected integer at pos {}",
                start
            )));
        }
        let s = std::str::from_utf8(&self.chars[start..self.pos])
            .map_err(|_| OpenfoamError::Parse("invalid utf8 in integer".into()))?;
        s.parse::<usize>()
            .map_err(|_| OpenfoamError::Parse(format!("invalid integer '{}'", s)))
    }

    fn read_word(&mut self) -> String {
        self.skip_whitespace();
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.advance();
            } else {
                break;
            }
        }
        if start == self.pos {
            return String::new();
        }
        String::from_utf8_lossy(&self.chars[start..self.pos]).to_string()
    }

    fn expect_char(&mut self, expected: u8) -> Result<(), OpenfoamError> {
        self.skip_whitespace();
        match self.peek() {
            Some(c) if c == expected => {
                self.advance();
                Ok(())
            }
            Some(c) => Err(OpenfoamError::Parse(format!(
                "expected '{}' but found '{}' at pos {}",
                expected as char, c as char, self.pos
            ))),
            None => Err(OpenfoamError::Parse(format!(
                "expected '{}' but reached end of file",
                expected as char
            ))),
        }
    }
}

#[derive(Debug, Clone)]
struct Patch {
    name: String,
    start_face: usize,
    n_faces: usize,
}

fn parse_points_content(content: &str) -> Result<Vec<[f32; 3]>, OpenfoamError> {
    let mut r = CharReader::new(content);
    r.skip_foam_header()?;
    let count = r.read_usize()?;
    r.expect_char(b'(')?;
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        r.skip_whitespace();
        r.expect_char(b'(')?;
        let x = r.read_f64()? as f32;
        let y = r.read_f64()? as f32;
        let z = r.read_f64()? as f32;
        r.expect_char(b')')?;
        points.push([x, y, z]);
    }
    r.skip_whitespace();
    r.expect_char(b')')?;
    Ok(points)
}

fn parse_faces_content(content: &str) -> Result<Vec<Vec<u32>>, OpenfoamError> {
    let mut r = CharReader::new(content);
    r.skip_foam_header()?;
    let count = r.read_usize()?;
    r.expect_char(b'(')?;
    let mut faces = Vec::with_capacity(count);
    for _ in 0..count {
        r.skip_whitespace();
        let n_verts = r.read_usize()?;
        r.expect_char(b'(')?;
        let mut verts = Vec::with_capacity(n_verts);
        for _ in 0..n_verts {
            verts.push(r.read_usize()? as u32);
        }
        r.expect_char(b')')?;
        faces.push(verts);
    }
    r.skip_whitespace();
    r.expect_char(b')')?;
    Ok(faces)
}

fn parse_int_list_content(content: &str) -> Result<Vec<u32>, OpenfoamError> {
    let mut r = CharReader::new(content);
    r.skip_foam_header()?;
    let count = r.read_usize()?;
    r.expect_char(b'(')?;
    let mut list = Vec::with_capacity(count);
    for _ in 0..count {
        list.push(r.read_usize()? as u32);
    }
    r.skip_whitespace();
    r.expect_char(b')')?;
    Ok(list)
}

fn parse_boundary_content(content: &str) -> Result<Vec<Patch>, OpenfoamError> {
    let mut r = CharReader::new(content);
    r.skip_foam_header()?;
    r.skip_whitespace();
    if r.peek().is_some_and(|c| c.is_ascii_digit()) {
        r.read_usize()?;
        r.skip_whitespace();
    }
    r.expect_char(b'(')?;
    let mut patches = Vec::new();
    loop {
        r.skip_whitespace();
        match r.peek() {
            Some(b')') => {
                r.advance();
                break;
            }
            None => break,
            Some(c) if !c.is_ascii_alphabetic() => {
                return Err(OpenfoamError::Parse(format!(
                    "expected patch name at pos {}",
                    r.pos
                )));
            }
            _ => {}
        }
        let name = r.read_word();
        r.skip_whitespace();
        r.expect_char(b'{')?;
        let mut start_face: Option<usize> = None;
        let mut n_faces: Option<usize> = None;
        loop {
            r.skip_whitespace();
            if r.peek() == Some(b'}') {
                r.advance();
                break;
            }
            let key = r.read_word();
            r.skip_whitespace();
            if key == "nFaces" {
                n_faces = Some(r.read_usize()?);
            } else if key == "startFace" {
                start_face = Some(r.read_usize()?);
            } else {
                let _ = r.read_word();
            }
            r.skip_whitespace();
            if r.peek() == Some(b';') {
                r.advance();
            }
        }
        let start_face = start_face
            .ok_or_else(|| OpenfoamError::Parse(format!("patch '{}' missing startFace", name)))?;
        let n_faces = n_faces
            .ok_or_else(|| OpenfoamError::Parse(format!("patch '{}' missing nFaces", name)))?;
        patches.push(Patch {
            name,
            start_face,
            n_faces,
        });
    }
    Ok(patches)
}

fn fan_triangulate(face: &[u32]) -> Vec<[u32; 3]> {
    if face.len() < 3 {
        return Vec::new();
    }
    if face.len() == 3 {
        return vec![[face[0], face[1], face[2]]];
    }
    let mut tris = Vec::with_capacity(face.len() - 2);
    for i in 1..face.len() - 1 {
        tris.push([face[0], face[i], face[i + 1]]);
    }
    tris
}

fn build_boundary_mesh(
    points: &[[f32; 3]],
    faces: &[Vec<u32>],
    patches: &[Patch],
) -> exl_core::geom::Mesh {
    let mut new_vertices: Vec<[f32; 3]> = Vec::new();
    let mut vertex_map: HashMap<u32, u32> = HashMap::new();
    let mut new_faces: Vec<[u32; 3]> = Vec::new();
    let mut face_groups: Vec<u32> = Vec::new();
    let mut group_names: Vec<String> = Vec::new();

    for (patch_idx, patch) in patches.iter().enumerate() {
        group_names.push(patch.name.clone());
        for face_verts in faces.iter().skip(patch.start_face).take(patch.n_faces) {
            let triangles = fan_triangulate(face_verts);
            for tri in &triangles {
                let mut new_tri = [0u32; 3];
                for (i, &old_idx) in tri.iter().enumerate() {
                    let new_idx = *vertex_map.entry(old_idx).or_insert_with(|| {
                        let idx = new_vertices.len() as u32;
                        new_vertices.push(points[old_idx as usize]);
                        idx
                    });
                    new_tri[i] = new_idx;
                }
                new_faces.push(new_tri);
                face_groups.push(patch_idx as u32);
            }
        }
    }

    let mut mesh = exl_core::geom::Mesh {
        vertices: new_vertices,
        faces: new_faces,
        ..Default::default()
    };
    if !group_names.is_empty() {
        mesh.face_groups = Some(face_groups);
        mesh.group_names = group_names;
    }
    mesh
}

fn extract_nu(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("nu ") || trimmed.starts_with("nu\t") || trimmed == "nu" {
            let after = if trimmed.len() > 2 {
                trimmed[2..].trim()
            } else {
                continue;
            };
            let tokens: Vec<&str> = after.split_whitespace().collect();
            if let Some(last) = tokens.last() {
                let val = last.trim_end_matches(';');
                if val.parse::<f64>().is_ok() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

fn extract_keyword(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(key) {
            let rest = rest.trim();
            if rest.is_empty() {
                continue;
            }
            let value = rest.split_whitespace().next()?.trim_end_matches(';');
            return Some(value.to_string());
        }
    }
    None
}

fn find_time_dir(case_dir: &Path) -> Option<std::path::PathBuf> {
    let mut time_dirs: Vec<(f64, std::path::PathBuf)> = fs::read_dir(case_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.parse::<f64>().ok().map(|v| (v, e.path()))
        })
        .collect();
    time_dirs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let zero = time_dirs.iter().find(|(v, _)| *v == 0.0);
    if let Some((_, path)) = zero {
        return Some(path.clone());
    }
    time_dirs.into_iter().next().map(|(_, p)| p)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

fn system_time_to_iso(t: SystemTime) -> String {
    let duration = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let mut y: i64 = 1970;
    let mut remaining_days = days as i64;

    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }

    let month_days: [i64; 12] = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m: usize = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md {
            m = i + 1;
            break;
        }
        remaining_days -= md;
    }

    let d = remaining_days + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, minutes, seconds
    )
}

pub fn import_openfoam(case_dir: &Path) -> Result<(Document, FidelityReport), OpenfoamError> {
    if !case_dir.is_dir() {
        return Err(OpenfoamError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "case directory not found",
        )));
    }

    let case_name = case_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("openfoam_case");

    let poly_mesh_dir = case_dir.join("constant").join("polyMesh");
    if !poly_mesh_dir.is_dir() {
        return Err(OpenfoamError::Parse(
            "constant/polyMesh directory not found".into(),
        ));
    }

    let mut report = FidelityReport::new("openfoam", "exl");

    let points_content = fs::read_to_string(poly_mesh_dir.join("points"))?;
    let faces_content = fs::read_to_string(poly_mesh_dir.join("faces"))?;
    let owner_content = fs::read_to_string(poly_mesh_dir.join("owner"))?;
    let boundary_content = fs::read_to_string(poly_mesh_dir.join("boundary"))?;

    let points = parse_points_content(&points_content)?;
    report.record(
        "polyMesh points",
        points.len(),
        EntityStatus::Lossless,
        None,
    );

    let faces = parse_faces_content(&faces_content)?;
    report.record("polyMesh faces", faces.len(), EntityStatus::Lossless, None);

    let _owner = parse_int_list_content(&owner_content)?;
    report.record("polyMesh owner", _owner.len(), EntityStatus::Lossless, None);

    let neighbour_path = poly_mesh_dir.join("neighbour");
    if neighbour_path.exists() {
        let nc = fs::read_to_string(&neighbour_path)?;
        let neighbour = parse_int_list_content(&nc)?;
        report.record(
            "polyMesh neighbour",
            neighbour.len(),
            EntityStatus::Lossless,
            None,
        );
    }

    let patches = parse_boundary_content(&boundary_content)?;
    report.record(
        "polyMesh boundary",
        patches.len(),
        EntityStatus::Lossless,
        None,
    );

    for zone_file in &["cellZones", "faceZones", "pointZones"] {
        let zone_path = poly_mesh_dir.join(zone_file);
        if zone_path.exists() {
            report.record(
                format!("polyMesh {}", zone_file),
                0,
                EntityStatus::Dropped,
                Some("zones not imported".into()),
            );
        }
    }

    let mesh = build_boundary_mesh(&points, &faces, &patches);

    let constant_dir = case_dir.join("constant");
    if let Ok(content) = fs::read_to_string(constant_dir.join("transportProperties")) {
        if let Some(nu_value) = extract_nu(&content) {
            report.record(
                "transportProperties",
                1,
                EntityStatus::Approximate,
                Some(format!(
                    "fluid property values recorded in fidelity only; nu={}",
                    nu_value
                )),
            );
        }
    }
    if constant_dir.join("turbulenceProperties").exists() {
        report.record(
            "turbulenceProperties",
            1,
            EntityStatus::Dropped,
            Some("turbulence model not imported".into()),
        );
    }

    let system_dir = case_dir.join("system");
    if let Ok(content) = fs::read_to_string(system_dir.join("controlDict")) {
        let app = extract_keyword(&content, "application");
        let dt = extract_keyword(&content, "deltaT");
        let end = extract_keyword(&content, "endTime");
        let parts: Vec<String> = [
            app.map(|a| format!("application={}", a)),
            dt.map(|d| format!("deltaT={}", d)),
            end.map(|e| format!("endTime={}", e)),
        ]
        .iter()
        .filter_map(|o| o.clone())
        .collect();
        if !parts.is_empty() {
            report.record(
                "system/controlDict",
                1,
                EntityStatus::Approximate,
                Some(format!(
                    "simulation parameters preserved as notes; {}",
                    parts.join(" ")
                )),
            );
        }
    }

    if let Some(time_dir) = find_time_dir(case_dir) {
        for field in &["U", "p"] {
            let fp = time_dir.join(field);
            if fp.exists() {
                report.record(
                    format!("field {}", field),
                    1,
                    EntityStatus::Approximate,
                    Some("field metadata preserved only".into()),
                );
            }
        }
    }

    let part = Part::new(case_name, GeometryPayload::Mesh(mesh));
    let mut doc = Document::new(vec![part]);
    doc.provenance.tool_of_origin = Some(ToolOfOrigin {
        name: "exl-openfoam".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        timestamp_iso: system_time_to_iso(SystemTime::now()),
    });
    doc.provenance.conversion_fidelity = Some(report.overall);
    doc.refresh_content_hash();

    Ok((doc, report))
}

pub use import_openfoam as import_of;

fn foam_file_header(class: &str, object: &str, location: &str) -> String {
    format!(
        r#"/*--------------------------------*- C++ -*----------------------------------*\
| =========                 |                                                 |
| \\      /  F ield         | OpenFOAM: breakform export                      |
|  \\    /   O peration     |                                                 |
|   \\  /    A nd           |                                                 |
|    \\/     M anipulation  |                                                 |
\*---------------------------------------------------------------------------*/
FoamFile
{{
    version     2.0;
    format      ascii;
    class       {};
    location    "{}";
    object      {};
}}
// * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * //
"#,
        class, location, object
    )
}

pub fn export_openfoam(doc: &Document, case_dir: &Path) -> Result<FidelityReport, OpenfoamError> {
    use std::fs;

    let mut all_vertices: Vec<[f32; 3]> = Vec::new();
    let mut all_faces: Vec<[u32; 3]> = Vec::new();
    let mut all_face_groups: Vec<u32> = Vec::new();
    let mut all_group_names: Vec<String> = Vec::new();
    let mut part_count = 0usize;
    let mut mesh_part_count = 0usize;
    let mut brep_part_count = 0usize;
    let mut skipped_brep = false;

    for part in &doc.parts {
        part_count += 1;
        match &part.geometry {
            GeometryPayload::Mesh(mesh) => {
                mesh_part_count += 1;
                let vert_offset = all_vertices.len() as u32;
                let group_offset = all_group_names.len() as u32;

                all_vertices.extend_from_slice(&mesh.vertices);

                for face in &mesh.faces {
                    all_faces.push([
                        face[0] + vert_offset,
                        face[1] + vert_offset,
                        face[2] + vert_offset,
                    ]);
                }

                if let Some(ref groups) = mesh.face_groups {
                    for &g in groups {
                        all_face_groups.push(g + group_offset);
                    }
                } else {
                    let default_group = all_group_names.len() as u32;
                    if default_group == all_group_names.len() as u32 {
                        all_group_names.push(format!("patch_{}", part.name));
                    }
                    for _ in 0..mesh.faces.len() {
                        all_face_groups.push(group_offset);
                    }
                }

                all_group_names.extend(mesh.group_names.iter().cloned());
            }
            GeometryPayload::Brep(_) => {
                brep_part_count += 1;
                skipped_brep = true;
            }
        }
    }

    if all_faces.is_empty() {
        return Err(OpenfoamError::Unsupported(
            "no mesh parts found in document".into(),
        ));
    }

    let n_faces = all_faces.len();
    let n_vertices = all_vertices.len();
    let n_patches = if all_group_names.is_empty() {
        1
    } else {
        all_group_names.len()
    };

    if all_face_groups.is_empty() {
        all_face_groups.resize(all_face_groups.len() + n_faces, 0);
        if all_group_names.is_empty() {
            all_group_names.push("default".into());
        }
    }

    let max_group = all_face_groups.iter().max().copied().unwrap_or(0) as usize;
    while all_group_names.len() <= max_group {
        all_group_names.push(format!("patch_{}", all_group_names.len()));
    }

    let mut group_face_counts: Vec<usize> = vec![0; all_group_names.len()];
    for &g in &all_face_groups {
        group_face_counts[g as usize] += 1;
    }

    let mut cumulative = 0usize;
    let mut group_start_faces: Vec<usize> = vec![0; all_group_names.len()];
    for (i, count) in group_face_counts.iter().enumerate() {
        group_start_faces[i] = cumulative;
        cumulative += count;
    }

    let mut reorder_map: Vec<usize> = vec![0; n_faces];
    let mut next_write: Vec<usize> = group_start_faces.clone();
    for (fi, &g) in all_face_groups.iter().enumerate() {
        let pos = next_write[g as usize];
        reorder_map[fi] = pos;
        next_write[g as usize] += 1;
    }

    let mut reordered_faces: Vec<[u32; 3]> = vec![[0, 0, 0]; n_faces];
    for (fi, &orig_idx) in reorder_map.iter().enumerate() {
        reordered_faces[orig_idx] = all_faces[fi];
    }

    let poly_mesh_dir = case_dir.join("constant").join("polyMesh");
    fs::create_dir_all(&poly_mesh_dir)?;

    {
        let mut s = foam_file_header("vectorField", "points", "\"constant/polyMesh\"");
        s.push_str(&format!("{}\n(\n", n_vertices));
        for v in &all_vertices {
            s.push_str(&format!("({} {} {})\n", v[0], v[1], v[2]));
        }
        s.push_str(")\n");
        fs::write(poly_mesh_dir.join("points"), s)?;
    }

    {
        let mut s = foam_file_header("faceList", "faces", "\"constant/polyMesh\"");
        s.push_str(&format!("{}\n(\n", n_faces));
        for face in &reordered_faces {
            s.push_str(&format!("3({} {} {})\n", face[0], face[1], face[2]));
        }
        s.push_str(")\n");
        fs::write(poly_mesh_dir.join("faces"), s)?;
    }

    {
        let mut s = foam_file_header("labelList", "owner", "\"constant/polyMesh\"");
        s.push_str(&format!("{}\n(\n", n_faces));
        for _ in 0..n_faces {
            s.push_str("0\n");
        }
        s.push_str(")\n");
        fs::write(poly_mesh_dir.join("owner"), s)?;
    }

    {
        let s = foam_file_header("labelList", "neighbour", "\"constant/polyMesh\"") + "0\n(\n)\n";
        fs::write(poly_mesh_dir.join("neighbour"), s)?;
    }

    {
        let mut s = foam_file_header("polyBoundaryMesh", "boundary", "\"constant/polyMesh\"");
        s.push_str(&format!("{}\n(\n", n_patches));
        for pi in 0..n_patches {
            if group_face_counts[pi] == 0 {
                continue;
            }
            s.push_str(&format!(
                "{}\n{{\n    type            wall;\n    nFaces          {};\n    startFace       {};\n}}\n",
                all_group_names[pi], group_face_counts[pi], group_start_faces[pi]
            ));
        }
        s.push_str(")\n");
        fs::write(poly_mesh_dir.join("boundary"), s)?;
    }

    let constant_dir = case_dir.join("constant");
    fs::create_dir_all(&constant_dir)?;
    {
        let tp = "transportModel  Newtonian;\nnu              nu [0 2 -1 0 0 0 0] 1e-06;\n";
        let mut s = foam_file_header("dictionary", "transportProperties", "\"constant\"");
        s.push_str(tp);
        fs::write(constant_dir.join("transportProperties"), s)?;
    }

    let system_dir = case_dir.join("system");
    fs::create_dir_all(&system_dir)?;
    {
        let cd =
            "application     breakformExport;\n\ndeltaT          0.001;\n\nendTime         1.0;\n";
        let mut s = foam_file_header("dictionary", "controlDict", "\"system\"");
        s.push_str(cd);
        fs::write(system_dir.join("controlDict"), s)?;
    }

    let mut report = FidelityReport::new("exl", "openfoam");
    report.record("parts", part_count, EntityStatus::Lossless, None);
    report.record("mesh parts", mesh_part_count, EntityStatus::Lossless, None);
    report.record("vertices", n_vertices, EntityStatus::Lossless, None);
    report.record("faces", n_faces, EntityStatus::Lossless, None);
    report.record("patches", n_patches, EntityStatus::Lossless, None);

    if skipped_brep {
        report.record(
            "brep parts",
            brep_part_count,
            EntityStatus::Dropped,
            Some("BRep geometry is not representable in OpenFOAM polyMesh".into()),
        );
    }

    Ok(report)
}

pub use export_openfoam as export_of;

#[cfg(test)]
mod tests {
    use super::*;

    fn foam_header(class: &str, object: &str) -> String {
        format!(
            r#"/*--------------------------------*- C++ -*----------------------------------*\
  =========                 |
  \\      /  F ield         | OpenFOAM: The Open Source CFD Toolbox
   \\    /   O peration     | Website:  https://openfoam.org
    \\  /    A nd           | Version:  10
     \\/     M anipulation  |
\*---------------------------------------------------------------------------*/
FoamFile
{{
    version     2.0;
    format      ascii;
    class       {};
    location    "constant/polyMesh";
    object      {};
}}
// * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * //
"#,
            class, object
        )
    }

    fn make_case_dir() -> (std::path::PathBuf, std::path::PathBuf) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp = std::env::temp_dir().join(format!("exl_of_test_{}_{}", std::process::id(), n));
        fs::create_dir_all(&tmp).unwrap();
        let poly_mesh = tmp.join("constant").join("polyMesh");
        fs::create_dir_all(&poly_mesh).unwrap();
        (tmp, poly_mesh)
    }

    #[test]
    fn test_import_minimal_cavity() {
        let (_tmp, poly_mesh) = make_case_dir();

        let vertices: [[f32; 3]; 8] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];

        let mut points_str = foam_header("vectorField", "points");
        points_str.push_str("8\n(\n");
        for v in &vertices {
            points_str.push_str(&format!("({} {} {})\n", v[0], v[1], v[2]));
        }
        points_str.push_str(")\n");
        fs::write(poly_mesh.join("points"), points_str).unwrap();

        let face_verts: [[u32; 4]; 6] = [
            [0, 3, 7, 4], // x-min (left)
            [1, 5, 6, 2], // x-max (right)
            [0, 1, 5, 4], // y-min (front)
            [3, 2, 6, 7], // y-max (back)
            [0, 4, 5, 1], // z-min (bottom)
            [4, 7, 6, 5], // z-max (top)
        ];

        let mut faces_str = foam_header("faceList", "faces");
        faces_str.push_str("6\n(\n");
        for fv in &face_verts {
            faces_str.push_str(&format!("4({} {} {} {})\n", fv[0], fv[1], fv[2], fv[3]));
        }
        faces_str.push_str(")\n");
        fs::write(poly_mesh.join("faces"), faces_str).unwrap();

        let mut owner_str = foam_header("labelList", "owner");
        owner_str.push_str("6\n(\n");
        for _ in 0..6 {
            owner_str.push_str("0\n");
        }
        owner_str.push_str(")\n");
        fs::write(poly_mesh.join("owner"), owner_str).unwrap();

        let neighbour_str = foam_header("labelList", "neighbour") + "0\n(\n)\n";
        fs::write(poly_mesh.join("neighbour"), neighbour_str).unwrap();

        let mut boundary_str = foam_header("polyBoundaryMesh", "boundary");
        boundary_str.push_str(
            "3\n(\n\
            movingWall\n\
            {\n    type        patch;\n    nFaces      1;\n    startFace   0;\n}\n\
            fixedWalls\n\
            {\n    type        wall;\n    nFaces      4;\n    startFace   1;\n}\n\
            frontAndBack\n\
            {\n    type        empty;\n    nFaces      1;\n    startFace   5;\n}\n\
            )\n",
        );
        fs::write(poly_mesh.join("boundary"), boundary_str).unwrap();

        let case_dir = _tmp.as_path();
        let (doc, report) = import_openfoam(case_dir).unwrap();

        assert_eq!(doc.parts.len(), 1);
        let mesh = match &doc.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };

        assert_eq!(mesh.vertices.len(), 8);
        assert_eq!(mesh.faces.len(), 12);
        assert!(mesh.face_groups.is_some());
        let group_names = &mesh.group_names;
        assert!(group_names.contains(&"movingWall".to_string()));
        assert!(group_names.contains(&"fixedWalls".to_string()));
        assert!(group_names.contains(&"frontAndBack".to_string()));

        assert!(doc.provenance.tool_of_origin.is_some());
        assert_eq!(report.source_format, "openfoam");
        assert_eq!(report.target_format, "exl");
    }

    #[test]
    fn test_triangulation() {
        let pentagon: Vec<u32> = vec![0, 1, 2, 3, 4];
        let triangles = fan_triangulate(&pentagon);
        assert_eq!(triangles.len(), 3);
        assert_eq!(triangles[0], [0, 1, 2]);
        assert_eq!(triangles[1], [0, 2, 3]);
        assert_eq!(triangles[2], [0, 3, 4]);

        let quad: Vec<u32> = vec![0, 1, 2, 3];
        let tris = fan_triangulate(&quad);
        assert_eq!(tris.len(), 2);

        let tri: Vec<u32> = vec![0, 1, 2];
        let tris = fan_triangulate(&tri);
        assert_eq!(tris.len(), 1);

        let degenerate: Vec<u32> = vec![0, 1];
        let tris = fan_triangulate(&degenerate);
        assert_eq!(tris.len(), 0);
    }

    #[test]
    fn test_missing_boundary_file() {
        let (_tmp, poly_mesh) = make_case_dir();

        fs::write(poly_mesh.join("points"), "0\n(\n)\n").unwrap();
        fs::write(poly_mesh.join("faces"), "0\n(\n)\n").unwrap();
        fs::write(poly_mesh.join("owner"), "0\n(\n)\n").unwrap();

        let case_dir = _tmp.as_path();
        let result = import_openfoam(case_dir);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(&err, OpenfoamError::Io(_)) || matches!(&err, OpenfoamError::Parse(_)),
            "expected error, got {:?}",
            err
        );
    }

    #[test]
    fn test_empty_case_dir() {
        let tmp = std::env::temp_dir().join(format!("exl_of_empty_{}", std::process::id()));
        let _ = fs::create_dir(&tmp);
        let result = import_openfoam(&tmp);
        let _ = fs::remove_dir_all(&tmp);
        assert!(result.is_err());
    }

    #[test]
    fn test_fan_triangulate_five_gon() {
        let face: Vec<u32> = vec![0, 1, 2, 3, 4];
        let tris = fan_triangulate(&face);
        assert_eq!(tris.len(), 3);
    }

    #[test]
    fn test_extract_nu() {
        let content = "transportModel  Newtonian;\nnu nu [0 2 -1 0 0 0 0] 0.01;\n";
        assert_eq!(extract_nu(content), Some("0.01".to_string()));
    }

    #[test]
    fn test_extract_nu_simple() {
        let content = "nu 1.5e-5;\n";
        assert_eq!(extract_nu(content), Some("1.5e-5".to_string()));
    }

    #[test]
    fn test_extract_keyword() {
        let content = "application     icoFoam;\ndeltaT          0.005;\nendTime         20;\n";
        assert_eq!(
            extract_keyword(content, "application"),
            Some("icoFoam".to_string())
        );
        assert_eq!(
            extract_keyword(content, "deltaT"),
            Some("0.005".to_string())
        );
        assert_eq!(extract_keyword(content, "endTime"), Some("20".to_string()));
    }

    #[test]
    fn test_parse_points() {
        let content = "2\n(\n(0 0 0)\n(1 2 3)\n)\n";
        let pts = parse_points_content(content).unwrap();
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0], [0.0, 0.0, 0.0]);
        assert_eq!(pts[1], [1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_parse_faces() {
        let content = "2\n(\n4(0 1 2 3)\n3(4 5 6)\n)\n";
        let faces = parse_faces_content(content).unwrap();
        assert_eq!(faces.len(), 2);
        assert_eq!(faces[0], vec![0, 1, 2, 3]);
        assert_eq!(faces[1], vec![4, 5, 6]);
    }

    #[test]
    fn test_parse_boundary() {
        let content = r#"2
(
movingWall
{
    type        patch;
    nFaces      1;
    startFace   0;
}
fixedWalls
{
    type        wall;
    nFaces      5;
    startFace   1;
}
)
"#;
        let patches = parse_boundary_content(content).unwrap();
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].name, "movingWall");
        assert_eq!(patches[0].n_faces, 1);
        assert_eq!(patches[0].start_face, 0);
        assert_eq!(patches[1].name, "fixedWalls");
        assert_eq!(patches[1].n_faces, 5);
        assert_eq!(patches[1].start_face, 1);
    }

    #[test]
    fn test_parse_boundary_without_count() {
        let content = r#"(
movingWall
{
    type        patch;
    nFaces      1;
    startFace   0;
}
)
"#;
        let patches = parse_boundary_content(content).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "movingWall");
    }

    #[test]
    fn test_export_roundtrip_box_mesh() {
        let mesh = exl_core::geom::Mesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, 1.0, 1.0],
                [0.0, 1.0, 1.0],
            ],
            faces: vec![
                [0, 3, 7],
                [0, 7, 4],
                [1, 5, 6],
                [1, 6, 2],
                [0, 1, 5],
                [0, 5, 4],
                [3, 2, 6],
                [3, 6, 7],
                [0, 4, 5],
                [0, 5, 1],
                [4, 7, 6],
                [4, 6, 5],
            ],
            face_groups: Some(vec![0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 2, 2]),
            group_names: vec!["inlet".into(), "outlet".into(), "walls".into()],
            ..Default::default()
        };

        let part = Part::new("box", GeometryPayload::Mesh(mesh));
        let doc = Document::new(vec![part]);

        let case_dir = {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            std::env::temp_dir().join(format!("exl_of_export_{}_{}", std::process::id(), n))
        };
        let _ = fs::remove_dir_all(&case_dir);
        fs::create_dir_all(&case_dir).unwrap();

        let report = export_openfoam(&doc, &case_dir).unwrap();
        assert_eq!(report.source_format, "exl");
        assert_eq!(report.target_format, "openfoam");

        assert!(case_dir
            .join("constant")
            .join("polyMesh")
            .join("points")
            .exists());
        assert!(case_dir
            .join("constant")
            .join("polyMesh")
            .join("faces")
            .exists());
        assert!(case_dir
            .join("constant")
            .join("polyMesh")
            .join("owner")
            .exists());
        assert!(case_dir
            .join("constant")
            .join("polyMesh")
            .join("neighbour")
            .exists());
        assert!(case_dir
            .join("constant")
            .join("polyMesh")
            .join("boundary")
            .exists());
        assert!(case_dir
            .join("constant")
            .join("transportProperties")
            .exists());
        assert!(case_dir.join("system").join("controlDict").exists());

        let (doc2, _report2) = import_openfoam(&case_dir).unwrap();
        let mesh2 = match &doc2.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };

        assert_eq!(mesh2.vertices.len(), 8);
        assert_eq!(mesh2.faces.len(), 12);
        assert!(mesh2.face_groups.is_some());
        let names = &mesh2.group_names;
        assert!(names.contains(&"inlet".to_string()));
        assert!(names.contains(&"outlet".to_string()));
        assert!(names.contains(&"walls".to_string()));

        let _ = fs::remove_dir_all(&case_dir);
    }

    #[test]
    fn generate_corpus_cavity() {
        let nx = 5usize;
        let ny = 5usize;
        let nz = 5usize;
        let ptx = nx + 1;
        let pty = ny + 1;
        let ptz = nz + 1;

        let mut points: Vec<[f32; 3]> = Vec::with_capacity(ptx * pty * ptz);
        for k in 0..ptz {
            for j in 0..pty {
                for i in 0..ptx {
                    points.push([i as f32, j as f32, k as f32]);
                }
            }
        }

        let pt = |i: usize, j: usize, k: usize| -> u32 { (k * pty * ptx + j * ptx + i) as u32 };
        let cell = |i: usize, j: usize, k: usize| -> u32 { (k * ny * nx + j * nx + i) as u32 };

        let mut faces: Vec<Vec<u32>> = Vec::new();
        let mut owner: Vec<u32> = Vec::new();

        let n_internal_x = (nx - 1) * ny * nz;
        let n_internal_y = nx * (ny - 1) * nz;
        let n_internal_z = nx * ny * (nz - 1);
        let _n_internal = n_internal_x + n_internal_y + n_internal_z;
        let n_boundary_x = 2 * ny * nz;
        let n_boundary_y = 2 * nx * nz;
        let n_boundary_z = 2 * nx * ny;

        for k in 0..nz {
            for j in 0..ny {
                for i in 0..(nx - 1) {
                    let o = cell(i, j, k);
                    faces.push(vec![
                        pt(i + 1, j, k),
                        pt(i + 1, j + 1, k),
                        pt(i + 1, j + 1, k + 1),
                        pt(i + 1, j, k + 1),
                    ]);
                    owner.push(o);
                }
            }
        }

        for k in 0..nz {
            for i in 0..nx {
                for j in 0..(ny - 1) {
                    let o = cell(i, j, k);
                    faces.push(vec![
                        pt(i, j + 1, k),
                        pt(i, j + 1, k + 1),
                        pt(i + 1, j + 1, k + 1),
                        pt(i + 1, j + 1, k),
                    ]);
                    owner.push(o);
                }
            }
        }

        for j in 0..ny {
            for i in 0..nx {
                for k in 0..(nz - 1) {
                    let o = cell(i, j, k);
                    faces.push(vec![
                        pt(i, j, k + 1),
                        pt(i + 1, j, k + 1),
                        pt(i + 1, j + 1, k + 1),
                        pt(i, j + 1, k + 1),
                    ]);
                    owner.push(o);
                }
            }
        }

        let mut x_min_faces = Vec::new();
        for k in 0..nz {
            for j in 0..ny {
                let o = cell(0, j, k);
                x_min_faces.push(vec![
                    pt(0, j, k),
                    pt(0, j + 1, k),
                    pt(0, j + 1, k + 1),
                    pt(0, j, k + 1),
                ]);
                owner.push(o);
            }
        }

        let mut x_max_faces = Vec::new();
        for k in 0..nz {
            for j in 0..ny {
                let o = cell(nx - 1, j, k);
                x_max_faces.push(vec![
                    pt(nx, j, k),
                    pt(nx, j, k + 1),
                    pt(nx, j + 1, k + 1),
                    pt(nx, j + 1, k),
                ]);
                owner.push(o);
            }
        }

        let mut y_min_faces = Vec::new();
        for k in 0..nz {
            for i in 0..nx {
                let o = cell(i, 0, k);
                y_min_faces.push(vec![
                    pt(i, 0, k),
                    pt(i, 0, k + 1),
                    pt(i + 1, 0, k + 1),
                    pt(i + 1, 0, k),
                ]);
                owner.push(o);
            }
        }

        let mut y_max_faces = Vec::new();
        for k in 0..nz {
            for i in 0..nx {
                let o = cell(i, ny - 1, k);
                y_max_faces.push(vec![
                    pt(i, ny, k),
                    pt(i + 1, ny, k),
                    pt(i + 1, ny, k + 1),
                    pt(i, ny, k + 1),
                ]);
                owner.push(o);
            }
        }

        let mut z_min_faces = Vec::new();
        for j in 0..ny {
            for i in 0..nx {
                let o = cell(i, j, 0);
                z_min_faces.push(vec![
                    pt(i, j, 0),
                    pt(i + 1, j, 0),
                    pt(i + 1, j + 1, 0),
                    pt(i, j + 1, 0),
                ]);
                owner.push(o);
            }
        }

        let mut z_max_faces = Vec::new();
        for j in 0..ny {
            for i in 0..nx {
                let o = cell(i, j, nz - 1);
                z_max_faces.push(vec![
                    pt(i, j, nz),
                    pt(i, j + 1, nz),
                    pt(i + 1, j + 1, nz),
                    pt(i + 1, j, nz),
                ]);
                owner.push(o);
            }
        }

        faces.extend(x_min_faces.clone());
        faces.extend(x_max_faces.clone());
        faces.extend(y_min_faces.clone());
        faces.extend(y_max_faces.clone());
        faces.extend(z_min_faces.clone());
        faces.extend(z_max_faces.clone());

        assert_eq!(
            faces.len(),
            n_internal_x + n_internal_y + n_internal_z + n_boundary_x + n_boundary_y + n_boundary_z
        );
        assert_eq!(points.len(), ptx * pty * ptz);
        assert!(owner.len() == faces.len());

        let mut neighbour = Vec::new();
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..(nx - 1) {
                    neighbour.push(cell(i + 1, j, k));
                }
            }
        }
        for k in 0..nz {
            for i in 0..nx {
                for j in 0..(ny - 1) {
                    neighbour.push(cell(i, j + 1, k));
                }
            }
        }
        for j in 0..ny {
            for i in 0..nx {
                for k in 0..(nz - 1) {
                    neighbour.push(cell(i, j, k + 1));
                }
            }
        }

        let internal_count = n_internal_x + n_internal_y + n_internal_z;
        assert_eq!(neighbour.len(), internal_count);

        let fixed_walls_n = x_min_faces.len()
            + x_max_faces.len()
            + y_min_faces.len()
            + y_max_faces.len()
            + z_min_faces.len();
        let start_fixed_walls = internal_count;
        let start_moving_wall = start_fixed_walls + fixed_walls_n;
        let moving_wall_n = z_max_faces.len();

        let corpus_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/openfoam-cavity/constant/polyMesh");
        fs::create_dir_all(&corpus_dir).unwrap();

        {
            let mut s = foam_header("vectorField", "points");
            s.push_str(&format!("{}\n(\n", points.len()));
            for p in &points {
                s.push_str(&format!("({} {} {})\n", p[0], p[1], p[2]));
            }
            s.push_str(")\n");
            fs::write(corpus_dir.join("points"), s).unwrap();
        }

        {
            let mut s = foam_header("faceList", "faces");
            s.push_str(&format!("{}\n(\n", faces.len()));
            for f in &faces {
                s.push_str(&format!("{}(", f.len()));
                for (vi, v) in f.iter().enumerate() {
                    if vi > 0 {
                        s.push(' ');
                    }
                    s.push_str(&v.to_string());
                }
                s.push_str(")\n");
            }
            s.push_str(")\n");
            fs::write(corpus_dir.join("faces"), s).unwrap();
        }

        {
            let mut s = foam_header("labelList", "owner");
            s.push_str(&format!("{}\n(\n", owner.len()));
            for o in &owner {
                s.push_str(&format!("{}\n", o));
            }
            s.push_str(")\n");
            fs::write(corpus_dir.join("owner"), s).unwrap();
        }

        {
            let mut s = foam_header("labelList", "neighbour");
            s.push_str(&format!("{}\n(\n", neighbour.len()));
            for n in &neighbour {
                s.push_str(&format!("{}\n", n));
            }
            s.push_str(")\n");
            fs::write(corpus_dir.join("neighbour"), s).unwrap();
        }

        {
            let mut s = foam_header("polyBoundaryMesh", "boundary");
            s.push_str("2\n(\n");
            s.push_str(&format!(
                "movingWall\n{{\n    type        patch;\n    nFaces      {};\n    startFace   {};\n}}\n",
                moving_wall_n, start_moving_wall
            ));
            s.push_str(&format!(
                "fixedWalls\n{{\n    type        wall;\n    nFaces      {};\n    startFace   {};\n}}\n",
                fixed_walls_n, start_fixed_walls
            ));
            s.push_str(")\n");
            fs::write(corpus_dir.join("boundary"), s).unwrap();
        }

        let case_dir = corpus_dir.parent().unwrap().parent().unwrap();

        if let Ok(existing) =
            fs::read_to_string(case_dir.join("constant").join("transportProperties"))
        {
            if existing.is_empty() {
                let tp = "transportModel  Newtonian;\nnu nu [0 2 -1 0 0 0 0] 0.01;\n";
                fs::write(case_dir.join("constant").join("transportProperties"), tp).unwrap();
            }
        } else {
            fs::create_dir_all(case_dir.join("constant")).unwrap();
            let tp = "transportModel  Newtonian;\nnu nu [0 2 -1 0 0 0 0] 0.01;\n";
            fs::write(case_dir.join("constant").join("transportProperties"), tp).unwrap();
        }

        let (doc, _report) = import_openfoam(case_dir).unwrap();
        let mesh = match &doc.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };
        assert_eq!(mesh.group_names.len(), 2);
        assert!(mesh.group_names.contains(&"movingWall".to_string()));
        assert!(mesh.group_names.contains(&"fixedWalls".to_string()));
        assert!(!mesh.faces.is_empty());
        assert!(!mesh.vertices.is_empty());
    }
}
