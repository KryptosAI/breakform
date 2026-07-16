use arrow::ipc::reader::StreamReader;
use arrow::ipc::writer::StreamWriter;
use arrow::record_batch::RecordBatch;
use exl_core::geom::Mesh;
use exl_core::Document;
use exl_core::GeometryPayload;
use memmap2::Mmap;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("unsupported extension: {0}")]
    UnsupportedExtension(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

const MAGIC: &[u8; 4] = b"EXLB";
const BINARY_VERSION_V1: u8 = 1;
const BINARY_VERSION_V2: u8 = 2;
const BINARY_VERSION_V3: u8 = 3;

#[derive(Serialize, Deserialize)]
struct V2Json {
    document: Document,
    meshes: Vec<V2MeshEntry>,
}

#[derive(Serialize, Deserialize)]
struct V2MeshEntry {
    part: usize,
    vertices: usize,
    faces: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    normals: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uvs: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face_groups: Option<usize>,
}

pub fn to_text(doc: &Document) -> String {
    let mut buf = String::new();
    buf.push_str("#exl ");
    buf.push_str(&doc.schema_version);
    buf.push('\n');
    let json = serde_json::to_string_pretty(doc).expect("serialization cannot fail");
    buf.push_str(&json);
    buf
}

pub fn from_text(s: &str) -> Result<Document, IoError> {
    let header_end = s
        .find('\n')
        .ok_or_else(|| IoError::Parse("missing header line".into()))?;
    let header = s[..header_end].trim();
    let prefix = "#exl ";
    if !header.starts_with(prefix) {
        return Err(IoError::Parse("missing #exl header".into()));
    }
    let version = header[prefix.len()..].trim();
    if !exl_core::SUPPORTED_SCHEMA_VERSIONS.contains(&version) {
        return Err(IoError::Parse(format!(
            "unsupported version '{}', supported: {}",
            version,
            exl_core::SUPPORTED_SCHEMA_VERSIONS.join(", ")
        )));
    }
    let json_str = &s[header_end + 1..];
    let doc: Document = serde_json::from_str(json_str)?;
    Ok(doc)
}

pub fn to_binary(doc: &Document) -> Result<Vec<u8>, IoError> {
    to_binary_v3(doc)
}

#[cfg(test)]
fn to_binary_v2(doc: &Document) -> Result<Vec<u8>, IoError> {
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    let mut mesh_entries: Vec<V2MeshEntry> = Vec::new();
    let mut mod_doc = doc.clone();

    for (part_idx, part) in mod_doc.parts.iter_mut().enumerate() {
        if let GeometryPayload::Mesh(ref mesh) = &part.geometry {
            let v_bytes: &[u8] = bytemuck::cast_slice(&mesh.vertices);
            let f_bytes: &[u8] = bytemuck::cast_slice(&mesh.faces);

            let v_idx = buffers.len();
            buffers.push(v_bytes.to_vec());
            let f_idx = buffers.len();
            buffers.push(f_bytes.to_vec());

            let n_idx = mesh.normals.as_ref().map(|n| {
                let b: &[u8] = bytemuck::cast_slice(n);
                let idx = buffers.len();
                buffers.push(b.to_vec());
                idx
            });

            let uv_idx = mesh.uvs.as_ref().map(|uv| {
                let b: &[u8] = bytemuck::cast_slice(uv);
                let idx = buffers.len();
                buffers.push(b.to_vec());
                idx
            });

            let fg_idx = mesh.face_groups.as_ref().map(|fg| {
                let b: &[u8] = bytemuck::cast_slice(fg);
                let idx = buffers.len();
                buffers.push(b.to_vec());
                idx
            });

            mesh_entries.push(V2MeshEntry {
                part: part_idx,
                vertices: v_idx,
                faces: f_idx,
                normals: n_idx,
                uvs: uv_idx,
                face_groups: fg_idx,
            });
        }
    }

    for part in mod_doc.parts.iter_mut() {
        if let GeometryPayload::Mesh(ref mut mesh) = &mut part.geometry {
            mesh.vertices.clear();
            mesh.faces.clear();
            mesh.normals = None;
            mesh.uvs = None;
            mesh.face_groups = None;
        }
    }

    let v2json = V2Json {
        document: mod_doc,
        meshes: mesh_entries,
    };

    let json_bytes = serde_json::to_vec(&v2json)?;
    let buffer_count = buffers.len() as u32;

    let header_size: u64 = 32;
    let table_size = buffer_count as u64 * 16;
    let table_end = header_size + table_size;
    let data_start = align_up(table_end, 64);

    let mut buf_offsets: Vec<u64> = Vec::with_capacity(buffers.len());
    let mut buf_lengths: Vec<u64> = Vec::with_capacity(buffers.len());
    let mut pos = data_start;
    for buf in &buffers {
        pos = align_up(pos, 64);
        buf_offsets.push(pos);
        buf_lengths.push(buf.len() as u64);
        pos += buf.len() as u64;
    }
    let json_offset = pos;
    let json_len = json_bytes.len() as u64;

    let total = json_offset as usize + json_bytes.len();
    let mut out = Vec::with_capacity(total);

    out.extend_from_slice(MAGIC);
    out.push(BINARY_VERSION_V2);
    out.extend_from_slice(&[0u8; 3]);
    out.extend_from_slice(&json_offset.to_le_bytes());
    out.extend_from_slice(&json_len.to_le_bytes());
    out.extend_from_slice(&buffer_count.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);

    for i in 0..buffers.len() {
        out.extend_from_slice(&buf_offsets[i].to_le_bytes());
        out.extend_from_slice(&buf_lengths[i].to_le_bytes());
    }

    while out.len() < data_start as usize {
        out.push(0);
    }

    for i in 0..buffers.len() {
        while out.len() < buf_offsets[i] as usize {
            out.push(0);
        }
        out.extend_from_slice(&buffers[i]);
    }

    while out.len() < json_offset as usize {
        out.push(0);
    }
    out.extend_from_slice(&json_bytes);

    Ok(out)
}

pub fn from_binary(bytes: &[u8]) -> Result<Document, IoError> {
    if bytes.len() < 9 {
        return Err(IoError::Parse("binary too short".into()));
    }
    if &bytes[..4] != MAGIC {
        return Err(IoError::Parse("bad magic bytes".into()));
    }
    let version = bytes[4];
    match version {
        BINARY_VERSION_V1 => from_binary_v1(bytes),
        BINARY_VERSION_V2 => from_binary_v2(bytes),
        BINARY_VERSION_V3 => from_binary_v3(bytes),
        _ => Err(IoError::Parse(format!(
            "unsupported binary version {}, expected {}, {}, or {}",
            version, BINARY_VERSION_V1, BINARY_VERSION_V2, BINARY_VERSION_V3
        ))),
    }
}

fn from_binary_v1(bytes: &[u8]) -> Result<Document, IoError> {
    let payload_len = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
    if bytes.len() < 9 + payload_len {
        return Err(IoError::Parse("payload truncated".into()));
    }
    let payload = &bytes[9..9 + payload_len];
    let doc: Document = serde_json::from_slice(payload)?;
    Ok(doc)
}

fn from_binary_v2(bytes: &[u8]) -> Result<Document, IoError> {
    let header = parse_v2_header(bytes)?;
    let buffer_table_bytes =
        &bytes[32..32 + header.buffer_count as usize * 16];

    let mut buf_offsets = Vec::with_capacity(header.buffer_count as usize);
    let mut buf_lengths = Vec::with_capacity(header.buffer_count as usize);
    for i in 0..header.buffer_count as usize {
        let base = i * 16;
        let offset = u64::from_le_bytes([
            buffer_table_bytes[base],
            buffer_table_bytes[base + 1],
            buffer_table_bytes[base + 2],
            buffer_table_bytes[base + 3],
            buffer_table_bytes[base + 4],
            buffer_table_bytes[base + 5],
            buffer_table_bytes[base + 6],
            buffer_table_bytes[base + 7],
        ]);
        let length = u64::from_le_bytes([
            buffer_table_bytes[base + 8],
            buffer_table_bytes[base + 9],
            buffer_table_bytes[base + 10],
            buffer_table_bytes[base + 11],
            buffer_table_bytes[base + 12],
            buffer_table_bytes[base + 13],
            buffer_table_bytes[base + 14],
            buffer_table_bytes[base + 15],
        ]);
        buf_offsets.push(offset);
        buf_lengths.push(length);
    }

    let json_start = header.json_offset as usize;
    let json_end = json_start + header.json_len as usize;
    let json_slice = bytes
        .get(json_start..json_end)
        .ok_or_else(|| IoError::Parse("json section out of range".into()))?;
    let v2json: V2Json = serde_json::from_slice(json_slice)?;
    let mut doc = v2json.document;

    for entry in &v2json.meshes {
        let verts = read_buffer_f32_3d(bytes, &buf_offsets, &buf_lengths, entry.vertices)?;
        let faces = read_buffer_u32_3d(bytes, &buf_offsets, &buf_lengths, entry.faces)?;

        let normals = match entry.normals {
            Some(i) => Some(read_buffer_f32_3d(bytes, &buf_offsets, &buf_lengths, i)?),
            None => None,
        };
        let uvs = match entry.uvs {
            Some(i) => Some(read_buffer_f32_2d(bytes, &buf_offsets, &buf_lengths, i)?),
            None => None,
        };
        let face_groups = match entry.face_groups {
            Some(i) => Some(read_buffer_u32_flat(bytes, &buf_offsets, &buf_lengths, i)?),
            None => None,
        };

        let part = doc
            .parts
            .get_mut(entry.part)
            .ok_or_else(|| IoError::Parse("mesh part index out of bounds".into()))?;
        let group_names = match &part.geometry {
            GeometryPayload::Mesh(m) => m.group_names.clone(),
            _ => Vec::new(),
        };

        part.geometry = GeometryPayload::Mesh(Mesh {
            vertices: verts,
            faces,
            normals,
            uvs,
            face_groups,
            group_names,
        });
    }

    Ok(doc)
}

struct V2Header {
    json_offset: u64,
    json_len: u64,
    buffer_count: u32,
}

fn parse_v2_header(bytes: &[u8]) -> Result<V2Header, IoError> {
    if bytes.len() < 32 {
        return Err(IoError::Parse("v2 header too short".into()));
    }
    let json_offset = u64::from_le_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ]);
    let json_len = u64::from_le_bytes([
        bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23],
    ]);
    let buffer_count = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);

    let max_pos = json_offset
        .checked_add(json_len)
        .ok_or_else(|| IoError::Parse("json offset+len overflow".into()))?;
    if max_pos > bytes.len() as u64 {
        return Err(IoError::Parse("json section beyond file".into()));
    }

    let table_bytes = buffer_count as u64 * 16;
    if bytes.len() < 32 + table_bytes as usize {
        return Err(IoError::Parse("buffer table truncated".into()));
    }

    Ok(V2Header {
        json_offset,
        json_len,
        buffer_count,
    })
}

