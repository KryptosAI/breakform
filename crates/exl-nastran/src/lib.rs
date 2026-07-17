use exl_core::{
    units::Quantity, BcType, BoundaryCondition, Document, EntityStatus, FidelityReport,
    GeometryPayload, Material, Part, ToolOfOrigin, Unit,
};
use exl_geom::Mesh;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum NastranError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
}

struct RawCard {
    name: String,
    lines: Vec<String>,
}

struct ElementFaces {
    pid: i64,
    faces: Vec<[i64; 3]>,
    has_parabolic_midsides: bool,
}

struct MaterialData {
    mid: i64,
    elastic_modulus: Option<f64>,
    poisson_ratio: Option<f64>,
    density: Option<f64>,
    is_composite_approximation: bool,
}

struct BcRecord {
    bc: BoundaryCondition,
    grid_ids: Vec<i64>,
}

fn iso_timestamp() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs();
    let z = total_secs / 86400 + 719468;
    let time_secs = total_secs % 86400;
    let h = time_secs / 3600;
    let min = (time_secs % 3600) / 60;
    let s = time_secs % 60;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, h, min, s)
}

fn parse_float(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.replace(['D', 'd'], "E");
    s.parse().ok()
}

fn parse_int(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    s.parse().ok()
}

fn peek_first_char(line: &str) -> Option<char> {
    line.chars().next()
}

fn parse_raw_cards(content: &str) -> Vec<RawCard> {
    let mut cards: Vec<RawCard> = Vec::new();
    let mut current: Option<RawCard> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('$') {
            continue;
        }

        let first = peek_first_char(line);
        if current.is_none() {
            let name = extract_card_name(line);
            if name.is_empty() {
                continue;
            }
            current = Some(RawCard {
                name,
                lines: vec![line.to_string()],
            });
        } else if first == Some(' ') || first == Some('+') {
            current.as_mut().unwrap().lines.push(line.to_string());
        } else {
            let prev = current.take().unwrap();
            cards.push(prev);
            let name = extract_card_name(line);
            if name.is_empty() {
                continue;
            }
            current = Some(RawCard {
                name,
                lines: vec![line.to_string()],
            });
        }
    }

    if let Some(c) = current {
        cards.push(c);
    }
    cards
}

fn extract_card_name(line: &str) -> String {
    if let Some(comma_pos) = line.find(',') {
        let name = line[..comma_pos].trim().to_ascii_uppercase();
        if !name.is_empty() {
            return name;
        }
        return String::new();
    }
    let end = 8.min(line.len());
    line[..end].trim().to_ascii_uppercase()
}

fn extract_fields(card: &RawCard) -> Vec<String> {
    let has_comma = card.lines.iter().any(|l| l.contains(','));
    if has_comma {
        extract_small_fields(card)
    } else {
        extract_large_fields(card)
    }
}

fn extract_small_fields(card: &RawCard) -> Vec<String> {
    let mut fields = Vec::new();
    for line in &card.lines {
        let start = if line.starts_with(' ') || line.starts_with('+') {
            0
        } else {
            line.find(',').unwrap_or(8.min(line.len()))
        };
        let rest = &line[start..];
        let rest = rest.trim_start_matches(',');
        if rest.is_empty() {
            continue;
        }
        for f in rest.split(',') {
            fields.push(f.trim().to_string());
        }
    }
    fields
}

fn extract_large_fields(card: &RawCard) -> Vec<String> {
    let mut fields = Vec::new();
    for line in &card.lines {
        let max_col = line.len().min(72);
        let mut pos = 8;
        while pos + 8 <= max_col {
            fields.push(line[pos..pos + 8].to_string());
            pos += 8;
        }
        if pos < max_col {
            fields.push(line[pos..max_col].to_string());
        }
    }
    fields
}

fn get_field(fields: &[String], idx: usize) -> Option<&str> {
    fields.get(idx).map(|s| s.as_str())
}

fn get_float(fields: &[String], idx: usize) -> Option<f64> {
    get_field(fields, idx).and_then(parse_float)
}

fn get_int(fields: &[String], idx: usize) -> Option<i64> {
    get_field(fields, idx).and_then(parse_int)
}

fn parse_grid(fields: &[String]) -> Option<(i64, [f64; 3], bool)> {
    let id = get_int(fields, 0)?;
    let x = get_float(fields, 2)?;
    let y = get_float(fields, 3)?;
    let z = get_float(fields, 4)?;
    let cp = get_int(fields, 1).unwrap_or(0);
    let has_nonzero_cp = cp != 0;
    Some((id, [x, y, z], has_nonzero_cp))
}

fn parse_ctria3(fields: &[String]) -> Option<ElementFaces> {
    let _eid = get_int(fields, 0)?;
    let pid = get_int(fields, 1).unwrap_or(0);
    let g1 = get_int(fields, 2)?;
    let g2 = get_int(fields, 3)?;
    let g3 = get_int(fields, 4)?;
    let has_dropped = get_float(fields, 5).map(|v| v != 0.0).unwrap_or(false)
        || get_float(fields, 6).map(|v| v != 0.0).unwrap_or(false);
    Some(ElementFaces {
        pid,
        faces: vec![[g1, g2, g3]],
        has_parabolic_midsides: has_dropped,
    })
}

