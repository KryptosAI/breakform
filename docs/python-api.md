# Python API Reference

```python
import exl
```

## Functions

### convert

```python
exl.convert(input: str, output: str, *,
            fidelity_report: Optional[str] = None,
            format_from: Optional[str] = None,
            format_to: Optional[str] = None) -> str
```

Convert between engineering data formats. Returns a JSON string containing
the fidelity report(s).

Format detection is automatic by file extension. Use `format_from`/`format_to`
to override.

**Parameters:**
- `input` — path to input file (`.stl`, `.obj`, `.step`, `.stp`, `.glb`, `.exl`, `.exlb`, `.bdf`, `.dat`, `.inp`, or OpenFOAM case directory)
- `output` — path for output file
- `fidelity_report` — optional path to write the fidelity report JSON
- `format_from` — override input format detection (e.g. `"stl"`)
- `format_to` — override output format detection (e.g. `"glb"`)

**Examples:**

```python
# STL to GLB
exl.convert("model.stl", "model.glb")

# STEP to native EXL with fidelity report
exl.convert("bracket.step", "bracket.exl", fidelity_report="report.json")

# Force format override
exl.convert("unknown.bin", "out.obj", format_from="stl")
```

### validate

```python
exl.validate(path: str, profile: str) -> str
```

Validate a `.exl` or `.exlb` file against a profile. Returns a JSON string
of validation findings.

**Profiles:** `mech`, `cfd`, `fea`, `strict`

**Example:**

```python
findings = exl.validate("model.exl", "mech")
```

### diff

```python
exl.diff(a: str, b: str) -> str
```

Compute a structured diff between two `.exl`/`.exlb` files. Returns a JSON
string with topology, transform, and metadata deltas.

**Example:**

```python
delta = exl.diff("v1.exl", "v2.exl")
```

### info

```python
exl.info(path: str, *, format: str = "text") -> str
```

Print document structure and metadata.

**Parameters:**
- `path` — path to a `.exl` or `.exlb` file
- `format` — `"text"` for human-readable summary, `"json"` for full document serialization

**Examples:**

```python
# Human-readable summary
print(exl.info("model.exl"))

# Full document as JSON
doc = exl.info("model.exl", format="json")
```

### load_json

```python
exl.load_json(path: str) -> str
```

Load a `.exl` or `.exlb` file and return the full document as JSON.

### content_hash

```python
exl.content_hash(path: str) -> str
```

Return the BLAKE3 content hash from a document's provenance record.

### save_document

```python
exl.save_document(json: str, path: str) -> str
```

Construct a Document from JSON and save it to `.exl` or `.exlb` format.
Returns the content hash.

**Example:**

```python
hash = exl.save_document('{"schema_version":"0.2","parts":[...]}', "out.exl")
```

## Supported Formats

| Format | Import | Export |
|---|---|---|
| STEP (.stp, .step) | yes | — |
| STL (.stl) | yes | yes |
| OBJ (.obj) | yes | yes |
| GLB (.glb) | yes | yes |
| EXL text (.exl) | yes | yes |
| EXL binary (.exlb) | yes | yes |
| Nastran (.bdf, .dat) | yes | yes |
| Abaqus (.inp) | yes | yes |
| OpenFOAM (case dir) | yes | yes |

With `pip install breakform[meshio]`: 27 additional import + 28 additional export
mesh/solver formats via the meshio bridge (ANSYS, Exodus, Gmsh, VTK/VTU, XDMF,
CGNS, MED, and more).
