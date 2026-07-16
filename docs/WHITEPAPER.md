# Breakform: An Open Interoperability Layer for Engineering Design and Simulation

**Break the format. Keep the truth.**

## White Paper — v0.3

**Date:** July 2026
**Status:** v0.2 implementation shipped — execution spec

---

## Abstract

Engineering software is an estimated $30B+ annual market across CAD, CAE, CAM, and EDA (per multiple analyst estimates), built on data formats from the 1980s. Geometry, materials, mesh data, and manufacturing specifications move between tools through lossy export/import steps — a "translation tax" that industry surveys consistently peg at 15-30% of engineering labor in multi-vendor toolchains.

We propose Breakform: an open interoperability layer — a schema, reference library, and CLI toolchain that does for mechanical and electrical engineering data what USD did for 3D graphics and Apache Arrow did for analytics. The initial version was built by a small team in months, is useful from day one (one successful conversion), and is positioned to become load-bearing infrastructure for the next generation of AI-driven and GPU-native engineering tools.

---

## 1. The Problem: The Translation Tax

### 1.1 The pipeline is broken at every seam

A typical hardware design loop crosses at least four tool boundaries:

```
CAD (SolidWorks/NX/Onshape)
  → CAE pre-processing (Ansys/HyperMesh)
    → Solver (Fluent/Abaqus/in-house)
      → Post-processing / PLM
        → CAM / manufacturing (Mastercam/fab handoff)
```

Each arrow is a lossy export. What is destroyed in transit:

| Category | What is lost |
|---|---|
| Design intent | Parametric features, constraints, design history — flatten to opaque B-rep or mesh |
| Metadata | Materials, tolerances, units, coordinate frames, load cases — stripped or guessed |
| Assembly structure | Mates, kinematics, part hierarchies — become disconnected geometry |
| Semantics | Named interfaces ("bolted flange A") dissolve into anonymous face IDs |
| Provenance | Which version of which model produced which result — no audit trail |

Industry surveys consistently estimate 15-30% of engineering time in multi-tool environments is consumed by model repair and format translation. This is a structural cost, not a friction point.

### 1.2 Existing standards are inadequate

| Format | Introduced | Core problem |
|---|---|---|
| IGES | 1980 | Frozen; ambiguous geometry mappings; effectively deprecated |
| STEP (AP203/214/242) | 1994 | EXPRESS schema is opaque; implementations diverge; extensions crawl through ISO committees; no native mesh or results model |
| STL | 1987 | Triangles only; no units, materials, topology, or assembly structure |
| JT / 3DXML / OBJ | 1995–2010 | Vendor-owned or scope-limited; no semantic or provenance layer |

Vendor-neutral kernels (Open Cascade) provide geometry operations, not interchange — they are C++ monoliths, not schemas. The wrong abstraction.

Adjacent industries solved their equivalent problem: graphics/VFX (USD), analytics (Arrow/Parquet), geospatial (GDAL), documents (pandoc). Engineering has no such layer.

### 1.3 Why now

Three conditions that did not exist five years ago:

1. **AI tooling for engineering is in production.** Foundation models for geometry generation, ML-based surrogates, and AI-assisted optimization (deployed at PhysicsX, Neural Concept, and within Ansys) require structured, semantically-rich training data. Without an interop layer, each team rebuilds the same brittle parsers.

2. **GPU-native and differentiable solvers are shipping.** Solvers built on JAX, PyTorch, and CUDA (NVIDIA Modulus, JAX-CFD, open-source efforts) need efficient geometry + boundary-condition ingestion without reimplementing STEP parsing or writing legacy solver-deck glue.

3. **Hardware teams are adopting versioned workflows.** Git + LFS patterns for hardware are spreading, but the atomic unit remains opaque binaries. Structured, content-addressable interchange is the missing primitive for diff, review, and CI in engineering workflows.

---

## 2. The Proposal

A schema, library, and CLI — no kernel, no solver, no PLM. The spec is the product.

### 2.1 Concrete technical decisions