fn read_buffer_f32_3d(
    bytes: &[u8],
    offsets: &[u64],
    lengths: &[u64],
    idx: usize,
) -> Result<Vec<[f32; 3]>, IoError> {
    let raw = read_buffer_bytes(bytes, offsets, lengths, idx)?;
    if raw.len() % 4 != 0 {
        return Err(IoError::Parse("buffer length not multiple of 4".into()));
    }
    if (raw.as_ptr() as usize) % 4 != 0 {
        return Err(IoError::Parse("buffer misaligned for f32".into()));
    }
    let f32s: &[f32] = bytemuck::cast_slice(raw);
    if f32s.len() % 3 != 0 {
        return Err(IoError::Parse("f32 buffer length not multiple of 3 for vec3".into()));
    }
    let out: Vec<[f32; 3]> = f32s
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    Ok(out)
}

fn read_buffer_f32_2d(
    bytes: &[u8],
    offsets: &[u64],
    lengths: &[u64],
    idx: usize,
) -> Result<Vec<[f32; 2]>, IoError> {
    let raw = read_buffer_bytes(bytes, offsets, lengths, idx)?;
    if raw.len() % 4 != 0 {
        return Err(IoError::Parse("buffer length not multiple of 4".into()));
    }
    if (raw.as_ptr() as usize) % 4 != 0 {
        return Err(IoError::Parse("buffer misaligned for f32".into()));
    }
    let f32s: &[f32] = bytemuck::cast_slice(raw);
    if f32s.len() % 2 != 0 {
        return Err(IoError::Parse("f32 buffer length not multiple of 2 for vec2".into()));
    }
    let out: Vec<[f32; 2]> = f32s.chunks_exact(2).map(|c| [c[0], c[1]]).collect();
    Ok(out)
}

