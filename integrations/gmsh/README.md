# Breakform Gmsh Plugin

Import/export `.exl` files in Gmsh using the Breakform engineering data format.

## Install

Requires Python 3.10+ with the `exl` and `gmsh` packages:

```bash
pip install breakform gmsh
```

## Usage

### As a Gmsh plugin (File > Open)

Copy `breakform_gmsh.py` into Gmsh's plugin directory. Then use **File > Open** and select any `.exl` file for import, or **File > Export** and choose the Breakform format.

### Via script

```python
import gmsh
from breakform_gmsh import import_exl, export_exl

gmsh.initialize()

import_exl("model.exl")
# ... inspect or modify the mesh in Gmsh ...
export_exl("out.exl")

gmsh.finalize()
```

## Limitations

- Import only handles mesh geometry (triangles). B-rep parts are skipped — Gmsh is a mesher, not a CAD kernel.
- Export captures 3-node triangles and 4-node quads (split into triangles). Higher-order elements are not supported.
