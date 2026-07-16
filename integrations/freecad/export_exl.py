"""Breakform_ExportEXL command — export selected FreeCAD objects to .exl."""
import json
import os
import uuid as _uuid_mod

import FreeCAD
import FreeCADGui
from PySide2 import QtWidgets

try:
    import Mesh
except ImportError:
    Mesh = None

try:
    import Part
except ImportError:
    Part = None


def _mesh_to_payload(mesh):
    """Extract vertices and faces from a Mesh.Mesh object."""
    pts = mesh.Points
    facets = mesh.Facets
    if not pts or not facets:
        return None
    vertices = [[p.x, p.y, p.z] for p in pts]
    faces = []
    for f in facets:
        if len(f.Points) >= 3:
            faces.append([f.Points[0], f.Points[1], f.Points[2]])
    return {"mesh": {"vertices": vertices, "faces": faces}}


def _shape_to_payload(shape):
    """Tessellate a Part.Shape and return mesh payload."""
    try:
        verts, faces_raw = shape.tessellate(1.0)
    except Exception:
        return None
    vertices = [[v.x, v.y, v.z] for v in verts]
    faces = []
    for i in range(0, len(faces_raw), 3):
        faces.append([faces_raw[i], faces_raw[i + 1], faces_raw[i + 2]])
    return {"mesh": {"vertices": vertices, "faces": faces}}


class ExportEXL:
    """Export selected FreeCAD objects to .exl."""

    def GetResources(self):
        return {
            "MenuText": "Export EXL...",
            "ToolTip": "Export selected objects to an .exl engineering data file",
            "Pixmap": "",
        }

    def IsActive(self):
        return FreeCADGui.Selection.hasSelection()

    def Activated(self):
        sel = FreeCADGui.Selection.getSelection()
        if not sel:
            QtWidgets.QMessageBox.warning(
                None, "Nothing Selected", "Select objects to export."
            )
            return

        path, _ = QtWidgets.QFileDialog.getSaveFileName(
            None,
            "Save EXL File",
            "",
            "EXL Files (*.exl);;All Files (*)",
        )
        if not path:
            return

        parts = []
        for obj in sel:
            name = obj.Label or obj.Name
            part_id = str(_uuid_mod.uuid4())

            geom_payload = None

            if Mesh and hasattr(obj, "Mesh"):
                geom_payload = _mesh_to_payload(obj.Mesh)
            elif Part and hasattr(obj, "Shape"):
                geom_payload = _shape_to_payload(obj.Shape)
            elif hasattr(obj, "Shape"):
                geom_payload = _shape_to_payload(obj.Shape)

            if geom_payload is None:
                continue

            parts.append(
                {
                    "id": part_id,
                    "name": name,
                    "geometry": geom_payload,
                    "semantics": {
                        "materials": [],
                        "boundary_conditions": [],
                        "coordinate_system": {
                            "origin": [0.0, 0.0, 0.0],
                            "x_axis": [1.0, 0.0, 0.0],
                            "z_axis": [0.0, 0.0, 1.0],
                            "length_unit": "mm",
                        },
                    },
                }
            )

        if not parts:
            QtWidgets.QMessageBox.warning(
                None,
                "Nothing Exportable",
                "No exportable geometry found in selection.",
            )
            return

        doc = {
            "schema_version": "0.2",
            "parts": parts,
            "assembly": {"instances": [], "mates": []},
            "provenance": {
                "uuid": str(_uuid_mod.uuid4()),
                "content_hash": "",
                "parent_hashes": [],
                "tool_of_origin": {
                    "name": "FreeCAD Breakform Workbench",
                    "version": "0.1.0",
                    "timestamp_iso": "",
                },
                "conversion_fidelity": "approximate",
            },
        }

        exl_text = "#exl 0.2\n" + json.dumps(doc, indent=2)

        try:
            with open(path, "w", encoding="utf-8") as f:
                f.write(exl_text)
        except Exception as e:
            QtWidgets.QMessageBox.critical(
                None, "Export Error", f"Failed to write {path}:\n{e}"
            )
            return

        QtWidgets.QMessageBox.information(
            None,
            "Export Complete",
            f"Exported {len(parts)} object(s) to {os.path.basename(path)}",
        )


FreeCADGui.addCommand("Breakform_ExportEXL", ExportEXL())
