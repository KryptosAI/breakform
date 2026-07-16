use exl_core::units::{Quantity, Unit};
use exl_core::{
    Assembly, BcType, BoundaryCondition, Document, EntityStatus, FidelityReport,
    GeometryPayload, Material, Part, Provenance, ToolOfOrigin,
};
use exl_geom::Mesh;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum AbaqusError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Unsupported: {0}")]
    Unsupported(String),
}

pub use import_abaqus as import_inp;

pub fn import_abaqus(path: &Path) -> Result<(Document, FidelityReport), AbaqusError> {
    let content = std::fs::read_to_string(path)?;
    parse_abaqus(&content)
}

fn tokenize(line: &str) -> Vec<String> {
    line.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn strip_comments(line: &str) -> String {
    if let Some(pos) = line.find("**") {
        line[..pos].to_string()
    } else {
        line.to_string()
    }
}

fn preprocess_lines(input: &str) -> Vec<String> {
    let mut logical_lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for raw in input.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("**") {
            continue;
        }
        let stripped = strip_comments(trimmed);
        let cleaned = stripped.trim();
        if cleaned.is_empty() {
            continue;
        }

        current.push_str(cleaned);
        if cleaned.ends_with(',') {
            continue;
        }

        logical_lines.push(current.clone());
        current.clear();
    }

    if !current.is_empty() {
        logical_lines.push(current);
    }

    logical_lines
}

fn parse_parameter(card: &str, key: &str) -> Option<String> {
    let upper = card.to_uppercase();
    let key_upper = key.to_uppercase();
    for part in upper.split(',').map(|s| s.trim()) {
        if let Some(eq_pos) = part.find('=') {
            let k = part[..eq_pos].trim();
            let v = part[eq_pos + 1..].trim();
            if k == key_upper {
                return Some(v.to_string());
            }
        } else if part == key_upper {
            return Some(String::new());
        }
    }
    None
}

fn iso_timestamp_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs() as i64;

    let days = total_secs / 86400;
    let sod = total_secs % 86400;

    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y + if m <= 2 { 1 } else { 0 };

    let h = sod / 3600;
    let min = (sod % 3600) / 60;
    let s = sod % 60;

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, h, min, s)
}