fn parse_cquad4(fields: &[String]) -> Option<ElementFaces> {
    let _eid = get_int(fields, 0)?;
    let pid = get_int(fields, 1).unwrap_or(0);
    let g1 = get_int(fields, 2)?;
    let g2 = get_int(fields, 3)?;
    let g3 = get_int(fields, 4)?;
    let g4 = get_int(fields, 5)?;
    let has_dropped = get_float(fields, 6).map(|v| v != 0.0).unwrap_or(false)
        || get_float(fields, 7).map(|v| v != 0.0).unwrap_or(false);
    Some(ElementFaces {
        pid,
        faces: vec![[g1, g2, g3], [g1, g3, g4]],
        has_parabolic_midsides: has_dropped,
    })
}

fn parse_ctetra(fields: &[String]) -> Option<ElementFaces> {
    let _eid = get_int(fields, 0)?;
    let pid = get_int(fields, 1).unwrap_or(0);
    let g1 = get_int(fields, 2)?;
    let g2 = get_int(fields, 3)?;
    let g3 = get_int(fields, 4)?;
    let g4 = get_int(fields, 5)?;
    let mut has_midsides = false;
    for i in 6..fields.len() {
        if let Some(v) = get_int(fields, i) {
            if v != 0 {
                has_midsides = true;
            }
        }
    }
    Some(ElementFaces {
        pid,
        faces: vec![[g1, g2, g3], [g1, g3, g4], [g1, g4, g2], [g2, g4, g3]],
        has_parabolic_midsides: has_midsides,
    })
}

fn parse_chexa(fields: &[String]) -> Option<ElementFaces> {
    let _eid = get_int(fields, 0)?;
    let pid = get_int(fields, 1).unwrap_or(0);
    let g1 = get_int(fields, 2)?;
    let g2 = get_int(fields, 3)?;
    let g3 = get_int(fields, 4)?;
    let g4 = get_int(fields, 5)?;
    let g5 = get_int(fields, 6)?;
    let g6 = get_int(fields, 7)?;
    let g7 = get_int(fields, 8)?;
    let g8 = get_int(fields, 9)?;
    let mut has_midsides = false;
    for i in 10..fields.len() {
        if let Some(v) = get_int(fields, i) {
            if v != 0 {
                has_midsides = true;
            }
        }
    }
    Some(ElementFaces {
        pid,
        faces: vec![
            [g1, g2, g3],
            [g1, g3, g4],
            [g5, g7, g6],
            [g5, g8, g7],
            [g1, g5, g6],
            [g1, g6, g2],
            [g2, g6, g7],
            [g2, g7, g3],
            [g3, g7, g8],
            [g3, g8, g4],
            [g4, g8, g5],
            [g4, g5, g1],
        ],
        has_parabolic_midsides: has_midsides,
    })
}

fn parse_mat1(fields: &[String]) -> Option<MaterialData> {
    let mid = get_int(fields, 0)?;
    let e = get_float(fields, 1);
    let nu = get_float(fields, 3);
    let rho = get_float(fields, 4);
    Some(MaterialData {
        mid,
        elastic_modulus: e,
        poisson_ratio: nu,
        density: rho,
        is_composite_approximation: false,
    })
}

fn parse_mat8(fields: &[String]) -> Option<MaterialData> {
    let mid = get_int(fields, 0)?;
    let e1 = get_float(fields, 1);
    let nu12 = get_float(fields, 3);
    let rho = get_float(fields, 7);
    Some(MaterialData {
        mid,
        elastic_modulus: e1,
        poisson_ratio: nu12,
        density: rho,
        is_composite_approximation: true,
    })
}

fn parse_pload(fields: &[String]) -> Option<(i64, f64, Vec<i64>)> {
    let sid = get_int(fields, 0)?;
    let p = get_float(fields, 1)?;
    let mut grids = Vec::new();
    for i in 2..6 {
        if let Some(g) = get_int(fields, i) {
            grids.push(g);
        }
    }
    Some((sid, p, grids))
}

fn parse_spc(fields: &[String]) -> Option<(i64, i64, String, f64)> {
    let sid = get_int(fields, 0)?;
    let g = get_int(fields, 1)?;
    let c = get_field(fields, 2).unwrap_or("").to_string();
    let d = get_float(fields, 3).unwrap_or(0.0);
    Some((sid, g, c, d))
}

fn has_translation_dofs(component: &str) -> bool {
    component.contains('1') || component.contains('2') || component.contains('3')
}

