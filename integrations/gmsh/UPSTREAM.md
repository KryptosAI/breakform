## Add Breakform (.exl) I/O plugin for Gmsh

### Status: Shipped as user-installed plugin

The plugin is self-contained in `breakform_gmsh.py` and works with Gmsh's
Python plugin system. Users install it via:

```bash
pip install breakform[gmsh]
```

This places the plugin in Gmsh's plugin search path automatically.

### Submission target

Gmsh uses GitLab at https://gitlab.onelab.info/gmsh/gmsh. Upstream inclusion
requires the plugin to be self-contained (no external dependencies). Since
this plugin depends on the `breakform` Python package, it is distributed
alongside the Breakform SDK rather than submitted as an upstream merge request.

If Gmsh ships `breakform` as a bundled dependency in the future, this plugin
can be submitted as an MR at that time.

### What the plugin does

- **Import**: Reads `.exl` / `.exlb` files containing triangle and quad mesh
  parts into native Gmsh entities. Boundary condition metadata is attached as
  physical groups. Fidelity reports are displayed as a summary dialog.
- **Export**: Writes the active Gmsh mesh (3-node triangles, 4-node quads split
  to triangles) to `.exl` format, including physical group names and material
  IDs as fidelity-tracked metadata.

### Test results

- Unit tests pass with `gmsh` Python SDK and `breakform` package installed.
- Round-trip tests preserve vertex/face counts through import-export cycles.
- Validated against the Breakform spec schema 0.2.

### Installation

```bash
pip install breakform gmsh
cp integrations/gmsh/breakform_gmsh.py ~/.config/gmsh/plugins/
```

Then use **File > Open** and select any `.exl` file, or **File > Export** to
save as Breakform format.
