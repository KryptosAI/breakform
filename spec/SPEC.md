# Breakform Specification v0.2

Breakform is an open interoperability layer for engineering data — a schema, a library, and a CLI. Break the format. Keep the truth. The interchange format and reference implementation are namespaced `exl` (.exl / .exlb files, exl-* crates).

## Text format (`.exl`)

The text format is newline-delimited JSON. Every `.exl` file begins with the header:

```
#exl 0.2
```

followed by the canonical JSON payload (single line or pretty-printed).

## Schema layers

### 1. Geometry layer (`Document.parts[].geometry`)

Stored as a tagged union: `mesh` or `brep`.

**Mesh** (`GeometryPayload::Mesh`):

| field | type | required | description |
|-------|------|----------|-------------|
| `vertices` | `Vec<[f32; 3]>` | yes | Vertex positions |
| `faces` | `Vec<[u32; 3]>` | yes | Triangle indices (0-based) |
| `normals` | `Option<Vec<[f32; 3]>>` | no | Per-vertex normals |
| `uvs` | `Option<Vec<[f32; 2]>>` | no | Per-vertex texture coords |
| `face_groups` | `Option<Vec<u32>>` | no | Per-face group id |
| `group_names` | `Vec<String>` | no | Group name lookup table |

**B-rep** (`GeometryPayload::Brep`):

| field | type | required | description |
|-------|------|----------|-------------|
| `vertices` | `Vec<BrepVertex>` | yes | Topological vertices |
| `edges` | `Vec<BrepEdge>` | yes | Curve-bounded edges |
| `faces` | `Vec<BrepFace>` | yes | Surface-bounded faces |
| `surface_params` | `Map<String, SurfaceParams>` | no | Per-face parametric surface definitions |
| `curve_params` | `Map<String, CurveParams>` | no | Per-edge parametric curve definitions |

`BrepVertex { id: String, point: [f64; 3] }`
`BrepEdge { id: String, curve: CurveType, vertices: [String; 2] }`
`BrepFace { id: String, surface: SurfaceType, edges: Vec<String> }`

CurveType: `line` | `circle` | `ellipse` | `nurbs` | `other`
SurfaceType: `plane` | `cylinder` | `cone` | `sphere` | `torus` | `extrusion` | `nurbs` | `other`

**SurfaceParams** (serde tagged with `"kind"`, snake_case):

| variant | fields |
|---------|--------|
| `plane` | `origin: [f64; 3]`, `normal: [f64; 3]` |
| `cylinder` | `origin: [f64; 3]`, `axis: [f64; 3]`, `radius: f64` |
| `cone` | `origin: [f64; 3]`, `axis: [f64; 3]`, `radius: f64`, `half_angle: f64` |
| `sphere` | `center: [f64; 3]`, `radius: f64` |
| `torus` | `center: [f64; 3]`, `axis: [f64; 3]`, `major_radius: f64`, `minor_radius: f64` |
| `nurbs_surface` | `degree_u: usize`, `degree_v: usize`, `control_points: Vec<Vec<[f64; 3]>>`, `knots_u: Vec<f64>`, `knots_v: Vec<f64>`, `weights: Option<Vec<Vec<f64>>>` |

**CurveParams** (serde tagged with `"kind"`, snake_case):

| variant | fields |
|---------|--------|
| `line` | `point: [f64; 3]`, `direction: [f64; 3]` |
| `circle` | `center: [f64; 3]`, `axis: [f64; 3]`, `radius: f64` |
| `ellipse` | `center: [f64; 3]`, `axis: [f64; 3]`, `semi_major: f64`, `semi_minor: f64` |
| `nurbs_curve` | `degree: usize`, `control_points: Vec<[f64; 3]>`, `knots: Vec<f64>`, `weights: Option<Vec<f64>>` |

`surface_params` maps face `id` to its parametric surface definition. `curve_params` maps edge `id` to its parametric curve definition. Both are optional and default to empty. The STEP importer populates these maps, providing lossless fidelity for parameterized surfaces.

