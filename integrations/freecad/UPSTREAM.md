## Add Breakform workbench to FreeCAD Addons

### Title

Breakform — Engineering data interchange with fidelity tracking

### Description

Breakform is an open format for FEA, CFD, and CAM data that embeds geometry,
materials, boundary conditions, and a machine-verifiable fidelity report in
every file. This FreeCAD workbench enables import and export of `.exl` and
`.exlb` files directly from the Part or Mesh workbenches.

### Features

- Import mesh parts as `Mesh::Feature` objects and B-rep parts as `Part::Feature`
  polygon compounds.
- Export selected mesh or Part objects to `.exl` text format.
- Fidelity report dialog surfaces any data loss from format conversion.
- Toolbar buttons for quick Import, Export, and Fidelity Report access.

### Screenshots (text description)

- **Workbench selector**: The "Breakform" entry appears in the FreeCAD workbench
  dropdown after installation.
- **Toolbar**: Three buttons — blue folder (Import EXL), green disk (Export EXL),
  and a clipboard icon (Fidelity Report).
- **Fidelity dialog**: A scrollable text panel listing each entity (e.g.,
  "nodes: 845 — Lossless", "materials: 1 — Approximate") with per-entity notes.

### Installation

Copy or symlink the `integrations/freecad/` directory into the FreeCAD Mod
folder:

macOS:
```
ln -s "$(pwd)/integrations/freecad" ~/Library/Preferences/FreeCAD/Mod/Breakform
```

Linux:
```
ln -s "$(pwd)/integrations/freecad" ~/.local/share/FreeCAD/Mod/Breakform
```

Requires the `breakform` Python package: `pip install breakform`.
