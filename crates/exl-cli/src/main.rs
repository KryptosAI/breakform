use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand, ValueEnum};
use exl_core::Document;
use exl_diff::diff;
use exl_validate::{validate, Finding, Profile, Severity};

#[derive(Parser)]
#[command(name = "eng", about = "EXL interoperability CLI", version)]
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
    },

    #[command(about = "Validate a native EXL document")]
    Validate {
        #[arg(value_enum, short, long)]
        profile: ValidateProfile,
        file: PathBuf,
    },

    #[command(about = "Diff two native EXL documents")]
    Diff {
        a: PathBuf,
        b: PathBuf,
    },

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

fn extension(path: &PathBuf) -> &str {
    path.extension().and_then(|e| e.to_str()).unwrap_or("")
}

fn import_doc(input: &PathBuf) -> Result<(Document, Option<exl_core::FidelityReport>), String> {
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
        _ => Err(format!(
            "unknown input extension '{}' — expected .exl, .exlb, .step, .stp, .stl, .obj, or .glb",
            ext
        )),
    }
}

fn load_native(path: &PathBuf) -> Result<Document, String> {
    let ext = extension(path);
    match ext {
        "exl" | "exlb" => {
            exl_io::load(path).map_err(|e| format!("failed to load {}: {}", path.display(), e))
        }
        _ => Err(format!(
            "'{}' is not a native EXL file (.exl/.exlb). Convert it first with `eng convert`.",
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

fn convert(
    input: PathBuf,
    output: PathBuf,
    fidelity_report: Option<PathBuf>,
    ascii: bool,
) -> Result<i32, String> {
    let out_ext = extension(&output);

    if matches!(out_ext, "step" | "stp") {
        return Err("unsupported output format: STEP export is not available".to_string());
    }

    let (doc, import_report) = import_doc(&input)?;

    let export_report = match out_ext {
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
        _ => {
            eprintln!(
                "error: unknown output extension '{}' — expected .exl, .exlb, .stl, .obj, or .glb",
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
    println!("parts: {} ({} mesh, {} brep)", total_parts, mesh_parts, brep_parts);
    println!("total vertices: {}, total faces: {}", vert_sum, face_sum);
    println!(
        "overall fidelity: {}",
        fidelity_label(&report_json)
    );

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
        } => convert(input, output, fidelity_report, ascii),
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
