#!/usr/bin/env python3
"""EXL benchmark runner & dashboard builder.

Usage:
  python3 bench/run_bench.py              # build + run benchmarks
  python3 bench/run_bench.py --no-build    # skip cargo build
  python3 bench/run_bench.py --dashboard-only  # rebuild HTML from existing results.json
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
BENCH_DIR = REPO_ROOT / "bench"
CORPUS_DIR = REPO_ROOT / "corpus"
TARGET_RELEASE = REPO_ROOT / "target" / "release" / "bf"
TARGET_DEBUG = REPO_ROOT / "target" / "debug" / "bf"
RESULTS_PATH = BENCH_DIR / "results.json"
DASHBOARD_PATH = BENCH_DIR / "index.html"

TIMEOUT_SEC = 30
CORPUS_GLOBS = ["*.stl", "*.obj", "*.step", "*.stp", "*.glb", "*.bdf", "*.dat", "*.inp"]


def find_binary():
    """Return path to bf binary. Prefer release, else debug."""
    if TARGET_RELEASE.exists():
        return str(TARGET_RELEASE)
    if TARGET_DEBUG.exists():
        return str(TARGET_DEBUG)
    return None


def build_binary():
    """Run cargo build --release -p exl-cli. Returns True on success."""
    print("[bench] building release binary (cargo build --release -p exl-cli)...")
    result = subprocess.run(
        ["cargo", "build", "--release", "-p", "exl-cli"],
        cwd=str(REPO_ROOT),
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print("[bench] build failed:", file=sys.stderr)
        print(result.stderr[-2000:], file=sys.stderr)
        return False
    if TARGET_RELEASE.exists():
        return True
    return False


def ensure_binary(skip_build=False):
    """Ensure we have a binary to run, building if needed. Returns path or None."""
    bin_path = find_binary()
    if bin_path:
        print(f"[bench] using binary: {bin_path}")
        return bin_path

    if skip_build:
        print("[bench] --no-build set and no binary found", file=sys.stderr)
        return None

    if build_binary():
        return str(TARGET_RELEASE)

    # Retry logic: sleep 90 then retry once
    print("[bench] build failed; waiting 90s then retrying once...")
    time.sleep(90)
    if build_binary():
        return str(TARGET_RELEASE)

    # Fallback: check debug again (maybe a sibling built it)
    bin_path = find_binary()
    if bin_path:
        print(f"[bench] fallback to existing binary: {bin_path}")
        return bin_path

    print("[bench] no binary available after all attempts", file=sys.stderr)
    return None


def gather_corpus_files():
    """Glob corpus/ for all supported formats (files + OpenFOAM case dirs). Returns list of Path objects sorted by name."""
    files = []
    for pattern in CORPUS_GLOBS:
        files.extend(CORPUS_DIR.glob(pattern))
    for d in sorted(CORPUS_DIR.iterdir()):
        if d.is_dir() and (d / "constant" / "polyMesh").exists():
            files.append(d)
    files.sort(key=lambda p: p.name.lower())
    return files


def parse_fidelity_json(path):
    """Parse a fidelity JSON file. Returns dict with overall, lossless, approximate, degraded, dropped counts.

    Handles both single-object and array-of-objects fidelity report formats.
    Returns None if file missing or unparseable.
    """
    if not path or not os.path.exists(path):
        return None
    try:
        with open(path, "r") as fh:
            data = json.load(fh)
    except (json.JSONDecodeError, OSError):
        return None

    reports = data if isinstance(data, list) else [data]

    overall_map = {"lossless": 0, "approximate": 1, "degraded": 2}
    worst = "lossless"
    worst_rank = 0

    counts = {"lossless": 0, "approximate": 0, "degraded": 0, "dropped": 0}

    for rep in reports:
        ov = (rep.get("overall") or "").lower()
        rank = overall_map.get(ov, 0)
        if rank > worst_rank:
            worst = ov
            worst_rank = rank
        for ent in rep.get("entities", []):
            st = (ent.get("status") or "").lower()
            cnt = ent.get("count", 0)
            if st in counts:
                counts[st] += cnt

    return {"overall": worst, **counts}


def run_convert(binary, input_path, output_path, fidelity_path):
    """Run bf convert. Returns (exit_code, wall_ms, stderr_text)."""
    args = [
        binary,
        "convert",
        str(input_path),
        str(output_path),
        "--fidelity-report",
        str(fidelity_path),
    ]
    t0 = time.monotonic()
    try:
        result = subprocess.run(
            args,
            capture_output=True,
            text=True,
            timeout=TIMEOUT_SEC,
            cwd=str(REPO_ROOT),
        )
        elapsed = (time.monotonic() - t0) * 1000.0
        return result.returncode, elapsed, result.stderr
    except subprocess.TimeoutExpired:
        elapsed = TIMEOUT_SEC * 1000.0
        return -1, elapsed, "timeout"
    except OSError as e:
        elapsed = (time.monotonic() - t0) * 1000.0
        return -2, elapsed, str(e)


def format_from_suffix(path):
    """Return canonical format string from file suffix or directory type."""
    suf = path.suffix.lower()
    if path.is_dir() and (path / "constant" / "polyMesh").exists():
        return "openfoam"
    if suf == ".stl":
        return "stl"
    if suf == ".obj":
        return "obj"
    if suf in (".step", ".stp"):
        return "step"
    if suf == ".glb":
        return "glb"
    if suf in (".bdf", ".dat"):
        return "nastran"
    if suf == ".inp":
        return "abaqus"
    return suf.lstrip(".")


def first_stderr_line(stderr_text):
    """Return first non-empty line of stderr, or empty string."""
    if not stderr_text:
        return ""
    for line in stderr_text.splitlines():
        stripped = line.strip()
        if stripped:
            return stripped
    return ""


def benchmark_all(binary, files):
    """Run benchmark for all corpus files. Returns list of result dicts."""
    results = []

    tmpdir = tempfile.mkdtemp(prefix="exl-bench-")

    for fpath in files:
        stem = fpath.stem
        fmt = format_from_suffix(fpath)
        size_bytes = fpath.stat().st_size
        expected_fail = "zz-" in stem.lower()

        out_path = os.path.join(tmpdir, f"{stem}.exl")
        fid_path = os.path.join(tmpdir, f"{stem}.fidelity.json")

        print(f"[bench] {fpath.name} ...", end=" ", flush=True)
        exit_code, wall_ms, stderr = run_convert(binary, fpath, out_path, fid_path)
        ok = exit_code == 0

        fidelity = parse_fidelity_json(fid_path) if ok else None
        error_snippet = first_stderr_line(stderr) if not ok else ""

        wall_ms_binary = None
        if ok:
            exlb_path = os.path.join(tmpdir, f"{stem}.exlb")
            t0 = time.monotonic()
            try:
                r = subprocess.run(
                    [binary, "convert", out_path, exlb_path],
                    capture_output=True,
                    text=True,
                    timeout=TIMEOUT_SEC,
                    cwd=str(REPO_ROOT),
                )
                wall_ms_binary = (time.monotonic() - t0) * 1000.0
            except (subprocess.TimeoutExpired, OSError):
                wall_ms_binary = None

        result = {
            "file": fpath.name,
            "format": fmt,
            "size_bytes": size_bytes,
            "wall_ms": round(wall_ms, 2),
            "exit_code": exit_code,
            "ok": ok,
            "expected_fail": expected_fail,
        }
        if fidelity:
            result["fidelity"] = fidelity
        if error_snippet:
            result["error_snippet"] = error_snippet
        if wall_ms_binary is not None:
            result["wall_ms_binary"] = round(wall_ms_binary, 2)

        results.append(result)

        if ok:
            tag = "PASS (expected fail)" if expected_fail else "PASS"
        else:
            tag = "FAIL (expected)" if expected_fail else "FAIL"
        print(f"{tag}  {wall_ms:.0f}ms")

    shutil.rmtree(tmpdir, ignore_errors=True)
    return results


def compute_summary(results):
    """Compute per-format and global summary from results list."""
    formats = {}
    for r in results:
        fmt = r["format"]
        if fmt not in formats:
            formats[fmt] = {
                "count": 0,
                "ok": 0,
                "failed": 0,
                "expected_failed": 0,
                "times_ms": [],
                "total_dropped_entities": 0,
            }
        f = formats[fmt]
        f["count"] += 1
        if r["ok"]:
            f["ok"] += 1
        else:
            f["failed"] += 1
            if r.get("expected_fail"):
                f["expected_failed"] += 1
        if r.get("wall_ms"):
            f["times_ms"].append(r["wall_ms"])
        if r.get("fidelity") and r["fidelity"].get("dropped", 0) > 0:
            f["total_dropped_entities"] += r["fidelity"]["dropped"]

    summary = {}
    for fmt, data in formats.items():
        times = sorted(data["times_ms"])
        mean = sum(times) / len(times) if times else 0
        p95_idx = max(0, int(len(times) * 0.95) - 1) if times else 0
        p95 = times[p95_idx] if times else 0
        summary[fmt] = {
            "count": data["count"],
            "ok": data["ok"],
            "failed": data["failed"],
            "expected_failed": data["expected_failed"],
            "mean_ms": round(mean, 2),
            "p95_ms": round(p95, 2),
            "total_dropped_entities": data["total_dropped_entities"],
        }
    return summary


def write_results(results, rustc_version):
    """Write bench/results.json."""
    summary = compute_summary(results)
    payload = {
        "generated_iso": datetime.now(timezone.utc).isoformat(),
        "toolchain": rustc_version,
        "results": results,
        "summary": summary,
    }
    BENCH_DIR.mkdir(parents=True, exist_ok=True)
    with open(RESULTS_PATH, "w") as fh:
        json.dump(payload, fh, indent=2)
    print(f"[bench] wrote {RESULTS_PATH}")


def get_rustc_version():
    """Return rustc --version output or 'unknown'."""
    try:
        r = subprocess.run(["rustc", "--version"], capture_output=True, text=True)
        return r.stdout.strip()
    except Exception:
        return "unknown"


def build_dashboard():
    """Read bench/results.json and write bench/index.html."""
    if not RESULTS_PATH.exists():
        print(f"[dashboard] {RESULTS_PATH} not found; run benchmarks first", file=sys.stderr)
        return False

    with open(RESULTS_PATH, "r") as fh:
        data = json.load(fh)

    results = data.get("results", [])
    summary = data.get("summary", {})
    generated = data.get("generated_iso", "")
    toolchain = data.get("toolchain", "")

    total = len(results)
    non_zz = [r for r in results if not r.get("expected_fail")]
    zz = [r for r in results if r.get("expected_fail")]
    ok_non_zz = sum(1 for r in non_zz if r["ok"])
    pass_rate = (ok_non_zz / len(non_zz) * 100) if non_zz else 100.0

    # fidelity-loss leaderboard
    leaderboard = []
    for r in results:
        fid = r.get("fidelity")
        if fid and fid.get("dropped", 0) > 0:
            leaderboard.append((r["file"], r["format"], fid["dropped"], fid.get("overall", "?")))
    leaderboard.sort(key=lambda x: -x[2])

    # Build summary cards HTML
    cards_html = f"""
    <div class="cards">
      <div class="card"><div class="card-value">{total}</div><div class="card-label">Models Tested</div></div>
      <div class="card"><div class="card-value">{pass_rate:.1f}%</div><div class="card-label">Pass Rate (non-zz)</div></div>
      <div class="card"><div class="card-value">{ok_non_zz}/{len(non_zz)}</div><div class="card-label">Passed / Non-zz</div></div>
      <div class="card"><div class="card-value">{len(zz)}</div><div class="card-label">Expected Fails</div></div>
    </div>
    """

    # Per-format stats
    fmt_rows = ""
    for fmt in sorted(summary.keys()):
        s = summary[fmt]
        fmt_rows += f"""
        <tr>
          <td>.{fmt}</td>
          <td>{s['count']}</td>
          <td>{s['ok']}</td>
          <td>{s['failed']}</td>
          <td>{s['mean_ms']:.0f}</td>
          <td>{s['p95_ms']:.0f}</td>
          <td>{s['total_dropped_entities']}</td>
        </tr>"""

    # Main results table rows (JSON embedded for JS sorting)
    results_json = json.dumps(results)

    # Leaderboard rows
    lb_rows = ""
    for item in leaderboard:
        lb_rows += f"""
        <tr>
          <td>{item[0]}</td>
          <td>.{item[1]}</td>
          <td class="dropped">{item[2]}</td>
          <td>{item[3]}</td>
        </tr>"""

    if not lb_rows:
        lb_rows = '<tr><td colspan="4" style="text-align:center;color:var(--green)">No dropped entities across any conversion.</td></tr>'

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>EXL Benchmark Report</title>
<style>
:root {{ --bg: #0d1117; --surface: #161b22; --border: #30363d; --text: #c9d1d9; --muted: #8b949e;
  --green: #3fb950; --red: #f85149; --amber: #d29922; --blue: #58a6ff; --purple: #bc8cff; }}
*, *::before, *::after {{ box-sizing: border-box; margin: 0; padding: 0; }}
body {{ font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace;
  background: var(--bg); color: var(--text); line-height: 1.5; padding: 24px; max-width: 1200px; margin: 0 auto; }}
h1 {{ font-size: 20px; font-weight: 600; margin-bottom: 4px; }}
h2 {{ font-size: 16px; font-weight: 600; margin: 32px 0 12px; padding-bottom: 6px; border-bottom: 1px solid var(--border); }}
.meta {{ color: var(--muted); font-size: 12px; margin-bottom: 24px; }}
.cards {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; margin-bottom: 32px; }}
.card {{ background: var(--surface); border: 1px solid var(--border); border-radius: 6px; padding: 16px; text-align: center; }}
.card-value {{ font-size: 28px; font-weight: 700; color: var(--blue); }}
.card-label {{ font-size: 12px; color: var(--muted); margin-top: 4px; text-transform: uppercase; letter-spacing: 0.5px; }}
table {{ width: 100%; border-collapse: collapse; font-size: 13px; }}
th {{ text-align: left; padding: 8px 12px; border-bottom: 2px solid var(--border); color: var(--muted);
  font-weight: 600; text-transform: uppercase; font-size: 11px; letter-spacing: 0.5px; cursor: pointer; user-select: none; }}
th:hover {{ color: var(--text); }}
th.sorted-asc::after {{ content: " \\2191"; }}
th.sorted-desc::after {{ content: " \\2193"; }}
td {{ padding: 6px 12px; border-bottom: 1px solid var(--border); }}
tr:hover {{ background: var(--surface); }}
.badge {{ display: inline-block; padding: 1px 8px; border-radius: 10px; font-size: 11px; font-weight: 600; }}
.badge-pass {{ background: #1b3826; color: var(--green); }}
.badge-fail {{ background: #3d1f1f; color: var(--red); }}
.badge-expected {{ background: #3d3320; color: var(--amber); }}
.badge-unexpected {{ background: #341a3d; color: var(--purple); }}
.dropped {{ color: var(--red); font-weight: 600; }}
.fidelity-lossless {{ color: var(--green); }}
.fidelity-approximate {{ color: var(--amber); }}
.fidelity-degraded {{ color: var(--red); }}
footer {{ margin-top: 40px; padding-top: 16px; border-top: 1px solid var(--border); color: var(--muted); font-size: 11px; }}
</style>
</head>
<body>
<h1>EXL Benchmark Report</h1>
<div class="meta">Generated: {generated} &middot; Toolchain: {toolchain}</div>

{cards_html}

<h2>Per-Format Summary</h2>
<table>
<thead><tr>
  <th>Format</th><th>Files</th><th>Passed</th><th>Failed</th><th>Mean ms</th><th>P95 ms</th><th>Dropped</th>
</tr></thead>
<tbody>{fmt_rows}</tbody>
</table>

<h2>Results</h2>
<table id="results-table">
<thead><tr>
  <th data-col="file">File</th>
  <th data-col="format">Format</th>
  <th data-col="size_bytes">Size</th>
  <th data-col="wall_ms">Time ms</th>
  <th data-col="fidelity_overall">Fidelity</th>
  <th data-col="dropped">Dropped</th>
  <th data-col="status">Status</th>
</tr></thead>
<tbody id="results-body"></tbody>
</table>

<h2>Fidelity-Loss Leaderboard</h2>
<table>
<thead><tr><th>File</th><th>Format</th><th>Dropped Entities</th><th>Overall</th></tr></thead>
<tbody>{lb_rows}</tbody>
</table>

<footer>EXL bench &middot; stdlib Python 3</footer>

<script>
var RESULTS = {results_json};

var sortCol = null;
var sortDir = 1;

function badge(r) {{
  if (r.ok && r.expected_fail) return '<span class="badge badge-unexpected">UNEXPECTED PASS</span>';
  if (r.ok && !r.expected_fail) return '<span class="badge badge-pass">PASS</span>';
  if (!r.ok && r.expected_fail) return '<span class="badge badge-expected">EXPECTED FAIL</span>';
  return '<span class="badge badge-fail">FAIL</span>';
}}

function fidelityClass(v) {{
  if (!v) return '';
  if (v === 'lossless') return 'fidelity-lossless';
  if (v === 'approximate') return 'fidelity-approximate';
  return 'fidelity-degraded';
}}

function fmtSize(b) {{
  if (b === null || b === undefined) return '-';
  if (b < 1024) return b + ' B';
  if (b < 1048576) return (b / 1024).toFixed(1) + ' KB';
  return (b / 1048576).toFixed(2) + ' MB';
}}

function renderTable(data) {{
  var tbody = document.getElementById('results-body');
  var html = '';
  for (var i = 0; i < data.length; i++) {{
    var r = data[i];
    var fid = r.fidelity || {{}};
    html += '<tr>' +
      '<td>' + r.file + '</td>' +
      '<td>.' + r.format + '</td>' +
      '<td>' + fmtSize(r.size_bytes) + '</td>' +
      '<td>' + (r.wall_ms != null ? r.wall_ms.toFixed(0) : '-') + '</td>' +
      '<td class="' + fidelityClass(fid.overall) + '">' + (fid.overall || '-') + '</td>' +
      '<td class="' + (fid.dropped ? 'dropped' : '') + '">' + (fid.dropped || 0) + '</td>' +
      '<td>' + badge(r) + '</td>' +
    '</tr>';
  }}
  tbody.innerHTML = html;
}}

function sortData() {{
  if (!sortCol) return;
  var col = sortCol;
  var dir = sortDir;
  RESULTS.sort(function(a, b) {{
    var va, vb;
    if (col === 'file' || col === 'format') {{
      va = (a[col] || '').toLowerCase();
      vb = (b[col] || '').toLowerCase();
      return va.localeCompare(vb) * dir;
    }}
    if (col === 'fidelity_overall') {{
      var rank = {{'lossless': 0, 'approximate': 1, 'degraded': 2, 'dropped': 3}};
      va = rank[(a.fidelity || {{}}).overall] || 99;
      vb = rank[(b.fidelity || {{}}).overall] || 99;
      return (va - vb) * dir;
    }}
    if (col === 'dropped') {{
      va = (a.fidelity || {{}}).dropped || 0;
      vb = (b.fidelity || {{}}).dropped || 0;
      return (va - vb) * dir;
    }}
    if (col === 'status') {{
      function rank(x) {{
        if (x.ok && !x.expected_fail) return 0;
        if (x.ok && x.expected_fail) return 1;
        if (!x.ok && x.expected_fail) return 2;
        return 3;
      }}
      return (rank(a) - rank(b)) * dir;
    }}
    va = a[col] != null ? a[col] : 0;
    vb = b[col] != null ? b[col] : 0;
    return (va - vb) * dir;
  }});
  renderTable(RESULTS);
}}

document.getElementById('results-table').addEventListener('click', function(e) {{
  var th = e.target.closest('th');
  if (!th || !th.dataset.col) return;
  var col = th.dataset.col;
  if (sortCol === col) {{
    sortDir *= -1;
  }} else {{
    sortCol = col;
    sortDir = 1;
  }}
  var headers = this.querySelectorAll('th');
  for (var i = 0; i < headers.length; i++) {{
    headers[i].className = headers[i].dataset.col === sortCol
      ? (sortDir === 1 ? 'sorted-asc' : 'sorted-desc')
      : '';
  }}
  sortData();
}});

renderTable(RESULTS);
</script>
</body>
</html>"""

    with open(DASHBOARD_PATH, "w") as fh:
        fh.write(html)
    print(f"[dashboard] wrote {DASHBOARD_PATH}")
    return True


