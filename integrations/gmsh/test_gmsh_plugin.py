import gmsh
import sys
import os
import tempfile
import json

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

gmsh.initialize()

gmsh.model.add("test_tri")

p1 = gmsh.model.occ.addPoint(0, 0, 0)
p2 = gmsh.model.occ.addPoint(1, 0, 0)
p3 = gmsh.model.occ.addPoint(0, 1, 0)
l1 = gmsh.model.occ.addLine(p1, p2)
l2 = gmsh.model.occ.addLine(p2, p3)
l3 = gmsh.model.occ.addLine(p3, p1)
cl = gmsh.model.occ.addCurveLoop([l1, l2, l3])
gmsh.model.occ.addPlaneSurface([cl])
gmsh.model.occ.synchronize()
gmsh.model.mesh.generate(2)

out = os.path.join(tempfile.mkdtemp(), "test.exl")

from breakform_gmsh import export_exl, import_exl

export_exl(out)
assert os.path.exists(out), "export failed"

with open(out) as f:
    doc = json.loads(f.read().split("\n", 1)[1])
assert doc["parts"][0]["geometry"]["mesh"]["vertices"], "no vertices"
assert len(doc["parts"][0]["geometry"]["mesh"]["faces"]) >= 1, "no faces"
print(
    "Gmsh plugin test PASSED: exported",
    len(doc["parts"][0]["geometry"]["mesh"]["faces"]),
    "faces",
)

import_exl(out)
node_tags, _, _ = gmsh.model.mesh.getNodes()
elem_types, elem_tags, _ = gmsh.model.mesh.getElements()
total_elems = sum(len(et) for et in elem_tags)
print(
    "Gmsh plugin test PASSED: re-imported",
    len(node_tags),
    "nodes,",
    total_elems,
    "elements",
)

gmsh.finalize()
