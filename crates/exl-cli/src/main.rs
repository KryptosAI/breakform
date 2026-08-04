use std::path::{Path, PathBuf};
use std::process::{self, Command as ProcessCommand, Stdio};

use clap::{Parser, Subcommand, ValueEnum};
use exl_core::Document;
use exl_diff::diff;
use exl_validate::{validate, Finding, Profile, Severity};

#[derive(Parser)]
#[command(
    name = "bf",
    version = "1.0.0",
    about = "Breakform — Break the format. Keep the truth.",
    long_about = "Breakform — Break the format. Keep the truth.\n\nConvert, validate, diff, and inspect engineering data with honest fidelity reports."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Convert between formats")]
    Convert {
        input: PathBuf,
        output: PathBuf,
        #[arg(long)]
        fidelity_report: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        ascii: bool,
        #[arg(long = "export-format", short = 'f')]
        export_format: Option<String>,
        #[arg(long = "meshio", short = 'm', default_value_t = false)]
        meshio: bool,
    },

    #[command(about = "Validate a native EXL document")]
    Validate {
        #[arg(value_enum, short, long)]
        profile: ValidateProfile,
        file: PathBuf,
    },

    #[command(about = "Diff two native EXL documents")]
    Diff { a: PathBuf, b: PathBuf },

    #[command(about = "Show document info")]
    Info {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, ValueEnum)]
enum ValidateProfile {
    Mech,
    Cfd,
    Fea,
    Strict,
}

fn extension(path: &Path) -> &str {
    path.extension().and_then(|e| e.to_str()).unwrap_or("")
}

fn import_doc(input: &Path) -> Result<(Document, Option<exl_core::FidelityReport>), String> {
    let ext = extension(input);
    match ext {
        "exl" | "exlb" => exl_io::load(input)
            .map(|d| (d, None))
            .map_err(|e| format!("failed to load {}: {}", input.display(), e)),
        "step" | "stp" => exl_step::import_step(input)
            .map(|(d, r)| (d, Some(r)))
            .map_err(|e| format!("failed to import step: {}", e)),
        "stl" => exl_fmt::import_stl(input)
            .map(|(d, r)| (d, Some(r)))
            .map_err(|e| format!("failed to import stl: {}", e)),
        "obj" => exl_fmt::import_obj(input)
            .map(|(d, r)| (d, Some(r)))
            .map_err(|e| format!("failed to import obj: {}", e)),
        "glb" => exl_gltf::import_gltf(input)
            .map(|(d, r)| (d, Some(r)))
            .map_err(|e| format!("failed to import gltf: {}", e)),
        "bdf" | "dat" => exl_nastran::import_nastran(input)
            .map(|(d, r)| (d, Some(r)))
            .map_err(|e| format!("failed to import nastran: {}", e)),
        "inp" => exl_abaqus::import_abaqus(input)
            .map(|(d, r)| (d, Some(r)))
            .map_err(|e| format!("failed to import abaqus: {}", e)),
        _ => {
            if input.is_dir() && input.join("constant").join("polyMesh").exists() {
                exl_openfoam::import_openfoam(input)
                    .map(|(d, r)| (d, Some(r)))
                    .map_err(|e| format!("failed to import openfoam: {}", e))
            } else {
                Err(format!(
                    "unknown input format for '{}' — expected .exl, .exlb, .step, .stp, .stl, .obj, .glb, .bdf, .dat, .inp, or an OpenFOAM case directory",
                    ext
                ))
            }
        }
    }
}

fn load_native(path: &Path) -> Result<Document, String> {
    let ext = extension(path);
    match ext {
        "exl" | "exlb" => {
            exl_io::load(path).map_err(|e| format!("failed to load {}: {}", path.display(), e))
        }
        _ => Err(format!(
            "'{}' is not a native EXL file (.exl/.exlb). Convert it first with `bf convert`.",
            path.display()
        )),
    }
}

fn fidelity_label(json_str: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return "n/a".into(),
    };
    if let Some(arr) = v.as_array() {
        arr.iter()
            .filter_map(|r| r.get("overall").and_then(|o| o.as_str()))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        v.get("overall")
            .and_then(|o| o.as_str())
            .unwrap_or("n/a")
            .to_string()
    }
}

