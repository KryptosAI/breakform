use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::OnceLock;

static BIN: OnceLock<PathBuf> = OnceLock::new();

fn bin() -> &'static PathBuf {
    BIN.get_or_init(|| PathBuf::from(env!("CARGO_BIN_EXE_eng")))
}

fn corpus_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative)
}

fn run(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().unwrap()
}

fn run_in_dir(args: &[&str], dir: &PathBuf) -> Output {
    Command::new(bin())
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("exl-test-{}-{}", prefix, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn step_to_exl() {
    let dir = temp_dir("step_to_exl");
    let input = corpus_path("bracket.step");
    let out_exl = dir.join("out.exl");
    let report_json = dir.join("report.json");

    let output = run_in_dir(
        &[
            "convert",
            input.to_str().unwrap(),
            out_exl.to_str().unwrap(),
            "--fidelity-report",
            report_json.to_str().unwrap(),
        ],
        &dir,
    );

    assert!(output.status.success(), "convert failed: {:?}", output);
    assert!(out_exl.exists(), "out.exl does not exist");
    let exl_content = std::fs::read_to_string(&out_exl).unwrap();
    assert!(exl_content.starts_with("#exl"), "out.exl does not start with #exl");

    assert!(report_json.exists(), "report.json does not exist");
    let report_raw = std::fs::read_to_string(&report_json).unwrap();
    let report: serde_json::Value = serde_json::from_str(&report_raw)
        .expect("report.json is not valid JSON");
    let source_format = report
        .get("source_format")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(!source_format.is_empty(), "source_format is empty");
}

#[test]
fn stl_roundtrip_pipeline() {
    let dir = temp_dir("stl_rtt");
    let input = corpus_path("cube-ascii.stl");

    let cube_exl = dir.join("cube.exl");
    let cube_obj = dir.join("cube.obj");
    let cube2_exl = dir.join("cube2.exl");

    let o1 = run_in_dir(
        &[
            "convert",
            input.to_str().unwrap(),
            cube_exl.to_str().unwrap(),
        ],
        &dir,
    );
    assert!(o1.status.success(), "stl->exl failed: {:?}", o1);

    let o2 = run_in_dir(
        &[
            "convert",
            cube_exl.to_str().unwrap(),
            cube_obj.to_str().unwrap(),
        ],
        &dir,
    );
    assert!(o2.status.success(), "exl->obj failed: {:?}", o2);

    let o3 = run_in_dir(
        &[
            "convert",
            cube_obj.to_str().unwrap(),
            cube2_exl.to_str().unwrap(),
        ],
        &dir,
    );
    assert!(o3.status.success(), "obj->exl failed: {:?}", o3);

    let info1 = run_in_dir(&["info", cube_exl.to_str().unwrap()], &dir);
    assert!(info1.status.success(), "info cube.exl failed: {:?}", info1);
    let stdout1 = String::from_utf8_lossy(&info1.stdout);

    let info2 = run_in_dir(&["info", cube2_exl.to_str().unwrap()], &dir);
    assert!(info2.status.success(), "info cube2.exl failed: {:?}", info2);
    let stdout2 = String::from_utf8_lossy(&info2.stdout);

    let count1 = extract_vertex_count(&stdout1);
    let count2 = extract_vertex_count(&stdout2);
    assert_eq!(count1, count2, "vertex counts differ: {} vs {}", count1, count2);
}

fn extract_part_count(stdout: &str) -> usize {
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("part count: ") {
            return rest.trim().parse().unwrap_or(0);
        }
    }
    0
}

fn extract_vertex_count(stdout: &str) -> usize {
    let mut total = 0usize;
    for line in stdout.lines() {
        if let Some(start) = line.find("verts=") {
            let after = &line[start + 6..];
            if let Some(end) = after.find(',') {
                if let Ok(n) = after[..end].trim().parse::<usize>() {
                    total += n;
                }
            }
        }
    }
    total
}

#[test]
fn exlb_binary_path() {
    let dir = temp_dir("exlb_path");
    let input = corpus_path("cube-ascii.stl");
    let out_exlb = dir.join("cube.exlb");

    let o1 = run_in_dir(
        &[
            "convert",
            input.to_str().unwrap(),
            out_exlb.to_str().unwrap(),
        ],
        &dir,
    );
    assert!(o1.status.success(), "stl->exlb failed: {:?}", o1);

    let info = run_in_dir(&["info", out_exlb.to_str().unwrap()], &dir);
    assert!(info.status.success(), "info exlb failed: {:?}", info);
    let stdout = String::from_utf8_lossy(&info.stdout);
    assert!(
        stdout.contains("part count: 1"),
        "expected 'part count: 1' in: {}",
        stdout
    );
}

#[test]
fn validate_exit_codes() {
    let dir = temp_dir("validate_codes");
    let input = corpus_path("quad.obj");
    let out_exl = dir.join("quad.exl");

    let o = run_in_dir(
        &[
            "convert",
            input.to_str().unwrap(),
            out_exl.to_str().unwrap(),
        ],
        &dir,
    );
    assert!(o.status.success(), "obj->exl failed: {:?}", o);

    let mech = run_in_dir(
        &["validate", "--profile", "mech", out_exl.to_str().unwrap()],
        &dir,
    );
    let mech_code = mech.status.code().unwrap_or(99);
    assert!(
        mech_code == 0 || mech_code == 1,
        "mech exit code was {} (expected 0 or 1)",
        mech_code
    );

    let strict = run_in_dir(
        &["validate", "--profile", "strict", out_exl.to_str().unwrap()],
        &dir,
    );
    let strict_code = strict.status.code().unwrap_or(99);
    assert_eq!(
        strict_code, 2,
        "strict exit code was {} (expected 2)\nstdout: {}\nstderr: {}",
        strict_code,
        String::from_utf8_lossy(&strict.stdout),
        String::from_utf8_lossy(&strict.stderr),
    );
}

#[test]
fn diff_self_empty() {
    let dir = temp_dir("diff_self");
    let input = corpus_path("cube-ascii.stl");
    let a_exl = dir.join("a.exl");
    let b_exl = dir.join("b.exl");

    let o1 = run_in_dir(
        &[
            "convert",
            input.to_str().unwrap(),
            a_exl.to_str().unwrap(),
        ],
        &dir,
    );
    assert!(o1.status.success(), "first convert failed: {:?}", o1);

    std::fs::copy(&a_exl, &b_exl).unwrap();

    let diff_out = run_in_dir(
        &["diff", a_exl.to_str().unwrap(), b_exl.to_str().unwrap()],
        &dir,
    );
    assert!(
        diff_out.status.success(),
        "diff exit code was {}",
        diff_out.status.code().unwrap_or(99)
    );
    let stdout = String::from_utf8_lossy(&diff_out.stdout);
    let report: serde_json::Value =
        serde_json::from_str(&stdout).expect("diff output is not valid JSON");
    let topo = report
        .get("topology")
        .unwrap_or_else(|| panic!("missing topology field in: {}", stdout));
    let added = topo
        .get("added")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("expected added array in: {}", stdout));
    let removed = topo
        .get("removed")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("expected removed array in: {}", stdout));
    let modified = topo
        .get("modified")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("expected modified array in: {}", stdout));
    assert!(added.is_empty(), "added not empty: {:?}", added);
    assert!(removed.is_empty(), "removed not empty: {:?}", removed);
    assert!(modified.is_empty(), "modified not empty: {:?}", modified);
}

