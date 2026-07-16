# bench

Benchmark runner for the `bf` CLI.  Converts every corpus file to `.exl`
(and round-trips to `.exlb`), collects wall-clock timing and fidelity data,
and produces a self-contained HTML dashboard.

## Quick start

    make bench                  # build release binary + run all benchmarks
    python3 bench/run_bench.py --no-build   # skip cargo build
    python3 bench/run_bench.py --dashboard-only  # rebuild HTML from results.json

## Metrics

| metric            | description |
|-------------------|-------------|
| wall_ms           | wall-clock convert duration (milliseconds) |
| wall_ms_binary    | round-trip .exl -> .exlb duration |
| fidelity overall  | worst-case fidelity tier: lossless / approximate / degraded |
| lossless / approximate / degraded / dropped | per-entity-status counts |
| expected_fail     | true when filename starts with `zz-` |
| exit_code         | 0 = success, non-zero = failure |