fn run_meshio_bridge(py_code: &str) -> Result<String, String> {
    let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".into());
    let child = ProcessCommand::new(&python)
        .args(["-c", py_code])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {}", python, e))?;

    let output = child
        .wait_with_output()
        .map_err(|e| format!("meshio bridge failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("meshio bridge error: {}", stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn import_via_meshio(
    input: &Path,
    output: &Path,
) -> Result<(Document, Option<exl_core::FidelityReport>), String> {
    let exl_py_dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("crates")
        .join("exl-py")
        .join("python");

    let exl_py_dir_str = exl_py_dir.to_string_lossy();

    let script = format!(
        r#"
import sys, json
sys.path.insert(0, {exl_py_dir:?})
from exl.meshio_bridge import import_via_meshio as ivm
doc, fid = ivm({input:?})
result = {{"doc": doc, "fid": fid}}
print(json.dumps(result))
"#,
        exl_py_dir = exl_py_dir_str,
        input = input.to_string_lossy(),
    );

    let stdout = run_meshio_bridge(&script)?;
    let result: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|e| format!("meshio bridge parse: {}", e))?;

    let fid_val = result["fid"].clone();
    let _fid_str = serde_json::to_string(&fid_val).unwrap_or_default();

    let fid: exl_core::FidelityReport =
        serde_json::from_value(fid_val).map_err(|e| format!("fidelity parse: {}", e))?;

    let doc_dict = &result["doc"];
    let doc: Document =
        serde_json::from_value(doc_dict.clone()).map_err(|e| format!("doc parse: {}", e))?;

    exl_io::save(&doc, output).map_err(|e| format!("save failed: {}", e))?;

    Ok((doc, Some(fid)))
}

fn export_via_meshio(
    input: &Path,
    output: &Path,
    format_hint: Option<&str>,
) -> Result<Option<exl_core::FidelityReport>, String> {
    let doc = load_native(input)?;
    let doc_dict = serde_json::to_value(&doc).map_err(|e| e.to_string())?;

    let exl_py_dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("crates")
        .join("exl-py")
        .join("python");
    let exl_py_dir_str = exl_py_dir.to_string_lossy();

    let fmt_arg = format_hint.map(|f| f.to_string()).unwrap_or_default();

    let script = format!(
        r#"
import sys, json
sys.path.insert(0, {exl_py_dir:?})
from exl.meshio_bridge import export_via_meshio as evm
doc = json.loads({doc_json:?})
result = evm(doc, {output:?}, format_hint={fmt:?} if {has_fmt} else None)
print(json.dumps(result))
"#,
        exl_py_dir = exl_py_dir_str,
        doc_json = serde_json::to_string(&doc_dict).unwrap_or_default(),
        output = output.to_string_lossy(),
        fmt = fmt_arg,
        has_fmt = format_hint.is_some(),
    );

    let stdout = run_meshio_bridge(&script)?;
    let result: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|e| format!("meshio bridge parse: {}", e))?;

    if let Some(err) = result.get("error") {
        return Err(err.as_str().unwrap_or("unknown error").to_string());
    }

    let fid: exl_core::FidelityReport =
        serde_json::from_value(result).map_err(|e| format!("fidelity parse: {}", e))?;

    Ok(Some(fid))
}

fn meshio_supported(ext: &str) -> bool {
    matches!(
        ext,
        "iges"
            | "igs"
            | "vtk"
            | "vtu"
            | "xdmf"
            | "ply"
            | "off"
            | "msh"
            | "gmsh"
            | "cgns"
            | "tecplot"
            | "med"
            | "exodus"
            | "su2"
            | "ugrid"
            | "h5m"
            | "ansys"
            | "fro"
            | "neu"
            | "permas"
            | "abaqus"
            | "fluent"
            | "nastran"
            | "gid"
            | "avsucd"
            | "mdpa"
            | "p3d"
            | "svg"
            | "wkt"
    )
}

