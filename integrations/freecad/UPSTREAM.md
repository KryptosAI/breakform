## Add Breakform workbench to FreeCAD Addons

### Status: Shipped as standalone workbench

The workbench files in this directory (`Init.py`, `InitGui.py`, `import_exl.py`,
`export_exl.py`, `fidelity_viewer.py`) form a complete FreeCAD workbench.

### Submission target

FreeCAD Addons are managed at https://github.com/FreeCAD/FreeCAD-addons.
To list this workbench, a separate Git repository containing these files
must be published, then an entry added to the `.gitmodules` file in
FreeCAD-addons via a pull request.

This workbench depends on the `breakform` Python package (`pip install breakform`).
The `pip install` step must be documented in the workbench README until
FreeCAD bundles the package.

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
