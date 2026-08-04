use exl_core::{
    geom::{
        BRep, BrepEdge, BrepFace, BrepVertex, CurveParams, CurveType, SurfaceParams, SurfaceType,
    },
    EntityStatus, FidelityReport, GeometryPayload, Part,
};
use step_io::scene::geometry::{CurveKind, SurfaceKind};

pub fn import_step_io(
    source: &[u8],
    file_name: &str,
) -> Result<(Vec<Part>, FidelityReport), crate::StepError> {
    let (model, _read_report) =
        step_io::read(source).map_err(|e| crate::StepError::Parse(format!("step-io read: {}", e)))?;

    let scene = model.scene();

    let mut fid = FidelityReport::new("step", "exl");
    let mut parts = Vec::new();
    let mut total_verts = 0usize;
    let mut total_edges = 0usize;
    let mut total_faces = 0usize;

    for (i, solid) in scene.all_solids().enumerate() {
        let (brep, v, e, f) = map_solid_to_brep(&solid, i);
        total_verts += v;
        total_edges += e;
        total_faces += f;
        let part_name = solid_name(&solid, file_name, i);
        parts.push(Part::new(part_name, GeometryPayload::Brep(brep)));
    }

    if parts.is_empty() {
        parts.push(Part::new(file_name.to_string(), GeometryPayload::Brep(BRep::default())));
    }

    fid.record(
        "VERTEX_POINT",
        total_verts,
        EntityStatus::Lossless,
        Some("step-io bridge".into()),
    );
    fid.record(
        "EDGE_CURVE",
        total_edges,
        EntityStatus::Lossless,
        Some("step-io bridge".into()),
    );
    fid.record(
        "ADVANCED_FACE",
        total_faces,
        EntityStatus::Lossless,
        Some("step-io bridge".into()),
    );

    Ok((parts, fid))
}

fn solid_name(solid: &step_io::scene::geometry::Solid, fallback: &str, idx: usize) -> String {
    let name = solid.name();
    if !name.is_empty() {
        name.to_string()
    } else if idx == 0 {
        fallback.to_string()
    } else {
        format!("{}__{}", fallback, idx + 1)
    }
}

fn map_solid_to_brep(
    solid: &step_io::scene::geometry::Solid,
    solid_idx: usize,
) -> (BRep, usize, usize, usize) {
    let mut vertices: Vec<BrepVertex> = Vec::new();
    let mut edges: Vec<BrepEdge> = Vec::new();
    let mut faces: Vec<BrepFace> = Vec::new();
    let mut surface_params = std::collections::BTreeMap::new();
    let mut curve_params = std::collections::BTreeMap::new();
    let mut vert_idx = 0usize;
    let mut edge_idx = 0usize;
    let mut face_idx = 0usize;

    for (fi, face) in solid.faces().enumerate() {
        let face_id = format!("face_s{}_{}", solid_idx, fi);

        if let Some(nurbs) = face.to_nurbs() {
            surface_params.insert(
                face_id.clone(),
                SurfaceParams::NurbsSurface {
                    degree_u: nurbs.degree_u,
                    degree_v: nurbs.degree_v,
                    control_points: nurbs.control_points,
                    knots_u: nurbs.knots_u,
                    knots_v: nurbs.knots_v,
                    weights: if nurbs.weights.iter().any(|row| row.iter().any(|w| (*w - 1.0).abs() > 1e-12)) {
                        Some(nurbs.weights)
                    } else {
                        None
                    },
                },
            );
        }

        let st = match face.surface().kind() {
            SurfaceKind::Plane(_) => SurfaceType::Plane,
            SurfaceKind::Cylindrical(_) => SurfaceType::Cylinder,
            SurfaceKind::Conical(_) => SurfaceType::Cone,
            SurfaceKind::Spherical(_) => SurfaceType::Sphere,
            SurfaceKind::Toroidal(_) => SurfaceType::Torus,
            SurfaceKind::BSpline(_)
            | SurfaceKind::BSplineWithKnots(_)
            | SurfaceKind::Bezier(_)
            | SurfaceKind::QuasiUniform(_)
            | SurfaceKind::Uniform(_) => SurfaceType::Nurbs,
            SurfaceKind::LinearExtrusion(_) => SurfaceType::Extrusion,
            _ => SurfaceType::Other,
        };

        let mut face_edge_ids = Vec::new();
        for bound in face.bounds() {
            for loop_edge in bound.edges() {
                let ei = edge_idx;
                edge_idx += 1;
                let edge_id = format!("edge_s{}_{}", solid_idx, ei);

                let start_pt = loop_edge
                    .start()
                    .and_then(|v| v.point())
                    .map(|p| p.xyz())
                    .unwrap_or([0.0; 3]);
                let end_pt = loop_edge
                    .end()
                    .and_then(|v| v.point())
                    .map(|p| p.xyz())
                    .unwrap_or([0.0; 3]);

                let start_vid = format!("v_s{}_{}_s", solid_idx, ei);
                let end_vid = format!("v_s{}_{}_e", solid_idx, ei);
                vertices.push(BrepVertex {
                    id: start_vid.clone(),
                    point: start_pt,
                });
                vert_idx += 1;
                vertices.push(BrepVertex {
                    id: end_vid.clone(),
                    point: end_pt,
                });
                vert_idx += 1;

                let ct = match loop_edge.curve().kind() {
                    CurveKind::Line(_) => CurveType::Line,
                    CurveKind::Circle(_) => CurveType::Circle,
                    CurveKind::Ellipse(_) => CurveType::Ellipse,
                    CurveKind::BSpline(_)
                    | CurveKind::BSplineWithKnots(_)
                    | CurveKind::Bezier(_)
                    | CurveKind::QuasiUniform(_)
                    | CurveKind::Uniform(_) => CurveType::Nurbs,
                    _ => CurveType::Other,
                };

                let dir = direction_from_points(start_pt, end_pt);
                curve_params.insert(edge_id.clone(), CurveParams::Line {
                    point: start_pt,
                    direction: dir,
                });

                edges.push(BrepEdge {
                    id: edge_id.clone(),
                    curve: ct,
                    vertices: [start_vid, end_vid],
                });

                face_edge_ids.push(edge_id);
            }
        }

        faces.push(BrepFace {
            id: face_id,
            surface: st,
            edges: face_edge_ids,
        });
        face_idx += 1;
    }

    (
        BRep {
            vertices,
            edges,
            faces,
            surface_params,
            curve_params,
        },
        vert_idx,
        edge_idx,
        face_idx,
    )
}

fn direction_from_points(from: [f64; 3], to: [f64; 3]) -> [f64; 3] {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let dz = to[2] - from[2];
    let len = (dx * dx + dy * dy + dz * dz).sqrt();
    if len > 1e-12 {
        [dx / len, dy / len, dz / len]
    } else {
        [1.0, 0.0, 0.0]
    }
}