fn assemble_parts(
    grids: &HashMap<i64, [f64; 3]>,
    elements: &[ElementFaces],
    materials: &HashMap<i64, MaterialData>,
    bcs: &[BcRecord],
    fidelity: &mut FidelityReport,
) -> Vec<Part> {
    let mut pid_groups: HashMap<i64, Vec<&ElementFaces>> = HashMap::new();
    for elem in elements {
        pid_groups.entry(elem.pid).or_default().push(elem);
    }

    if pid_groups.is_empty() {
        return Vec::new();
    }

    let all_pid_zero = pid_groups.keys().all(|&k| k == 0);
    let mut parts = Vec::new();

    for (&pid, elems) in &pid_groups {
        let mut grid_set: std::collections::HashSet<i64> = std::collections::HashSet::new();
        for elem in elems {
            for face in &elem.faces {
                for &g in face {
                    grid_set.insert(g);
                }
            }
        }

        let mut local_idx: HashMap<i64, u32> = HashMap::new();
        let mut verts: Vec<[f32; 3]> = Vec::new();
        for &g in &grid_set {
            if let Some(coord) = grids.get(&g) {
                local_idx.insert(g, verts.len() as u32);
                verts.push([coord[0] as f32, coord[1] as f32, coord[2] as f32]);
            }
        }

        let mut faces: Vec<[u32; 3]> = Vec::new();
        for elem in elems {
            for face in &elem.faces {
                if let (Some(&a), Some(&b), Some(&c)) = (
                    local_idx.get(&face[0]),
                    local_idx.get(&face[1]),
                    local_idx.get(&face[2]),
                ) {
                    faces.push([a, b, c]);
                }
            }
        }

        let mesh = Mesh {
            vertices: verts,
            faces,
            ..Default::default()
        };

        let part_name = if all_pid_zero {
            "nastran_model".to_string()
        } else {
            format!("pid_{}", pid)
        };

        let mut part = Part::new(part_name, GeometryPayload::Mesh(mesh));

        if let Some(mat_data) = materials.get(&pid) {
            let mut material = Material {
                name: format!("mat_{}", mat_data.mid),
                ..Default::default()
            };
            if let Some(e) = mat_data.elastic_modulus {
                material.elastic_modulus = Some(Quantity::new(e, Unit::Pascal));
            }
            if let Some(nu) = mat_data.poisson_ratio {
                material.poisson_ratio = Some(nu);
            }
            if let Some(rho) = mat_data.density {
                material.density = Some(Quantity::new(rho, Unit::KilogramPerCubicMeter));
            }
            if mat_data.is_composite_approximation {
                fidelity.record(
                    "MAT8_approximation",
                    1,
                    EntityStatus::Approximate,
                    Some(format!(
                        "MAT8 composite MID {} stored as isotropic approximation",
                        mat_data.mid
                    )),
                );
            }
            part.semantics.materials.push(material);
        } else if pid != 0 {
            fidelity.record(
                "property_card_not_parsed".to_string(),
                1,
                EntityStatus::Dropped,
                Some(format!(
                    "PID {} has no matching MAT1/MAT8; PSHELL/PSOLID not parsed",
                    pid
                )),
            );
        }

        for bc_rec in bcs {
            let any_grid_in_part = bc_rec.grid_ids.iter().any(|g| local_idx.contains_key(g));
            if any_grid_in_part {
                part.semantics.boundary_conditions.push(bc_rec.bc.clone());
            }
        }

        parts.push(part);
    }

    parts
}