fn convert(
    input: PathBuf,
    output: PathBuf,
    fidelity_report: Option<PathBuf>,
    ascii: bool,
    export_format: Option<String>,
    meshio: bool,
) -> Result<i32, String> {
    let effective_format = export_format.as_deref().unwrap_or(extension(&output));
    let out_ext = effective_format.to_lowercase();

    if matches!(out_ext.as_str(), "step" | "stp") {
        return Err("unsupported output format: STEP export is not available".to_string());
    }

    let in_ext = extension(&input);

    if meshio && meshio_supported(in_ext) {
        let intermediate =
            std::env::temp_dir().join(format!("bf_meshio_intermediate_{}.exl", std::process::id()));
        let (doc, import_report) = import_via_meshio(&input, &intermediate)?;

        let out_ext_eff = export_format.as_deref().unwrap_or(extension(&output));
        if meshio_supported(out_ext_eff) || meshio_supported(extension(&output)) {
            let export_report =
                export_via_meshio(&intermediate, &output, export_format.as_deref())?;

            let fi_str = if let Some(ref ir) = import_report {
                serde_json::to_string_pretty(&ir).unwrap_or_default()
            } else {
                String::new()
            };
            let fe_str = if let Some(ref er) = export_report {
                serde_json::to_string_pretty(&er).unwrap_or_default()
            } else {
                String::new()
            };

            if let Some(rp) = &fidelity_report {
                let merged = if !fi_str.is_empty() && !fe_str.is_empty() {
                    format!("[\n{},\n{}\n]", fi_str, fe_str)
                } else {
                    (if !fi_str.is_empty() { &fi_str } else { &fe_str }).to_string()
                };
                if !merged.is_empty() {
                    std::fs::write(rp, &merged)
                        .map_err(|e| format!("failed to write fidelity report: {}", e))?;
                }
            }

            let total_parts = doc.parts.len();
            let mut vert_sum = 0usize;
            let mut face_sum = 0usize;
            for p in &doc.parts {
                match &p.geometry {
                    exl_core::GeometryPayload::Mesh(m) => {
                        vert_sum += m.vertices.len();
                        face_sum += m.faces.len();
                    }
                    exl_core::GeometryPayload::Brep(b) => {
                        vert_sum += b.vertices.len();
                        face_sum += b.faces.len();
                    }
                }
            }
            println!("converted {} -> {}", input.display(), output.display());
            println!("parts: {}", total_parts);
            println!("total vertices: {}, total faces: {}", vert_sum, face_sum);
            println!(
                "overall fidelity: {}",
                fidelity_label(if !fi_str.is_empty() {
                    &fi_str
                } else if !fe_str.is_empty() {
                    &fe_str
                } else {
                    ""
                })
            );
            let _ = std::fs::remove_file(&intermediate);
            return Ok(0);
        }

        let _ = std::fs::remove_file(&intermediate);
        eprintln!(
            "error: meshio export format '{}' not recognized — use --meshio for meshio-supported output formats or remove --meshio for native export",
            out_ext_eff
        );
        process::exit(2);
    }

    let (doc, import_report) = import_doc(&input)?;

    let export_report = match out_ext.as_str() {
        "exl" | "exlb" => {
            exl_io::save(&doc, &output).map_err(|e| format!("failed to save: {}", e))?;
            None
        }
        "stl" => Some(
            exl_fmt::export_stl(&doc, &output, ascii)
                .map_err(|e| format!("failed to export stl: {}", e))?,
        ),
        "obj" => Some(
            exl_fmt::export_obj(&doc, &output)
                .map_err(|e| format!("failed to export obj: {}", e))?,
        ),
        "glb" => Some(
            exl_gltf::export_gltf(&doc, &output)
                .map_err(|e| format!("failed to export gltf: {}", e))?,
        ),
        "bdf" | "dat" => Some(
            exl_nastran::export_nastran(&doc, &output)
                .map_err(|e| format!("failed to export nastran: {}", e))?,
        ),
        "openfoam" => Some(
            exl_openfoam::export_openfoam(&doc, &output)
                .map_err(|e| format!("failed to export openfoam: {}", e))?,
        ),
        "inp" => Some(
            exl_abaqus::export_abaqus(&doc, &output)
                .map_err(|e| format!("failed to export abaqus: {}", e))?,
        ),
        _ => {
            eprintln!(
                "error: unknown output format '{}' — expected .exl, .exlb, .stl, .obj, .glb, .bdf, .dat, .inp, or --export-format openfoam",
                out_ext
            );
            process::exit(2);
        }
    };

    let report_json = match (import_report, export_report) {
        (Some(ir), Some(er)) => {
            let merged = vec![ir, er];
            serde_json::to_string_pretty(&merged).unwrap()
        }
        (Some(r), None) | (None, Some(r)) => serde_json::to_string_pretty(&r).unwrap(),
        (None, None) => String::new(),
    };

    if let Some(rp) = &fidelity_report {
        if !report_json.is_empty() {
            std::fs::write(rp, &report_json)
                .map_err(|e| format!("failed to write fidelity report: {}", e))?;
        }
    }

    let total_parts = doc.parts.len();
    let mut mesh_parts = 0usize;
    let mut brep_parts = 0usize;
    let mut vert_sum = 0usize;
    let mut face_sum = 0usize;

    for p in &doc.parts {
        match &p.geometry {
            exl_core::GeometryPayload::Mesh(m) => {
                mesh_parts += 1;
                vert_sum += m.vertices.len();
                face_sum += m.faces.len();
            }
            exl_core::GeometryPayload::Brep(b) => {
                brep_parts += 1;
                vert_sum += b.vertices.len();
                face_sum += b.faces.len();
            }
        }
    }

    println!("converted {} -> {}", input.display(), output.display());
    println!(
        "parts: {} ({} mesh, {} brep)",
        total_parts, mesh_parts, brep_parts
    );
    println!("total vertices: {}, total faces: {}", vert_sum, face_sum);
    println!("overall fidelity: {}", fidelity_label(&report_json));

    Ok(0)
}

