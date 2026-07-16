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