pub fn import_nastran(path: &Path) -> Result<(Document, FidelityReport), NastranError> {
    let content = std::fs::read_to_string(path)?;
    let mut fidelity = FidelityReport::new("nastran", "exl");

    let raw_cards = parse_raw_cards(&content);

    let mut grids: HashMap<i64, [f64; 3]> = HashMap::new();
    let mut elements: Vec<ElementFaces> = Vec::new();
    let mut materials: HashMap<i64, MaterialData> = HashMap::new();
    let mut bcs: Vec<BcRecord> = Vec::new();

    let mut grid_count: usize = 0;
    let mut ctria3_count: usize = 0;
    let mut cquad4_count: usize = 0;
    let mut ctetra_count: usize = 0;
    let mut ctetra_midside_count: usize = 0;
    let mut chexa_count: usize = 0;
    let mut chexa_midside_count: usize = 0;
    let mut mat1_count: usize = 0;
    let mut mat8_count: usize = 0;
    let mut pload_count: usize = 0;
    let mut force_count: usize = 0;
    let mut moment_count: usize = 0;
    let mut spc_count: usize = 0;
    let mut coord_sys_grids: usize = 0;
    let mut dropped_unknown: HashMap<String, usize> = HashMap::new();

    for card in &raw_cards {
        let fields = extract_fields(card);
        match card.name.as_str() {
            "GRID" => {
                grid_count += 1;
                if let Some((id, coord, has_cp)) = parse_grid(&fields) {
                    if has_cp {
                        coord_sys_grids += 1;
                    }
                    grids.insert(id, coord);
                }
            }
            "CTRIA3" => {
                ctria3_count += 1;
                if let Some(elem) = parse_ctria3(&fields) {
                    elements.push(elem);
                }
            }
            "CQUAD4" => {
                cquad4_count += 1;
                if let Some(elem) = parse_cquad4(&fields) {
                    elements.push(elem);
                }
            }
            "CTETRA" => {
                ctetra_count += 1;
                if let Some(elem) = parse_ctetra(&fields) {
                    if elem.has_parabolic_midsides {
                        ctetra_midside_count += 1;
                    }
                    elements.push(elem);
                }
            }
            "CHEXA" => {
                chexa_count += 1;
                if let Some(elem) = parse_chexa(&fields) {
                    if elem.has_parabolic_midsides {
                        chexa_midside_count += 1;
                    }
                    elements.push(elem);
                }
            }
            "CPENTA" | "CPYRAM" => {
                *dropped_unknown.entry(card.name.clone()).or_default() += 1;
            }
            "MAT1" => {
                mat1_count += 1;
                if let Some(mat) = parse_mat1(&fields) {
                    materials.insert(mat.mid, mat);
                }
            }
            "MAT8" => {
                mat8_count += 1;
                if let Some(mat) = parse_mat8(&fields) {
                    materials.insert(mat.mid, mat);
                }
            }
            "PLOAD" => {
                pload_count += 1;
                if let Some((sid, p, gids)) = parse_pload(&fields) {
                    bcs.push(BcRecord {
                        bc: BoundaryCondition {
                            face_group: format!("pl_{}", sid),
                            bc_type: BcType::Pressure,
                            value: Quantity::new(p, Unit::Pascal),
                            direction: None,
                        },
                        grid_ids: gids,
                    });
                }
            }
            "FORCE" => {
                force_count += 1;
            }
            "MOMENT" => {
                moment_count += 1;
            }
            "SPC" => {
                spc_count += 1;
                if let Some((_sid, g, c, d)) = parse_spc(&fields) {
                    if has_translation_dofs(&c) {
                        bcs.push(BcRecord {
                            bc: BoundaryCondition {
                                face_group: format!("{}", g),
                                bc_type: BcType::FixedDisplacement,
                                value: Quantity::new(d, Unit::Meter),
                                direction: None,
                            },
                            grid_ids: vec![g],
                        });
                    }
                }
            }
            _ => {
                *dropped_unknown.entry(card.name.clone()).or_default() += 1;
            }
        }
    }

    if grid_count > 0 {
        fidelity.record("GRID", grid_count, EntityStatus::Lossless, None);
    }
    if ctria3_count > 0 {
        fidelity.record("CTRIA3", ctria3_count, EntityStatus::Lossless, None);
    }
    if cquad4_count > 0 {
        fidelity.record("CQUAD4", cquad4_count, EntityStatus::Lossless, None);
    }
    if ctetra_count > 0 {
        let status = if ctetra_midside_count > 0 {
            EntityStatus::Approximate
        } else {
            EntityStatus::Lossless
        };
        let note = if ctetra_midside_count > 0 {
            Some(format!(
                "{} parabolic tets had mid-side nodes dropped",
                ctetra_midside_count
            ))
        } else {
            None
        };
        fidelity.record("CTETRA", ctetra_count, status, note);
    }
    if chexa_count > 0 {
        let status = if chexa_midside_count > 0 {
            EntityStatus::Approximate
        } else {
            EntityStatus::Lossless
        };
        let note = if chexa_midside_count > 0 {
            Some(format!(
                "{} parabolic hexes had mid-side nodes dropped",
                chexa_midside_count
            ))
        } else {
            None
        };
        fidelity.record("CHEXA", chexa_count, status, note);
    }
    if mat1_count > 0 {
        fidelity.record("MAT1", mat1_count, EntityStatus::Lossless, None);
    }
    if mat8_count > 0 {
        fidelity.record(
            "MAT8",
            mat8_count,
            EntityStatus::Approximate,
            Some("composite material approximated as isotropic using E1/NU12/RHO".into()),
        );
    }
    if pload_count > 0 {
        fidelity.record("PLOAD", pload_count, EntityStatus::Lossless, None);
    }
    if spc_count > 0 {
        fidelity.record("SPC", spc_count, EntityStatus::Lossless, None);
    }
    if force_count > 0 {
        fidelity.record(
            "FORCE",
            force_count,
            EntityStatus::Dropped,
            Some("point load type not represented".into()),
        );
    }
    if moment_count > 0 {
        fidelity.record(
            "MOMENT",
            moment_count,
            EntityStatus::Dropped,
            Some("point load type not represented".into()),
        );
    }
    if coord_sys_grids > 0 {
        fidelity.record(
            "coordinate_system",
            coord_sys_grids,
            EntityStatus::Dropped,
            Some("non-zero CP on GRID cards ignored".into()),
        );
    }
    for (name, count) in &dropped_unknown {
        fidelity.record(
            name.clone(),
            *count,
            EntityStatus::Dropped,
            Some("unsupported card type".into()),
        );
    }

    fidelity.record(
        "units_assumed_SI",
        1,
        EntityStatus::Lossless,
        Some("input assumed in SI unless unit spec present".into()),
    );

    let parts = assemble_parts(&grids, &elements, &materials, &bcs, &mut fidelity);

    let mut doc = Document::new(parts);
    doc.provenance.tool_of_origin = Some(ToolOfOrigin {
        name: "exl-nastran".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        timestamp_iso: iso_timestamp(),
    });
    doc.provenance.conversion_fidelity = Some(fidelity.overall);
    doc.refresh_content_hash();

    Ok((doc, fidelity))
}

pub use import_nastran as import_bdf;