fn read_buffer_u32_3d(
    bytes: &[u8],
    offsets: &[u64],
    lengths: &[u64],
    idx: usize,
) -> Result<Vec<[u32; 3]>, IoError> {
    let raw = read_buffer_bytes(bytes, offsets, lengths, idx)?;
    if raw.len() % 4 != 0 {
        return Err(IoError::Parse("buffer length not multiple of 4".into()));
    }
    if (raw.as_ptr() as usize) % 4 != 0 {
        return Err(IoError::Parse("buffer misaligned for u32".into()));
    }
    let u32s: &[u32] = bytemuck::cast_slice(raw);
    if u32s.len() % 3 != 0 {
        return Err(IoError::Parse("u32 buffer length not multiple of 3 for face".into()));
    }
    let out: Vec<[u32; 3]> = u32s
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    Ok(out)
}

fn read_buffer_u32_flat(
    bytes: &[u8],
    offsets: &[u64],
    lengths: &[u64],
    idx: usize,
) -> Result<Vec<u32>, IoError> {
    let raw = read_buffer_bytes(bytes, offsets, lengths, idx)?;
    if raw.len() % 4 != 0 {
        return Err(IoError::Parse("buffer length not multiple of 4".into()));
    }
    if (raw.as_ptr() as usize) % 4 != 0 {
        return Err(IoError::Parse("buffer misaligned for u32".into()));
    }
    let u32s: &[u32] = bytemuck::cast_slice(raw);
    Ok(u32s.to_vec())
}

fn read_buffer_bytes<'a>(
    bytes: &'a [u8],
    offsets: &[u64],
    lengths: &[u64],
    idx: usize,
) -> Result<&'a [u8], IoError> {
    let offset = offsets.get(idx).copied().ok_or_else(|| {
        IoError::Parse(format!("buffer index {} out of range", idx))
    })?;
    let length = lengths.get(idx).copied().ok_or_else(|| {
        IoError::Parse(format!("buffer index {} out of range", idx))
    })?;
    if offset % 64 != 0 {
        return Err(IoError::Parse(format!(
            "buffer offset {} not 64-byte aligned",
            offset
        )));
    }
    let end = offset
        .checked_add(length)
        .ok_or_else(|| IoError::Parse("buffer offset+len overflow".into()))?;
    bytes
        .get(offset as usize..end as usize)
        .ok_or_else(|| IoError::Parse(format!("buffer {} out of file range", idx)))
}

pub fn save(doc: &Document, path: &Path) -> Result<(), IoError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());
    match ext.as_deref() {
        Some("exl") => {
            let text = to_text(doc);
            std::fs::write(path, text)?;
            Ok(())
        }
        Some("exlb") => {
            let data = to_binary(doc)?;
            std::fs::write(path, data)?;
            Ok(())
        }
        _ => Err(IoError::UnsupportedExtension(
            path.to_string_lossy().into_owned(),
        )),
    }
}