#[derive(Debug, Clone, Default)]
struct ElementTypeTracker {
    truss_count: usize,
    wedge_count: usize,
    parabolic_count: usize,
    c3d8r_count: usize,
    c3d4_count: usize,
    other_dropped: Vec<(String, usize)>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct AbaqusElement {
    elem_id: u32,
    elem_type: String,
    node_ids: Vec<u32>,
    elset: Option<String>,
}

#[derive(Debug, Clone)]
struct BoundaryRecord {
    node_id: u32,
    dof_start: u32,
    dof_end: u32,
    value: f64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct CloadRecord {
    node_id: u32,
    dof: u32,
    magnitude: f64,
}

#[derive(Debug, Clone)]
struct DloadRecord {
    elem_id: u32,
    face_label: String,
    pressure: f64,
}

fn parse_abaqus(input: &str) -> Result<(Document, FidelityReport), AbaqusError> {
    let lines = preprocess_lines(input);
    if lines.is_empty() {
        return Err(AbaqusError::Parse("empty input".into()));
    }

    let mut nodes: HashMap<u32, [f32; 3]> = HashMap::new();
    let mut elements: Vec<AbaqusElement> = Vec::new();
    let mut materials: HashMap<String, Material> = HashMap::new();
    let mut current_material_name: Option<String> = None;
    let mut elset_material: HashMap<String, String> = HashMap::new();
    let mut boundaries: Vec<BoundaryRecord> = Vec::new();
    let mut cloads: Vec<CloadRecord> = Vec::new();
    let mut dloads: Vec<DloadRecord> = Vec::new();
    let mut tracker = ElementTypeTracker::default();
    let mut fidelity = FidelityReport::new("abaqus", "exl");
    let mut step_type: Option<String> = None;
    let part_name = String::from("default");

    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        let upper = line.to_uppercase();

        if upper.starts_with('*') {
            let stripped = line[1..].trim();
            let upper_stripped = stripped.to_uppercase();

            if upper_stripped == "HEADING" {
                i += 1;
                while i < lines.len() && !lines[i].starts_with('*') {
                    i += 1;
                }
                continue;
            }

            if upper_stripped == "PREPRINT" {
                i += 1;
                while i < lines.len() && !lines[i].starts_with('*') {
                    i += 1;
                }
                continue;
            }

            if upper_stripped.starts_with("NODE")
                && !upper_stripped.starts_with("NODE ")
                && !upper_stripped.contains("OUTPUT")
                && !upper_stripped.contains("PRINT")
                && !upper_stripped.contains("FILE")
            {
                i += 1;
                while i < lines.len() && !lines[i].starts_with('*') {
                    let tokens = tokenize(&lines[i]);
                    if tokens.len() >= 4 {
                        let nid: u32 = tokens[0]
                            .parse()
                            .map_err(|_| AbaqusError::Parse(format!("bad node id: {}", tokens[0])))?;
                        let x: f32 = tokens[1]
                            .parse()
                            .map_err(|_| AbaqusError::Parse(format!("bad x: {}", tokens[1])))?;
                        let y: f32 = tokens[2]
                            .parse()
                            .map_err(|_| AbaqusError::Parse(format!("bad y: {}", tokens[2])))?;
                        let z: f32 = tokens[3]
                            .parse()
                            .map_err(|_| AbaqusError::Parse(format!("bad z: {}", tokens[3])))?;
                        nodes.insert(nid, [x, y, z]);
                    }
                    i += 1;
                }
                continue;
            }

            if upper_stripped.starts_with("ELEMENT")
                && !upper_stripped.starts_with("ELEMENT ")
                && !upper_stripped.contains("OUTPUT")
                && !upper_stripped.contains("PRINT")
                && !upper_stripped.contains("FILE")
            {
                let elem_type = parse_parameter(&stripped, "TYPE")
                    .unwrap_or_default()
                    .to_uppercase();
                let elset = parse_parameter(&stripped, "ELSET");

                i += 1;
                while i < lines.len() && !lines[i].starts_with('*') {
                    let tokens = tokenize(&lines[i]);
                    if tokens.len() >= 2 {
                        let eid: u32 = tokens[0]
                            .parse()
                            .map_err(|_| AbaqusError::Parse(format!("bad elem id: {}", tokens[0])))?;
                        let nids: Result<Vec<u32>, _> =
                            tokens[1..].iter().map(|t| t.parse::<u32>()).collect();
                        let nids = nids.map_err(|_| {
                            AbaqusError::Parse(format!("bad node id in element: {}", lines[i]))
                        })?;
                        elements.push(AbaqusElement {
                            elem_id: eid,
                            elem_type: elem_type.clone(),
                            node_ids: nids,
                            elset: elset.clone(),
                        });
                    }
                    i += 1;
                }
                continue;
            }

            if upper_stripped.starts_with("MATERIAL") {
                let name = parse_parameter(&stripped, "NAME")
                    .unwrap_or_else(|| "UNNAMED".to_string());
                current_material_name = Some(name.clone());
                materials.entry(name.clone()).or_insert_with(|| Material {
                    name: name.clone(),
                    ..Default::default()
                });

                i += 1;
                while i < lines.len() && lines[i].starts_with('*') {
                    let sub_line = &lines[i];
                    let sub_upper = sub_line[1..].trim().to_uppercase();

                    if sub_upper.starts_with("ELASTIC") {
                        i += 1;
                        if i < lines.len() && !lines[i].starts_with('*') {
                            let tokens = tokenize(&lines[i]);
                            if tokens.len() >= 2 {
                                let e_mod: f64 = tokens[0].parse().unwrap_or(0.0);
                                let nu: f64 = tokens[1].parse().unwrap_or(0.0);
                                if let Some(ref mat_name) = current_material_name {
                                    if let Some(mat) = materials.get_mut(mat_name) {
                                        mat.elastic_modulus =
                                            Some(Quantity::new(e_mod, Unit::Megapascal));
                                        mat.poisson_ratio = Some(nu);
                                    }
                                }
                            }
                            i += 1;
                        }
                    } else if sub_upper.starts_with("DENSITY") {
                        i += 1;
                        if i < lines.len() && !lines[i].starts_with('*') {
                            let tokens = tokenize(&lines[i]);
                            if !tokens.is_empty() {
                                let rho: f64 = tokens[0].parse().unwrap_or(0.0);
                                if let Some(ref mat_name) = current_material_name {
                                    if let Some(mat) = materials.get_mut(mat_name) {
                                        mat.density = Some(Quantity::new(
                                            rho,
                                            Unit::KilogramPerCubicMeter,
                                        ));
                                    }
                                }
                            }
                            i += 1;
                        }
                    } else if sub_upper.starts_with("PLASTIC") {
                        i += 1;
                        while i < lines.len() && !lines[i].starts_with('*') {
                            i += 1;
                        }
                        fidelity.record(
                            "plastic_behavior",
                            1,
                            EntityStatus::Approximate,
                            Some("plastic behavior not represented".into()),
                        );
                    } else {
                        break;
                    }
                }
                current_material_name = None;
                continue;
            }

            if upper_stripped.starts_with("SOLID SECTION") {
                let elset = parse_parameter(&stripped, "ELSET");
                let mat = parse_parameter(&stripped, "MATERIAL");
                if let (Some(es), Some(m)) = (elset, mat) {
                    elset_material.insert(es.to_uppercase(), m);
                }
                i += 1;
                continue;
            }

            if upper_stripped == "STEP" {
                step_type = None;
                boundaries.clear();
                cloads.clear();
                dloads.clear();
                i += 1;
                while i < lines.len() {
                    let sl = &lines[i];
                    if !sl.starts_with('*') {
                        i += 1;
                        continue;
                    }
                    let sl_stripped = sl[1..].trim().to_uppercase();

                    if sl_stripped == "END STEP" {
                        i += 1;
                        break;
                    }

                    if sl_stripped.starts_with("STATIC") {
                        step_type = Some("static".into());
                        i += 1;
                        while i < lines.len() && !lines[i].starts_with('*') {
                            i += 1;
                        }
                    } else if sl_stripped.starts_with("DYNAMIC") {
                        step_type = Some("dynamic".into());
                        i += 1;
                        while i < lines.len() && !lines[i].starts_with('*') {
                            i += 1;
                        }
                    } else if sl_stripped.starts_with("BOUNDARY") {
                        i += 1;
                        while i < lines.len() && !lines[i].starts_with('*') {
                            let tokens = tokenize(&lines[i]);
                            if tokens.len() >= 2 {
                                let nid: u32 = tokens[0].parse().unwrap_or(0);
                                let d1: u32 = tokens[1].parse().unwrap_or(0);
                                let d2: u32 = if tokens.len() >= 3 {
                                    tokens[2].parse().unwrap_or(d1)
                                } else {
                                    d1
                                };
                                let val: f64 = if tokens.len() >= 4 {
                                    tokens[3].parse().unwrap_or(0.0)
                                } else {
                                    0.0
                                };
                                boundaries.push(BoundaryRecord {
                                    node_id: nid,
                                    dof_start: d1,
                                    dof_end: d2,
                                    value: val,
                                });
                            }
                            i += 1;
                        }
                    } else if sl_stripped.starts_with("CLOAD") {
                        i += 1;
                        while i < lines.len() && !lines[i].starts_with('*') {
                            let tokens = tokenize(&lines[i]);
                            if tokens.len() >= 3 {
                                let nid: u32 = tokens[0].parse().unwrap_or(0);
                                let dof: u32 = tokens[1].parse().unwrap_or(0);
                                let mag: f64 = tokens[2].parse().unwrap_or(0.0);
                                cloads.push(CloadRecord {
                                    node_id: nid,
                                    dof,
                                    magnitude: mag,
                                });
                            }
                            i += 1;
                        }
                    } else if sl_stripped.starts_with("DLOAD") {
                        i += 1;
                        while i < lines.len() && !lines[i].starts_with('*') {
                            let tokens = tokenize(&lines[i]);
                            if tokens.len() >= 3 {
                                let eid: u32 = tokens[0].parse().unwrap_or(0);
                                let fl = tokens[1].clone();
                                let pres: f64 = tokens[2].parse().unwrap_or(0.0);
                                dloads.push(DloadRecord {
                                    elem_id: eid,
                                    face_label: fl.to_uppercase(),
                                    pressure: pres,
                                });
                            }
                            i += 1;
                        }
                    } else if sl_stripped.starts_with("NSET")
                        || sl_stripped.starts_with("ELSET")
                    {
                        i += 1;
                        while i < lines.len() && !lines[i].starts_with('*') {
                            i += 1;
                        }
                    } else if sl_stripped == "OUTPUT"
                        || sl_stripped == "NODE OUTPUT"
                        || sl_stripped == "ELEMENT OUTPUT"
                        || sl_stripped == "NODE PRINT"
                        || sl_stripped == "EL PRINT"
                        || sl_stripped == "ENERGY OUTPUT"
                        || sl_stripped == "RESTART, WRITE"
                        || sl_stripped.starts_with("OUTPUT")
                        || sl_stripped.starts_with("FILE FORMAT")
                        || sl_stripped.starts_with("CONTACT")
                        || sl_stripped.starts_with("CONTROLS")
                        || sl_stripped.starts_with("NODE FILE")
                        || sl_stripped.starts_with("EL FILE")
                    {
                        i += 1;
                    } else {
                        fidelity.record(
                            format!("keyword_{}", sl_stripped),
                            1,
                            EntityStatus::Dropped,
                            Some("unsupported step keyword".into()),
                        );
                        i += 1;
                        while i < lines.len() && !lines[i].starts_with('*') {
                            i += 1;
                        }
                    }
                }

                if let Some(ref st) = step_type {
                    fidelity.record(
                        format!("step_type_{}", st),
                        1,
                        EntityStatus::Dropped,
                        Some("step type recorded, no geometry extracted".into()),
                    );
                }

                let pinned_dof_count: usize =
                    boundaries.iter().map(|b| (b.dof_end - b.dof_start + 1) as usize).sum();
                if pinned_dof_count > 0 {
                    fidelity.record(
                        "boundary_condition",
                        boundaries.len(),
                        EntityStatus::Lossless,
                        Some(format!("{} pinned DOFs", pinned_dof_count)),
                    );
                }
                if !cloads.is_empty() {
                    fidelity.record(
                        "point_load",
                        cloads.len(),
                        EntityStatus::Dropped,
                        Some("point load not representable; add BcType::Force".into()),
                    );
                }
                continue;
            }

            fidelity.record(
                format!("keyword_{}", upper_stripped),
                1,
                EntityStatus::Dropped,
                Some(format!("unsupported keyword: {}", upper_stripped)),
            );
            i += 1;
            while i < lines.len() && !lines[i].starts_with('*') {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    for e in &elements {
        let et = &e.elem_type;
        if et == "C3D8R" || et == "C3D8" {
            tracker.c3d8r_count += 1;
        } else if et == "C3D4" {
            tracker.c3d4_count += 1;
        } else if et == "C3D6" {
            tracker.wedge_count += 1;
        } else if et == "C3D10" || et == "C3D20" || et == "C3D15" {
            tracker.parabolic_count += 1;
        } else if et == "TRUSS2" || et == "T3D2" {
            tracker.truss_count += 1;
        } else {
            let entry = tracker
                .other_dropped
                .iter_mut()
                .find(|(t, _)| t == et);
            if let Some((_, cnt)) = entry {
                *cnt += 1;
            } else {
                tracker.other_dropped.push((et.clone(), 1));
            }
        }
    }

    let mut node_index: HashMap<u32, usize> = HashMap::new();
    let mut sorted_node_ids: Vec<u32> = nodes.keys().copied().collect();
    sorted_node_ids.sort();
    for (vi, nid) in sorted_node_ids.iter().enumerate() {
        node_index.insert(*nid, vi);
    }
    let vertices: Vec<[f32; 3]> = sorted_node_ids.iter().map(|nid| nodes[nid]).collect();

    let mut faces: Vec<[u32; 3]> = Vec::new();
    let mut face_groups: Vec<u32> = Vec::new();
    let mut group_names: Vec<String> = Vec::new();
    let mut next_group_id: u32 = 0;
    let mut group_name_to_id: HashMap<String, u32> = HashMap::new();

    let mut get_or_create_group = |name: &str| -> u32 {
        if let Some(&gid) = group_name_to_id.get(name) {
            gid
        } else {
            let gid = next_group_id;
            next_group_id += 1;
            group_name_to_id.insert(name.to_string(), gid);
            group_names.push(name.to_string());
            gid
        }
    };

    for e in &elements {
        let nids: Vec<usize> = e
            .node_ids
            .iter()
            .map(|nid| node_index.get(nid).copied().unwrap_or(usize::MAX))
            .collect();

        if nids.iter().any(|&v| v == usize::MAX) {
            continue;
        }

        let et = &e.elem_type;
        if et == "C3D8R" || et == "C3D8" {
            for fi in 0..6 {
                let gname = format!("C3D8R_S{}", fi + 1);
                let gid = get_or_create_group(&gname);
                let indices: [(usize, usize, usize, usize); 6] = [
                    (0, 1, 2, 3),
                    (4, 7, 6, 5),
                    (0, 4, 5, 1),
                    (1, 5, 6, 2),
                    (2, 6, 7, 3),
                    (3, 7, 4, 0),
                ];
                let (a, b, c, d) = indices[fi];
                let a = nids[a] as u32;
                let b = nids[b] as u32;
                let c = nids[c] as u32;
                let d = nids[d] as u32;
                faces.push([a, b, c]);
                face_groups.push(gid);
                faces.push([a, c, d]);
                face_groups.push(gid);
            }
        } else if et == "C3D4" {
            for fi in 0..4 {
                let gname = format!("C3D4_S{}", fi + 1);
                let gid = get_or_create_group(&gname);
                let indices: [(usize, usize, usize); 4] = [
                    (0, 1, 2),
                    (0, 3, 1),
                    (1, 3, 2),
                    (2, 3, 0),
                ];
                let (a, b, c) = indices[fi];
                let a = nids[a] as u32;
                let b = nids[b] as u32;
                let c = nids[c] as u32;
                faces.push([a, b, c]);
                face_groups.push(gid);
            }
        }
    }

    let material_list: Vec<Material> = materials.values().cloned().collect();

    let mut part_materials: Vec<(String, Vec<Material>)> = Vec::new();

    if !elset_material.is_empty() && !elements.is_empty() {
        for (elset_name, mat_name) in &elset_material {
            let mat = materials.get(mat_name).cloned();
            let pname = format!("part_{}", elset_name.to_lowercase());
            part_materials.push((pname, mat.into_iter().collect()));
        }
    }

    if part_materials.is_empty() {
        if !nodes.is_empty() {
            part_materials.push((part_name.clone(), material_list));
        }
    }

    if part_materials.len() > 1 {
        fidelity.record(
            "multi_part_assembly",
            part_materials.len(),
            EntityStatus::Approximate,
            Some("multiple parts created from SOLID SECTION linking".into()),
        );
    }

    let mut parts: Vec<Part> = Vec::new();
    for (pname, mats) in &part_materials {
        let mut mesh = Mesh {
            vertices: vertices.clone(),
            faces: faces.clone(),
            ..Default::default()
        };

        if !face_groups.is_empty() {
            mesh.face_groups = Some(face_groups.clone());
            mesh.group_names = group_names.clone();
        }

        let mut semantics = exl_core::Semantics::default();
        semantics.materials = mats.clone();

        let node_to_faces: HashMap<usize, Vec<usize>> = {
            let mut mm: HashMap<usize, Vec<usize>> = HashMap::new();
            for (fi, face) in faces.iter().enumerate() {
                for &v in &[face[0] as usize, face[1] as usize, face[2] as usize] {
                    mm.entry(v).or_default().push(fi);
                }
            }
            mm
        };

        let mut bc_face_indices: Vec<usize> = Vec::new();
        for br in &boundaries {
            if let Some(&vi) = node_index.get(&br.node_id) {
                if let Some(adj) = node_to_faces.get(&vi) {
                    bc_face_indices.extend(adj);
                }
            }
        }

        if !bc_face_indices.is_empty() {
            let bc_group_name = "BC_fixed".to_string();
            let bc_gid = next_group_id;
            next_group_id += 1;
            group_names.push(bc_group_name.clone());
            mesh.group_names = group_names.clone();

            if mesh.face_groups.is_none() {
                mesh.face_groups = Some(vec![0; faces.len()]);
            }
            if let Some(ref mut fg) = mesh.face_groups {
                for &fi in &bc_face_indices {
                    fg[fi] = bc_gid;
                }
            }

            semantics.boundary_conditions.push(BoundaryCondition {
                face_group: bc_group_name,
                bc_type: BcType::FixedDisplacement,
                value: Quantity::new(0.0, Unit::Millimeter),
                direction: None,
            });
        } else if !boundaries.is_empty() {
            for br in &boundaries {
                semantics.boundary_conditions.push(BoundaryCondition {
                    face_group: format!("node_{}", br.node_id),
                    bc_type: BcType::FixedDisplacement,
                    value: Quantity::new(br.value, Unit::Millimeter),
                    direction: None,
                });
            }
        }

        for dl in &dloads {
            let fl = &dl.face_label;
            let mut matched = false;
            for gn in &group_names {
                let gn_upper = gn.to_uppercase();
                if gn_upper.contains(fl.as_str()) || gn_upper.ends_with(fl.as_str()) {
                    semantics.boundary_conditions.push(BoundaryCondition {
                        face_group: gn.clone(),
                        bc_type: BcType::Pressure,
                        value: Quantity::new(dl.pressure, Unit::Megapascal),
                        direction: None,
                    });
                    matched = true;
                    break;
                }
            }
            if !matched {
                semantics.boundary_conditions.push(BoundaryCondition {
                    face_group: format!("elem_{}_face_{}", dl.elem_id, dl.face_label),
                    bc_type: BcType::Pressure,
                    value: Quantity::new(dl.pressure, Unit::Megapascal),
                    direction: None,
                });
            }
        }
        if !dloads.is_empty() {
            fidelity.record(
                "distributed_load",
                dloads.len(),
                EntityStatus::Lossless,
                Some("mapped to pressure BC".into()),
            );
        }

        let mut part = Part::new(pname.as_str(), GeometryPayload::Mesh(mesh));
        part.semantics = semantics;
        parts.push(part);
    }

    if tracker.c3d8r_count > 0 {
        fidelity.record("element_C3D8R", tracker.c3d8r_count, EntityStatus::Lossless, None);
    }
    if tracker.c3d4_count > 0 {
        fidelity.record("element_C3D4", tracker.c3d4_count, EntityStatus::Lossless, None);
    }
    if tracker.wedge_count > 0 {
        fidelity.record(
            "element_C3D6",
            tracker.wedge_count,
            EntityStatus::Dropped,
            Some("C3D6 element faces not decomposed".into()),
        );
    }
    if tracker.parabolic_count > 0 {
        fidelity.record(
            "element_parabolic",
            tracker.parabolic_count,
            EntityStatus::Approximate,
            Some("midside_nodes_stripped".into()),
        );
    }
    if tracker.truss_count > 0 {
        fidelity.record(
            "element_line",
            tracker.truss_count,
            EntityStatus::Dropped,
            Some("line element not representable as mesh face".into()),
        );
    }
    for (et, cnt) in &tracker.other_dropped {
        fidelity.record(
            format!("element_{}", et),
            *cnt,
            EntityStatus::Dropped,
            Some(format!("unsupported element type: {}", et)),
        );
    }

    if !nodes.is_empty() {
        fidelity.record("node", nodes.len(), EntityStatus::Lossless, None);
    }
    if !faces.is_empty() {
        fidelity.record("mesh_face", faces.len(), EntityStatus::Lossless, None);
    }

    let provenance = Provenance {
        uuid: exl_core::new_uuid(),
        content_hash: String::new(),
        parent_hashes: Vec::new(),
        tool_of_origin: Some(ToolOfOrigin {
            name: "exl-abaqus".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            timestamp_iso: iso_timestamp_now(),
        }),
        conversion_fidelity: Some(fidelity.overall),
    };

    let mut doc = Document {
        schema_version: exl_core::SCHEMA_VERSION.to_string(),
        parts,
        assembly: Assembly::default(),
        provenance,
    };
    doc.refresh_content_hash();

    Ok((doc, fidelity))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_inp() -> String {
        r#"*HEADING
Cube test
*NODE
1, 0.0, 0.0, 0.0
2, 1.0, 0.0, 0.0
3, 1.0, 1.0, 0.0
4, 0.0, 1.0, 0.0
5, 0.0, 0.0, 1.0
6, 1.0, 0.0, 1.0
7, 1.0, 1.0, 1.0
8, 0.0, 1.0, 1.0
*ELEMENT, TYPE=C3D8R, ELSET=CUBE
1, 1, 2, 3, 4, 5, 6, 7, 8
*SOLID SECTION, ELSET=CUBE, MATERIAL=STEEL
*MATERIAL, NAME=STEEL
*ELASTIC
200000.0, 0.3
*DENSITY
7.85e-09
"#
        .to_string()
    }

    #[test]
    fn cube_import() {
        let inp = cube_inp();
        let (doc, fidelity) = parse_abaqus(&inp).unwrap();

        assert_eq!(doc.parts.len(), 1, "expected 1 part");
        let part = &doc.parts[0];

        if let GeometryPayload::Mesh(ref mesh) = part.geometry {
            assert_eq!(mesh.vertices.len(), 8, "expected 8 vertices");
            assert_eq!(mesh.faces.len(), 12, "expected 12 faces");
            assert!(mesh.face_groups.is_some(), "expected face groups");
            assert_eq!(mesh.group_names.len(), 6, "expected 6 group names");
        } else {
            panic!("expected mesh geometry");
        }

        assert_eq!(part.semantics.materials.len(), 1, "expected 1 material");
        let mat = &part.semantics.materials[0];
        assert_eq!(mat.name, "STEEL");
        assert!(mat.elastic_modulus.is_some(), "expected elastic modulus");
        assert_eq!(
            mat.elastic_modulus.unwrap().value,
            200000.0,
            "expected E=200000"
        );
        assert!(mat.poisson_ratio.is_some(), "expected poisson ratio");
        assert_eq!(mat.poisson_ratio.unwrap(), 0.3, "expected nu=0.3");
        assert!(mat.density.is_some(), "expected density");
        assert_eq!(
            mat.density.unwrap().value,
            7.85e-09,
            "expected density value"
        );

        assert!(
            fidelity.entities.iter().any(|e| e.entity == "element_C3D8R" && e.count == 1),
            "expected C3D8R count 1"
        );
        assert!(
            fidelity.entities.iter().any(|e| e.entity == "node" && e.count == 8),
            "expected node count 8"
        );
        assert!(
            fidelity.entities.iter().any(|e| e.entity == "mesh_face" && e.count == 12),
            "expected mesh_face count 12"
        );
    }

    fn truss_inp() -> String {
        r#"*HEADING
Truss test
*NODE
1, 0.0, 0.0, 0.0
2, 1.0, 0.0, 0.0
3, 2.0, 0.0, 0.0
4, 3.0, 0.0, 0.0
5, 4.0, 0.0, 0.0
6, 5.0, 0.0, 0.0
7, 6.0, 0.0, 0.0
8, 7.0, 0.0, 0.0
9, 8.0, 0.0, 0.0
10, 9.0, 0.0, 0.0
11, 10.0, 0.0, 0.0
*ELEMENT, TYPE=TRUSS2
1, 1, 2
2, 2, 3
3, 3, 4
4, 4, 5
5, 5, 6
6, 6, 7
7, 7, 8
8, 8, 9
9, 9, 10
10, 10, 11
*MATERIAL, NAME=STEEL
*ELASTIC
200000.0, 0.3
*STEP
*STATIC
*BOUNDARY
1, 1, 3, 0.0
*CLOAD
11, 2, 1000.0
*END STEP
"#
        .to_string()
    }

    #[test]
    fn truss_import() {
        let inp = truss_inp();
        let (doc, fidelity) = parse_abaqus(&inp).unwrap();

        assert_eq!(doc.parts.len(), 1, "expected 1 part");
        let part = &doc.parts[0];

        if let GeometryPayload::Mesh(ref mesh) = part.geometry {
            assert_eq!(mesh.vertices.len(), 11, "expected 11 vertices");
            assert_eq!(mesh.faces.len(), 0, "expected 0 faces");
        } else {
            panic!("expected mesh geometry");
        }

        assert!(
            !part.semantics.boundary_conditions.is_empty(),
            "expected boundary conditions"
        );
        assert!(
            fidelity
                .entities
                .iter()
                .any(|e| e.entity == "boundary_condition"),
            "expected boundary_condition in fidelity"
        );
        assert!(
            fidelity
                .entities
                .iter()
                .any(|e| e.entity == "point_load" && e.count == 1),
            "expected point_load count 1"
        );
        assert!(
            fidelity
                .entities
                .iter()
                .any(|e| e.entity == "element_line" && e.count == 10),
            "expected element_line count 10"
        );
        assert!(
            fidelity
                .entities
                .iter()
                .any(|e| e.entity == "step_type_static"),
            "expected step_type_static"
        );
    }

    #[test]
    fn continuation_lines() {
        let inp = r#"*NODE
1, 0.0, 0.0,
0.0
2, 1.0, 0.0,
0.0
"#;
        let (doc, _) = parse_abaqus(inp).unwrap();
        if let GeometryPayload::Mesh(ref mesh) = doc.parts[0].geometry {
            assert_eq!(mesh.vertices.len(), 2);
            assert_eq!(mesh.vertices[0], [0.0, 0.0, 0.0]);
            assert_eq!(mesh.vertices[1], [1.0, 0.0, 0.0]);
        }
    }

    #[test]
    fn comment_handling() {
        let inp = r#"** This is a header comment
*NODE
** node 1
1, 10.0, 20.0, 30.0
2, 40.0, 50.0, 60.0 ** inline comment
"#;
        let (doc, _) = parse_abaqus(inp).unwrap();
        if let GeometryPayload::Mesh(ref mesh) = doc.parts[0].geometry {
            assert_eq!(mesh.vertices.len(), 2);
            assert_eq!(mesh.vertices[0], [10.0, 20.0, 30.0]);
            assert_eq!(mesh.vertices[1], [40.0, 50.0, 60.0]);
        }
    }

    #[test]
    fn write_corpus_files() {
        let corpus_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("corpus");

        std::fs::create_dir_all(&corpus_dir).unwrap();

        let cube_path = corpus_dir.join("abaqus-cube.inp");
        let truss_path = corpus_dir.join("abaqus-truss.inp");

        std::fs::write(&cube_path, cube_inp()).unwrap();
        std::fs::write(&truss_path, truss_inp()).unwrap();

        assert!(cube_path.exists());
        assert!(truss_path.exists());

        let cube_content = std::fs::read_to_string(&cube_path).unwrap();
        assert!(cube_content.contains("*ELEMENT, TYPE=C3D8R"));
        let truss_content = std::fs::read_to_string(&truss_path).unwrap();
        assert!(truss_content.contains("*BOUNDARY"));
    }
}