- **Binary representation:** Custom columnar layout (`.exlb` v2). 64-byte-aligned buffers with a JSON metadata section, enabling mmap-backed zero-copy reads — a GPU solver maps vertex arrays directly into device memory. Arrow IPC compatibility is on the Phase 1 roadmap; the current format delivers equivalent zero-copy performance with a simpler layout.
- **Text form:** Newline-delimited JSON format (`.exl` files). Line-diffable, reviewable in pull requests. Header line `#exl 0.2` for format identification.
- **ID scheme:** BLAKE3 content hash (32 bytes, hardware-accelerated, fast on large files) for content addressing; 128-bit UUID for stable logical identity across revisions. Every document carries both.
- **Geometry representations (v0):** (a) **Mesh** — indexed tri face-vertex lists with per-vertex positions (`f32×3`), normals, UVs, per-face group/material IDs. (b) **B-rep** — faces/edges/vertices referencing NURBS or analytic surfaces/curves, imported from STEP. SDF/implicit deferred to v1.
- **Unit system:** Every numeric quantity is dimensioned. `mm`, `inch`, `deg`, `rad`, SI base units. Serialization without unit tags is a parse error, not a warning.

### 2.2 v0 schema layers

Four layers, independently serializable, referencing each other by UUID:

1. **`geometry`** — Geometry payload.
   - `brep` (optional): topological face→surface, edge→curve, vertex→point graph. Surface types: plane, cylinder, cone, sphere, torus, extrusion, NURBS. Curve types: line, circle, ellipse, NURBS. Parametric surface and curve definitions stored losslessly (`surface_params`, `curve_params` maps).
   - `mesh` (optional): `vertices: f32[N×3]`, `faces: u32[M×3]`, optional `normals`, `uvs`, `face_groups: u32[M]` mapping to named groups.
   - `bounding_box` (optional): `f64[6]`.
   - Exactly one of `brep` or `mesh` required.

2. **`semantics`** — Engineering meaning.
   - `materials`: list of `{ name, density (kg/m³), elastic_modulus (Pa), poisson_ratio, yield_strength (Pa), thermal_conductivity (W/m·K) }` — all optional, all unit-tagged.
   - `boundary_conditions`: named face groups → `{ type: pressure|fixed_displacement|heat_flux|convection, value: f64+unit, direction: f64×3 }`.
   - `tolerances`: `{ linear: f64+unit, angular: f64+unit }`.
   - `coordinate_system`: origin, basis vectors, length unit (default: mm, Z-up).

3. **`assembly`** — Hierarchical composition.
   - `instances`: list of `{ part_ref: UUID, transform: f64[4×4], name: string }`.
   - `mates` (optional, v0 subset): `{ type: fixed|revolute|prismatic, parts: [UUID, UUID], axis: f64×3, limits: { min, max }? }`.

4. **`provenance`** — Audit and diff DAG.
   - `uuid` (required): stable identity.
   - `content_hash` (required): BLAKE3 of geometry+semantics payload.
   - `parent_hashes` (optional): list of predecessor hashes.
   - `tool_of_origin`: `{ name, version, timestamp_iso }`.
   - `conversion_fidelity`: enum `lossless|approximate|degraded` + per-field detail.

### 2.3 v0 converters

- **STEP (.stp, .step) → .exl/.exlb:** Import-only. B-rep via Open Cascade. Emits fidelity report (surface type coverage, dropped annotations). Multi-solid extraction and assembly hierarchy preserved. NURBS surfaces and curves stored losslessly with full control-point data; rational (weighted) complex-entity forms remain approximate.
- **STL (binary + ASCII) ↔ .exl:** Import + export. Mesh path only.
- **glTF 2.0 (.glb) ↔ .exl:** Import + export. Mesh + material pass-through. Full TRS node transform support. Animations/graphics materials dropped with fidelity note.
- **OBJ ↔ .exl:** Import + export. Group names → named face groups.
- **.exl ↔ .exl:** Full lossless round-trip.
- **.exlb ↔ .exl:** Full lossless round-trip between text and binary forms.

v1 adds: Nastran (.bdf), Abaqus (.inp), OpenFOAM cases, Gmsh (.msh), Parasolid (.x_t, commercial plugin).

### 2.4 v0 CLI

```
bf convert [--from <fmt>] [--to <fmt>] [--fidelity-report <path>] <input> <output>
bf validate [--profile <mech|cfd|fea|strict>] <file>
bf diff [--type <topology|transform|metadata|all>] <file1> <file2>
bf info [--json] <file>
```