pub fn export_nastran(doc: &Document, path: &Path) -> Result<FidelityReport, NastranError> {
    let mut file = std::fs::File::create(path)?;
    let mut report = FidelityReport::new("exl", "nastran");

    let timestamp = iso_timestamp();
    writeln!(file, "$ Breakform Nastran export")?;
    writeln!(file, "$ Generated: {}", timestamp)?;
    writeln!(file)?;

    let mut total_verts: usize = 0;
    let mut total_faces: usize = 0;
    let mut material_written = false;
    let mut has_fixed_bc = false;
    let mut has_pressure_bc = false;

    for part in &doc.parts {
        let mesh = match &part.geometry {
            GeometryPayload::Mesh(m) => m,
            _ => continue,
        };

        let n_verts = mesh.vertices.len();
        let n_faces = mesh.faces.len();
        if n_verts == 0 && n_faces == 0 {
            continue;
        }

        total_verts += n_verts;
        total_faces += n_faces;

        writeln!(file, "$ Part: {}", part.name)?;

        for (i, v) in mesh.vertices.iter().enumerate() {
            let gid = i + 1;
            writeln!(file, "GRID,{},,{:.6},{:.6},{:.6}", gid, v[0], v[1], v[2])?;
        }

        for (i, face) in mesh.faces.iter().enumerate() {
            let eid = i + 1;
            let pid = 1i32;
            let g1 = face[0] + 1;
            let g2 = face[1] + 1;
            let g3 = face[2] + 1;
            writeln!(file, "CTRIA3,{},{},{},{},{}", eid, pid, g1, g2, g3)?;
        }

        if !material_written && !part.semantics.materials.is_empty() {
            let mat = &part.semantics.materials[0];
            let mid = 1i32;
            let e_val = mat
                .elastic_modulus
                .as_ref()
                .map(|q| q.to_si())
                .unwrap_or(0.0);
            let nu_val = mat.poisson_ratio.unwrap_or(0.0);
            let rho_val = mat.density.as_ref().map(|q| q.to_si()).unwrap_or(0.0);

            let e_str = if e_val == 0.0 {
                String::new()
            } else {
                format!("{:.6E}", e_val)
            };
            let nu_str = if nu_val == 0.0 {
                String::new()
            } else {
                format!("{}", nu_val)
            };
            let rho_str = if rho_val == 0.0 {
                String::new()
            } else {
                format!("{}", rho_val)
            };

            writeln!(file, "MAT1,{},{},,{},{}", mid, e_str, nu_str, rho_str)?;
            material_written = true;

            report.record(
                "materials",
                part.semantics.materials.len(),
                EntityStatus::Lossless,
                None,
            );
        }

        for bc in &part.semantics.boundary_conditions {
            match bc.bc_type {
                BcType::FixedDisplacement => {
                    has_fixed_bc = true;
                    let sid = 1i32;
                    let val_str = if bc.value.value == 0.0 {
                        String::new()
                    } else {
                        format!("{:.6}", bc.value.to_si())
                    };
                    for i in 0..mesh.vertices.len() {
                        let gid = i as i32 + 1;
                        writeln!(file, "SPC,{},{},123456,{}", sid, gid, val_str)?;
                    }
                }
                BcType::Pressure => {
                    has_pressure_bc = true;
                }
                _ => {}
            }
        }

        writeln!(file)?;
    }

    if total_verts > 0 {
        report.record("vertices", total_verts, EntityStatus::Lossless, None);
    }
    if total_faces > 0 {
        report.record("faces", total_faces, EntityStatus::Lossless, None);
    }
    if has_fixed_bc {
        report.record("fixed_BC", 1, EntityStatus::Lossless, None);
    }
    if has_pressure_bc {
        report.record(
            "pressure_BC",
            1,
            EntityStatus::Dropped,
            Some("PLOAD4 requires face-element mapping not available at v0 export".into()),
        );
    }

    writeln!(file, "ENDDATA")?;

    Ok(report)
}

