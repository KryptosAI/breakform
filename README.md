<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo/breakform-logo-dark.png">
    <img src="assets/logo/breakform-logo-light.png" alt="Breakform" width="360">
  </picture>
</p>

# Breakform

**Break the format. Keep the truth.**

[![CI](https://github.com/KryptosAI/breakform/actions/workflows/ci.yml/badge.svg)](https://github.com/KryptosAI/breakform/actions/workflows/ci.yml)
[![PyPI version](https://img.shields.io/pypi/v/exl)](https://pypi.org/project/exl/)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/KryptosAI/breakform/blob/main/LICENSE)

Open interoperability layer for engineering data — schema, library, and CLI.

Breakform provides a vendor-neutral interchange format for 3D geometry (mesh + B-rep), material semantics, boundary conditions, assembly structure, and parametric surface definitions. Every conversion emits a machine-readable fidelity report — it never silently loses data. It ships as a Rust workspace with a CLI tool (`bf`) for converting, validating, diffing, and inspecting engineering data.

## Features

- Mesh and B-rep geometry with parametric surface/curve definitions (lossless STEP fidelity)
- Zero-copy binary format (.exlb v2) — mmap-backed geometry buffers for solver ingestion
- STEP import with multi-solid and assembly extraction
- STL, OBJ, GLB round-trip conversion
- meshio Python bridge: 27 import + 28 export mesh formats (ANSYS, Exodus, Gmsh, VTK/VTU, XDMF, CGNS, MED, and more)
- Fidelity reports tracking entity-level conversion quality
- Structured diff between two documents
- Profile-based validation (mech, cfd, fea, strict)
- Python bindings via PyO3/maturin
- 69-model corpus for regression testing
- Benchmark dashboard

## Quickstart

```bash
cargo build --release

# Convert STL to GLB
./target/release/bf convert corpus/cube-ascii.stl cube.glb

# Convert a STEP bracket to native EXL with fidelity report
./target/release/bf convert corpus/bracket.step out.exl --fidelity-report fidelity.json

# Validate the result
./target/release/bf validate --profile mech out.exl

# Inspect document structure
./target/release/bf info out.exl
```

Run benchmarks (dashboard at `bench/index.html`):

```bash
make bench
```

## Python

Quickstart:

```bash
pip install breakform              # core: convert, validate, diff
pip install breakform[meshio]      # +27 mesh/solver formats via meshio
pip install breakform[gmsh]        # Gmsh plugin
pip install breakform[all]         # everything: meshio + Gmsh plugin
python -c "import exl; exl.convert('model.step', 'model.exl')"
```

> **Note:** The wheel bundles the Python bridge code but not the meshio or gmsh PyPI dependencies. To use meshio-based formats (ANSYS, Exodus, Gmsh, VTK/VTU, XDMF, CGNS, MED, and others), install with `pip install breakform[meshio]`.

Development install:

```bash
cd crates/exl-py && maturin develop
```

```python
import exl
exl.convert("input.step", "output.exl")
```

## Crate map

| crate | purpose |
|-------|---------|
| `exl-core` | Schema types: `Document`, `Part`, `FidelityReport`, units |
| `exl-geom` | Geometry primitives: `Mesh`, `BRep`, `BoundingBox`, `Transform`, `SurfaceParams`, `CurveParams` |
| `exl-io` | Native `.exl`/`.exlb` read/write (v1 + v2) |
| `exl-fmt` | Format import/export: STL, OBJ |
| `exl-step` | ISO-10303-21 STEP import |
| `exl-gltf` | glTF/GLB import/export |
| `exl-diff` | Structural diff between two documents |
| `exl-validate` | Profile-based model validation (mech/cfd/fea/strict) |
| `exl-py` | Python bindings |
| `exl-nastran` | Nastran .bdf/.dat import/export |
| `exl-abaqus` | Abaqus .inp import/export |
| `exl-openfoam` | OpenFOAM case directory import/export |
| `exl-arrow` | Arrow IPC binary layout |
| `exl-cli` | `bf` CLI binary |

## Corpus

A 50-model regression corpus is maintained under `corpus/`. Generate it with:

```bash
python scripts/gen_corpus.py
```

## Integrations

### Gmsh — mesh I/O plugin

```bash
pip install breakform[gmsh]
```

Installs the Gmsh plugin automatically. Import `.exl` / `.exlb` files via
**File > Open** or export via **File > Export**. Boundary conditions are
mapped to physical groups; fidelity reports are shown in a summary dialog.

See [integrations/gmsh/](integrations/gmsh/) for details.

### FreeCAD — import/export workbench

Copy the workbench into FreeCAD's Mod directory:

```bash
# macOS
ln -s "$(pwd)/integrations/freecad" ~/Library/Preferences/FreeCAD/Mod/Breakform
# Linux
ln -s "$(pwd)/integrations/freecad" ~/.local/share/FreeCAD/Mod/Breakform
```

Restart FreeCAD and select **Breakform** from the workbench dropdown.
Import `.exl` / `.exlb` files, export selected objects, and view fidelity
reports. Requires `pip install breakform`.

See [integrations/freecad/](integrations/freecad/) for details.

## Specification

See [spec/SPEC.md](spec/SPEC.md) for the v0.2 format specification.

## Brand & namespace

The project is **Breakform**; the CLI binary is `bf`. The interchange format and library namespace remain `exl` (.exl/.exlb files, `#exl` header, `EXLB` magic, exl-* crates) — a deliberate separation: the brand names the project, while `exl` is the stable wire-format identifier. See [ADR-0001](docs/decisions/0001-brand-and-namespace.md) for rationale.

## Open core

This repository contains the complete spec, schema, all converters, CLI, Python bindings, corpus, and benchmark suite — free forever under Apache-2.0. Commercial services (hosted conversion/validation API, team model registry, proprietary-kernel bridges for Parasolid/ACIS, enterprise SSO/compliance) live outside this repo. Paid tiers sell throughput, assurance, and proprietary-kernel access — never data access. See [OPEN-CORE.md](OPEN-CORE.md) for the full boundary.

## License

Apache-2.0

## Status

v1.0 — stable schema and APIs.