fn cmd_validate(profile: ValidateProfile, file: PathBuf) -> Result<i32, String> {
    let p = match profile {
        ValidateProfile::Mech => Profile::Mech,
        ValidateProfile::Cfd => Profile::Cfd,
        ValidateProfile::Fea => Profile::Fea,
        ValidateProfile::Strict => Profile::Strict,
    };

    let doc = load_native(&file)?;
    let findings: Vec<Finding> = validate(&doc, p);

    let mut max_code = 0i32;

    for f in &findings {
        let (sev_str, code) = match &f.severity {
            Severity::Error => ("ERROR", 2i32),
            Severity::Warning => ("WARN", 1i32),
        };
        if code > max_code {
            max_code = code;
        }
        match &f.part {
            Some(part) => {
                println!("{} {}: {} [{}]", sev_str, f.check, f.message, part);
            }
            None => {
                println!("{} {}: {}", sev_str, f.check, f.message);
            }
        }
    }

    Ok(max_code)
}

fn cmd_diff(a: PathBuf, b: PathBuf) -> Result<i32, String> {
    let doc_a = load_native(&a)?;
    let doc_b = load_native(&b)?;
    let report = diff(&doc_a, &doc_b);
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    println!("{}", json);
    Ok(if report.is_empty() { 0 } else { 1 })
}

fn cmd_info(file: PathBuf, json: bool) -> Result<i32, String> {
    let doc = load_native(&file)?;

    if json {
        let s = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
        println!("{}", s);
        return Ok(0);
    }

    println!(
        "name: {}",
        file.file_stem().unwrap_or_default().to_string_lossy()
    );
    println!("schema_version: {}", doc.schema_version);
    println!("content_hash: {}", doc.provenance.content_hash);
    println!("part count: {}", doc.parts.len());

    for part in &doc.parts {
        print!("  {}: {} — ", part.id, part.name);
        match &part.geometry {
            exl_core::GeometryPayload::Mesh(m) => {
                print!("mesh (verts={}, faces={})", m.vertices.len(), m.faces.len());
                if m.is_watertight() {
                    print!(" watertight");
                }
            }
            exl_core::GeometryPayload::Brep(b) => {
                print!(
                    "brep (verts={}, edges={}, faces={})",
                    b.vertices.len(),
                    b.edges.len(),
                    b.faces.len()
                );
            }
        }

        if let Some(bb) = &part.bounding_box {
            print!(
                " bbox=[{:.3},{:.3},{:.3}][{:.3},{:.3},{:.3}]",
                bb.min[0], bb.min[1], bb.min[2], bb.max[0], bb.max[1], bb.max[2],
            );
        }
        println!();
    }

    Ok(0)
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Convert {
            input,
            output,
            fidelity_report,
            ascii,
            export_format,
            meshio,
        } => convert(input, output, fidelity_report, ascii, export_format, meshio),
        Command::Validate { profile, file } => cmd_validate(profile, file),
        Command::Diff { a, b } => cmd_diff(a, b),
        Command::Info { file, json } => cmd_info(file, json),
    };

    match result {
        Ok(code) => process::exit(code),
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(2);
        }
    }
}
