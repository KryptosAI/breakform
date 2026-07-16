# Breakform Integrations

Breakform ships with plugins and workbenches for popular engineering tools.

## Gmsh Plugin

Location: [integrations/gmsh/](gmsh/)

A Gmsh plugin that adds native .exl import and export to the Gmsh mesher. Load and save Breakform documents directly from the Gmsh GUI or Python API.

### Install

Copy `breakform_gmsh.py` into your Gmsh plugin directory or load it via `gmsh.merge("breakform_gmsh.py")`.

### Format coverage

| direction | formats |
|-----------|---------|
| import | .exl, .exlb (mesh payloads only; B-rep geometry is skipped) |
| export | .exl (mesh payloads only) |

## FreeCAD Workbench

Location: [integrations/freecad/](freecad/)

A FreeCAD workbench providing Import/Export commands for .exl files and a dockable fidelity report viewer panel. The viewer displays entity-level conversion quality after each import.

### Install

Symlink or copy the workbench directory into FreeCAD's Mod folder:

```bash
ln -s "$PWD/integrations/freecad" ~/.local/share/FreeCAD/Mod/Breakform
```

### Features

| feature | description |
|---------|-------------|
| Import .exl | Loads mesh and B-rep geometry into FreeCAD documents |
| Export .exl | Writes selected objects to a Breakform document |
| Fidelity viewer | Dockable panel showing per-entity conversion status |

### Format coverage

Supported via the `exl` Python package (`pip install exl`) and meshio bridge (27 import + 28 export mesh formats).