`convert` auto-detects format by extension; `--from`/`--to` override. `validate` exits 0 (pass), 1 (warnings), 2 (errors). `diff` outputs JSON delta per layer.

### 2.5 Explicitly out of scope

- CAD authoring kernel (we consume B-rep, never produce it).
- Solver execution (we feed geometry + BCs to solvers).
- PLM / version-control system (we provide the diff+provenance primitives).
- Universal ontology (we ship four profiles: `mech`, `cfd`, `fea`, `strict`; more grow with adoption).
- Multibody dynamics, contact definitions, manufacturing toolpaths, parameter trees.

---

## 3. Why This Wins as an Open-Source MVP

### 3.1 Position relative to alternatives

| Criterion | Breakform | GPU solver startup | Cloud CAD kernel |
|---|---|---|---|
| Time to first user value | Days (one conversion) | Years (accuracy is table-stakes) | 5–10 years |
| Minimum team | 2–4 engineers | Numerics PhDs + GPU specialists | 20+ geometry experts |
| Contributor funnel | Wide (anyone who hit a broken import) | Narrow | Very narrow |
| Scope bounded | Yes (formats are finite) | No (physics is unbounded) | No |
| Adoption pattern | Dependency growth (pandoc/GDAL) | Benchmark-hype → churn | N/A |
| Incumbent resistance | Moderate | Low | Extreme |

A converter is the right wedge: one working conversion creates value; library dependency creates irreplaceability.

### 3.2 The adoption flywheel

1. **Individual engineers** adopt the CLI to fix a broken import today — utility-first, no permission needed.
2. **Open-source tools** (OpenFOAM, Gmsh, FreeCAD, GPU solvers) adopt the library to shed their own parsers — we become a dependency.
3. **AI/ML teams** adopt the schema as their training-data format — it is the only structured, semantic representation available.
4. **Greenfield CAE startups** build natively on it — no legacy format loyalty to overcome.
5. **Enterprises** demand vendor support once it is in their pipelines — open-core monetization begins.

Each stage strengthens the next. The endgame is GDAL-like ubiquity: invisible, assumed, load-bearing.

### 3.3 Precedents

| Project | Domain | Outcome |
|---|---|---|
| USD | Graphics/VFX | Proprietary tool → open standard → NVIDIA Omniverse platform, Apple Vision Pro backbone |
| Apache Arrow | Data analytics | Interchange spec → substrate of modern data stack; Voltron Data monetization |
| GDAL | Geospatial | One library → embedded in every GIS product; consortium-sustained |
| ffmpeg / pandoc | General | "Universal converter" pattern → irreplaceable infrastructure via dependency growth |

### 3.4 What falsifies this thesis

The thesis is wrong if any of the following occur within 18–24 months:

1. **No anchor integration.** If no major open-source engineering tool (Gmsh, FreeCAD, OpenFOAM, or a GPU solver) adopts the library as a dependency, the flywheel does not spin. Utility adoption without platform dependency is insufficient.
2. **Conversion fidelity loses to existing STEP toolchains.** If round-tripping real-world assemblies routinely produces worse fidelity than commercial STEP translators, the "better pipes" claim fails. The benchmark corpus must demonstrate parity or superiority.
3. **AI/ML teams ignore the schema.** If the format does not appear in published ML-for-engineering papers, datasets, or tooling within 24 months, the future-demand thesis is disproven.
4. **CLI growth flatlines.** If weekly active usage of `bf convert` does not show sustained month-over-month growth after the benchmark-corpus launch, the utility-first wedge is not working.

---

## 4. Commercial Model

### 4.1 Licensing

- **Core library:** Apache 2.0. No dual-license, no future re-license contingency. Permissive, patent-granted, adoption-optimized.
- **Specification:** Community Specification License 1.0. Patent peace for implementers; spec stays open for reference and extension while the commercial entity stewards the compliance mark. Forking the spec is explicitly permitted — but forks lose the trademark, which is the soft moat.
- **Trademark:** The company holds the wordmarks; anyone shipping a compliant implementation may use them freely; non-compliant forks may not. This is the only enforcement lever needed.
- **Contributions:** Developer Certificate of Origin (DCO), no CLA — mirrors the kernel/Arrow model and removes "who owns my contribution" friction.

### 4.2 Tiered offerings

