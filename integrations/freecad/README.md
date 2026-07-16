# Breakform — FreeCAD Workbench

Import and export EXL engineering data files with fidelity reports.

## Installation

### Option A: Symlink (recommended for development)

**macOS:**
```bash
ln -s "$(pwd)/integrations/freecad" ~/Library/Preferences/FreeCAD/Mod/Breakform
```

**Linux:**
```bash
ln -s "$(pwd)/integrations/freecad" ~/.local/share/FreeCAD/Mod/Breakform
```

**Windows (PowerShell, admin):**
```powershell
New-Item -ItemType SymbolicLink -Path "$env:APPDATA\FreeCAD\Mod\Breakform" -Target ".\integrations\freecad"
```

### Option B: Copy

Copy the `integrations/freecad/` directory to `~/Library/Preferences/FreeCAD/Mod/Breakform/` (macOS) or `~/.local/share/FreeCAD/Mod/Breakform/` (Linux).

### Dependencies

```bash
pip install exl
```

The `exl` Python package provides `load_json`, `convert`, `validate`, `diff`, and `content_hash` functions.

## Usage

1. Open FreeCAD
2. Select **Breakform** from the workbench dropdown
3. Use **File > Breakform** or the toolbar buttons:
   - **Import EXL...** — Load .exl / .exlb engineering data files
   - **Export EXL...** — Export selected objects to .exl
   - **Fidelity Report** — View the last import's fidelity report

## Features

### Import

- **Mesh parts**: Creates `Mesh::Feature` objects with vertices and faces
- **B-rep parts**: Creates `Part::Feature` compounds from face vertex loops as polygon faces
- A fidelity summary dialog is shown after import
- Each imported object is tagged with geometry fidelity metadata

### Export

- **Mesh objects**: Extracts vertices and faces directly
- **Part/Shape objects**: Tessellates to triangle mesh using FreeCAD's tessellator
- Writes standard `.exl` text format (`#exl 0.2` header + JSON)

## Known Limitations

- B-rep import creates discrete faceted polygon faces; fidelity loss relative to parametric NURBS surfaces is documented in the fidelity report
- Export tessellates all B-rep geometry to mesh, losing parametric surface information
- Assembly transforms and mate relationships are not preserved on import
- Material and boundary condition data is not reconstructed

## Notes

- This workbench is pure Python and does not require FreeCAD at runtime to pass validation
- PySide2 is used for Qt dialogs (bundled with FreeCAD 0.19+)
- All FreeCAD module imports use try/except for graceful fallback outside FreeCAD
