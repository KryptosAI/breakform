pub mod units;

use exl_geom::{BRep, BoundingBox, Mesh, Transform};
use serde::{Deserialize, Serialize};
use units::Quantity;
use uuid::Uuid;

pub use exl_geom as geom;
pub use units::{Dimension, Unit};

pub const SCHEMA_VERSION: &str = "0.2";
pub const SUPPORTED_SCHEMA_VERSIONS: [&str; 2] = ["0.1", "0.2"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeometryPayload {
    Mesh(Mesh),
    Brep(BRep),
}

impl GeometryPayload {
    pub fn bounding_box(&self) -> BoundingBox {
        match self {
            GeometryPayload::Mesh(m) => m.bounding_box(),
            GeometryPayload::Brep(b) => b.bounding_box(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub density: Option<Quantity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elastic_modulus: Option<Quantity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poisson_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yield_strength: Option<Quantity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_conductivity: Option<Quantity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BcType {
    Pressure,
    FixedDisplacement,
    HeatFlux,
    Convection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundaryCondition {
    pub face_group: String,
    #[serde(rename = "type")]
    pub bc_type: BcType,
    pub value: Quantity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<[f64; 3]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tolerances {
    pub linear: Quantity,
    pub angular: Quantity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinateSystem {
    pub origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub z_axis: [f64; 3],
    pub length_unit: Unit,
}

impl Default for CoordinateSystem {
    fn default() -> Self {
        CoordinateSystem {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            z_axis: [0.0, 0.0, 1.0],
            length_unit: Unit::Millimeter,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Semantics {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<Material>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boundary_conditions: Vec<BoundaryCondition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerances: Option<Tolerances>,
    #[serde(default)]
    pub coordinate_system: CoordinateSystem,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub id: String,
    pub name: String,
    pub geometry: GeometryPayload,
    #[serde(default)]
    pub semantics: Semantics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounding_box: Option<BoundingBox>,
}

impl Part {
    pub fn new(name: impl Into<String>, geometry: GeometryPayload) -> Self {
        let bb = geometry.bounding_box();
        Part {
            id: new_uuid(),
            name: name.into(),
            geometry,
            semantics: Semantics::default(),
            bounding_box: Some(bb),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MateType {
    Fixed,
    Revolute,
    Prismatic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mate {
    #[serde(rename = "type")]
    pub mate_type: MateType,
    pub parts: [String; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub part_ref: String,
    pub name: String,
    #[serde(default)]
    pub transform: Transform,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Assembly {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<Instance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mates: Vec<Mate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    Lossless,
    Approximate,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOfOrigin {
    pub name: String,
    pub version: String,
    pub timestamp_iso: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub uuid: String,
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parent_hashes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_of_origin: Option<ToolOfOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversion_fidelity: Option<Fidelity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityStatus {
    Lossless,
    Approximate,
    Degraded,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityFidelity {
    pub entity: String,
    pub count: usize,
    pub status: EntityStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FidelityReport {
    pub source_format: String,
    pub target_format: String,
    pub overall: Fidelity,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<EntityFidelity>,
}

impl FidelityReport {
    pub fn new(source: impl Into<String>, target: impl Into<String>) -> Self {
        FidelityReport {
            source_format: source.into(),
            target_format: target.into(),
            overall: Fidelity::Lossless,
            entities: Vec::new(),
        }
    }

    pub fn record(
        &mut self,
        entity: impl Into<String>,
        count: usize,
        status: EntityStatus,
        note: Option<String>,
    ) {
        match status {
            EntityStatus::Dropped | EntityStatus::Degraded => self.overall = Fidelity::Degraded,
            EntityStatus::Approximate => {
                if self.overall == Fidelity::Lossless {
                    self.overall = Fidelity::Approximate;
                }
            }
            EntityStatus::Lossless => {}
        }
        self.entities.push(EntityFidelity {
            entity: entity.into(),
            count,
            status,
            note,
        });
    }
}

/// Top-level document: the unit of interchange.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: String,
    pub parts: Vec<Part>,
    #[serde(default)]
    pub assembly: Assembly,
    pub provenance: Provenance,
}

impl Document {
    pub fn new(parts: Vec<Part>) -> Self {
        let mut doc = Document {
            schema_version: SCHEMA_VERSION.to_string(),
            parts,
            assembly: Assembly::default(),
            provenance: Provenance {
                uuid: new_uuid(),
                content_hash: String::new(),
                parent_hashes: Vec::new(),
                tool_of_origin: None,
                conversion_fidelity: None,
            },
        };
        doc.provenance.content_hash = doc.compute_content_hash();
        doc
    }

    /// BLAKE3 over the canonical JSON of parts + assembly (geometry + semantics payload).
    pub fn compute_content_hash(&self) -> String {
        #[derive(Serialize)]
        struct Payload<'a> {
            parts: &'a Vec<Part>,
            assembly: &'a Assembly,
        }
        let bytes = serde_json::to_vec(&Payload {
            parts: &self.parts,
            assembly: &self.assembly,
        })
        .expect("serialization cannot fail");
        blake3::hash(&bytes).to_hex().to_string()
    }

    pub fn refresh_content_hash(&mut self) {
        self.provenance.content_hash = self.compute_content_hash();
    }
}

pub fn new_uuid() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use exl_geom::Mesh;

    fn doc() -> Document {
        let mesh = Mesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            faces: vec![[0, 1, 2]],
            ..Default::default()
        };
        Document::new(vec![Part::new("tri", GeometryPayload::Mesh(mesh))])
    }

    #[test]
    fn content_hash_stable() {
        let d = doc();
        assert_eq!(d.provenance.content_hash, d.compute_content_hash());
        assert_eq!(d.provenance.content_hash.len(), 64);
    }

    #[test]
    fn geometry_change_bumps_hash() {
        let mut d = doc();
        let before = d.provenance.content_hash.clone();
        if let GeometryPayload::Mesh(m) = &mut d.parts[0].geometry {
            m.vertices[0] = [9.0, 9.0, 9.0];
        }
        d.refresh_content_hash();
        assert_ne!(before, d.provenance.content_hash);
    }

    #[test]
    fn json_round_trip() {
        let d = doc();
        let s = serde_json::to_string(&d).unwrap();
        let back: Document = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }
}
