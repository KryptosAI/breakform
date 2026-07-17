use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SurfaceParams {
    Plane {
        origin: [f64; 3],
        normal: [f64; 3],
    },
    Cylinder {
        origin: [f64; 3],
        axis: [f64; 3],
        radius: f64,
    },
    Cone {
        origin: [f64; 3],
        axis: [f64; 3],
        radius: f64,
        half_angle: f64,
    },
    Sphere {
        center: [f64; 3],
        radius: f64,
    },
    Torus {
        center: [f64; 3],
        axis: [f64; 3],
        major_radius: f64,
        minor_radius: f64,
    },
    NurbsSurface {
        degree_u: usize,
        degree_v: usize,
        control_points: Vec<Vec<[f64; 3]>>,
        knots_u: Vec<f64>,
        knots_v: Vec<f64>,
        weights: Option<Vec<Vec<f64>>>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CurveParams {
    Line {
        point: [f64; 3],
        direction: [f64; 3],
    },
    Circle {
        center: [f64; 3],
        axis: [f64; 3],
        radius: f64,
    },
    Ellipse {
        center: [f64; 3],
        axis: [f64; 3],
        semi_major: f64,
        semi_minor: f64,
    },
    NurbsCurve {
        degree: usize,
        control_points: Vec<[f64; 3]>,
        knots: Vec<f64>,
        weights: Option<Vec<f64>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform(pub [[f64; 4]; 4]);

impl Transform {
    pub fn identity() -> Self {
        let mut m = [[0.0; 4]; 4];
        for (i, row) in m.iter_mut().enumerate() {
            row[i] = 1.0;
        }
        Transform(m)
    }

    pub fn translation(&self) -> [f64; 3] {
        [self.0[0][3], self.0[1][3], self.0[2][3]]
    }

    pub fn is_identity(&self, tol: f64) -> bool {
        let id = Transform::identity();
        self.approx_eq(&id, tol)
    }

    pub fn approx_eq(&self, other: &Transform, tol: f64) -> bool {
        self.0
            .iter()
            .flatten()
            .zip(other.0.iter().flatten())
            .all(|(a, b)| (a - b).abs() <= tol)
    }
}

impl Default for Transform {
    fn default() -> Self {
        Transform::identity()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Mesh {
    pub vertices: Vec<[f32; 3]>,
    pub faces: Vec<[u32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normals: Option<Vec<[f32; 3]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uvs: Option<Vec<[f32; 2]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_groups: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group_names: Vec<String>,
}

impl Mesh {
    pub fn bounding_box(&self) -> BoundingBox {
        if self.vertices.is_empty() {
            return BoundingBox::default();
        }
        let mut min = [f64::MAX; 3];
        let mut max = [f64::MIN; 3];
        for v in &self.vertices {
            for i in 0..3 {
                min[i] = min[i].min(v[i] as f64);
                max[i] = max[i].max(v[i] as f64);
            }
        }
        BoundingBox { min, max }
    }

    /// Set of edges that belong to exactly one face. Empty => watertight.
    pub fn boundary_edge_count(&self) -> usize {
        use std::collections::HashMap;
        let mut counts: HashMap<(u32, u32), usize> = HashMap::new();
        for f in &self.faces {
            for (a, b) in [(f[0], f[1]), (f[1], f[2]), (f[2], f[0])] {
                let key = if a < b { (a, b) } else { (b, a) };
                *counts.entry(key).or_insert(0) += 1;
            }
        }
        counts.values().filter(|&&c| c == 1).count()
    }

    pub fn is_watertight(&self) -> bool {
        !self.faces.is_empty() && self.boundary_edge_count() == 0
    }

    /// Every edge shared by at most two faces.
    pub fn is_manifold(&self) -> bool {
        use std::collections::HashMap;
        let mut counts: HashMap<(u32, u32), usize> = HashMap::new();
        for f in &self.faces {
            for (a, b) in [(f[0], f[1]), (f[1], f[2]), (f[2], f[0])] {
                let key = if a < b { (a, b) } else { (b, a) };
                *counts.entry(key).or_insert(0) += 1;
            }
        }
        counts.values().all(|&c| c <= 2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceType {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    Extrusion,
    Nurbs,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveType {
    Line,
    Circle,
    Ellipse,
    Nurbs,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrepVertex {
    pub id: String,
    pub point: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrepEdge {
    pub id: String,
    pub curve: CurveType,
    pub vertices: [String; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrepFace {
    pub id: String,
    pub surface: SurfaceType,
    pub edges: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BRep {
    pub vertices: Vec<BrepVertex>,
    pub edges: Vec<BrepEdge>,
    pub faces: Vec<BrepFace>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub surface_params: BTreeMap<String, SurfaceParams>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub curve_params: BTreeMap<String, CurveParams>,
}

impl BRep {
    pub fn bounding_box(&self) -> BoundingBox {
        if self.vertices.is_empty() {
            return BoundingBox::default();
        }
        let mut min = [f64::MAX; 3];
        let mut max = [f64::MIN; 3];
        for v in &self.vertices {
            for i in 0..3 {
                min[i] = min[i].min(v.point[i]);
                max[i] = max[i].max(v.point[i]);
            }
        }
        BoundingBox { min, max }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetra() -> Mesh {
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
    fn tetra_is_watertight_and_manifold() {
        let m = tetra();
        assert!(m.is_watertight());
        assert!(m.is_manifold());
    }

    #[test]
    fn open_mesh_is_not_watertight() {
        let mut m = tetra();
        m.faces.pop();
        assert!(!m.is_watertight());
    }

    #[test]
    fn bbox() {
        let bb = tetra().bounding_box();
        assert_eq!(bb.min, [0.0, 0.0, 0.0]);
        assert_eq!(bb.max, [1.0, 1.0, 1.0]);
    }
}