#[test]
fn unknown_extension_fails() {
    let dir = temp_dir("unknown_ext");
    let input = corpus_path("cube-ascii.stl");
    let out_xyz = dir.join("cube.xyz");

    let output = run_in_dir(
        &[
            "convert",
            input.to_str().unwrap(),
            out_xyz.to_str().unwrap(),
        ],
        &dir,
    );
    let code = output.status.code().unwrap_or(99);
    assert_eq!(
        code, 2,
        "expected exit code 2, got {}",
        code
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.is_empty(),
        "expected stderr message, got empty"
    );
}

#[test]
fn gltf_roundtrip() {
    let dir = temp_dir("gltf_rtt");
    let input = corpus_path("cube-ascii.stl");
    let ref_exl = dir.join("ref.exl");
    let cube_glb = dir.join("cube.glb");
    let cube2_exl = dir.join("cube2.exl");

    let o0 = run_in_dir(
        &[
            "convert",
            input.to_str().unwrap(),
            ref_exl.to_str().unwrap(),
        ],
        &dir,
    );
    assert!(o0.status.success(), "stl->exl failed: {:?}", o0);

    let o1 = run_in_dir(
        &[
            "convert",
            input.to_str().unwrap(),
            cube_glb.to_str().unwrap(),
        ],
        &dir,
    );
    assert!(o1.status.success(), "stl->glb failed: {:?}", o1);

    let o2 = run_in_dir(
        &[
            "convert",
            cube_glb.to_str().unwrap(),
            cube2_exl.to_str().unwrap(),
        ],
        &dir,
    );
    assert!(o2.status.success(), "glb->exl failed: {:?}", o2);

    let info_ref = run_in_dir(&["info", ref_exl.to_str().unwrap()], &dir);
    let info_back = run_in_dir(&["info", cube2_exl.to_str().unwrap()], &dir);
    assert!(
        info_ref.status.success() && info_back.status.success(),
        "info commands failed"
    );
    let stdout_ref = String::from_utf8_lossy(&info_ref.stdout);
    let stdout_back = String::from_utf8_lossy(&info_back.stdout);

    let count_ref = extract_vertex_count(&stdout_ref);
    let count_back = extract_vertex_count(&stdout_back);
    assert_eq!(
        count_ref, count_back,
        "vertex counts differ: {} vs {}",
        count_ref, count_back
    );
}
