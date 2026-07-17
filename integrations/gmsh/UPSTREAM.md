## Add Breakform (.exl) I/O plugin for Gmsh

### Description

Breakform is an open, vendor-neutral engineering data format that preserves
mesh geometry, boundary conditions, materials, and a signed fidelity report in
every file. It targets interoperability between FEA, CFD, and CAM tools.

This plugin adds native `.exl` import and export to Gmsh via the existing
Python plugin system. Both text (`.exl`) and binary (`.exlb`) formats are
supported.

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