### 2. Semantics layer (`Document.parts[].semantics`)

| field | type | required | description |
|-------|------|----------|-------------|
| `materials` | `Vec<Material>` | no | Material assignments |
| `boundary_conditions` | `Vec<BoundaryCondition>` | no | BCs for simulation |
| `tolerances` | `Option<Tolerances>` | no | Manufacturing tolerances |
| `coordinate_system` | `CoordinateSystem` | yes | Part-local coordinate system |

`Material { name: String, density?: Quantity, elastic_modulus?: Quantity, poisson_ratio?: f64, yield_strength?: Quantity, thermal_conductivity?: Quantity }`

`BoundaryCondition { face_group: String, type: BcType, value: Quantity, direction?: [f64; 3] }`
BcType: `pressure` | `fixed_displacement` | `heat_flux` | `convection`

`Tolerances { linear: Quantity, angular: Quantity }`

`CoordinateSystem { origin: [f64; 3], x_axis: [f64; 3], z_axis: [f64; 3], length_unit: Unit }`
Default: origin [0,0,0], x [1,0,0], z [0,0,1], unit mm.

### 3. Assembly layer (`Document.assembly`)

| field | type | required | description |
|-------|------|----------|-------------|
| `instances` | `Vec<Instance>` | no | Part instances with transforms |
| `mates` | `Vec<Mate>` | no | Kinematic relationships |

`Instance { part_ref: String, name: String, transform: Transform }`
`Mate { type: MateType, parts: [String; 2], axis?: [f64; 3], limits?: [f64; 2] }`
MateType: `fixed` | `revolute` | `prismatic`

Transform is a 4x4 row-major matrix `[[f64; 4]; 4]`.

### 4. Provenance layer (`Document.provenance`)

| field | type | required | description |
|-------|------|----------|-------------|
| `uuid` | `String` | yes | Document UUID v4 |
| `content_hash` | `String` | yes | BLAKE3 hash of parts+assembly JSON |
| `parent_hashes` | `Vec<String>` | no | Source document hashes |
| `tool_of_origin` | `Option<ToolOfOrigin>` | no | Authoring tool info |
| `conversion_fidelity` | `Option<Fidelity>` | no | Overall conversion fidelity |

`ToolOfOrigin { name: String, version: String, timestamp_iso: String }`
Fidelity: `lossless` | `approximate` | `degraded`

## Binary format (`.exlb`)

### v2 layout (writers emit v2; readers accept v1 and v2)

| offset | size | content |
|--------|------|---------|
| 0 | 4 | Magic bytes `EXLB` |
| 4 | 1 | Format version `0x02` |
| 5 | 3 | Reserved (zero) |
| 8 | 8 | JSON offset (u64 LE) |
| 16 | 8 | JSON length (u64 LE) |
| 24 | 4 | Buffer count (u32 LE) |
| 28 | 4 | Reserved (zero) |
| 32 | N x 16 | Buffer table: N entries of (offset u64 LE, length u64 LE) |
| 32+Nx16 | variable | Buffer data, each 64-byte aligned, little-endian: f32 for vertices/normals/uvs; u32 for faces/face_groups |
| json_offset | json_len | JSON section |

The JSON section is structured as:
```json
{
  "document": <Document with mesh arrays emptied>,
  "meshes": [
    {
      "part": idx,
      "vertices": bufIdx,
      "faces": bufIdx,
      "normals": bufIdx|null,
      "uvs": bufIdx|null,
      "face_groups": bufIdx|null
    }
  ]
}
```

**Rationale**: 64-byte aligned buffers enable mmap-backed zero-copy access — geometry arrays can be sliced directly from the file mapping without deserializing or copying, allowing solver ingestion at memory-bandwidth speeds. The `MappedExlb` reader maps the file and exposes each mesh as a `MeshView` with typed slice references into the mapped region.

### v1 layout (legacy, read-only)