pub fn load(path: &Path) -> Result<Document, IoError> {
    let mut file = std::fs::File::open(path)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    match ext.as_deref() {
        Some("exl") => {
            let mut s = String::new();
            file.read_to_string(&mut s)?;
            from_text(&s)
        }
        Some("exlb") => {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            from_binary(&buf)
        }
        _ => {
            let mut head = [0u8; 4];
            file.read_exact(&mut head)?;
            if &head == b"#exl" || &head == b"EXLB" {
                let mut rest = Vec::new();
                if head == *b"#exl" {
                    rest.extend_from_slice(b"#exl");
                } else {
                    rest.extend_from_slice(b"EXLB");
                }
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;
                rest.extend_from_slice(&buf);
                if head == *b"#exl" {
                    let s = String::from_utf8(rest).map_err(|e| IoError::Parse(e.to_string()))?;
                    from_text(&s)
                } else {
                    from_binary(&rest)
                }
            } else {
                Err(IoError::UnsupportedExtension(
                    path.to_string_lossy().into_owned(),
                ))
            }
        }
    }
}

enum Storage {
    Mapped(Mmap),
    Owned(Vec<u8>),
}

impl Storage {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Storage::Mapped(m) => m,
            Storage::Owned(v) => v,
        }
    }
}

pub struct MappedExlb {
    storage: Storage,
    buf_offsets: Vec<u64>,
    buf_lengths: Vec<u64>,
    document_meta: Document,
    mesh_entries: Vec<V2MeshEntry>,
}

pub struct MeshView<'a> {
    pub vertices: &'a [f32],
    pub faces: &'a [u32],
    pub normals: Option<&'a [f32]>,
    pub uvs: Option<&'a [f32]>,
    pub face_groups: Option<&'a [u32]>,
}

impl MappedExlb {
    pub fn open(path: &Path) -> Result<Self, IoError> {
        let file = std::fs::File::open(path)?;
        let storage = match unsafe { Mmap::map(&file) } {
            Ok(mmap) => Storage::Mapped(mmap),
            Err(_) => {
                let mut buf = Vec::new();
                let mut f = std::fs::File::open(path)?;
                f.read_to_end(&mut buf)?;
                Storage::Owned(buf)
            }
        };
        Self::from_storage(storage)
    }

    fn from_storage(storage: Storage) -> Result<Self, IoError> {
        let bytes = storage.as_bytes();
        if bytes.len() < 9 {
            return Err(IoError::Parse("binary too short".into()));
        }
        if &bytes[..4] != MAGIC {
            return Err(IoError::Parse("bad magic bytes".into()));
        }
        if bytes[4] != BINARY_VERSION_V2 {
            return Err(IoError::Parse(format!(
                "MappedExlb requires v2, got version {}",
                bytes[4]
            )));
        }

        let header = parse_v2_header(bytes)?;

        let table_byte_count = header.buffer_count as usize * 16;
        let table_bytes = &bytes[32..32 + table_byte_count];
        let mut buf_offsets = Vec::with_capacity(header.buffer_count as usize);
        let mut buf_lengths = Vec::with_capacity(header.buffer_count as usize);
        for i in 0..header.buffer_count as usize {
            let base = i * 16;
            let offset = u64::from_le_bytes([
                table_bytes[base],
                table_bytes[base + 1],
                table_bytes[base + 2],
                table_bytes[base + 3],
                table_bytes[base + 4],
                table_bytes[base + 5],
                table_bytes[base + 6],
                table_bytes[base + 7],
            ]);
            let length = u64::from_le_bytes([
                table_bytes[base + 8],
                table_bytes[base + 9],
                table_bytes[base + 10],
                table_bytes[base + 11],
                table_bytes[base + 12],
                table_bytes[base + 13],
                table_bytes[base + 14],
                table_bytes[base + 15],
            ]);
            buf_offsets.push(offset);
            buf_lengths.push(length);
        }

        for i in 0..header.buffer_count as usize {
            let offset = buf_offsets[i];
            let length = buf_lengths[i];
            if offset % 64 != 0 {
                return Err(IoError::Parse(format!(
                    "buffer {} offset {} not 64-byte aligned",
                    i, offset
                )));
            }
            if length % 4 != 0 {
                return Err(IoError::Parse(format!(
                    "buffer {} length {} not a multiple of 4",
                    i, length
                )));
            }
            let end = offset
                .checked_add(length)
                .ok_or_else(|| IoError::Parse(format!("buffer {} offset+len overflow", i)))?;
            if end > bytes.len() as u64 {
                return Err(IoError::Parse(format!("buffer {} extends beyond file", i)));
            }
        }

        let json_start = header.json_offset as usize;
        let json_end = json_start + header.json_len as usize;
        let json_slice = bytes
            .get(json_start..json_end)
            .ok_or_else(|| IoError::Parse("json section out of range".into()))?;
        let v2json: V2Json = serde_json::from_slice(json_slice)?;

        Ok(MappedExlb {
            storage,
            buf_offsets,
            buf_lengths,
            document_meta: v2json.document,
            mesh_entries: v2json.meshes,
        })
    }

    pub fn document_meta(&self) -> Result<Document, IoError> {
        Ok(self.document_meta.clone())
    }

    pub fn mesh_count(&self) -> usize {
        self.mesh_entries.len()
    }

