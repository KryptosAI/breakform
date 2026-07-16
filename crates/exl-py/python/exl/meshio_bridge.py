import json
import os
import tempfile
import uuid as _uuid
from datetime import datetime, timezone
from pathlib import Path

import meshio

SUPPORTED_IMPORT = {}
SUPPORTED_EXPORT = {}


def _detect_meshio_formats():
    from meshio._helpers import reader_map, _writer_map

    for k in sorted(reader_map.keys()):
        if k not in ("stl", "obj"):
            SUPPORTED_IMPORT[k] = k

    for k in sorted(_writer_map.keys()):
        if k not in ("stl", "obj", "gmsh22", "vtk42", "vtk51"):
            SUPPORTED_EXPORT[k] = k


_detect_meshio_formats()


def is_meshio_format(ext):
    key = ext.lower().lstrip(".")
    return key in SUPPORTED_IMPORT or key in SUPPORTED_EXPORT


def import_via_meshio(input_path):
    try:
        mesh = meshio.read(input_path)
    except Exception as e:
        return None, f"meshio read failed: {e}"

    fid = {
        "source_format": "meshio",
        "target_format": "exl",
        "overall": "lossless",
        "entities": [],
    }

    vertices = mesh.points.tolist()
    faces = []

    for cell_block in mesh.cells:
        dtype = cell_block.type
        data = cell_block.data

        if dtype == "triangle":
            faces.extend(data.tolist())
            fid["entities"].append(
                {
                    "entity": f"cell_{dtype}",
                    "count": len(data),
                    "status": "lossless",
                }
            )
        elif dtype == "quad":
            for q in data:
                faces.append([int(q[0]), int(q[1]), int(q[2])])
                faces.append([int(q[0]), int(q[2]), int(q[3])])
            fid["entities"].append(
                {
                    "entity": f"cell_{dtype}",
                    "count": len(data),
                    "status": "lossless",
                    "note": "triangulated",
                }
            )
        elif dtype == "tetra":
            face_counts = {}
            for t in data:
                for a, b, c in [(0, 1, 2), (0, 1, 3), (0, 2, 3), (1, 2, 3)]:
                    f = tuple(sorted([int(t[a]), int(t[b]), int(t[c])]))
                    face_counts[f] = face_counts.get(f, 0) + 1
            hull_count = 0
            for f, count in face_counts.items():
                if count == 1:
                    faces.append(list(f))
                    hull_count += 1
            fid["entities"].append(
                {
                    "entity": f"cell_{dtype}",
                    "count": len(data),
                    "status": "lossless",
                    "note": "hull faces extracted",
                }
            )
        elif dtype == "hexahedron":
            hex_quads = [
                (0, 1, 2, 3),
                (4, 5, 6, 7),
                (0, 1, 5, 4),
                (1, 2, 6, 5),
                (2, 3, 7, 6),
                (3, 0, 4, 7),
            ]
            tri_face_counts = {}
            for h in data:
                for qi in hex_quads:
                    q = [int(h[qi[0]]), int(h[qi[1]]), int(h[qi[2]]), int(h[qi[3]])]
                    for tri_indices in [(0, 1, 2), (0, 2, 3)]:
                        tf = tuple(
                            sorted(
                                [q[tri_indices[0]], q[tri_indices[1]], q[tri_indices[2]]]
                            )
                        )
                        tri_face_counts[tf] = tri_face_counts.get(tf, 0) + 1
            hull_count = 0
            for f, count in tri_face_counts.items():
                if count == 1:
                    faces.append(list(f))
                    hull_count += 1
            fid["entities"].append(
                {
                    "entity": f"cell_{dtype}",
                    "count": len(data),
                    "status": "lossless",
                    "note": "triangulated hull",
                }
            )
        else:
            fid["entities"].append(
                {
                    "entity": f"cell_{dtype}",
                    "count": len(data),
                    "status": "dropped",
                    "note": f"unsupported cell type: {dtype}",
                }
            )

    for k, v in (mesh.point_data or {}).items():
        fid["entities"].append(
            {
                "entity": f"point_data.{k}",
                "count": len(v) if hasattr(v, "__len__") else 1,
                "status": "dropped",
            }
        )
    for k, v in (mesh.cell_data or {}).items():
        fid["entities"].append(
            {
                "entity": f"cell_data.{k}",
                "count": len(v) if hasattr(v, "__len__") else 1,
                "status": "dropped",
            }
        )

    if any(e["status"] == "dropped" for e in fid["entities"]):
        fid["overall"] = "lossy"

    bb_min = [min(v[i] for v in vertices) for i in range(3)] if vertices else [0, 0, 0]
    bb_max = [max(v[i] for v in vertices) for i in range(3)] if vertices else [0, 0, 0]

    part = {
        "id": str(_uuid.uuid4()),
        "name": Path(input_path).stem,
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
                "name": "meshio-bridge",
                "version": meshio.__version__,
                "timestamp_iso": datetime.now(timezone.utc).isoformat(),
            },
        },
    }

    return doc, fid


def export_via_meshio(doc_dict, output_path, format_hint=None):
    import numpy as np

    fmt = format_hint
    if not fmt:
        ext = Path(output_path).suffix.lstrip(".") or Path(output_path).name
        from meshio._helpers import extension_to_filetypes

        candidates = extension_to_filetypes.get("." + ext, [])
        for c in candidates:
            if c in SUPPORTED_EXPORT:
                fmt = c
                break
        if not fmt and candidates:
            fmt = candidates[0]

    if not fmt:
        return {"error": f"unable to determine meshio format for {output_path}"}

    all_vertices = []
    all_faces = []
    vertex_offset = 0

    for part in doc_dict.get("parts", []):
        mesh = part.get("geometry", {}).get("mesh", {})
        verts = mesh.get("vertices", [])
        faces_part = mesh.get("faces", [])
        if verts:
            all_vertices.extend(verts)
        if faces_part:
            adjusted = []
            for f in faces_part:
                adjusted.append([idx + vertex_offset for idx in f])
            all_faces.extend(adjusted)
        vertex_offset += len(verts)

    if not all_vertices:
        return {"error": "no mesh vertices found in document"}

    points = np.array(all_vertices, dtype=float)
    cells = []
    if all_faces:
        cells.append(meshio.CellBlock("triangle", np.array(all_faces, dtype=int)))

    m = meshio.Mesh(points, cells)

    try:
        m.write(output_path, file_format=fmt)
    except Exception as e:
        return {"error": f"meshio write failed: {e}"}

    fid = {
        "source_format": "exl",
        "target_format": fmt,
        "overall": "lossless",
        "entities": [
            {"entity": "vertices", "count": len(all_vertices), "status": "lossless"},
            {"entity": "triangles", "count": len(all_faces), "status": "lossless"},
        ],
    }

    return fid
