use exl_core::{Document, GeometryPayload, Part};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TopologyDelta {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<ModifiedNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifiedNode {
    pub id: String,
    pub change: String,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformDelta {
    pub part: String,
    pub kind: String,
    pub translation: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataDelta {
    pub path: String,
    pub old: serde_json::Value,
    pub new: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiffReport {
    pub topology: TopologyDelta,
    pub transforms: Vec<TransformDelta>,
    pub metadata: Vec<MetadataDelta>,
}

impl DiffReport {
    pub fn is_empty(&self) -> bool {
        self.topology.added.is_empty()
            && self.topology.removed.is_empty()
            && self.topology.modified.is_empty()
            && self.transforms.is_empty()
            && self.metadata.is_empty()
    }
}

fn compare_parts(a_part: &Part, b_part: &Part, report: &mut DiffReport) {
    compare_geometry(a_part, b_part, report);
    compare_metadata(a_part, b_part, report);
}

fn compare_geometry(a_part: &Part, b_part: &Part, report: &mut DiffReport) {
    match (&a_part.geometry, &b_part.geometry) {
        (GeometryPayload::Mesh(a_m), GeometryPayload::Mesh(b_m)) => {
            if a_m.vertices.len() != b_m.vertices.len() || a_m.faces.len() != b_m.faces.len() {
                report.topology.modified.push(ModifiedNode {
                    id: a_part.id.clone(),
                    change: "mesh".to_string(),
                    old: format!("{} verts/{} faces", a_m.vertices.len(), a_m.faces.len()),
                    new: format!("{} verts/{} faces", b_m.vertices.len(), b_m.faces.len()),
                });
            }
        }
        (GeometryPayload::Brep(a_b), GeometryPayload::Brep(b_b)) => {
            let a_face_ids: HashSet<&str> = a_b.faces.iter().map(|f| f.id.as_str()).collect();
            let b_face_ids: HashSet<&str> = b_b.faces.iter().map(|f| f.id.as_str()).collect();

            for fid in &b_face_ids {
                if !a_face_ids.contains(fid) {
                    report.topology.added.push(format!("face:{}", fid));
                }
            }
            for fid in &a_face_ids {
                if !b_face_ids.contains(fid) {
                    report.topology.removed.push(format!("face:{}", fid));
                }
            }

            for a_face in &a_b.faces {
                if let Some(b_face) = b_b.faces.iter().find(|f| f.id == a_face.id) {
                    if a_face.surface != b_face.surface {
                        report.topology.modified.push(ModifiedNode {
                            id: a_face.id.clone(),
                            change: "surface_type".to_string(),
                            old: format!("{:?}", a_face.surface),
                            new: format!("{:?}", b_face.surface),
                        });
                    }
                }
            }
        }
        _ => {}
    }
}

fn compare_metadata(a_part: &Part, b_part: &Part, report: &mut DiffReport) {
    let a_val = serde_json::to_value(&a_part.semantics).unwrap();
    let b_val = serde_json::to_value(&b_part.semantics).unwrap();
    let prefix = format!("parts[{}]", a_part.name);
    diff_json_values(&prefix, &a_val, &b_val, report);
}

fn diff_json_values(
    path: &str,
    a: &serde_json::Value,
    b: &serde_json::Value,
    report: &mut DiffReport,
) {
    match (a, b) {
        (serde_json::Value::Object(a_map), serde_json::Value::Object(b_map)) => {
            for (key, a_v) in a_map {
                let new_path = format!("{}.{}", path, key);
                if let Some(b_v) = b_map.get(key) {
                    diff_json_values(&new_path, a_v, b_v, report);
                } else {
                    report.metadata.push(MetadataDelta {
                        path: new_path,
                        old: a_v.clone(),
                        new: serde_json::Value::Null,
                    });
                }
            }
            for (key, b_v) in b_map {
                if !a_map.contains_key(key) {
                    let new_path = format!("{}.{}", path, key);
                    report.metadata.push(MetadataDelta {
                        path: new_path,
                        old: serde_json::Value::Null,
                        new: b_v.clone(),
                    });
                }
            }
        }
        (serde_json::Value::Array(a_arr), serde_json::Value::Array(b_arr)) => {
            let max_len = a_arr.len().max(b_arr.len());
            for i in 0..max_len {
                let new_path = format!("{}[{}]", path, i);
                match (a_arr.get(i), b_arr.get(i)) {
                    (Some(a_v), Some(b_v)) => diff_json_values(&new_path, a_v, b_v, report),
                    (Some(a_v), None) => {
                        report.metadata.push(MetadataDelta {
                            path: new_path,
                            old: a_v.clone(),
                            new: serde_json::Value::Null,
                        });
                    }
                    (None, Some(b_v)) => {
                        report.metadata.push(MetadataDelta {
                            path: new_path,
                            old: serde_json::Value::Null,
                            new: b_v.clone(),
                        });
                    }
                    (None, None) => {}
                }
            }
        }
        _ => {
            if a != b {
                report.metadata.push(MetadataDelta {
                    path: path.to_string(),
                    old: a.clone(),
                    new: b.clone(),
                });
            }
        }
    }
}

pub fn diff(a: &Document, b: &Document) -> DiffReport {
    let mut report = DiffReport::default();

    let mut b_matched_ids: HashSet<&str> = HashSet::new();
    let mut a_matched_ids: HashSet<&str> = HashSet::new();

    for a_part in &a.parts {
        if let Some(b_part) = b.parts.iter().find(|bp| bp.id == a_part.id) {
            compare_parts(a_part, b_part, &mut report);
            a_matched_ids.insert(a_part.id.as_str());
            b_matched_ids.insert(b_part.id.as_str());
        }
    }

    for a_part in &a.parts {
        if a_matched_ids.contains(a_part.id.as_str()) {
            continue;
        }
        if let Some(b_part) = b
            .parts
            .iter()
            .find(|bp| bp.name == a_part.name && !b_matched_ids.contains(bp.id.as_str()))
        {
            compare_parts(a_part, b_part, &mut report);
            a_matched_ids.insert(a_part.id.as_str());
            b_matched_ids.insert(b_part.id.as_str());
        }
    }

    for b_part in &b.parts {
        if !b_matched_ids.contains(b_part.id.as_str()) {
            report.topology.added.push(format!("part:{}", b_part.id));
        }
    }

    for a_part in &a.parts {
        if !a_matched_ids.contains(a_part.id.as_str()) {
            report.topology.removed.push(format!("part:{}", a_part.id));
        }
    }

    let a_instances: Vec<(&str, &exl_core::Instance)> = a
        .assembly
        .instances
        .iter()
        .map(|i| (i.name.as_str(), i))
        .collect();
    let b_instances: Vec<(&str, &exl_core::Instance)> = b
        .assembly
        .instances
        .iter()
        .map(|i| (i.name.as_str(), i))
        .collect();

    for (name, a_inst) in &a_instances {
        if let Some((_, b_inst)) = b_instances.iter().find(|(n, _)| n == name) {
            let a_t = a_inst.transform;
            let b_t = b_inst.transform;

            if a_t.is_identity(1e-9) && b_t.is_identity(1e-9) {
                continue;
            }

            if !a_t.approx_eq(&b_t, 1e-9) {
                let a_trans = a_t.translation();
                let b_trans = b_t.translation();
                report.transforms.push(TransformDelta {
                    part: name.to_string(),
                    kind: "rigid_body".to_string(),
                    translation: [
                        b_trans[0] - a_trans[0],
                        b_trans[1] - a_trans[1],
                        b_trans[2] - a_trans[2],
                    ],
                });
            }
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use exl_core::geom::{BRep, BrepFace, Mesh, SurfaceType, Transform};
    use exl_core::units::{Quantity, Unit};
    use exl_core::{Assembly, Document, GeometryPayload, Instance, Material, Part};

    fn make_doc(parts: Vec<Part>, assembly: Assembly) -> Document {
        let mut doc = Document {
            schema_version: exl_core::SCHEMA_VERSION.to_string(),
            parts,
            assembly,
            provenance: exl_core::Provenance {
                uuid: exl_core::new_uuid(),
                content_hash: String::new(),
                parent_hashes: vec![],
                tool_of_origin: None,
                conversion_fidelity: None,
            },
        };
        doc.provenance.content_hash = doc.compute_content_hash();
        doc
    }

    fn simple_mesh() -> Mesh {
        Mesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            faces: vec![[0, 1, 2]],
            ..Default::default()
        }
    }

    #[test]
    fn identical_docs_is_empty() {
        let mesh = simple_mesh();
        let part = Part::new("test", GeometryPayload::Mesh(mesh));
        let doc = make_doc(vec![part], Assembly::default());
        let report = diff(&doc, &doc);
        assert!(report.is_empty());
    }

    #[test]
    fn changed_material_value_metadata_delta() {
        let mesh = simple_mesh();
        let mut a_part = Part::new("test", GeometryPayload::Mesh(mesh.clone()));
        a_part.semantics.materials = vec![Material {
            name: "steel".into(),
            elastic_modulus: Some(Quantity::new(200.0, Unit::Gigapascal)),
            ..Default::default()
        }];

        let mut b_part = Part::new("test", GeometryPayload::Mesh(mesh));
        b_part.semantics.materials = vec![Material {
            name: "steel".into(),
            elastic_modulus: Some(Quantity::new(210.0, Unit::Gigapascal)),
            ..Default::default()
        }];

        let a = make_doc(vec![a_part], Assembly::default());
        let b = make_doc(vec![b_part], Assembly::default());

        let report = diff(&a, &b);
        assert!(!report.metadata.is_empty());
        assert!(report
            .metadata
            .iter()
            .any(|m| m.path.contains("materials") && m.path.contains("elastic_modulus")));
    }

    #[test]
    fn added_part_topology_added() {
        let mesh = simple_mesh();
        let a_part = Part::new("existing", GeometryPayload::Mesh(mesh.clone()));
        let b_part1 = Part::new("existing", GeometryPayload::Mesh(mesh));
        let b_part2 = Part::new("new_part", GeometryPayload::Mesh(simple_mesh()));
        let b_part2_id = b_part2.id.clone();

        let a = make_doc(vec![a_part], Assembly::default());
        let b = make_doc(vec![b_part1, b_part2], Assembly::default());

        let report = diff(&a, &b);
        assert!(!report.topology.added.is_empty());
        assert!(report
            .topology
            .added
            .iter()
            .any(|s| s == &format!("part:{}", b_part2_id)));
    }

    #[test]
    fn moved_instance_rigid_body_transform_delta() {
        let mesh = simple_mesh();
        let part = Part::new("test", GeometryPayload::Mesh(mesh));

        let a_inst = Instance {
            part_ref: part.id.clone(),
            name: "inst1".into(),
            transform: Transform::identity(),
        };

        let mut b_inst = a_inst.clone();
        b_inst.transform = Transform([
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]);

        let a = make_doc(
            vec![part.clone()],
            Assembly {
                instances: vec![a_inst],
                ..Default::default()
            },
        );
        let b = make_doc(
            vec![part],
            Assembly {
                instances: vec![b_inst],
                ..Default::default()
            },
        );

        let report = diff(&a, &b);
        assert_eq!(report.transforms.len(), 1);
        assert_eq!(report.transforms[0].kind, "rigid_body");
        assert_eq!(report.transforms[0].part, "inst1");
        assert_eq!(report.transforms[0].translation, [10.0, 0.0, 0.0]);
    }

    #[test]
    fn brep_surface_change_modified_node() {
        let a_brep = BRep {
            faces: vec![BrepFace {
                id: "f1".into(),
                surface: SurfaceType::Plane,
                edges: vec![],
            }],
            ..Default::default()
        };
        let b_brep = BRep {
            faces: vec![BrepFace {
                id: "f1".into(),
                surface: SurfaceType::Cylinder,
                edges: vec![],
            }],
            ..Default::default()
        };

        let a_part = Part::new("brep_part", GeometryPayload::Brep(a_brep));
        let b_part = Part::new("brep_part", GeometryPayload::Brep(b_brep));

        let a = make_doc(vec![a_part], Assembly::default());
        let b = make_doc(vec![b_part], Assembly::default());

        let report = diff(&a, &b);
        assert!(report
            .topology
            .modified
            .iter()
            .any(|m| m.change == "surface_type" && m.id == "f1"));
    }
}