| Tier | SKUs | Buyer | Trigger event |
|---|---|---|---|
| **OSS** | (1) Core library + CLI + standard converters (STEP, STL, glTF, OBJ, EXL/EXLB); (2) Spec + public compliance test suite; (3) Community support | Individual engineers, OSS solver projects | "My STEP import broke and I need something that works in 10 minutes" |
| **Cloud** | (1) Hosted conversion + validation API with fidelity reports; (2) Team model registry with provenance DAG; (3) Pay-per-use solver-ready artifact generation | GPU-solver startups, AI-for-engineering teams, mid-market CAE groups | "I need 10,000 conversions/day and I'm not running a parser farm" |
| **Enterprise** | (1) Commercial-kernel bridges (Parasolid, ACIS); (2) On-prem with SSO, audit logging, air-gapped operation; (3) SLAs + compliance profiles (ITAR/EAR, NIST 800-171) | Aerospace primes, automotive OEMs, medical-device manufacturers | "Procurement won't sign off without a vendor to call" |

The line: **never paywall a format.** Paid tiers sell throughput, proprietary-kernel access, and operational assurance — not data access. The OSS tier must be good enough to build a business on, or the community moat never forms.

### 4.3 First dollar

The highest-probability first-revenue product is the **hosted conversion + validation API with machine-readable fidelity reports**. Upload a file, get back structured, validated, solver-ready output with a fidelity scorecard — no toolchain install, no kernel licensing.

Pricing is usage-based, metered by model-complexity tier (vertex/face/assembly count) per job, not per seat. No minimum commit — a $0-to-paid ramp an individual engineer can expense before procurement ever sees it.

The first buyer is not an aerospace prime. It is a **GPU-solver startup or AI-for-engineering team**: money, urgency, zero procurement friction, no legacy vendor relationship. Aerospace buys in year 3 through a sales motion, not month 6 through a landing page.

### 4.4 Governance

BDFL editorship through spec v1.0 — design-by-committee is the mechanism STEP used to freeze itself. When three or more external vendors ship production products on the spec (measured by the compliance test suite, not self-declaration), spec governance transfers to a neutral foundation (Linux Foundation / JDF model). The commercial entity retains (a) services and hosting, (b) trademark portfolio and compliance program, (c) editorial seats on the TSC. The core library remains Apache 2.0 irrespective of foundation status.

---

## 5. Technical Architecture

### 5.1 Crate structure (Rust workspace + PyO3/maturin)

```
exl-core/      Schema structs (Part, Assembly, Material, Layer), ID types (Blake3Hash, Uuid),
               Unit<f64> dimensioned scalars, unit-conversion tables.
exl-io/        Native serialization: text-form parser+emitter, EXLB v2 binary I/O (64-byte-aligned
               columnar buffers + JSON metadata), mmap-able file format, version header.
exl-geom/      Geometry: Mesh (indexed face-vertex + attribute arrays), BRep (topology graph),
               Surface/Curve enums (with SurfaceParams/CurveParams), Transform (4×4 f64),
               bounding volume.
exl-step/      STEP importer: reads ISO-10303-21 files. BRep graph extraction from AP203/214/242.
               Multi-solid files produce one Part per MANIFOLD_SOLID_BREP. Assemblies via
               NEXT_ASSEMBLY_USAGE_OCCURRENCE → Instance records. Parameterized surfaces
               (plane, cylinder, cone, sphere, torus) and curves stored losslessly. B-spline
               surfaces/curves stored with full control points and knot vectors. FFI to Open Cascade.
               Emits conversion-fidelity report per entity type.
exl-fmt/       Lightweight formats: STL (binary/ASCII), OBJ.
exl-gltf/      glTF/GLB import/export.
exl-diff/      Layer-wise differencing. Matches nodes by UUID → spatial hash fallback. Produces
               JSON patch: topological delta, transform delta, metadata delta.
exl-validate/  Rule engine. Checks: units presence, watertightness (empty boundary-edge set),
               manifoldness, metadata completeness. Profile dispatcher (mech/cfd/fea/strict).
exl-cli/       Binary crate. clap subcommands: convert, diff, validate, info.
exl-py/        Python bindings (PyO3). Exposes Part, Mesh, BRep, convert, diff, validate.
               maturin build → `pip install exl` → `import exl`.
```

