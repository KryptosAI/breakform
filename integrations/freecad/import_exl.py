"""Breakform_ImportEXL command — import .exl/.exlb into FreeCAD."""
import json
import os

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

_last_fidelity_report = None


def _polygon_points_from_edges(edge_ids, edge_map, vert_map):
    """Return ordered vertex points [f64;3] for a face edge loop."""
    points = []
    visited = set()
    remaining = list(edge_ids)
    prev_end = None

    while remaining:
        found = False
        for eid in remaining:
            if eid in visited:
                continue
            e = edge_map[eid]
            v0 = vert_map[e["vertices"][0]]
            v1 = vert_map[e["vertices"][1]]

            if prev_end is None:
                points.append(v0)
                points.append(v1)
                prev_end = v1
                visited.add(eid)
                found = True
                break

            dist0 = sum((a - b) ** 2 for a, b in zip(v0, prev_end)) ** 0.5
            dist1 = sum((a - b) ** 2 for a, b in zip(v1, prev_end)) ** 0.5

            if dist0 < 1e-6:
                points.append(v1)
                prev_end = v1
                visited.add(eid)
                found = True
                break
            elif dist1 < 1e-6:
                points.append(v0)
                prev_end = v0
                visited.add(eid)
                found = True
                break

        if not found:
            break

    return points


def _brep_to_shape(brep):
    """Build a Part compound from B-rep face edge loops."""
    edge_map = {e["id"]: e for e in brep.get("edges", [])}
    vert_map = {v["id"]: v["point"] for v in brep.get("vertices", [])}

    shapes = []
    for face in brep.get("faces", []):
        edge_ids = face.get("edges", [])
        if not edge_ids:
            continue
        pts = _polygon_points_from_edges(edge_ids, edge_map, vert_map)
        if len(pts) < 3:
            continue
        vecs = [FreeCAD.Vector(*p) for p in pts]
        wire = Part.makePolygon(vecs + [vecs[0]])
        try:
            shapes.append(Part.Face(wire))
        except Exception:
            shapes.append(wire)

    if not shapes:
        return None
    if len(shapes) == 1:
        return shapes[0]
    return Part.makeCompound(shapes)


def _build_fidelity_text(doc):
    lines = [
        f"Schema version: {doc.get('schema_version', '?')}",
        f"Parts: {len(doc.get('parts', []))}",
    ]
    for p in doc.get("parts", []):
        geom = p.get("geometry", {})
        gtype = "brep" if "brep" in geom else "mesh"
        lines.append(f"  {p.get('name', p.get('id', '?'))}: {gtype}")
    prov = doc.get("provenance", {})
    fid = prov.get("conversion_fidelity", None)
    if fid:
        lines.append(f"Conversion fidelity: {fid}")
    tool = prov.get("tool_of_origin", {})
    if tool:
        lines.append(f"Tool: {tool.get('name', '?')} v{tool.get('version', '?')}")
    return "\n".join(lines)


class ImportEXL:

    def GetResources(self):
        return {
            "MenuText": "Import EXL...",
            "ToolTip": "Import an .exl or .exlb engineering data file",
        }

    def IsActive(self):
        return True

    def Activated(self):
        global _last_fidelity_report

        try:
            import exl
        except ImportError:
            QtWidgets.QMessageBox.critical(
                None, "Missing Dependency",
                "The `exl` Python package is not installed.\n\n"
                "Install it with: pip install exl",
            )
            return

        path, _ = QtWidgets.QFileDialog.getOpenFileName(
            None, "Open EXL File", "",
            "EXL Files (*.exl *.exlb);;All Files (*)",
        )
        if not path:
            return

        try:
            doc = json.loads(exl.load_json(path))
        except Exception as e:
            QtWidgets.QMessageBox.critical(
                None, "Import Error",
                f"Failed to load {os.path.basename(path)}:\n{e}",
            )
            return

        fc_doc = FreeCAD.ActiveDocument or FreeCAD.newDocument("Breakform")
        created = 0
        doc_label = os.path.basename(path)

        for part in doc.get("parts", []):
            geom = part.get("geometry", {})
            name = part.get("name", part.get("id", "part"))

            if "mesh" in geom and Mesh is not None:
                m = geom["mesh"]
                verts = m.get("vertices", [])
                faces = m.get("faces", [])
                if verts and faces:
                    fc_verts = [FreeCAD.Vector(*v) for v in verts]
                    mesh_obj = Mesh.Mesh()
                    for f in faces:
                        mesh_obj.addFacet(fc_verts[f[0]], fc_verts[f[1]], fc_verts[f[2]])
                    fc_obj = fc_doc.addObject("Mesh::Feature", name)
                    fc_obj.Mesh = mesh_obj
                    created += 1
                    continue

            if "brep" in geom and Part is not None:
                shape = _brep_to_shape(geom["brep"])
                if shape:
                    fc_obj = fc_doc.addObject("Part::Feature", name)
                    fc_obj.Shape = shape
                    created += 1

        _last_fidelity_report = json.dumps(doc, indent=2)

        QtWidgets.QMessageBox.information(
            None, f"Import Complete — {doc_label}",
            f"Created {created} object(s).\n\n{_build_fidelity_text(doc)}",
        )


FreeCADGui.addCommand("Breakform_ImportEXL", ImportEXL())
