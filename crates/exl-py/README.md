# breakform (Python bindings)

Python bindings for Breakform — open engineering-data interop with honest fidelity reports.

## Build

```bash
pipx run maturin develop --release --manifest-path crates/exl-py/Cargo.toml
```

Or build a wheel:

```bash
pipx run maturin build --release --manifest-path crates/exl-py/Cargo.toml
pip install <wheel>
```

## Usage

```python
import exl

exl.convert("input.stl", "output.exl")
print(exl.load_json("output.exl"))
print(exl.validate("output.exl", "mech"))
print(exl.content_hash("output.exl"))
```

## Extras

Breakform's optional Python dependencies are gated behind extras so the core install stays lean:

| extra    | `pip install breakform[<extra>]` | brings                                                  |
|----------|----------------------------------|---------------------------------------------------------|
| (none)   | `pip install breakform`          | core: convert, validate, diff                           |
| `meshio` | `pip install breakform[meshio]`  | 27 import + 28 export mesh/solver formats via meshio   |
| `gmsh`   | `pip install breakform[gmsh]`    | Gmsh Python API                                         |
| `all`    | `pip install breakform[all]`     | everything: meshio + Gmsh plugin                        |
