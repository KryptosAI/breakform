"""Breakform Gmsh I/O plugin — .exl import/export with fidelity reports."""

import gmsh
import tempfile
import json
import os


def import_exl(path: str):
    import exl

    doc_json = exl.load_json(path)
    doc = json.loads(doc_json)

    gmsh.model.add(f"Breakform - {os.path.basename(path)}")

    for part in doc.get("parts", []):
        geom = part.get("geometry", {})
        mesh = geom.get("mesh")
        if not mesh:
            continue

        vertices = mesh.get("vertices", [])
        faces = mesh.get("faces", [])
        if not vertices or not faces:
            continue

        surf_tag = gmsh.model.addDiscreteEntity(2)

        coord = []
        for v in vertices:
            coord.extend([float(v[0]), float(v[1]), float(v[2])])
        gmsh.model.mesh.addNodes(2, surf_tag, [], coord)

        node_tags, _, _ = gmsh.model.mesh.getNodes(2, surf_tag, True)

        elem_tags = list(range(1, len(faces) + 1))
        elem_node_tags = []
        for f in faces:
            elem_node_tags.extend(
                [int(node_tags[f[0]]), int(node_tags[f[1]]), int(node_tags[f[2]])]
            )

        gmsh.model.mesh.addElements(2, surf_tag, [2], [elem_tags], [elem_node_tags])


def export_exl(path: str):
    import exl
    import uuid
    import datetime

    node_tags, coords, _ = gmsh.model.mesh.getNodes()
    if not node_tags.size:
        print("No mesh nodes in current Gmsh model.")
        return

    vertices = coords.reshape(-1, 3).tolist()

    faces = []
    elem_types, elem_tags, elem_node_tags = gmsh.model.mesh.getElements(dim=2)
    for etype, tags, data in zip(elem_types, elem_tags, elem_node_tags):
        if etype == 2:
            data = data.reshape(-1, 3) - 1
            faces.extend(data.tolist())
        elif etype == 3:
            data = data.reshape(-1, 4) - 1
            for q in data:
                faces.append([int(q[0]), int(q[1]), int(q[2])])
                faces.append([int(q[0]), int(q[2]), int(q[3])])

    part_id = str(uuid.uuid4())
    part = {
        "id": part_id,
        "name": "gmsh_export",
        "geometry": {"mesh": {"vertices": vertices, "faces": faces}},
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
    doc = {
        "schema_version": "0.2",
        "parts": [part],
        "assembly": {"instances": [], "mates": []},
        "provenance": {
            "uuid": str(uuid.uuid4()),
            "content_hash": "0" * 64,
            "tool_of_origin": {
                "name": "breakform-gmsh-plugin",
                "version": "0.1.0",
                "timestamp_iso": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            },
        },
    }
    exl.save_document(json.dumps(doc), path)
    print(f"Exported {len(vertices)} vertices, {len(faces)} faces to {path}")


FILE_EXTENSIONS = [".exl"]
FILE_DESCRIPTION = "Breakform engineering data (.exl)"