    pub fn mesh_view(&self, index: usize) -> Result<MeshView<'_>, IoError> {
        let entry = self
            .mesh_entries
            .get(index)
            .ok_or_else(|| IoError::Parse("mesh index out of bounds".into()))?;
        let data = self.storage.as_bytes();

        let vertices = get_f32_slice(data, &self.buf_offsets, &self.buf_lengths, entry.vertices)?;
        let faces = get_u32_slice(data, &self.buf_offsets, &self.buf_lengths, entry.faces)?;
        let normals = match entry.normals {
            Some(i) => Some(get_f32_slice(data, &self.buf_offsets, &self.buf_lengths, i)?),
            None => None,
        };
        let uvs = match entry.uvs {
            Some(i) => Some(get_f32_slice(data, &self.buf_offsets, &self.buf_lengths, i)?),
            None => None,
        };
        let face_groups = match entry.face_groups {
            Some(i) => Some(get_u32_slice(data, &self.buf_offsets, &self.buf_lengths, i)?),
            None => None,
        };

        Ok(MeshView {
            vertices,
            faces,
            normals,
            uvs,
            face_groups,
        })
    }

    pub fn to_document(&self) -> Result<Document, IoError> {
        let mut doc = self.document_meta()?;
        let data = self.storage.as_bytes();

        for entry in &self.mesh_entries {
            let verts = get_f32_slice(data, &self.buf_offsets, &self.buf_lengths, entry.vertices)?;
            let faces = get_u32_slice(data, &self.buf_offsets, &self.buf_lengths, entry.faces)?;

            let normals: Option<Vec<[f32; 3]>> = match entry.normals {
                Some(i) => {
                    let s = get_f32_slice(data, &self.buf_offsets, &self.buf_lengths, i)?;
                    Some(s.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect())
                }
                None => None,
            };
            let uvs: Option<Vec<[f32; 2]>> = match entry.uvs {
                Some(i) => {
                    let s = get_f32_slice(data, &self.buf_offsets, &self.buf_lengths, i)?;
                    Some(s.chunks_exact(2).map(|c| [c[0], c[1]]).collect())
                }
                None => None,
            };
            let face_groups: Option<Vec<u32>> = match entry.face_groups {
                Some(i) => {
                    let s = get_u32_slice(data, &self.buf_offsets, &self.buf_lengths, i)?;
                    Some(s.to_vec())
                }
                None => None,
            };

            let vertices: Vec<[f32; 3]> = verts
                .chunks_exact(3)
                .map(|c| [c[0], c[1], c[2]])
                .collect();
            let faces: Vec<[u32; 3]> = faces
                .chunks_exact(3)
                .map(|c| [c[0], c[1], c[2]])
                .collect();

            let part = doc
                .parts
                .get_mut(entry.part)
                .ok_or_else(|| IoError::Parse("mesh part index out of bounds".into()))?;
            let group_names = match &part.geometry {
                GeometryPayload::Mesh(m) => m.group_names.clone(),
                _ => Vec::new(),
            };

            part.geometry = GeometryPayload::Mesh(Mesh {
                vertices,
                faces,
                normals,
                uvs,
                face_groups,
                group_names,
            });
        }

        Ok(doc)
    }
}

fn buffer_entry(offsets: &[u64], lengths: &[u64], idx: usize) -> Result<(u64, u64), IoError> {
    let offset = offsets
        .get(idx)
        .copied()
        .ok_or_else(|| IoError::Parse(format!("buffer index {} out of range", idx)))?;
    let length = lengths
        .get(idx)
        .copied()
        .ok_or_else(|| IoError::Parse(format!("buffer index {} out of range", idx)))?;
    if offset % 64 != 0 {
        return Err(IoError::Parse(format!(
            "buffer offset {} not 64-byte aligned",
            offset
        )));
    }
    Ok((offset, length))
}

fn get_f32_slice<'a>(
    data: &'a [u8],
    offsets: &[u64],
    lengths: &[u64],
    idx: usize,
) -> Result<&'a [f32], IoError> {
    let (offset, length) = buffer_entry(offsets, lengths, idx)?;
    let end = offset
        .checked_add(length)
        .ok_or_else(|| IoError::Parse("buffer offset+len overflow".into()))?;
    let bytes = data
        .get(offset as usize..end as usize)
        .ok_or_else(|| IoError::Parse(format!("buffer {} out of range", idx)))?;
    if bytes.len() % 4 != 0 {
        return Err(IoError::Parse("buffer length not multiple of 4 for f32".into()));
    }
    if (bytes.as_ptr() as usize) % 4 != 0 {
        return Err(IoError::Parse("buffer misaligned for f32".into()));
    }
    Ok(bytemuck::cast_slice(bytes))
}

fn get_u32_slice<'a>(
    data: &'a [u8],
    offsets: &[u64],
    lengths: &[u64],
    idx: usize,
) -> Result<&'a [u32], IoError> {
    let (offset, length) = buffer_entry(offsets, lengths, idx)?;
    let end = offset
        .checked_add(length)
        .ok_or_else(|| IoError::Parse("buffer offset+len overflow".into()))?;
    let bytes = data
        .get(offset as usize..end as usize)
        .ok_or_else(|| IoError::Parse(format!("buffer {} out of range", idx)))?;
    if bytes.len() % 4 != 0 {
        return Err(IoError::Parse("buffer length not multiple of 4 for u32".into()));
    }
    if (bytes.as_ptr() as usize) % 4 != 0 {
        return Err(IoError::Parse("buffer misaligned for u32".into()));
    }
    Ok(bytemuck::cast_slice(bytes))
}

