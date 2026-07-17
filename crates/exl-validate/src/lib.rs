use exl_core::{Document, GeometryPayload};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Profile {
    Mech,
    Cfd,
    Fea,
    Strict,
}

impl FromStr for Profile {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mech" => Ok(Profile::Mech),
            "cfd" => Ok(Profile::Cfd),
            "fea" => Ok(Profile::Fea),
            "strict" => Ok(Profile::Strict),
            other => Err(format!("unknown profile: {}", other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub check: String,
    pub severity: Severity,
    pub message: String,
    pub part: Option<String>,
}

pub fn validate(doc: &Document, profile: Profile) -> Vec<Finding> {
    let mut findings = Vec::new();

    if doc.schema_version.is_empty() {
        findings.push(Finding {
            check: "schema_version".into(),
            severity: Severity::Error,
            message: "schema_version is missing".into(),
            part: None,
        });
    }

    let computed_hash = doc.compute_content_hash();
    if doc.provenance.content_hash != computed_hash {
        findings.push(Finding {
            check: "stale_content_hash".into(),
            severity: Severity::Error,
            message: format!(
                "provenance.content_hash ({}) does not match computed ({})",
                doc.provenance.content_hash, computed_hash
            ),
            part: None,
        });
    }

    let part_ids: HashSet<&str> = doc.parts.iter().map(|p| p.id.as_str()).collect();

    for inst in &doc.assembly.instances {
        if !part_ids.contains(inst.part_ref.as_str()) {
            findings.push(Finding {
                check: "dangling_instance_ref".into(),
                severity: Severity::Error,
                message: format!(
                    "instance '{}' references nonexistent part '{}'",
                    inst.name, inst.part_ref
                ),
                part: Some(inst.part_ref.clone()),
            });
        }
    }

    for part in &doc.parts {
        if part.name.is_empty() {
            findings.push(Finding {
                check: "empty_name".into(),
                severity: Severity::Error,
                message: "part has empty name".into(),
                part: Some(part.id.clone()),
            });
        }

        match &part.geometry {
            GeometryPayload::Mesh(mesh) => {
                validate_mesh(mesh, part, profile, &mut findings);
            }
            GeometryPayload::Brep(brep) => {
                if let Some(ref stored_bb) = part.bounding_box {
                    let computed_bb = brep.bounding_box();
                    let tol = 1e-6;
                    if !bbox_eq(stored_bb, &computed_bb, tol) {
                        findings.push(Finding {
                            check: "stale_bounding_box".into(),
                            severity: Severity::Warning,
                            message: "bounding_box does not match computed".into(),
                            part: Some(part.id.clone()),
                        });
                    }
                }
            }
        }

        if matches!(profile, Profile::Fea | Profile::Strict) {
            if part.semantics.materials.is_empty() {
                findings.push(Finding {
                    check: "missing_material".into(),
                    severity: Severity::Warning,
                    message: "part has no materials assigned".into(),
                    part: Some(part.id.clone()),
                });
            }
        }

        if profile == Profile::Strict {
            if part.semantics.tolerances.is_none() {
                findings.push(Finding {
                    check: "missing_tolerances".into(),
                    severity: Severity::Warning,
                    message: "tolerances not specified".into(),
                    part: Some(part.id.clone()),
                });
            }
        }
    }

    findings
}

fn validate_mesh(
    mesh: &exl_core::geom::Mesh,
    part: &exl_core::Part,
    profile: Profile,
    findings: &mut Vec<Finding>,
) {
    if mesh.vertices.is_empty() || mesh.faces.is_empty() {
        findings.push(Finding {
            check: "empty_mesh".into(),
            severity: Severity::Error,
            message: "mesh has no vertices or faces".into(),
            part: Some(part.id.clone()),
        });
        return;
    }

    let vertex_count = mesh.vertices.len() as u32;
    for (fi, face) in mesh.faces.iter().enumerate() {
        for &idx in &[face[0], face[1], face[2]] {
            if idx >= vertex_count {
                findings.push(Finding {
                    check: "index_out_of_range".into(),
                    severity: Severity::Error,
                    message: format!(
                        "face {} references vertex index {} but only {} vertices exist",
                        fi, idx, vertex_count
                    ),
                    part: Some(part.id.clone()),
                });
            }
        }
    }

    if !mesh.is_manifold() {
        findings.push(Finding {
            check: "non_manifold".into(),
            severity: Severity::Error,
            message: "mesh is non-manifold".into(),
            part: Some(part.id.clone()),
        });
    }

    if !mesh.is_watertight() {
        let severity = match profile {
            Profile::Mech => Severity::Warning,
            Profile::Cfd | Profile::Fea | Profile::Strict => Severity::Error,
        };
        findings.push(Finding {
            check: "not_watertight".into(),
            severity,
            message: "mesh is not watertight".into(),
            part: Some(part.id.clone()),
        });
    }

    if let Some(ref stored_bb) = part.bounding_box {
        let computed_bb = mesh.bounding_box();
        let tol = 1e-6;
        if !bbox_eq(stored_bb, &computed_bb, tol) {
            findings.push(Finding {
                check: "stale_bounding_box".into(),
                severity: Severity::Warning,
                message: "bounding_box does not match computed".into(),
                part: Some(part.id.clone()),
            });
        }
    }

    if matches!(profile, Profile::Cfd | Profile::Fea | Profile::Strict) {
        let group_names: HashSet<&str> = mesh.group_names.iter().map(|s| s.as_str()).collect();
        for bc in &part.semantics.boundary_conditions {
            if !group_names.contains(bc.face_group.as_str()) {
                findings.push(Finding {
                    check: "unknown_face_group".into(),
                    severity: Severity::Error,
                    message: format!(
                        "boundary condition references unknown face_group '{}'",
                        bc.face_group
                    ),
                    part: Some(part.id.clone()),
                });
            }
        }
    }
}

fn bbox_eq(a: &exl_core::geom::BoundingBox, b: &exl_core::geom::BoundingBox, tol: f64) -> bool {
    (a.min[0] - b.min[0]).abs() < tol
        && (a.min[1] - b.min[1]).abs() < tol
        && (a.min[2] - b.min[2]).abs() < tol
        && (a.max[0] - b.max[0]).abs() < tol
        && (a.max[1] - b.max[1]).abs() < tol
        && (a.max[2] - b.max[2]).abs() < tol
}

#[cfg(test)]
mod tests {
    use super::*;
    use exl_core::geom::Mesh;
    use exl_core::units::{Quantity, Unit};
    use exl_core::{BcType, BoundaryCondition, Document, GeometryPayload, Material, Part};

    fn make_doc(parts: Vec<Part>) -> Document {
        Document::new(parts)
    }

    fn tetra_mesh() -> Mesh {
        Mesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            faces: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            ..Default::default()
        }
    }

    #[test]
    fn clean_watertight_passes_mech() {
        let mesh = tetra_mesh();
        let part = Part::new("test", GeometryPayload::Mesh(mesh));
        let doc = make_doc(vec![part]);
        let findings = validate(&doc, Profile::Mech);
        assert!(findings.is_empty());
    }

    #[test]
    fn open_mesh_errors_cfd_warns_mech() {
        let mut mesh = tetra_mesh();
        mesh.faces.pop();

        let part = Part::new("test", GeometryPayload::Mesh(mesh));
        let doc = make_doc(vec![part]);

        let cfd_findings = validate(&doc, Profile::Cfd);
        let mech_findings = validate(&doc, Profile::Mech);

        assert!(cfd_findings
            .iter()
            .any(|f| f.check == "not_watertight" && f.severity == Severity::Error));
        assert!(mech_findings
            .iter()
            .any(|f| f.check == "not_watertight" && f.severity == Severity::Warning));
    }

    #[test]
    fn stale_hash_detected() {
        let mesh = tetra_mesh();
        let part = Part::new("test", GeometryPayload::Mesh(mesh));
        let mut doc = make_doc(vec![part]);
        doc.provenance.content_hash = "deadbeef".into();

        let findings = validate(&doc, Profile::Mech);
        assert!(findings.iter().any(|f| f.check == "stale_content_hash"));
    }

    #[test]
    fn dangling_instance_ref_detected() {
        let mesh = tetra_mesh();
        let part = Part::new("test", GeometryPayload::Mesh(mesh));
        let mut doc = make_doc(vec![part]);
        doc.assembly.instances = vec![exl_core::Instance {
            part_ref: "nonexistent".into(),
            name: "ghost".into(),
            transform: Default::default(),
        }];

        let findings = validate(&doc, Profile::Mech);
        assert!(findings.iter().any(|f| f.check == "dangling_instance_ref"));
    }

    #[test]
    fn profile_from_str() {
        assert_eq!("mech".parse::<Profile>().unwrap(), Profile::Mech);
        assert_eq!("cfd".parse::<Profile>().unwrap(), Profile::Cfd);
        assert_eq!("fea".parse::<Profile>().unwrap(), Profile::Fea);
        assert_eq!("strict".parse::<Profile>().unwrap(), Profile::Strict);
        assert_eq!("MECH".parse::<Profile>().unwrap(), Profile::Mech);
        assert!("unknown".parse::<Profile>().is_err());
    }

    #[test]
    fn missing_material_fea_warns() {
        let mesh = tetra_mesh();
        let part = Part::new("test", GeometryPayload::Mesh(mesh));
        let doc = make_doc(vec![part]);

        let findings = validate(&doc, Profile::Fea);
        assert!(findings.iter().any(|f| f.check == "missing_material"));

        let mech_findings = validate(&doc, Profile::Mech);
        assert!(!mech_findings.iter().any(|f| f.check == "missing_material"));
    }

    #[test]
    fn unknown_face_group_detected() {
        let mesh = tetra_mesh();
        let mut part = Part::new("test", GeometryPayload::Mesh(mesh));
        part.semantics.boundary_conditions = vec![BoundaryCondition {
            face_group: "nonexistent_group".into(),
            bc_type: BcType::Pressure,
            value: Quantity::new(101325.0, Unit::Pascal),
            direction: None,
        }];
        let doc = make_doc(vec![part]);

        let findings = validate(&doc, Profile::Fea);
        assert!(findings.iter().any(|f| f.check == "unknown_face_group"));
    }

    #[test]
    fn missing_tolerances_strict_warns() {
        let mesh = tetra_mesh();
        let mut part = Part::new("test", GeometryPayload::Mesh(mesh));
        part.semantics.materials = vec![Material {
            name: "steel".into(),
            ..Default::default()
        }];
        let doc = make_doc(vec![part]);

        let strict_findings = validate(&doc, Profile::Strict);
        assert!(strict_findings
            .iter()
            .any(|f| f.check == "missing_tolerances"));

        let mech_findings = validate(&doc, Profile::Mech);
        assert!(!mech_findings
            .iter()
            .any(|f| f.check == "missing_tolerances"));
    }

    #[test]
    fn empty_name_error() {
        let mesh = tetra_mesh();
        let mut part = Part::new("test", GeometryPayload::Mesh(mesh));
        part.name.clear();
        let doc = make_doc(vec![part]);

        let findings = validate(&doc, Profile::Mech);
        assert!(findings.iter().any(|f| f.check == "empty_name"));
    }

    #[test]
    fn index_out_of_range_detected() {
        let mesh = Mesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            faces: vec![[0, 1, 99]],
            ..Default::default()
        };
        let part = Part::new("test", GeometryPayload::Mesh(mesh));
        let doc = make_doc(vec![part]);

        let findings = validate(&doc, Profile::Mech);
        assert!(findings.iter().any(|f| f.check == "index_out_of_range"));
    }
}