def validate_html():
    """Check that bench/index.html is parseable HTML."""
    from html.parser import HTMLParser

    if not DASHBOARD_PATH.exists():
        print("[validate] index.html not found", file=sys.stderr)
        return False

    with open(DASHBOARD_PATH, "r") as fh:
        content = fh.read()

    class Validator(HTMLParser):
        def __init__(self):
            super().__init__()
            self.errors = []

        def handle_starttag(self, tag, attrs):
            pass

        def handle_endtag(self, tag):
            pass

    v = Validator()
    try:
        v.feed(content)
        v.close()
        print("[validate] index.html is valid HTML")
        return True
    except Exception as e:
        print(f"[validate] HTML parse error: {e}", file=sys.stderr)
        return False


def main():
    parser = argparse.ArgumentParser(description="EXL benchmark runner")
    parser.add_argument("--no-build", action="store_true", help="Skip cargo build")
    parser.add_argument("--dashboard-only", action="store_true", help="Only rebuild dashboard from existing results.json")
    args = parser.parse_args()

    if args.dashboard_only:
        ok = build_dashboard()
        validate_html()
        sys.exit(0 if ok else 1)

    binary = ensure_binary(skip_build=args.no_build)
    if not binary:
        print("[bench] fatal: no bf binary found", file=sys.stderr)
        sys.exit(1)

    files = gather_corpus_files()
    if not files:
        print("[bench] no corpus files found", file=sys.stderr)
        sys.exit(1)

    print(f"[bench] testing {len(files)} corpus file(s)\n")

    results = benchmark_all(binary, files)
    rustc_ver = get_rustc_version()

    write_results(results, rustc_ver)
    build_dashboard()
    validate_html()

    # Compute exit code
    non_zz_failures = [r for r in results if not r.get("expected_fail") and not r["ok"]]
    zz_passes = [r for r in results if r.get("expected_fail") and r["ok"]]

    ok_non_zz = len([r for r in results if not r.get("expected_fail") and r["ok"]])
    total_non_zz = len([r for r in results if not r.get("expected_fail")])

    print(f"\n[bench] {len(results)} files tested")
    print(f"[bench] non-zz: {ok_non_zz}/{total_non_zz} passed")
    if non_zz_failures:
        print(f"[bench] unexpected failures: {[r['file'] for r in non_zz_failures]}")
    if zz_passes:
        print(f"[bench] unexpected zz- passes: {[r['file'] for r in zz_passes]}")

    if non_zz_failures:
        sys.exit(1)
    sys.exit(0)


if __name__ == "__main__":
    main()