#[cfg(test)]
fn align_up(v: u64, align: u64) -> u64 {
    (v + align - 1) & !(align - 1)
}

fn to_binary_v3(doc: &Document) -> Result<Vec<u8>, IoError> {
    let mut mesh_refs: Vec<(usize, &Mesh)> = Vec::new();
    for (part_idx, part) in doc.parts.iter().enumerate() {
        if let GeometryPayload::Mesh(ref mesh) = &part.geometry {
            mesh_refs.push((part_idx, mesh));
        }
    }

    let batches = exl_arrow::mesh_to_record_batches(&mesh_refs);

    let mut mod_doc = doc.clone();
    for part in mod_doc.parts.iter_mut() {
        if let GeometryPayload::Mesh(ref mut mesh) = &mut part.geometry {
            mesh.vertices.clear();
            mesh.faces.clear();
            mesh.normals = None;
            mesh.uvs = None;
            mesh.face_groups = None;
        }
    }

    let mesh_entries: Vec<V2MeshEntry> = mesh_refs
        .iter()
        .map(|(part_idx, _)| V2MeshEntry {
            part: *part_idx,
            vertices: 0,
            faces: 0,
            normals: None,
            uvs: None,
            face_groups: None,
        })
        .collect();

    let v2json = V2Json {
        document: mod_doc,
        meshes: mesh_entries,
    };

    let json_bytes = serde_json::to_vec(&v2json)?;
    let json_offset: u64 = 24;
    let json_len = json_bytes.len() as u64;

    let mut arrow_buf = Cursor::new(Vec::new());
    if !batches.is_empty() {
        let first_schema = (*batches[0].schema()).clone();
        let mut writer = StreamWriter::try_new(&mut arrow_buf, &first_schema)
            .map_err(|e| IoError::Parse(format!("Arrow IPC writer creation: {}", e)))?;
        for batch in &batches {
            writer
                .write(batch)
                .map_err(|e| IoError::Parse(format!("Arrow IPC write: {}", e)))?;
        }
        writer
            .finish()
            .map_err(|e| IoError::Parse(format!("Arrow IPC finish: {}", e)))?;
    }
    let arrow_bytes = arrow_buf.into_inner();

    let total = json_offset as usize + json_len as usize + arrow_bytes.len();
    let mut out = Vec::with_capacity(total);

    out.extend_from_slice(MAGIC);
    out.push(BINARY_VERSION_V3);
    out.extend_from_slice(&[0u8; 3]);
    out.extend_from_slice(&json_offset.to_le_bytes());
    out.extend_from_slice(&json_len.to_le_bytes());
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&arrow_bytes);

    Ok(out)
}

fn from_binary_v3(bytes: &[u8]) -> Result<Document, IoError> {
    if bytes.len() < 24 {
        return Err(IoError::Parse("v3 header too short".into()));
    }
    let json_offset = u64::from_le_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ]);
    let json_len = u64::from_le_bytes([
        bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23],
    ]);

    let json_start = json_offset as usize;
    let json_end = json_start + json_len as usize;
    if json_end > bytes.len() {
        return Err(IoError::Parse("json section beyond file".into()));
    }
    let json_slice = &bytes[json_start..json_end];
    let v2json: V2Json = serde_json::from_slice(json_slice)?;
    let mut doc = v2json.document;

    let arrow_bytes = &bytes[json_end..];
    if arrow_bytes.is_empty() {
        return Ok(doc);
    }

    let cursor = Cursor::new(arrow_bytes.to_vec());
    let reader = StreamReader::try_new(cursor, None)
        .map_err(|e| IoError::Parse(format!("Arrow IPC reader: {}", e)))?;
    let batches: Vec<RecordBatch> = reader
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| IoError::Parse(format!("Arrow IPC read batches: {}", e)))?;

    let meshes = exl_arrow::record_batches_to_meshes(&batches);

    for (part_idx, mesh) in meshes {
        let part = doc
            .parts
            .get_mut(part_idx)
            .ok_or_else(|| IoError::Parse("mesh part index out of bounds".into()))?;
        let group_names = match &part.geometry {
            GeometryPayload::Mesh(m) => m.group_names.clone(),
            _ => Vec::new(),
        };
        part.geometry = GeometryPayload::Mesh(Mesh {
            group_names,
            ..mesh
        });
    }

    Ok(doc)
}

