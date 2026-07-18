use std::path::Path;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use exl_validate::Profile;

fn extension(p: &str) -> &str {
    Path::new(p)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
}

fn is_openfoam_case(p: &str) -> bool {
    let path = Path::new(p);
    path.is_dir() && path.join("constant").join("polyMesh").is_dir()
}

#[pyfunction]
#[pyo3(signature = (input, output, *, fidelity_report=None, format_from=None, format_to=None))]
fn convert(
    input: &str,
    output: &str,
    fidelity_report: Option<&str>,
    format_from: Option<&str>,
    format_to: Option<&str>,
) -> PyResult<String> {
    let in_ext = format_from.unwrap_or_else(|| extension(input));
    let out_ext = format_to.unwrap_or_else(|| extension(output));

    if matches!(out_ext, "step" | "stp") {
        return Err(PyValueError::new_err(
            "unsupported output format: STEP export is not available",
        ));
    }

    let (doc, import_report) = match in_ext {
        "exl" | "exlb" => {
            let doc = exl_io::load(Path::new(input))
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            (doc, None)
        }
        "step" | "stp" => {
            let (doc, report) = exl_step::import_step(Path::new(input))
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            (doc, Some(report))
        }
        "stl" => {
            let (doc, report) = exl_fmt::import_stl(Path::new(input))
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            (doc, Some(report))
        }
        "obj" => {
            let (doc, report) = exl_fmt::import_obj(Path::new(input))
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            (doc, Some(report))
        }
        "glb" => {
            let (doc, report) = exl_gltf::import_gltf(Path::new(input))
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            (doc, Some(report))
        }
        "bdf" | "dat" => {
            let (doc, report) = exl_nastran::import_nastran(Path::new(input))
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            (doc, Some(report))
        }
        "inp" => {
            let (doc, report) = exl_abaqus::import_abaqus(Path::new(input))
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            (doc, Some(report))
        }
        _ => {
            if is_openfoam_case(input) {
                let (doc, report) = exl_openfoam::import_openfoam(Path::new(input))
                    .map_err(|e| PyValueError::new_err(e.to_string()))?;
                (doc, Some(report))
            } else {
                return Err(PyValueError::new_err(format!(
                    "unknown input extension '{}' — expected .exl, .exlb, .step, .stp, .stl, .obj, .glb, .bdf, .dat, .inp, or an OpenFOAM case directory",
                    in_ext
                )));
            }
        }
    };

    let export_report = match out_ext {
        "exl" | "exlb" => {
            exl_io::save(&doc, Path::new(output))
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            None
        }
        "stl" => Some(
            exl_fmt::export_stl(&doc, Path::new(output), false)
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
        ),
        "obj" => Some(
            exl_fmt::export_obj(&doc, Path::new(output))
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
        ),
        "glb" => Some(
            exl_gltf::export_gltf(&doc, Path::new(output))
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
        ),
        _ => {
            return Err(PyValueError::new_err(format!(
                "unknown output extension '{}' — expected .exl, .exlb, .stl, .obj, or .glb",
                out_ext
            )));
        }
    };

    let json = match (import_report, export_report) {
        (Some(ir), Some(er)) => serde_json::to_string(&vec![ir, er]).unwrap(),
        (Some(r), None) | (None, Some(r)) => serde_json::to_string(&vec![r]).unwrap(),
        (None, None) => "[]".to_string(),
    };

    if let Some(report_path) = fidelity_report {
        std::fs::write(report_path, &json)
            .map_err(|e| PyValueError::new_err(format!("failed to write fidelity report: {}", e)))?;
    }

    Ok(json)
}

#[pyfunction]
fn load_json(path: &str) -> PyResult<String> {
    let doc = exl_io::load(Path::new(path))
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    serde_json::to_string(&doc).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn validate(path: &str, profile: &str) -> PyResult<String> {
    let doc = exl_io::load(Path::new(path))
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let profile: Profile = profile
        .parse()
        .map_err(|e| PyValueError::new_err(e))?;
    let findings = exl_validate::validate(&doc, profile);
    serde_json::to_string(&findings).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn diff(a: &str, b: &str) -> PyResult<String> {
    let doc_a = exl_io::load(Path::new(a))
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let doc_b = exl_io::load(Path::new(b))
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let report = exl_diff::diff(&doc_a, &doc_b);
    serde_json::to_string(&report).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn content_hash(path: &str) -> PyResult<String> {
    let doc = exl_io::load(Path::new(path))
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(doc.provenance.content_hash)
}

#[pyfunction]
fn save_document(json: &str, path: &str) -> PyResult<String> {
    let doc: exl_core::Document = serde_json::from_str(json)
        .map_err(|e| PyValueError::new_err(format!("invalid document JSON: {}", e)))?;
    exl_io::save(&doc, Path::new(path))
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(doc.provenance.content_hash)
}

#[pyfunction]
#[pyo3(signature = (path, *, format="text"))]
fn info(path: &str, format: &str) -> PyResult<String> {
    let doc = exl_io::load(Path::new(path))
        .map_err(|e| PyValueError::new_err(e.to_string()))?;

    if format == "json" {
        return serde_json::to_string_pretty(&doc)
            .map_err(|e| PyValueError::new_err(e.to_string()));
    }

    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let mut out = String::new();
    out.push_str(&format!("name: {}\n", stem));
    out.push_str(&format!("schema_version: {}\n", doc.schema_version));
    out.push_str(&format!("content_hash: {}\n", doc.provenance.content_hash));
    out.push_str(&format!("part count: {}\n", doc.parts.len()));

    for part in &doc.parts {
        out.push_str(&format!("  {}: {}", part.id, part.name));
        match &part.geometry {
            exl_core::GeometryPayload::Mesh(m) => {
                out.push_str(&format!(" — mesh (verts={}, faces={})", m.vertices.len(), m.faces.len()));
                if m.is_watertight() {
                    out.push_str(" watertight");
                }
            }
            exl_core::GeometryPayload::Brep(b) => {
                out.push_str(&format!(
                    " — brep (verts={}, edges={}, faces={})",
                    b.vertices.len(),
                    b.edges.len(),
                    b.faces.len()
                ));
            }
        }

        if let Some(bb) = &part.bounding_box {
            out.push_str(&format!(
                " bbox=[{:.3},{:.3},{:.3}][{:.3},{:.3},{:.3}]",
                bb.min[0], bb.min[1], bb.min[2], bb.max[0], bb.max[1], bb.max[2],
            ));
        }
        out.push('\n');
    }

    Ok(out)
}

#[pymodule]
fn exl(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(convert, m)?)?;
    m.add_function(wrap_pyfunction!(load_json, m)?)?;
    m.add_function(wrap_pyfunction!(validate, m)?)?;
    m.add_function(wrap_pyfunction!(diff, m)?)?;
    m.add_function(wrap_pyfunction!(content_hash, m)?)?;
    m.add_function(wrap_pyfunction!(save_document, m)?)?;
    m.add_function(wrap_pyfunction!(info, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
