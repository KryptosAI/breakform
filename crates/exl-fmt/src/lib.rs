use exl_core::geom::Mesh;
use exl_core::{Document, GeometryPayload, Part, ToolOfOrigin};

#[derive(Debug, thiserror::Error)]
pub enum FmtError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Unsupported: {0}")]
    Unsupported(String),
}

pub(crate) fn iso8601_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs() as i64;
    let sec = (total_secs % 60) as u32;
    let total_mins = total_secs / 60;
    let min = (total_mins % 60) as u32;
    let total_hours = total_mins / 60;
    let hour = (total_hours % 24) as u32;
    let total_days = total_hours / 24;
    let z = total_days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hour, min, sec
    )
}

pub(crate) fn fresh_doc(parts: Vec<Part>, tool_name: &str) -> Document {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let ts = iso8601_now();
    let mut doc = Document::new(parts);
    doc.provenance.tool_of_origin = Some(ToolOfOrigin {
        name: tool_name.into(),
        version,
        timestamp_iso: ts,
    });
    doc.refresh_content_hash();
    doc
}

pub(crate) fn doc_meshes(doc: &Document) -> Result<Vec<(&Part, &Mesh)>, FmtError> {
    let mut out = Vec::new();
    for part in &doc.parts {
        match &part.geometry {
            GeometryPayload::Mesh(m) => out.push((part, m)),
            GeometryPayload::Brep(_) => {
                return Err(FmtError::Unsupported(
                    "BRep geometry not supported for mesh format export".into(),
                ));
            }
        }
    }
    Ok(out)
}

mod obj;
mod stl;

pub use obj::{export_obj, import_obj};
pub use stl::{export_stl, import_stl};