pub use export_nastran as export_bdf;

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("exl-nastran-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn field8(s: &str) -> String {
        format!("{:>8}", s)
    }

    fn nastran_line(name: &str, fields: &[&str]) -> String {
        let mut line = format!("{:<8}", name);
        for f in fields {
            line.push_str(&field8(f));
        }
        line
    }

    #[test]
    fn flat_plate_import() {
        let mut bdf = String::new();
        bdf.push_str("$ Simple flat plate\n");
        bdf.push_str(&nastran_line("GRID", &["1", "", "0.0", "0.0", "0.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["2", "", "1.0", "0.0", "0.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["3", "", "1.0", "1.0", "0.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["4", "", "0.0", "1.0", "0.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("CQUAD4", &["100", "1", "1", "2", "3", "4"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("MAT1", &["1", "200.e9", "", "0.3", "7800."]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("PSOLID", &["1", "1"]));
        bdf.push('\n');

        let path = write_temp("flat_plate.bdf", &bdf);
        let (doc, report) = import_nastran(&path).unwrap();

        assert_eq!(doc.parts.len(), 1);
        let part = &doc.parts[0];
        assert_eq!(part.name, "pid_1");
        assert_eq!(part.semantics.materials.len(), 1);
        let mat = &part.semantics.materials[0];
        assert!(mat.elastic_modulus.is_some());
        assert_eq!(mat.elastic_modulus.as_ref().unwrap().value, 200.0e9);
        assert_eq!(mat.poisson_ratio, Some(0.3));
        assert!(mat.density.is_some());
        assert_eq!(mat.density.as_ref().unwrap().value, 7800.0);

        if let GeometryPayload::Mesh(m) = &part.geometry {
            assert_eq!(m.vertices.len(), 4);
            assert_eq!(m.faces.len(), 2);
        } else {
            panic!("expected mesh geometry");
        }

        assert_eq!(report.source_format, "nastran");
        assert_eq!(report.target_format, "exl");
        assert!(report
            .entities
            .iter()
            .any(|e| e.entity == "GRID" && e.status == EntityStatus::Lossless));
        assert!(report
            .entities
            .iter()
            .any(|e| e.entity == "CQUAD4" && e.status == EntityStatus::Lossless));
        assert!(report
            .entities
            .iter()
            .any(|e| e.entity == "MAT1" && e.status == EntityStatus::Lossless));
        assert!(doc.provenance.tool_of_origin.is_some());
        assert_eq!(
            doc.provenance.tool_of_origin.as_ref().unwrap().name,
            "exl-nastran"
        );
    }

    #[test]
    fn cantilever_beam_import() {
        let mut bdf = String::new();
        bdf.push_str("$ Cantilever beam\n");
        bdf.push_str(&nastran_line("GRID", &["1", "", "0.0", "0.0", "0.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["2", "", "10.0", "0.0", "0.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["3", "", "10.0", "1.0", "0.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["4", "", "0.0", "1.0", "0.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["5", "", "0.0", "0.0", "1.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["6", "", "10.0", "0.0", "1.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["7", "", "10.0", "1.0", "1.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["8", "", "0.0", "1.0", "1.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["9", "", "5.0", "0.5", "0.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["10", "", "5.0", "0.5", "1.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("CQUAD4", &["201", "1", "1", "2", "3", "4"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("CQUAD4", &["202", "1", "5", "6", "7", "8"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("CQUAD4", &["203", "1", "1", "2", "6", "5"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("CQUAD4", &["204", "1", "2", "3", "7", "6"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("CQUAD4", &["205", "1", "3", "4", "8", "7"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("MAT1", &["1", "200.e9", "", "0.3", "7800."]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("SPC", &["100", "1", "123456"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line(
            "FORCE",
            &["200", "10", "", "0.", "-1000.", "0.", "1."],
        ));
        bdf.push('\n');

        let path = write_temp("beam.bdf", &bdf);
        let (doc, report) = import_nastran(&path).unwrap();

        assert_eq!(doc.parts.len(), 1);
        let part = &doc.parts[0];
        assert_eq!(part.name, "pid_1");

        if let GeometryPayload::Mesh(m) = &part.geometry {
            assert!(m.vertices.len() >= 8);
            assert_eq!(m.faces.len(), 10);
        } else {
            panic!("expected mesh geometry");
        }

        assert!(!part.semantics.boundary_conditions.is_empty());
        let spc_bc = part
            .semantics
            .boundary_conditions
            .iter()
            .find(|bc| matches!(bc.bc_type, BcType::FixedDisplacement));
        assert!(spc_bc.is_some());
        assert_eq!(spc_bc.unwrap().face_group, "1");

        let force_entity = report
            .entities
            .iter()
            .find(|e| e.entity == "FORCE")
            .expect("FORCE should be in fidelity report");
        assert_eq!(force_entity.status, EntityStatus::Dropped);

        assert!(doc.provenance.tool_of_origin.is_some());
    }

    #[test]
    fn ctria3_explicit() {
        let bdf = r#"GRID,1,,0.0,0.0,0.0
GRID,2,,1.0,0.0,0.0
GRID,3,,0.0,1.0,0.0
CTRIA3,100,1,1,2,3
MAT1,1,70.e9,,0.33,2700.
"#;
        let path = write_temp("ctria3.bdf", bdf);
        let (doc, _report) = import_nastran(&path).unwrap();

        assert_eq!(doc.parts.len(), 1);
        let part = &doc.parts[0];
        if let GeometryPayload::Mesh(m) = &part.geometry {
            assert_eq!(m.vertices.len(), 3);
            assert_eq!(m.faces.len(), 1);
        } else {
            panic!("expected mesh");
        }
    }

    #[test]
    fn ctetra_four_faces() {
        let bdf = r#"GRID,1,,0.0,0.0,0.0
GRID,2,,1.0,0.0,0.0
GRID,3,,0.0,1.0,0.0
GRID,4,,0.0,0.0,1.0
CTETRA,10,1,1,2,3,4
"#;
        let path = write_temp("ctetra.bdf", bdf);
        let (doc, _report) = import_nastran(&path).unwrap();

        assert_eq!(doc.parts.len(), 1);
        let part = &doc.parts[0];
        if let GeometryPayload::Mesh(m) = &part.geometry {
            assert_eq!(m.vertices.len(), 4);
            assert_eq!(m.faces.len(), 4);
        } else {
            panic!("expected mesh");
        }
    }

    #[test]
    fn handles_comments_and_continuation() {
        let bdf = r#"$ This is a comment
GRID,1,,0.0,0.0,0.0
$ another comment
GRID,2,,1.0,0.0,0.0
GRID,3,,0.0,1.0,0.0
$ element
CTRIA3,100,1,1,2,3
$ done
"#;
        let path = write_temp("comments.bdf", bdf);
        let (doc, _report) = import_nastran(&path).unwrap();
        let part = &doc.parts[0];
        if let GeometryPayload::Mesh(m) = &part.geometry {
            assert_eq!(m.vertices.len(), 3);
            assert_eq!(m.faces.len(), 1);
        } else {
            panic!("expected mesh");
        }
    }

    #[test]
    fn fidelity_report_includes_unknown_cards() {
        let bdf = r#"GRID,1,,0.0,0.0,0.0
GRID,2,,1.0,0.0,0.0
GRID,3,,0.0,1.0,0.0
CTRIA3,1,1,1,2,3
EIGRL,10,,,100.0
RBE2,20,1,1,2,3
"#;
        let path = write_temp("unknown.bdf", bdf);
        let (_doc, report) = import_nastran(&path).unwrap();

        let eigrl = report
            .entities
            .iter()
            .find(|e| e.entity == "EIGRL")
            .expect("EIGRL should be in report");
        assert_eq!(eigrl.status, EntityStatus::Dropped);

        let rbe2 = report
            .entities
            .iter()
            .find(|e| e.entity == "RBE2")
            .expect("RBE2 should be in report");
        assert_eq!(rbe2.status, EntityStatus::Dropped);
    }

    #[test]
    fn large_field_format() {
        let mut bdf = String::new();
        bdf.push_str(&nastran_line("GRID", &["1", "", "0.0", "1.0", "2.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["2", "", "3.0", "4.0", "5.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("GRID", &["3", "", "6.0", "7.0", "8.0"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("CTRIA3", &["200", "2", "1", "2", "3"]));
        bdf.push('\n');
        bdf.push_str(&nastran_line("MAT1", &["2", "100.e9", "", "0.3", "2700."]));
        bdf.push('\n');

        let path = write_temp("large.bdf", &bdf);
        let (doc, _report) = import_nastran(&path).unwrap();

        let part = &doc.parts[0];
        assert_eq!(part.name, "pid_2");

        if let GeometryPayload::Mesh(m) = &part.geometry {
            assert_eq!(m.vertices.len(), 3);
            assert_eq!(m.faces.len(), 1);
        } else {
            panic!("expected mesh");
        }

        assert_eq!(part.semantics.materials.len(), 1);
    }

    #[test]
    fn import_bdf_alias_works() {
        let bdf = r#"GRID,1,,0.0,0.0,0.0
GRID,2,,1.0,0.0,0.0
GRID,3,,0.0,1.0,0.0
CTRIA3,1,1,1,2,3
"#;
        let path = write_temp("alias.bdf", bdf);
        let (doc1, _) = import_nastran(&path).unwrap();
        let (doc2, _) = import_bdf(&path).unwrap();
        assert_eq!(doc1.parts.len(), doc2.parts.len());
    }

    #[test]
    fn fortsd_exponent_parsed() {
        let bdf = r#"GRID,1,,1.0D+3,2.0d-2,3.0D0
GRID,2,,4.0,5.0,6.0
GRID,3,,7.0,8.0,9.0
CTRIA3,1,1,1,2,3
"#;
        let path = write_temp("fortsd.bdf", bdf);
        let (doc, _report) = import_nastran(&path).unwrap();
        let part = &doc.parts[0];
        if let GeometryPayload::Mesh(m) = &part.geometry {
            let v: Vec<[f32; 3]> = m.vertices.clone();
            let has_1000 = v.iter().any(|vtx| (vtx[0] - 1000.0).abs() < 0.1);
            let has_002 = v.iter().any(|vtx| (vtx[1] - 0.02).abs() < 0.001);
            let has_30 = v.iter().any(|vtx| (vtx[2] - 3.0).abs() < 0.001);
            assert!(has_1000, "grid 1.0D+3 should be present");
            assert!(has_002, "grid 2.0d-2 should be present");
            assert!(has_30, "grid 3.0D0 should be present");
        }
    }

    #[test]
    fn no_elements_yields_empty_parts() {
        let bdf = r#"GRID,1,,0.0,0.0,0.0
GRID,2,,1.0,0.0,0.0
"#;
        let path = write_temp("empty.bdf", bdf);
        let (doc, _report) = import_nastran(&path).unwrap();
        assert!(doc.parts.is_empty());
    }

    #[test]
    fn corpus_simple_plate() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/nastran-simple.bdf");
        let (doc, _report) = import_nastran(&path).unwrap();
        assert_eq!(doc.parts.len(), 1);
        if let GeometryPayload::Mesh(m) = &doc.parts[0].geometry {
            assert_eq!(m.vertices.len(), 4);
            assert_eq!(m.faces.len(), 2);
        } else {
            panic!("expected mesh");
        }
    }

    #[test]
    fn corpus_beam() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/nastran-beam.bdf");
        let (doc, report) = import_nastran(&path).unwrap();
        assert_eq!(doc.parts.len(), 1);
        let part = &doc.parts[0];
        if let GeometryPayload::Mesh(m) = &part.geometry {
            assert!(m.vertices.len() >= 8);
            assert_eq!(m.faces.len(), 10);
        } else {
            panic!("expected mesh");
        }
        let spc_bc = part
            .semantics
            .boundary_conditions
            .iter()
            .find(|bc| matches!(bc.bc_type, BcType::FixedDisplacement));
        assert!(spc_bc.is_some());
        assert!(report
            .entities
            .iter()
            .any(|e| e.entity == "FORCE" && e.status == EntityStatus::Dropped));
    }

    #[test]
    fn export_round_trip_mesh_material_fixed_bc() {
        let mesh = Mesh {
            vertices: vec![
                [0.0f32, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            faces: vec![[0u32, 1, 2], [0, 2, 3]],
            ..Default::default()
        };

        let mut part = Part::new("test_part", GeometryPayload::Mesh(mesh));
        part.semantics.materials.push(Material {
            name: "aluminum".into(),
            elastic_modulus: Some(Quantity::new(70e9, Unit::Pascal)),
            poisson_ratio: Some(0.33),
            density: Some(Quantity::new(2700.0, Unit::KilogramPerCubicMeter)),
            ..Default::default()
        });
        part.semantics.boundary_conditions.push(BoundaryCondition {
            face_group: "1".into(),
            bc_type: BcType::FixedDisplacement,
            value: Quantity::new(0.0, Unit::Meter),
            direction: None,
        });

        let doc = Document::new(vec![part]);

        let dir = std::env::temp_dir().join("exl-nastran-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("roundtrip.bdf");

        let report = export_nastran(&doc, &path).unwrap();
        assert_eq!(report.source_format, "exl");
        assert_eq!(report.target_format, "nastran");
        assert!(report
            .entities
            .iter()
            .any(|e| e.entity == "vertices" && e.status == EntityStatus::Lossless));
        assert!(report
            .entities
            .iter()
            .any(|e| e.entity == "faces" && e.status == EntityStatus::Lossless));
        assert!(report
            .entities
            .iter()
            .any(|e| e.entity == "materials" && e.status == EntityStatus::Lossless));
        assert!(report
            .entities
            .iter()
            .any(|e| e.entity == "fixed_BC" && e.status == EntityStatus::Lossless));

        let (imported, import_report) = import_nastran(&path).unwrap();
        assert_eq!(imported.parts.len(), 1);
        let imported_part = &imported.parts[0];

        if let GeometryPayload::Mesh(m) = &imported_part.geometry {
            assert_eq!(m.vertices.len(), 4);
            assert_eq!(m.faces.len(), 2);
        } else {
            panic!("expected mesh");
        }

        assert_eq!(imported_part.semantics.materials.len(), 1);
        let mat = &imported_part.semantics.materials[0];
        let e_si = mat.elastic_modulus.as_ref().unwrap().to_si();
        assert!((e_si - 70e9).abs() < 1e6);
        let nu = mat.poisson_ratio.unwrap();
        assert!((nu - 0.33).abs() < 1e-6);
        let rho_si = mat.density.as_ref().unwrap().to_si();
        assert!((rho_si - 2700.0).abs() < 0.1);

        assert!(!imported_part.semantics.boundary_conditions.is_empty());
        let has_fixed = imported_part
            .semantics
            .boundary_conditions
            .iter()
            .any(|bc| matches!(bc.bc_type, BcType::FixedDisplacement));
        assert!(has_fixed);

        assert_eq!(import_report.source_format, "nastran");
        assert_eq!(import_report.target_format, "exl");
    }

    #[test]
    fn export_bdf_alias_works() {
        let mesh = Mesh {
            vertices: vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            faces: vec![[0u32, 1, 2]],
            ..Default::default()
        };
        let doc = Document::new(vec![Part::new("tri", GeometryPayload::Mesh(mesh))]);

        let dir = std::env::temp_dir().join("exl-nastran-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path1 = dir.join("alias1.bdf");
        let path2 = dir.join("alias2.bdf");

        let r1 = export_nastran(&doc, &path1).unwrap();
        let r2 = export_bdf(&doc, &path2).unwrap();
        assert_eq!(r1.source_format, r2.source_format);
        assert_eq!(r1.target_format, r2.target_format);
    }

    #[test]
    fn export_pressure_bc_dropped() {
        let mesh = Mesh {
            vertices: vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            faces: vec![[0u32, 1, 2]],
            ..Default::default()
        };
        let mut part = Part::new("tri", GeometryPayload::Mesh(mesh));
        part.semantics.boundary_conditions.push(BoundaryCondition {
            face_group: "top".into(),
            bc_type: BcType::Pressure,
            value: Quantity::new(101325.0, Unit::Pascal),
            direction: None,
        });
        let doc = Document::new(vec![part]);

        let dir = std::env::temp_dir().join("exl-nastran-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pressure.bdf");

        let report = export_nastran(&doc, &path).unwrap();
        let pressure_entity = report
            .entities
            .iter()
            .find(|e| e.entity == "pressure_BC")
            .expect("pressure_BC should be in fidelity report");
        assert_eq!(pressure_entity.status, EntityStatus::Dropped);
    }

    #[test]
    fn export_handles_empty_doc() {
        let doc = Document::new(vec![]);
        let dir = std::env::temp_dir().join("exl-nastran-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.bdf");
        let report = export_nastran(&doc, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("ENDDATA"));
        assert!(report
            .entities
            .iter()
            .all(|e| e.status == EntityStatus::Lossless || e.status == EntityStatus::Dropped));
    }
}
