use exl_core::geom::{Mesh, Transform};
use exl_core::{
    Assembly, Document, EntityStatus, FidelityReport, GeometryPayload, Instance, Part, ToolOfOrigin,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum GltfError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GltfRoot {
    #[serde(default)]
    accessors: Vec<GltfAccessor>,
    #[serde(default)]
    buffer_views: Vec<GltfBufferView>,
    #[serde(default)]
    buffers: Vec<GltfBuffer>,
    #[serde(default)]
    meshes: Vec<GltfMesh>,
    #[serde(default)]
    nodes: Vec<GltfNode>,
    #[serde(default)]
    scenes: Vec<GltfScene>,
    #[serde(default)]
    scene: Option<u32>,
    #[serde(default)]
    materials: Vec<serde_json::Value>,
    #[serde(default)]
    textures: Vec<serde_json::Value>,
    #[serde(default)]
    images: Vec<serde_json::Value>,
    #[serde(default)]
    animations: Vec<serde_json::Value>,
    #[serde(default)]
    skins: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GltfAccessor {
    #[serde(default)]
    buffer_view: Option<u32>,
    #[serde(default)]
    byte_offset: u32,
    component_type: u32,
    count: u32,
    #[serde(rename = "type")]
    accessor_type: String,
    #[serde(default)]
    min: Option<Vec<f32>>,
    #[serde(default)]
    max: Option<Vec<f32>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GltfBufferView {
    buffer: u32,
    #[serde(default)]
    byte_offset: u32,
    byte_length: u32,
    #[serde(default)]
    byte_stride: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GltfBuffer {
    byte_length: u32,
    #[serde(default)]
    uri: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GltfMesh {
    #[serde(default)]
    name: Option<String>,
    primitives: Vec<GltfPrimitive>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GltfPrimitive {
    attributes: HashMap<String, u32>,
    #[serde(default)]
    indices: Option<u32>,
    #[serde(default)]
    material: Option<u32>,
    #[serde(default = "default_mode")]
    mode: u32,
}

fn default_mode() -> u32 {
    4
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GltfNode {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mesh: Option<u32>,
    #[serde(default)]
    children: Option<Vec<u32>>,
    #[serde(default)]
    matrix: Option<[f64; 16]>,
    #[serde(default)]
    translation: Option<[f64; 3]>,
    #[serde(default)]
    rotation: Option<[f64; 4]>,
    #[serde(default)]
    scale: Option<[f64; 3]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GltfScene {
    #[serde(default)]
    nodes: Vec<u32>,
}

fn read_le_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

fn read_le_f32(data: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

fn read_le_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

fn read_accessor_vec3(
    bin: &[u8],
    bv: &GltfBufferView,
    acc: &GltfAccessor,
) -> Result<Vec<[f32; 3]>, GltfError> {
    if acc.component_type != 5126 || acc.accessor_type != "VEC3" {
        return Err(GltfError::Parse(format!(
            "expected VEC3/FLOAT accessor, got {}/{}",
            acc.accessor_type, acc.component_type
        )));
    }
    let stride = bv.byte_stride.unwrap_or(12) as usize;
    let base = bv.byte_offset as usize + acc.byte_offset as usize;
    let end = base + stride * acc.count as usize;
    if end > bin.len() {
        return Err(GltfError::Parse("accessor data exceeds buffer bounds".into()));
    }
    let mut out = Vec::with_capacity(acc.count as usize);
    for i in 0..acc.count as usize {
        let off = base + i * stride;
        out.push([
            read_le_f32(bin, off),
            read_le_f32(bin, off + 4),
            read_le_f32(bin, off + 8),
        ]);
    }
    Ok(out)
}

fn read_accessor_vec2(
    bin: &[u8],
    bv: &GltfBufferView,
    acc: &GltfAccessor,
) -> Result<Vec<[f32; 2]>, GltfError> {
    if acc.component_type != 5126 || acc.accessor_type != "VEC2" {
        return Err(GltfError::Parse(format!(
            "expected VEC2/FLOAT accessor, got {}/{}",
            acc.accessor_type, acc.component_type
        )));
    }
    let stride = bv.byte_stride.unwrap_or(8) as usize;
    let base = bv.byte_offset as usize + acc.byte_offset as usize;
    let end = base + stride * acc.count as usize;
    if end > bin.len() {
        return Err(GltfError::Parse("accessor data exceeds buffer bounds".into()));
    }
    let mut out = Vec::with_capacity(acc.count as usize);
    for i in 0..acc.count as usize {
        let off = base + i * stride;
        out.push([read_le_f32(bin, off), read_le_f32(bin, off + 4)]);
    }
    Ok(out)
}

fn component_size(ct: u32) -> Option<usize> {
    Some(match ct {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        5125 => 4,
        5126 => 4,
        _ => return None,
    })
}

fn read_accessor_indices(
    bin: &[u8],
    bv: &GltfBufferView,
    acc: &GltfAccessor,
) -> Result<Vec<u32>, GltfError> {
    if acc.accessor_type != "SCALAR" {
        return Err(GltfError::Parse(format!(
            "expected SCALAR accessor for indices, got {}",
            acc.accessor_type
        )));
    }
    let cs = component_size(acc.component_type).ok_or_else(|| {
        GltfError::Parse(format!(
            "unsupported index component_type {}",
            acc.component_type
        ))
    })?;
    let stride = bv.byte_stride.unwrap_or(cs as u32) as usize;
    let base = bv.byte_offset as usize + acc.byte_offset as usize;
    let end = base + stride * acc.count as usize;
    if end > bin.len() {
        return Err(GltfError::Parse("index accessor data exceeds buffer bounds".into()));
    }
    let mut out = Vec::with_capacity(acc.count as usize);
    for i in 0..acc.count as usize {
        let off = base + i * stride;
        let val: u32 = match acc.component_type {
            5121 => bin[off] as u32,
            5123 => read_le_u16(bin, off) as u32,
            5125 => read_le_u32(bin, off),
            _ => {
                return Err(GltfError::Parse(format!(
                    "unsupported index component_type {}",
                    acc.component_type
                )))
            }
        };
        out.push(val);
    }
    Ok(out)
}

fn quat_to_rot_matrix(qx: f64, qy: f64, qz: f64, qw: f64) -> [[f64; 3]; 3] {
    let r00 = 1.0 - 2.0 * qy * qy - 2.0 * qz * qz;
    let r01 = 2.0 * qx * qy - 2.0 * qz * qw;
    let r02 = 2.0 * qx * qz + 2.0 * qy * qw;
    let r10 = 2.0 * qx * qy + 2.0 * qz * qw;
    let r11 = 1.0 - 2.0 * qx * qx - 2.0 * qz * qz;
    let r12 = 2.0 * qy * qz - 2.0 * qx * qw;
    let r20 = 2.0 * qx * qz - 2.0 * qy * qw;
    let r21 = 2.0 * qy * qz + 2.0 * qx * qw;
    let r22 = 1.0 - 2.0 * qx * qx - 2.0 * qy * qy;
    [[r00, r01, r02], [r10, r11, r12], [r20, r21, r22]]
}

fn decode_node_transform(node: &GltfNode) -> Transform {
    if let Some(ref matrix) = node.matrix {
        let mut t = [[0.0f64; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                t[r][c] = matrix[c * 4 + r];
            }
        }
        return Transform(t);
    }

    let trans = node.translation.unwrap_or([0.0f64; 3]);
    let rot = node.rotation.unwrap_or([0.0f64, 0.0, 0.0, 1.0]);
    let scl = node.scale.unwrap_or([1.0f64; 3]);

    let r = quat_to_rot_matrix(rot[0], rot[1], rot[2], rot[3]);

    let mut m = [[0.0f64; 4]; 4];
    m[0][0] = r[0][0] * scl[0];
    m[0][1] = r[0][1] * scl[1];
    m[0][2] = r[0][2] * scl[2];
    m[1][0] = r[1][0] * scl[0];
    m[1][1] = r[1][1] * scl[1];
    m[1][2] = r[1][2] * scl[2];
    m[2][0] = r[2][0] * scl[0];
    m[2][1] = r[2][1] * scl[1];
    m[2][2] = r[2][2] * scl[2];
    m[0][3] = trans[0];
    m[1][3] = trans[1];
    m[2][3] = trans[2];
    m[3][3] = 1.0;

    Transform(m)
}

fn timestamp_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let days = (secs / 86400) as i64;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        y, m, d, hours, minutes, seconds
    )
}

pub fn import_gltf(path: &Path) -> Result<(Document, FidelityReport), GltfError> {
    let data = std::fs::read(path)?;

    if data.len() < 20 {
        return Err(GltfError::Unsupported(
            "file too small; only GLB binary format is supported".into(),
        ));
    }

    let magic = read_le_u32(&data, 0);
    if magic != 0x46546C67 {
        return Err(GltfError::Unsupported(
            "not a valid GLB file; only GLB binary format is supported".into(),
        ));
    }

    let version = read_le_u32(&data, 4);
    if version != 2 {
        return Err(GltfError::Parse(format!(
            "unsupported glTF version {}",
            version
        )));
    }

    let total_len = read_le_u32(&data, 8) as usize;

    let mut json_bytes_opt: Option<Vec<u8>> = None;
    let mut bin_bytes_opt: Option<Vec<u8>> = None;

    let mut offset = 12usize;
    while offset + 8 <= data.len().min(offset + total_len.saturating_sub(offset)) {
        let chunk_len = read_le_u32(&data, offset) as usize;
        let chunk_type = read_le_u32(&data, offset + 4);
        offset += 8;
        let chunk_end = offset.saturating_add(chunk_len);
        if chunk_end > data.len() {
            break;
        }
        match chunk_type {
            0x4E4F534A => json_bytes_opt = Some(data[offset..chunk_end].to_vec()),
            0x004E4942 => bin_bytes_opt = Some(data[offset..chunk_end].to_vec()),
            _ => {}
        }
        offset = chunk_end;
        if offset % 4 != 0 {
            offset += 4 - offset % 4;
        }
    }

    let json_bytes =
        json_bytes_opt.ok_or_else(|| GltfError::Parse("missing JSON chunk in GLB".into()))?;
    let bin_bytes = bin_bytes_opt.unwrap_or_default();

    let gltf: GltfRoot = serde_json::from_slice(&json_bytes)?;

    for buf in &gltf.buffers {
        if buf.uri.is_some() {
            return Err(GltfError::Unsupported(
                "external buffer URIs not supported; only self-contained GLB is supported".into(),
            ));
        }
    }

    let mut fid = FidelityReport::new("glTF 2.0 (GLB)", "exl");

    let mat_count = gltf.materials.len();
    let tex_count = gltf.textures.len();
    let img_count = gltf.images.len();
    let anim_count = gltf.animations.len();
    let skin_count = gltf.skins.len();

    if mat_count > 0 {
        fid.record(
            "materials",
            mat_count,
            EntityStatus::Dropped,
            Some("material support not implemented".into()),
        );
    }
    if tex_count > 0 {
        fid.record(
            "textures",
            tex_count,
            EntityStatus::Dropped,
            Some("texture support not implemented".into()),
        );
    }
    if img_count > 0 {
        fid.record(
            "images",
            img_count,
            EntityStatus::Dropped,
            Some("image support not implemented".into()),
        );
    }
    if anim_count > 0 {
        fid.record(
            "animations",
            anim_count,
            EntityStatus::Dropped,
            Some("animation support not implemented".into()),
        );
    }
    if skin_count > 0 {
        fid.record(
            "skins",
            skin_count,
            EntityStatus::Dropped,
            Some("skin support not implemented".into()),
        );
    }

    let mut parts: Vec<Part> = Vec::new();
    let mut part_ids: Vec<String> = Vec::new();
    let mut mesh_prim_indices: Vec<Vec<usize>> = Vec::new();

    let mut total_vertices: usize = 0;
    let mut total_faces: usize = 0;
    let mut total_normals: usize = 0;
    let mut total_uvs: usize = 0;
    let mut mesh_count_imported: usize = 0;

    for (mi, mesh) in gltf.meshes.iter().enumerate() {
        let mut prim_indices: Vec<usize> = Vec::new();
        for (pi, prim) in mesh.primitives.iter().enumerate() {
            if prim.mode != 4 {
                continue;
            }

            let pos_acc_idx = prim
                .attributes
                .get("POSITION")
                .ok_or_else(|| GltfError::Parse("primitive missing POSITION attribute".into()))?;
            let pos_acc = gltf
                .accessors
                .get(*pos_acc_idx as usize)
                .ok_or_else(|| GltfError::Parse("invalid POSITION accessor index".into()))?;
            let pos_bv_idx = pos_acc
                .buffer_view
                .ok_or_else(|| GltfError::Parse("POSITION accessor missing bufferView".into()))?;
            let pos_bv = gltf
                .buffer_views
                .get(pos_bv_idx as usize)
                .ok_or_else(|| GltfError::Parse("invalid POSITION bufferView index".into()))?;

            let vertices = read_accessor_vec3(&bin_bytes, pos_bv, pos_acc)?;
            total_vertices += vertices.len();

            let faces: Vec<[u32; 3]> = if let Some(idx_acc_idx) = prim.indices {
                let idx_acc = gltf
                    .accessors
                    .get(idx_acc_idx as usize)
                    .ok_or_else(|| GltfError::Parse("invalid indices accessor index".into()))?;
                let idx_bv_idx = idx_acc
                    .buffer_view
                    .ok_or_else(|| GltfError::Parse("indices accessor missing bufferView".into()))?;
                let idx_bv = gltf
                    .buffer_views
                    .get(idx_bv_idx as usize)
                    .ok_or_else(|| GltfError::Parse("invalid indices bufferView index".into()))?;
                let indices = read_accessor_indices(&bin_bytes, idx_bv, idx_acc)?;
                indices
                    .chunks(3)
                    .map(|c| [c[0], c[1], c[2]])
                    .collect()
            } else {
                let vc = vertices.len() as u32;
                (0..vc / 3)
                    .map(|i| [i * 3, i * 3 + 1, i * 3 + 2])
                    .collect()
            };
            total_faces += faces.len();

            let normals = if let Some(normal_acc_idx) = prim.attributes.get("NORMAL") {
                let acc = gltf
                    .accessors
                    .get(*normal_acc_idx as usize)
                    .ok_or_else(|| GltfError::Parse("invalid NORMAL accessor index".into()))?;
                let bv_idx = acc
                    .buffer_view
                    .ok_or_else(|| GltfError::Parse("NORMAL accessor missing bufferView".into()))?;
                let bv = gltf
                    .buffer_views
                    .get(bv_idx as usize)
                    .ok_or_else(|| GltfError::Parse("invalid NORMAL bufferView index".into()))?;
                let n = read_accessor_vec3(&bin_bytes, bv, acc)?;
                total_normals += n.len();
                Some(n)
            } else {
                None
            };

            let uvs = if let Some(uv_acc_idx) = prim.attributes.get("TEXCOORD_0") {
                let acc = gltf
                    .accessors
                    .get(*uv_acc_idx as usize)
                    .ok_or_else(|| GltfError::Parse("invalid TEXCOORD_0 accessor index".into()))?;
                let bv_idx = acc
                    .buffer_view
                    .ok_or_else(|| {
                        GltfError::Parse("TEXCOORD_0 accessor missing bufferView".into())
                    })?;
                let bv = gltf
                    .buffer_views
                    .get(bv_idx as usize)
                    .ok_or_else(|| {
                        GltfError::Parse("invalid TEXCOORD_0 bufferView index".into())
                    })?;
                let u = read_accessor_vec2(&bin_bytes, bv, acc)?;
                total_uvs += u.len();
                Some(u)
            } else {
                None
            };

            let part_name = mesh
                .name
                .as_deref()
                .unwrap_or(&format!("mesh_{}", mi))
                .to_string()
                + &format!("_p{}", pi);

            let m = Mesh {
                vertices,
                faces,
                normals,
                uvs,
                face_groups: None,
                group_names: vec![],
            };
            let part = Part::new(&part_name, GeometryPayload::Mesh(m));
            part_ids.push(part.id.clone());
            parts.push(part);
            prim_indices.push(parts.len() - 1);
            mesh_count_imported += 1;
        }
        mesh_prim_indices.push(prim_indices);
    }

    fid.record("meshes", mesh_count_imported, EntityStatus::Lossless, None);
    fid.record(
        "positions",
        total_vertices,
        EntityStatus::Lossless,
        None,
    );
    fid.record(
        "indices",
        total_faces * 3,
        EntityStatus::Lossless,
        None,
    );
    if total_normals > 0 {
        fid.record("normals", total_normals, EntityStatus::Lossless, None);
    }
    if total_uvs > 0 {
        fid.record("uvs", total_uvs, EntityStatus::Lossless, None);
    }

    let mut instances: Vec<Instance> = Vec::new();
    let mut node_transform_count: usize = 0;
    for node in &gltf.nodes {
        if let Some(mesh_idx) = node.mesh {
            let transform = decode_node_transform(node);
            if !transform.is_identity(1e-12) {
                node_transform_count += 1;
            }
            let iname = node
                .name
                .clone()
                .unwrap_or_else(|| format!("node_m{}", mesh_idx));
            if let Some(pi_list) = mesh_prim_indices.get(mesh_idx as usize) {
                for &pi in pi_list {
                    instances.push(Instance {
                        part_ref: part_ids[pi].clone(),
                        name: iname.clone(),
                        transform,
                    });
                }
            }
        }
    }

    if node_transform_count > 0 {
        fid.record(
            "node_transforms",
            node_transform_count,
            EntityStatus::Lossless,
            None,
        );
    }

    let mut doc = Document::new(parts);
    doc.assembly = Assembly {
        instances,
        mates: vec![],
    };
    doc.provenance.tool_of_origin = Some(ToolOfOrigin {
        name: "exl-gltf".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        timestamp_iso: timestamp_iso(),
    });
    doc.refresh_content_hash();

    Ok((doc, fid))
}

pub fn export_gltf(doc: &Document, path: &Path) -> Result<FidelityReport, GltfError> {
    let mut fid = FidelityReport::new("exl", "glTF 2.0 (GLB)");
    let mut buffer_data: Vec<u8> = Vec::new();
    let mut accessors: Vec<serde_json::Value> = Vec::new();
    let mut buffer_views: Vec<serde_json::Value> = Vec::new();
    let mut meshes: Vec<serde_json::Value> = Vec::new();
    let mut nodes: Vec<serde_json::Value> = Vec::new();

    let mut mesh_idx_counter: u32 = 0;
    let mut bv_counter: u32 = 0;
    let mut acc_counter: u32 = 0;

    let part_transform: HashMap<&str, &Transform> = doc
        .assembly
        .instances
        .iter()
        .map(|inst| (inst.part_ref.as_str(), &inst.transform))
        .collect();

    for part in &doc.parts {
        match &part.geometry {
            GeometryPayload::Mesh(mesh) => {
                let pos_start = buffer_data.len() as u32;
                for v in &mesh.vertices {
                    buffer_data.extend_from_slice(&v[0].to_le_bytes());
                    buffer_data.extend_from_slice(&v[1].to_le_bytes());
                    buffer_data.extend_from_slice(&v[2].to_le_bytes());
                }
                let pos_len = (buffer_data.len() as u32) - pos_start;
                let vc = mesh.vertices.len() as u32;
                let (pos_min, pos_max) = compute_min_max_vec3(&mesh.vertices);

                buffer_views.push(serde_json::json!({
                    "buffer": 0,
                    "byteOffset": pos_start,
                    "byteLength": pos_len,
                }));
                let pos_bv_idx = bv_counter;
                bv_counter += 1;

                accessors.push(serde_json::json!({
                    "bufferView": pos_bv_idx,
                    "byteOffset": 0,
                    "componentType": 5126,
                    "count": vc,
                    "type": "VEC3",
                    "min": pos_min,
                    "max": pos_max,
                }));
                let pos_acc_idx = acc_counter;
                acc_counter += 1;

                let normal_acc_idx: Option<u32> =
                    if let Some(ref normals) = mesh.normals {
                        let ns = buffer_data.len() as u32;
                        for n in normals {
                            buffer_data.extend_from_slice(&n[0].to_le_bytes());
                            buffer_data.extend_from_slice(&n[1].to_le_bytes());
                            buffer_data.extend_from_slice(&n[2].to_le_bytes());
                        }
                        let nl = (buffer_data.len() as u32) - ns;

                        buffer_views.push(serde_json::json!({
                            "buffer": 0,
                            "byteOffset": ns,
                            "byteLength": nl,
                        }));
                        let bv_idx = bv_counter;
                        bv_counter += 1;

                        accessors.push(serde_json::json!({
                            "bufferView": bv_idx,
                            "byteOffset": 0,
                            "componentType": 5126,
                            "count": vc,
                            "type": "VEC3",
                        }));
                        let ai = acc_counter;
                        acc_counter += 1;
                        Some(ai)
                    } else {
                        None
                    };

                let uv_acc_idx: Option<u32> = if let Some(ref uvs) = mesh.uvs {
                    let us = buffer_data.len() as u32;
                    for uv in uvs {
                        buffer_data.extend_from_slice(&uv[0].to_le_bytes());
                        buffer_data.extend_from_slice(&uv[1].to_le_bytes());
                    }
                    let ul = (buffer_data.len() as u32) - us;

                    buffer_views.push(serde_json::json!({
                        "buffer": 0,
                        "byteOffset": us,
                        "byteLength": ul,
                    }));
                    let bv_idx = bv_counter;
                    bv_counter += 1;

                    accessors.push(serde_json::json!({
                        "bufferView": bv_idx,
                        "byteOffset": 0,
                        "componentType": 5126,
                        "count": vc,
                        "type": "VEC2",
                    }));
                    let ai = acc_counter;
                    acc_counter += 1;
                    Some(ai)
                } else {
                    None
                };

                let idx_start = buffer_data.len() as u32;
                for face in &mesh.faces {
                    buffer_data.extend_from_slice(&face[0].to_le_bytes());
                    buffer_data.extend_from_slice(&face[1].to_le_bytes());
                    buffer_data.extend_from_slice(&face[2].to_le_bytes());
                }
                let idx_len = (buffer_data.len() as u32) - idx_start;
                let idx_count = (mesh.faces.len() * 3) as u32;

                buffer_views.push(serde_json::json!({
                    "buffer": 0,
                    "byteOffset": idx_start,
                    "byteLength": idx_len,
                }));
                let idx_bv_idx = bv_counter;
                bv_counter += 1;

                accessors.push(serde_json::json!({
                    "bufferView": idx_bv_idx,
                    "byteOffset": 0,
                    "componentType": 5125,
                    "count": idx_count,
                    "type": "SCALAR",
                }));
                let idx_acc_idx = acc_counter;
                acc_counter += 1;

                let mut attrs = serde_json::json!({
                    "POSITION": pos_acc_idx,
                });
                if let Some(nai) = normal_acc_idx {
                    attrs["NORMAL"] = serde_json::json!(nai);
                }
                if let Some(uai) = uv_acc_idx {
                    attrs["TEXCOORD_0"] = serde_json::json!(uai);
                }

                meshes.push(serde_json::json!({
                    "name": part.name,
                    "primitives": [{
                        "attributes": attrs,
                        "indices": idx_acc_idx,
                        "mode": 4,
                    }],
                }));
                let msh_idx = mesh_idx_counter;
                mesh_idx_counter += 1;

                let identity = Transform::identity();
                let t = part_transform
                    .get(part.id.as_str())
                    .copied()
                    .unwrap_or(&identity);
                if t.approx_eq(&identity, 1e-12) {
                    nodes.push(serde_json::json!({
                        "mesh": msh_idx,
                        "name": part.name,
                    }));
                } else {
                    let cm = to_column_major(t);
                    nodes.push(serde_json::json!({
                        "mesh": msh_idx,
                        "matrix": cm,
                        "name": part.name,
                    }));
                }

                let has_fg = mesh.face_groups.as_ref().map_or(false, |fg| !fg.is_empty());
                let has_gn = !mesh.group_names.is_empty();
                if has_fg || has_gn {
                    fid.record(
                        "face_groups",
                        1,
                        EntityStatus::Dropped,
                        Some("face groups not representable in glTF".into()),
                    );
                }
            }
            GeometryPayload::Brep(_) => {
                fid.record(
                    "brep",
                    1,
                    EntityStatus::Dropped,
                    Some("brep not representable in glTF".into()),
                );
            }
        }
    }

    let node_indices: Vec<u32> = (0..nodes.len() as u32).collect();

    let json_val = serde_json::json!({
        "asset": {
            "version": "2.0",
            "generator": "exl-gltf",
        },
        "scene": 0,
        "scenes": [{
            "nodes": node_indices,
        }],
        "nodes": nodes,
        "meshes": meshes,
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{
            "byteLength": buffer_data.len(),
        }],
    });

    let json_str = serde_json::to_string(&json_val)?;
    let json_bytes = json_str.as_bytes();
    let json_chunk_len = json_bytes.len();
    let json_pad = (4 - (json_chunk_len % 4)) % 4;

    let bin_chunk_len = buffer_data.len();
    let bin_pad = (4 - (bin_chunk_len % 4)) % 4;

    let total_len =
        12 + 8 + json_chunk_len + json_pad + 8 + bin_chunk_len + bin_pad;

    let mut out = Vec::with_capacity(total_len);

    out.extend_from_slice(&0x46546C67u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total_len as u32).to_le_bytes());

    out.extend_from_slice(&(json_chunk_len as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
    out.extend_from_slice(json_bytes);
    for _ in 0..json_pad {
        out.push(0x20u8);
    }

    out.extend_from_slice(&(bin_chunk_len as u32).to_le_bytes());
    out.extend_from_slice(&0x004E4942u32.to_le_bytes());
    out.extend_from_slice(&buffer_data);
    for _ in 0..bin_pad {
        out.push(0x00u8);
    }

    std::fs::write(path, out)?;

    Ok(fid)
}

fn compute_min_max_vec3(data: &[[f32; 3]]) -> (Vec<f32>, Vec<f32>) {
    if data.is_empty() {
        return (vec![0.0_f32, 0.0, 0.0], vec![0.0_f32, 0.0, 0.0]);
    }
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for v in data {
        for i in 0..3 {
            if v[i] < min[i] {
                min[i] = v[i];
            }
            if v[i] > max[i] {
                max[i] = v[i];
            }
        }
    }
    (min.to_vec(), max.to_vec())
}

fn to_column_major(t: &Transform) -> [f64; 16] {
    let mut m = [0.0f64; 16];
    for r in 0..4 {
        for c in 0..4 {
            m[c * 4 + r] = t.0[r][c];
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use exl_core::geom::Mesh as ExlMesh;
    use std::fs;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("exl_gltf_test_{}.glb", name));
        p
    }

    fn cube_mesh() -> ExlMesh {
        ExlMesh {
            vertices: vec![
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [-0.5, 0.5, -0.5],
            ],
            faces: vec![
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
            ],
            normals: Some(vec![
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, -1.0],
                [0.0, 0.0, -1.0],
                [0.0, 0.0, -1.0],
                [0.0, 0.0, -1.0],
            ]),
            uvs: None,
            face_groups: None,
            group_names: vec![],
        }
    }

    #[test]
    fn cube_round_trip_vertex_face_normals_bbox() {
        let mesh = cube_mesh();
        let bb_orig = mesh.bounding_box();
        let part = Part::new("cube", GeometryPayload::Mesh(mesh));
        let mut doc = Document::new(vec![part]);
        doc.provenance.tool_of_origin = Some(ToolOfOrigin {
            name: "exl-gltf".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            timestamp_iso: timestamp_iso(),
        });
        doc.refresh_content_hash();

        let path = temp_path("cube_rt");
        let _ = export_gltf(&doc, &path).expect("export");
        let (imported, _fid) = import_gltf(&path).expect("import");
        fs::remove_file(&path).ok();

        assert_eq!(imported.parts.len(), 1);
        let im = match &imported.parts[0].geometry {
            GeometryPayload::Mesh(m) => m,
            _ => panic!("expected mesh"),
        };
        assert_eq!(im.vertices.len(), 8);
        assert_eq!(im.faces.len(), 12);
        assert!(im.normals.is_some());
        assert_eq!(im.normals.as_ref().unwrap().len(), 8);

        let bb = im.bounding_box();
        assert!((bb_orig.min[0] - bb.min[0]).abs() < 0.001);
        assert!((bb_orig.min[1] - bb.min[1]).abs() < 0.001);
        assert!((bb_orig.min[2] - bb.min[2]).abs() < 0.001);
        assert!((bb_orig.max[0] - bb.max[0]).abs() < 0.001);
        assert!((bb_orig.max[1] - bb.max[1]).abs() < 0.001);
        assert!((bb_orig.max[2] - bb.max[2]).abs() < 0.001);
    }

    #[test]
    fn two_parts_assembly_instance_translation_survives() {
        let m1 = ExlMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            faces: vec![[0, 1, 2]],
            ..Default::default()
        };
        let m2 = ExlMesh {
            vertices: vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            faces: vec![[0, 1, 2]],
            ..Default::default()
        };

        let p1 = Part::new("tri_xy", GeometryPayload::Mesh(m1));
        let p2 = Part::new("tri_yz", GeometryPayload::Mesh(m2));
        let p1_id = p1.id.clone();
        let p2_id = p2.id.clone();

        let mut transl = Transform::identity();
        transl.0[0][3] = 5.0;
        transl.0[1][3] = 10.0;
        transl.0[2][3] = 15.0;

        let mut doc = Document::new(vec![p1, p2]);
        doc.assembly = Assembly {
            instances: vec![
                Instance {
                    part_ref: p1_id,
                    name: "tri_xy_inst".into(),
                    transform: Transform::identity(),
                },
                Instance {
                    part_ref: p2_id,
                    name: "tri_yz_inst".into(),
                    transform: transl,
                },
            ],
            mates: vec![],
        };
        doc.provenance.tool_of_origin = Some(ToolOfOrigin {
            name: "exl-gltf".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            timestamp_iso: timestamp_iso(),
        });
        doc.refresh_content_hash();

        let path = temp_path("two_parts");
        let _ = export_gltf(&doc, &path).expect("export");
        let (imported, _fid) = import_gltf(&path).expect("import");
        fs::remove_file(&path).ok();

        assert_eq!(imported.parts.len(), 2);
        assert_eq!(imported.assembly.instances.len(), 2);

        let inst_yz = imported
            .assembly
            .instances
            .iter()
            .find(|i| i.name.contains("tri_yz"))
            .expect("yz instance");
        let tr = inst_yz.transform.translation();
        assert!((tr[0] - 5.0).abs() < 0.001);
        assert!((tr[1] - 10.0).abs() < 0.001);
        assert!((tr[2] - 15.0).abs() < 0.001);
    }

    #[test]
    fn rejects_bad_magic() {
        let path = temp_path("bad_magic");
        fs::write(&path, b"this is not a glb file").expect("write");
        let result = import_gltf(&path);
        fs::remove_file(&path).ok();
        match result {
            Err(GltfError::Unsupported(_)) => {}
            other => panic!("expected Unsupported, got {:?}", other),
        }
    }

    #[test]
    fn index_widening_u16_path() {
        let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1,"mode":4}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},{"bufferView":1,"componentType":5123,"count":6,"type":"SCALAR"}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":12}],"buffers":[{"byteLength":48}]}"#;

        let mut bin = Vec::new();
        for v in &[0.0f32, 0.0f32, 0.0f32] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for v in &[1.0f32, 0.0f32, 0.0f32] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for v in &[0.0f32, 1.0f32, 0.0f32] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for v in &[0u16, 1u16, 2u16, 0u16, 2u16, 1u16] {
            bin.extend_from_slice(&v.to_le_bytes());
        }

        let glb = make_glb(json, &bin);
        let path = temp_path("u16_idx");
        fs::write(&path, &glb).expect("write");
        let (doc, _fid) = import_gltf(&path).expect("import");
        fs::remove_file(&path).ok();

        assert_eq!(doc.parts.len(), 1);
        if let GeometryPayload::Mesh(m) = &doc.parts[0].geometry {
            assert_eq!(m.vertices.len(), 3);
            assert_eq!(m.faces.len(), 2);
            assert_eq!(m.faces[0], [0, 1, 2]);
            assert_eq!(m.faces[1], [0, 2, 1]);
        } else {
            panic!("expected mesh");
        }
    }

    #[test]
    fn padding_correctness() {
        let mesh = cube_mesh();
        let part = Part::new("cube", GeometryPayload::Mesh(mesh));
        let mut doc = Document::new(vec![part]);
        doc.provenance.tool_of_origin = Some(ToolOfOrigin {
            name: "exl-gltf".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            timestamp_iso: timestamp_iso(),
        });
        doc.refresh_content_hash();

        let path = temp_path("padding");
        let _ = export_gltf(&doc, &path).expect("export");
        let data = fs::read(&path).expect("read");
        fs::remove_file(&path).ok();

        assert_eq!(data.len() % 4, 0, "GLB file length must be multiple of 4");
        let total_len = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
        assert_eq!(data.len(), total_len, "actual length must match header total length");

        let json_chunk_len =
            u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
        let json_chunk_type =
            u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        assert_eq!(json_chunk_type, 0x4E4F534A, "JSON chunk type");

        let json_pad = (4 - (json_chunk_len % 4)) % 4;
        let bin_offset = 12 + 8 + json_chunk_len + json_pad;

        let bin_chunk_len =
            u32::from_le_bytes([data[bin_offset], data[bin_offset + 1], data[bin_offset + 2], data[bin_offset + 3]]) as usize;
        let bin_chunk_type = u32::from_le_bytes([
            data[bin_offset + 4],
            data[bin_offset + 5],
            data[bin_offset + 6],
            data[bin_offset + 7],
        ]);
        assert_eq!(bin_chunk_type, 0x004E4942, "BIN chunk type");

        let bin_pad = (4 - (bin_chunk_len % 4)) % 4;
        let expected_total = 12 + 8 + json_chunk_len + json_pad + 8 + bin_chunk_len + bin_pad;
        assert_eq!(data.len(), expected_total);

        for i in 0..json_pad {
            let idx = 12 + 8 + json_chunk_len + i;
            assert_eq!(data[idx], 0x20u8, "JSON padding must be space (0x20)");
        }
        for i in 0..bin_pad {
            let idx = bin_offset + 8 + bin_chunk_len + i;
            assert_eq!(data[idx], 0x00u8, "BIN padding must be 0x00");
        }
    }

    #[test]
    fn trs_roundtrip() {
        let rotated_scaled: [[f64; 3]; 3] = [
            [0.0, -2.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 0.0, 2.0],
        ];

        let mut expected = Transform::identity();
        expected.0[0][0] = rotated_scaled[0][0];
        expected.0[0][1] = rotated_scaled[0][1];
        expected.0[0][2] = rotated_scaled[0][2];
        expected.0[1][0] = rotated_scaled[1][0];
        expected.0[1][1] = rotated_scaled[1][1];
        expected.0[1][2] = rotated_scaled[1][2];
        expected.0[2][0] = rotated_scaled[2][0];
        expected.0[2][1] = rotated_scaled[2][1];
        expected.0[2][2] = rotated_scaled[2][2];
        expected.0[0][3] = 1.0;
        expected.0[1][3] = 2.0;
        expected.0[2][3] = 3.0;

        let mesh = cube_mesh();
        let part = Part::new("cube", GeometryPayload::Mesh(mesh));
        let part_id = part.id.clone();

        let mut doc = Document::new(vec![part]);
        doc.assembly = Assembly {
            instances: vec![Instance {
                part_ref: part_id,
                name: "rot_scaled_transl".into(),
                transform: expected,
            }],
            mates: vec![],
        };
        doc.provenance.tool_of_origin = Some(ToolOfOrigin {
            name: "exl-gltf".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            timestamp_iso: timestamp_iso(),
        });
        doc.refresh_content_hash();

        let path = temp_path("trs_rt");
        let _ = export_gltf(&doc, &path).expect("export");
        let (imported, _fid) = import_gltf(&path).expect("import");
        fs::remove_file(&path).ok();

        assert_eq!(imported.assembly.instances.len(), 1);
        let inst = &imported.assembly.instances[0];
        assert!(inst.transform.approx_eq(&expected, 1e-9));
    }

    #[test]
    fn quaternion_import() {
        let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0,"translation":[5,0,0],"rotation":[0,1,0,0],"scale":[1,1,1]}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1,"mode":4}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},{"bufferView":1,"componentType":5125,"count":3,"type":"SCALAR"}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":12}],"buffers":[{"byteLength":48}]}"#;

        let mut bin = Vec::new();
        for v in &[0.0f32, 0.0f32, 0.0f32] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for v in &[1.0f32, 0.0f32, 0.0f32] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for v in &[0.0f32, 1.0f32, 0.0f32] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for v in &[0u32, 1u32, 2u32] {
            bin.extend_from_slice(&v.to_le_bytes());
        }

        let glb = make_glb(json, &bin);
        let path = temp_path("quat_import");
        fs::write(&path, &glb).expect("write");
        let (doc, _fid) = import_gltf(&path).expect("import");
        fs::remove_file(&path).ok();

        let mut expected = Transform::identity();
        expected.0[0][0] = -1.0;
        expected.0[2][2] = -1.0;
        expected.0[0][3] = 5.0;

        assert_eq!(doc.assembly.instances.len(), 1);
        let inst = &doc.assembly.instances[0];
        assert!(inst.transform.approx_eq(&expected, 1e-9));
    }

    #[test]
    fn identity_omitted() {
        let mesh = cube_mesh();
        let part = Part::new("cube", GeometryPayload::Mesh(mesh));
        let part_id = part.id.clone();

        let mut doc = Document::new(vec![part]);
        doc.assembly = Assembly {
            instances: vec![Instance {
                part_ref: part_id,
                name: "identity_inst".into(),
                transform: Transform::identity(),
            }],
            mates: vec![],
        };
        doc.provenance.tool_of_origin = Some(ToolOfOrigin {
            name: "exl-gltf".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            timestamp_iso: timestamp_iso(),
        });
        doc.refresh_content_hash();

        let path = temp_path("identity_omit");
        let _ = export_gltf(&doc, &path).expect("export");
        let data = fs::read(&path).expect("read");
        fs::remove_file(&path).ok();

        let json_chunk_len =
            u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
        let json_bytes = &data[20..20 + json_chunk_len];
        let json_val: serde_json::Value =
            serde_json::from_slice(json_bytes).expect("parse json chunk");
        let node = &json_val["nodes"][0];
        assert!(node.get("matrix").is_none(), "identity node must not have matrix");
        assert!(
            node.get("translation").is_none(),
            "identity node must not have translation"
        );
    }

    fn make_glb(json: &str, bin: &[u8]) -> Vec<u8> {
        let json_bytes = json.as_bytes();
        let json_pad = (4 - (json_bytes.len() % 4)) % 4;
        let bin_pad = (4 - (bin.len() % 4)) % 4;

        let total_len = 12 + 8 + json_bytes.len() + json_pad + 8 + bin.len() + bin_pad;
        let mut out = Vec::with_capacity(total_len);

        out.extend_from_slice(&0x46546C67u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total_len as u32).to_le_bytes());

        out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        out.extend_from_slice(json_bytes);
        for _ in 0..json_pad {
            out.push(0x20u8);
        }

        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E4942u32.to_le_bytes());
        out.extend_from_slice(bin);
        for _ in 0..bin_pad {
            out.push(0x00u8);
        }

        out
    }
}
