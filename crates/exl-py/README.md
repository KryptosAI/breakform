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
