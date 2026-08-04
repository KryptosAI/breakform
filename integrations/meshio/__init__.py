"""
meshio plugin for Breakform (.exl, .exlb) format.

Install: pip install meshio breakform
Activate: import breakform_integrations.meshio  # noqa — auto-registers

After registration, meshio.read("model.exl") and meshio.write("model.exl")
work natively, bridging all 30+ meshio formats to breakform's fidelity-aware
pipeline.
"""

import json as _json

import meshio


def _load_exl(filename: str) -> meshio.Mesh:
    """Import a native Breakform (.exl / .exlb) file as a meshio Mesh."""
    import sys as _sys
    _sys.path.insert(0, "")
    try:
        from exl.io import load as _load_exl_doc
    except ImportError:
        from exl import load as _load_exl_doc

    doc = _load_exl_doc(filename)
    raw = _json.loads(_json.dumps(doc, default=str))

    all_vertices = []
    all_faces = []
    vertex_offset = 0

    for part in raw.get("parts", []):
        mesh = part.get("geometry", {}).get("mesh", {})
        brep = part.get("geometry", {}).get("brep", {})
        verts = mesh.get("vertices", [])
        faces_part = mesh.get("faces", [])

        if not verts and brep:
            verts = [v["point"] for v in brep.get("vertices", [])]

        if verts:
            all_vertices.extend(verts)
        if faces_part:
            adjusted = [[idx + vertex_offset for idx in f] for f in faces_part]
            all_faces.extend(adjusted)
        vertex_offset += len(verts)

    import numpy as _np
    points = _np.array(all_vertices, dtype=float) if all_vertices else _np.zeros((0, 3))
    cells = []
    if all_faces:
        cells.append(meshio.CellBlock("triangle", _np.array(all_faces, dtype=int)))

    return meshio.Mesh(points, cells)


def _save_exl(filename: str, mesh: meshio.Mesh) -> None:
    """Export a meshio Mesh to native Breakform (.exl) format."""
    import uuid as _uuid
    from datetime import datetime, timezone as _timezone

    vertices = mesh.points.tolist()
    faces = []
    for cell_block in mesh.cells:
        if cell_block.type == "triangle":
            faces.extend(cell_block.data.tolist())
        elif cell_block.type == "quad":
            for q in cell_block.data:
                faces.append([int(q[0]), int(q[1]), int(q[2])])
                faces.append([int(q[0]), int(q[2]), int(q[3])])

    bb_min = [min(v[i] for v in vertices) for i in range(3)] if vertices else [0, 0, 0]
    bb_max = [max(v[i] for v in vertices) for i in range(3)] if vertices else [0, 0, 0]

    part = {
        "id": str(_uuid.uuid4()),
        "name": "imported_mesh",
        "geometry": {"mesh": {"vertices": vertices, "faces": faces}},
        "semantics": {
            "coordinate_system": {
                "origin": [0.0, 0.0, 0.0],
                "x_axis": [1.0, 0.0, 0.0],
                "z_axis": [0.0, 0.0, 1.0],
                "length_unit": "mm",
            }
        },
        "bounding_box": {"min": bb_min, "max": bb_max},
    }

    doc = {
        "schema_version": "0.2",
        "parts": [part],
        "assembly": {},
        "provenance": {
            "uuid": str(_uuid.uuid4()),
            "content_hash": "",
            "tool_of_origin": {
                "name": "meshio-plugin",
                "version": "1.0.0",
                "timestamp_iso": datetime.now(_timezone.utc).isoformat(),
            },
        },
    }

    import sys as _sys
    _sys.path.insert(0, "")
    try:
        from exl.io import save as _save_exl_doc
    except ImportError:
        from exl import save as _save_exl_doc

    _save_exl_doc(doc, filename)


meshio.register_format(
    "breakform",
    [".exl", ".exlb"],
    _load_exl,
    {
        "breakform-ascii": _save_exl,
    },
)

__all__ = ["_load_exl", "_save_exl"]