### 5.2 Geometric diff (v0 definition)

Compares two `.exl` files layer-by-layer and emits a JSON-patch delta.

**Diffed:**
- **Topology graph:** added/removed face/edge/vertex UUIDs. Face marked "modified" when surface type or adjacency changes. Node matching: UUID first, spatial hash (BLAKE3 of control points + bounding box) as fallback.
- **Transforms:** per-instance 4×4 delta vs. tolerance (1e-9). Classified as identity, rigid-body, scaled, or sheared.
- **Metadata:** materials, BC values, tolerances, units — field-by-field, reported at JSON-path level.

**Out of scope at v0:** exact surface comparison (needs intersection kernel — v2), volumetric mesh differencing, parametric feature comparison, semantic equivalence across different UUIDs.

**Output** (excerpt):
```json
{
  "topology": {
    "added": ["face-7a3b"],
    "removed": ["face-12e0"],
    "modified": [{"id": "face-9f01", "change": "surface_type", "old": "plane", "new": "nurbs"}]
  },
  "transforms": [
    {"part": "bracket", "delta": {"type": "rigid_body", "translation": [1.2, 0, 0]}}
  ],
  "metadata": [
    {"path": "materials[0].elastic_modulus", "old": {"value": 200e9, "unit": "Pa"}, "new": {"value": 210e9, "unit": "Pa"}}
  ]
}
```

### 5.3 Architecture principles

- **Zero-copy solver ingestion:** geometry arrays in `.exlb` v2 files are aligned to 64-byte boundaries. A solver mmaps the file, gets zero-copy `&[f32]` slices via `MappedExlb`, launches GPU kernels — no parse step.
- **Layered hashing:** geometry, semantics, and assembly carry independent content hashes. Changing a material bumps only the semantics hash. This is the primitive that unlocks git-for-hardware.
- **Machine-readable fidelity:** every `bf convert` writes a JSON report listing what survived, was approximated, or dropped — per entity type. Nothing silently degrades.
- **Unit enforcement at the type level:** `Unit<f64>` wrapper in Rust; serialization gate rejects untyped numerics. Mars Climate Orbiter was avoidable.

---

## 6. Roadmap & Execution Plan

### Phase 0 — First 90 days (COMPLETE — v0.2 shipped)

**Foundation**

- [x] Repo scaffolding: Rust workspace (10 crates), CI (build + lint + test on x86/ARM), PyO3 binding stub, contributor guide
- [x] Spec v0 in-repo as `spec/SPEC.md`: geometry, semantics, units, serialization forms; profiles `mech`/`cfd`/`fea`/`strict`
- [x] Benchmark corpus seed and generation scripts

**First converters**

- [x] STEP import via Open Cascade bindings; B-rep → native geometry layer including multi-solid extraction and assembly hierarchy
- [x] Analytic surface/curve params: plane, cylinder, cone, sphere, torus stored losslessly; NURBS surfaces/curves with full control points and knot vectors
- [x] STL read/write (binary + ASCII), glTF/GLB read/write, OBJ read/write
- [x] Native ↔ native lossless round-trip (serialize → deserialize → compare); text (.exl) ↔ binary (.exlb) round-trip
- [x] Unit system library: `Unit<f64>` dimensioned scalars with parse/validate

**Ship**

- [x] `bf convert` + `bf validate --profile <p>` + `bf diff` + `bf info` CLI
- [x] Python bindings: maturin wheel, `pip install exl` → `import exl`
- [x] Corpus: 55 synthetic models across STEP/STL/OBJ/GLB; automated regression suite (76 tests)
- [x] Machine-readable JSON fidelity report per conversion
- [x] Zero-copy EXLB v2 binary format: mmap-backed, 64-byte-aligned columnar buffers
- [x] Benchmark dashboard (`make bench`) with reproducible results
- [x] Public repo: github.com/KryptosAI/breakform, Apache-2.0

**Notable deviations from original Phase 0 plan:**

| Item | Original plan | Actual |
|---|---|---|
| Binary format | Apache Arrow IPC | Custom columnar layout (EXLB v2) — equivalent zero-copy performance; Arrow IPC on Phase 1 roadmap |
| Corpus composition | ~50 mixed models | 55 synthetic models; real-world corpus in progress |
| Anchor integrations | — | None yet; Phase 1 priority |
| IGES support | Planned | Deferred |