#[cfg(test)]
fn open_reader(bytes: &[u8]) -> Result<MappedExlb, IoError> {
    MappedExlb::from_storage(Storage::Owned(bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use exl_core::geom::{BRep, Mesh, Transform};
    use exl_core::*;
    use std::io::Write;

    fn test_doc() -> Document {
        let mesh = Mesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            faces: vec![[0, 1, 2]],
            ..Default::default()
        };
        Document::new(vec![Part::new("tri", GeometryPayload::Mesh(mesh))])
    }

    fn multi_part_doc() -> Document {
        let mesh = Mesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            faces: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            normals: Some(vec![
                [0.0, 0.0, -1.0],
                [0.0, -1.0, 0.0],
                [-1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
            ]),
            uvs: Some(vec![
                [0.0, 0.0],
                [1.0, 0.0],
                [0.0, 1.0],
                [1.0, 1.0],
            ]),
            face_groups: Some(vec![0, 0, 1, 1]),
            group_names: vec!["group_a".into(), "group_b".into()],
        };

        let brep = BRep::default();

        Document {
            parts: vec![
                Part::new("mesh_part", GeometryPayload::Mesh(mesh)),
                Part::new("brep_part", GeometryPayload::Brep(brep)),
            ],
            assembly: Assembly {
                instances: vec![Instance {
                    part_ref: "mesh_part".into(),
                    name: "inst1".into(),
                    transform: Transform::default(),
                }],
                mates: vec![],
            },
            ..Document::new(vec![])
        }
    }

    #[test]
    fn text_round_trip() {
        let doc = test_doc();
        let s = to_text(&doc);
        let back = from_text(&s).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn binary_round_trip() {
        let doc = test_doc();
        let data = to_binary(&doc).unwrap();
        let back = from_binary(&data).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn text_header_required() {
        let s = "{\"schema_version\":\"0.1\"}";
        assert!(from_text(s).is_err());
    }

    #[test]
    fn version_mismatch() {
        let s = "#exl 9.9\n{}";
        assert!(from_text(s).is_err());
    }

    #[test]
    fn bad_magic() {
        let buf = b"xxxxrest".to_vec();
        assert!(from_binary(&buf).is_err());
    }

    #[test]
    fn binary_too_short() {
        assert!(from_binary(b"EXLB\x01").is_err());
    }

    #[test]
    fn binary_bad_version() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"EXLB");
        buf.push(9);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.push(b'{');
        assert!(from_binary(&buf).is_err());
    }

    #[test]
    fn save_load_dispatch() {
        let doc = test_doc();
        let dir = std::env::temp_dir().join("exl-io-test-save-load");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let txt_path = dir.join("test.exl");
        save(&doc, &txt_path).unwrap();
        let loaded = load(&txt_path).unwrap();
        assert_eq!(doc, loaded);

        let bin_path = dir.join("test.exlb");
        save(&doc, &bin_path).unwrap();
        let loaded = load(&bin_path).unwrap();
        assert_eq!(doc, loaded);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sniff_text_without_extension() {
        let doc = test_doc();
        let dir = std::env::temp_dir().join("exl-io-test-sniff");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("noext");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(f, "#exl 0.1\n").unwrap();
            serde_json::to_writer(&mut f, &doc).unwrap();
        }
        let loaded = load(&path).unwrap();
        assert_eq!(doc, loaded);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sniff_binary_without_extension() {
        let doc = test_doc();
        let dir = std::env::temp_dir().join("exl-io-test-sniff-bin");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("noext");
        let data = to_binary(&doc).unwrap();
        std::fs::write(&path, &data).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(doc, loaded);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unchanged_after_refresh() {
        let mut doc = test_doc();
        let s1 = to_text(&doc);
        doc.refresh_content_hash();
        let s2 = to_text(&doc);
        assert_eq!(s1, s2);
    }

    #[test]
    fn three_part_doc_text_round_trip() {
        let m1 = Mesh {
            vertices: vec![[0.0; 3]],
            faces: vec![[0, 0, 0]],
            ..Default::default()
        };
        let m2 = Mesh {
            vertices: vec![[1.0; 3]],
            faces: vec![[0, 0, 0]],
            ..Default::default()
        };
        let m3 = Mesh {
            vertices: vec![[2.0; 3]],
            faces: vec![[0, 0, 0]],
            ..Default::default()
        };
        let doc = Document::new(vec![
            Part::new("a", GeometryPayload::Mesh(m1)),
            Part::new("b", GeometryPayload::Mesh(m2)),
            Part::new("c", GeometryPayload::Mesh(m3)),
        ]);
        let s = to_text(&doc);
        let back = from_text(&s).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn v3_round_trip_simple() {
        let doc = test_doc();
        let data = to_binary(&doc).unwrap();
        assert_eq!(data[4], BINARY_VERSION_V3);
        let back = from_binary(&data).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn v3_round_trip_multi_part() {
        let doc = multi_part_doc();
        let data = to_binary(&doc).unwrap();
        assert_eq!(data[4], BINARY_VERSION_V3);
        let back = from_binary(&data).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn v3_round_trip_multi_part_twice() {
        let doc = multi_part_doc();
        let data = to_binary(&doc).unwrap();
        let back1 = from_binary(&data).unwrap();
        let back2 = from_binary(&data).unwrap();
        assert_eq!(doc, back1);
        assert_eq!(doc, back2);
        assert_eq!(back1, back2);
    }

    #[test]
    fn v2_round_trip_backward_compat() {
        let doc = test_doc();
        let data = to_binary_v2(&doc).unwrap();
        assert_eq!(data[4], BINARY_VERSION_V2);
        let back = from_binary(&data).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn v2_multi_part_backward_compat() {
        let doc = multi_part_doc();
        let data = to_binary_v2(&doc).unwrap();
        assert_eq!(data[4], BINARY_VERSION_V2);
        let back = from_binary(&data).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn v1_payload_still_readable() {
        let doc = test_doc();
        let json = serde_json::to_vec(&doc).unwrap();
        let payload_len = json.len() as u32;
        let mut v1_buf = Vec::new();
        v1_buf.extend_from_slice(MAGIC);
        v1_buf.push(BINARY_VERSION_V1);
        v1_buf.extend_from_slice(&payload_len.to_le_bytes());
        v1_buf.extend_from_slice(&json);
        let back = from_binary(&v1_buf).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn mapped_exlb_open_round_trip() {
        let doc = multi_part_doc();
        let dir = std::env::temp_dir().join("exl-io-test-mapped");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("test.exlb");
        let data = to_binary_v2(&doc).unwrap();
        std::fs::write(&path, &data).unwrap();

        let mapped = MappedExlb::open(&path).unwrap();
        assert_eq!(mapped.mesh_count(), 1);

        let meta = mapped.document_meta().unwrap();
        if let GeometryPayload::Mesh(ref m) = &meta.parts[0].geometry {
            assert!(m.vertices.is_empty());
            assert!(m.faces.is_empty());
            assert!(m.normals.is_none());
            assert!(m.uvs.is_none());
            assert!(m.face_groups.is_none());
            assert_eq!(m.group_names.len(), 2);
        } else {
            panic!("expected mesh part");
        }

        let view = mapped.mesh_view(0).unwrap();
        assert_eq!(view.vertices.len(), 4 * 3);
        assert_eq!(view.faces.len(), 4 * 3);
        assert!(view.normals.is_some());
        assert!(view.uvs.is_some());
        assert!(view.face_groups.is_some());

        let full = mapped.to_document().unwrap();
        assert_eq!(doc, full);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mapped_exlb_mesh_view_matches_original() {
        let mesh = Mesh {
            vertices: vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]],
            faces: vec![[0, 1, 2]],
            normals: Some(vec![[0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]]),
            uvs: Some(vec![[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]]),
            face_groups: Some(vec![42]),
            group_names: vec!["main".into()],
        };
        let doc = Document::new(vec![Part::new("m", GeometryPayload::Mesh(mesh))]);

        let dir = std::env::temp_dir().join("exl-io-test-mapped-view");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.exlb");
        let data = to_binary_v2(&doc).unwrap();
        std::fs::write(&path, &data).unwrap();

        let mapped = MappedExlb::open(&path).unwrap();
        let view = mapped.mesh_view(0).unwrap();

        assert_eq!(view.vertices, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        assert_eq!(view.faces, [0, 1, 2]);
        assert_eq!(
            view.normals.unwrap(),
            [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]
        );
        assert_eq!(view.uvs.unwrap(), [0.0, 0.0, 1.0, 0.0, 0.5, 1.0]);
        assert_eq!(view.face_groups.unwrap(), [42]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn v3_empty_mesh_round_trip() {
        let mesh = Mesh::default();
        let doc = Document::new(vec![Part::new("empty", GeometryPayload::Mesh(mesh))]);
        let data = to_binary(&doc).unwrap();
        let back = from_binary(&data).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn v2_empty_mesh_backward_compat() {
        let mesh = Mesh::default();
        let doc = Document::new(vec![Part::new("empty", GeometryPayload::Mesh(mesh))]);
        let data = to_binary_v2(&doc).unwrap();
        let back = from_binary(&data).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn v2_misaligned_buffer_rejected() {
        let doc = test_doc();
        let mut data = to_binary_v2(&doc).unwrap();

        let buffer_count = u32::from_le_bytes([data[24], data[25], data[26], data[27]]) as usize;
        if buffer_count > 0 {
            let mut base = 32;
            for _i in 0..buffer_count {
                let off_bytes = [
                    data[base],
                    data[base + 1],
                    data[base + 2],
                    data[base + 3],
                    data[base + 4],
                    data[base + 5],
                    data[base + 6],
                    data[base + 7],
                ];
                let offset = u64::from_le_bytes(off_bytes);
                if offset >= 32 && offset % 64 == 0 {
                    let mut corrupt_offset = offset + 4;
                    while corrupt_offset % 64 == 0 {
                        corrupt_offset += 4;
                    }
                    data[base..base + 8].copy_from_slice(&corrupt_offset.to_le_bytes());
                    break;
                }
                base += 16;
            }
        }

        assert!(open_reader(&data).is_err());
    }

    #[test]
    fn v2_corrupt_buffer_table_rejected() {
        let doc = test_doc();
        let mut data = to_binary_v2(&doc).unwrap();
        let buffer_count = u32::from_le_bytes([data[24], data[25], data[26], data[27]]) as usize;
        if buffer_count > 0 {
            let fake_end = data.len() as u64;
            data[40..48].copy_from_slice(&fake_end.to_le_bytes());
            data[48..56].copy_from_slice(&100u64.to_le_bytes());
        }
        let result = from_binary(&data);
        assert!(result.is_err());
    }

    #[test]
    fn mapped_exlb_empty_mesh_doc() {
        let doc = test_doc();
        let dir = std::env::temp_dir().join("exl-io-test-mapped-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.exlb");
        let data = to_binary_v2(&doc).unwrap();
        std::fs::write(&path, &data).unwrap();

        let mapped = MappedExlb::open(&path).unwrap();
        let meta = mapped.document_meta().unwrap();
        assert!(!meta.parts.is_empty());

        let full = mapped.to_document().unwrap();
        assert_eq!(doc, full);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
