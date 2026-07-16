# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Real-world corpus expansion
- Solver deck converters (Nastran, Abaqus, OpenFOAM)
- Arrow IPC binary layout
- Anchor integrations

## [0.2.1] - 2026-07-16

### Added
- meshio Python bridge: 27 import formats + 28 export formats (ANSYS, Exodus, Gmsh, VTK/VTU, XDMF, CGNS, MED, and 20+ more)
- Gmsh I/O plugin: import/export .exl directly in Gmsh mesher
- FreeCAD workbench: Import/Export .exl + fidelity report viewer panel
- `save_document` PyO3 binding for programmatic document creation
- PyPI package: `pip install exl`

### Changed
- Python bindings dispatch: Nastran (.bdf/.dat), Abaqus (.inp), OpenFOAM case dirs
- `exl.__init__.py` now a proper package with meshio bridge integration
- bench extended to 69 corpus entries including Nastran/Abaqus/OpenFOAM fixtures

## [0.2.0] - 2026-07-15

### Added

- Schema with mandatory units and BLAKE3 provenance
- `.exl` text format and zero-copy `.exlb` v2 binary format with mmap-backed geometry buffers
- STEP import with analytic and NURBS parameterized surface/curve definitions, multi-solid support, and assembly extraction
- STL, OBJ, and GLB round-trip conversion
- Fidelity reports tracking entity-level conversion quality
- Structured diff between two documents
- Profile-based validation (`mech`, `cfd`, `fea`, `strict`)
- `bf` CLI tool (renamed from `eng` in this release)
- Python bindings via PyO3/maturin
- 55-model corpus for regression testing
- Benchmark dashboard
- CI and release workflows
- Apache-2.0 license with DCO governance