### Phase 1 — Active (Months 4–12)

- **Solver decks:** Nastran `.bdf`, Abaqus `.inp`, OpenFOAM case → semantics layer (nodes, elements, BCs, materials, loads)
- **Anchor integrations (2–3):** Gmsh (mesh I/O), FreeCAD (workbench plugin), one GPU-solver startup
- **Diff hardening:** structural + geometric diff → text + machine-readable patch; GitHub Actions plugin that fails builds on geometry regressions
- **Hosted API alpha:** POST conversion, GET status, download result; usage metered; free tier gated by corpus size
- **Real-world corpus:** expand beyond synthetic models to production CAD assemblies from partner toolchains
- **Arrow IPC binary format:** add Arrow IPC as an alternative binary serialization path alongside EXLB v2

### Year 2+

- Assembly + kinematics layer: hierarchical instances, transforms, joints, motion intent
- Model registry: content-addressed storage, versioned lineage, access control — "Docker Hub for parts"
- Foundation move: spec + core library to neutral entity; company retains hosted services + enterprise plugins
- Certification program: "reads/writes vX compliant" mark for tool vendors
- EDA bridge: `pcb` profile seed — board outline, stackup, placement, netlist-level semantics

### Team & cost

| Role | Owns | FTE |
|---|---|---|
| Systems/geometry engineer | Rust core, OCCT bridge, STEP/STL/glTF converters, binary serialization | 1.0 |
| Full-stack / CLI engineer | CLI, PyO3 bindings, CI/CD, pip packaging, hosted API scaffolding | 1.0 |
| Domain expert / spec editor | Spec, benchmark corpus curation, fidelity semantics, validation profiles, anchor outreach | 0.5–1.0 |
| GPU/numerics engineer (optional, Phase 1+) | Solver deck ingestion, mmap solver ingestion path, GPU buffer layout | 0–0.5 |

**Budget guidance:** 2 senior FTE + fractional domain expert ≈ $350–450k annual burn. A seed round ($1.5–2M) covers 18–24 months including infra and optional commercial kernel licensing.

---

## 7. Risks and Mitigations

| Risk | Severity | Mitigation (action, not hope) |
|---|---|---|
| **Open Cascade STEP fidelity ceiling — the wedge fails** | Critical | Publish a public benchmark vs. FreeCAD, CAD Assistant, and Gmsh on real models **before** declaring v1.0. If the fidelity delta vs. existing free tools is zero, narrow scope to formats where we deliver measurable improvement — or kill the project. |
| **Chicken-and-egg adoption** | High | Land 2–3 named anchor integrations with public letters of intent in Phase 1, **before** building converter #4. The first three downstream dependencies are recruited, not waited for. |
| **Semantic garbage-in — CAD exports lack the metadata we promise to preserve** | High | Per-conversion fidelity report quantifying metadata completeness, plus a **published per-tool export-quality scorecard naming vendors that strip data** — doubles as advocacy leverage. |
| **3MF / AOUSD (USD-for-industry) encroachment** | Medium | Publish a 1-page differentiation table vs. AOUSD roadmap and 3MF extensions. Build first the three capabilities neither addresses: assembly joints, solver boundary conditions, content-addressed provenance. |
| **Maintainer burnout / bus factor at 2–4 people** | High | Recruit a second full-time maintainer; document release + governance procedures so no individual blocks releases. Hosted-tier revenue in year 1 funds the hire — funding the project *is* the burnout mitigation. |
| **Enterprise procurement gravity — orgs buy incumbents regardless** | Medium | Target AI/ML data pipelines first (no procurement) and GPU-solver startups second (greenfield). **Enterprise pipeline is year 2+ only, gated on 3 public reference adoptions.** |
| **Incumbent embrace-extend-extinguish** | Medium | Register the compliance trademark; move the spec to a neutral foundation **before** any vendor ships a branded "compatible" product; publish an independently runnable pass/fail conformance suite. Licensing + trademark is the mechanism — not community goodwill. |
| **Spec sprawl / committee death** | High | One editor (BDFL) through v1; **no spec change accepted without a shipped implementation in the reference library**; governance RFCs gated on adoption numbers, not committee attendance. |

---

## 8. Conclusion