| offset | size | content |
|--------|------|---------|
| 0 | 4 | Magic bytes `EXLB` |
| 4 | 1 | Version byte `0x01` |
| 5 | 4 | Payload length (u32 LE) |
| 9 | N | JSON payload (UTF-8) |

Binary files have no `#exl 0.2` header; the JSON is the same schema as the text format.

## STEP importer

The STEP importer (exl-step) reads ISO-10303-21 files:

| feature | behavior |
|---------|----------|
| Multi-solid files | Each `MANIFOLD_SOLID_BREP` entity produces one `Part` |
| Assembly extraction | `NEXT_ASSEMBLY_USAGE_OCCURRENCE` entities yield `Instance` records with translation transforms |
| Parameterized surfaces | Planar, cylindrical, conical, spherical, and toroidal surfaces are stored losslessly in `surface_params` and `curve_params` |
| B-spline surfaces | `B_SPLINE_CURVE_WITH_KNOTS` / `B_SPLINE_SURFACE_WITH_KNOTS` stored losslessly as `nurbs_curve` / `nurbs_surface` params (degree, control points, knot vectors expanded from multiplicities); rational (weighted) complex-entity forms remain approximate |

## glTF converter (exl-gltf)

| feature | behavior |
|---------|----------|
| Container | GLB only |
| Primitive mode | `TRIANGLES` |
| Attributes | `POSITION`, `NORMAL`, `TEXCOORD_0` + indices |
| Node transforms | Full TRS (translation, quaternion rotation, scale) composed to matrices on import; exported as node `matrix` (identity omitted); lossless |
| Materials / animations | Dropped; recorded in fidelity report as `EntityStatus::Dropped` |
| B-rep parts | Not representable in glTF; dropped with fidelity note |

## Supported formats

| format | import | export |
|--------|--------|--------|
| STEP (.stp, .step) | yes | — |
| STL (.stl) | yes | yes |
| OBJ (.obj) | yes | yes |
| GLB (.glb) | yes | yes |
| EXL text (.exl) | yes | yes |
| EXL binary (.exlb) | yes | yes |

## Document root (`Document`)

| field | type | required | description |
|-------|------|----------|-------------|
| `schema_version` | `String` | yes | Schema semver (currently `"0.2"`) |
| `parts` | `Vec<Part>` | yes | Geometry parts (>=0) |
| `assembly` | `Assembly` | yes | Assembly graph |
| `provenance` | `Provenance` | yes | Origin and integrity metadata |

`Part { id: String, name: String, geometry: GeometryPayload, semantics: Semantics, bounding_box?: BoundingBox }`

## Units

All dimensioned values use `Quantity { value: f64, unit: Unit }`.

| dimension | unit symbols |
|-----------|-------------|
| Length | `mm`, `cm`, `m`, `inch` |
| Angle | `deg`, `rad` |
| Mass | `kg` |
| Pressure | `Pa`, `MPa`, `GPa` |
| Density | `kg/m3` |
| Temperature | `K` |
| ThermalConductivity | `W/m.K` |
| Dimensionless | `1` |

## Fidelity report schema

`FidelityReport { source_format: String, target_format: String, overall: Fidelity, entities: Vec<EntityFidelity> }`

`EntityFidelity { entity: String, count: usize, status: EntityStatus, note?: String }`
EntityStatus: `lossless` | `approximate` | `degraded` | `dropped`

## Versioning policy

Minor versions (`0.x`) are additive only: new fields, new variants, new enum members. Backward-compatible readers ignore unknown fields. Major versions (`1.0+`) may remove or rename fields after a deprecation cycle.

v0.1 binary files (magic `EXLB` version `0x01`) remain readable by v0.2 readers.

## Out of scope (v0.2)

- Parametric/feature-tree history
- Material property databases (only inline assignments)
- Sparse/split geometry representations (e.g. NURBS control point networks)
- Assembly kinematic constraint solving
- Units other than those listed in the units table
- Formal schema validation (JSON Schema / XSD)
- Web/HTTP API, gRPC transport
- Encryption, digital signatures beyond content_hash
- Point-cloud / voxel geometry payloads