Breakform delivers a CLI-first STEP/glTF/STL/OBJ converter with mandatory units and a machine-readable fidelity report — utility that works standalone, today, with no platform dependency. v0.2 is shipped with 10 Rust crates, 76 tests, and a 55-model corpus.

Success within Phase 1 means three measurable signals: (1) a public benchmark corpus quantifying per-tool translation losses, (2) 2–3 anchor integrations shipping inside downstream OSS tools, and (3) at least one AI/ML team consuming the schema as its training-data format by its own choice.

The single biggest bet: a permissively-licensed, dependency-first interop library wins ubiquity before incumbents organize a response — because the history of USD, Arrow, and GDAL says the format that works first in the open becomes infrastructure, and infrastructure is never dislodged by committees.

---

## Appendix A: Implementation Status (v0.2)

| Capability | Status | Evidence |
|---|---|---|
| **STEP import** | Shipped | Handles multi-solid (up to 5 tested), assemblies (NEXT_ASSEMBLY_USAGE_OCCURRENCE), analytic surfaces (plane/cylinder/cone/sphere/torus), NURBS curves/surfaces with full control points, fidelity reports per entity type |
| **STL import/export** | Shipped | Binary and ASCII, both directions, 36 STL corpus models |
| **OBJ import/export** | Shipped | Tri/quad, groups (→ named face groups), normals/UVs, multi-group, 20 OBJ corpus models |
| **GLB import/export** | Shipped | GLB container, TRIANGLES primitives, full TRS node transforms, dropped materials/animations in fidelity report |
| **EXL text format** | Shipped | Newline-delimited JSON, `#exl 0.2` header, full schema round-trip |
| **EXLB v2 binary format** | Shipped | Magic `EXLB` + version 0x02, 64-byte-aligned columnar buffers, JSON metadata section, mmap-backed zero-copy reads via `MappedExlb`, backward-compatible v1 reader |
| **Diff** | Shipped | Layer-wise topology/transform/metadata delta, JSON-patch output, UUID + spatial-hash matching |
| **Validate** | Shipped | 4 profiles (mech/cfd/fea/strict), pass/warn/fail exit codes, units-presence, watertightness checks |
| **Python bindings** | Shipped | PyO3/maturin wheel, `convert`, `validate`, `diff` exposed |
| **CLI** | Shipped | `bf convert`/`bf validate`/`bf diff`/`bf info` subcommands, format auto-detection, fidelity-report output |
| **Corpus** | 55 models | Synthetic across STEP/STL/OBJ/GLB; generation via `python scripts/gen_corpus.py`; real-world corpus in progress |
| **Regression tests** | 76 tests | `cargo test --workspace` across all crates |
| **Benchmark dashboard** | Shipped | `make bench` → `bench/index.html` |
| **CI** | Shipped | Build + lint + test on x86/ARM |
| **Arrow IPC binary** | Phase 1 | Not yet implemented; current EXLB v2 provides equivalent zero-copy mmap performance |
| **Solver deck import** | Phase 1 | Nastran/Abaqus/OpenFOAM — not started |
| **Anchor integrations** | Phase 1 | No downstream adopters yet |
| **IGES import** | Deferred | Not in current scope |
| **Real-world corpus** | In progress | Expanding beyond synthetic models |

---

## Appendix B: Format Support Matrix

| format | import | export | fidelity level |
|---|---|---|---|
| STEP (.stp, .step) | yes | — | lossless for analytic surfaces and NURBS; assemblies and multi-solid extracted |
| STL (.stl) | yes | yes | mesh only (format inherent) |
| OBJ (.obj) | yes | yes | mesh + groups |
| GLB (.glb) | yes | yes | mesh + transforms; graphics materials/animations dropped |
| EXL text (.exl) | yes | yes | lossless native round-trip |
| EXL binary (.exlb) | yes | yes | lossless native round-trip; mmap zero-copy |
| Nastran (.bdf) | planned | — | Phase 1 |
| Abaqus (.inp) | planned | — | Phase 1 |
| OpenFOAM case | planned | — | Phase 1 |
| IGES (.igs, .iges) | deferred | — | — |

---

*Breakform v0.3 — updated from v0.2 whitepaper with v0.2 implementation shipped. Repo: github.com/KryptosAI/breakform. License: Apache-2.0.*
