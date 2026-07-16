import importlib.util
import json
import os
import shutil
import sys
import tempfile

# When running from this directory, the local exl/ package shadows the installed
# native .so. Prefer the installed package by removing the script directory from
# the front of sys.path if it contains a source-only exl/ without the .so.
_script_dir = os.path.dirname(os.path.abspath(__file__))
_local_exl = os.path.join(_script_dir, "exl", "__init__.py")
if os.path.exists(_local_exl) and not os.path.exists(os.path.join(_script_dir, "exl", "exl.abi3.so")):
    sys.path = [p for p in sys.path if os.path.abspath(p) != _script_dir]

from exl import content_hash, convert, diff, load_json, validate, save_document, __version__  # noqa: F401


def _meshio_bridge():
    spec = importlib.util.spec_from_file_location(
        "exl.meshio_bridge",
        os.path.join(_script_dir, "exl", "meshio_bridge.py"),
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


CORPUS = os.path.join(os.path.dirname(__file__), "..", "..", "corpus")
if not os.path.isdir(CORPUS):
    CORPUS = os.path.join(os.path.dirname(__file__), "..", "..", "..", "corpus")


def resolve(path):
    p = os.path.join(CORPUS, path)
    if not os.path.exists(p):
        raise FileNotFoundError(f"Cannot find {path} at {p}")
    return p


def convert_and_verify(input_path, tmpdir):
    exl_path = os.path.join(tmpdir, "out.exl")
    reports = convert(input_path, exl_path)
    reports_parsed = json.loads(reports)
    assert isinstance(reports_parsed, list)

    doc_json = load_json(exl_path)
    doc = json.loads(doc_json)
    assert "parts" in doc

    findings = validate(exl_path, "mech")
    findings_list = json.loads(findings)
    assert isinstance(findings_list, list)
    errors = [f for f in findings_list if f.get("severity") == "error"]
    assert len(errors) == 0, f"Unexpected validation errors: {errors}"

    h = content_hash(exl_path)
    assert len(h) == 64, f"Expected hash length 64, got {len(h)}"
    assert all(c in "0123456789abcdef" for c in h)

    return exl_path


def test():
    assert isinstance(__version__, str)
    assert len(__version__) > 0

    stl_path = resolve("cube-ascii.stl")

    tmpdir = tempfile.mkdtemp()
    try:
        exl_path = convert_and_verify(stl_path, tmpdir)

        diff_report = diff(exl_path, exl_path)
        diff_obj = json.loads(diff_report)
        assert diff_obj["topology"]["added"] == []
        assert diff_obj["topology"]["removed"] == []
        assert diff_obj["topology"]["modified"] == []
        assert diff_obj["transforms"] == []
        assert diff_obj["metadata"] == []

        print("OK: core smoke tests passed")
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def test_nastran():
    nastran_path = resolve("nastran-simple.bdf")
    tmpdir = tempfile.mkdtemp()
    try:
        exl_path = convert_and_verify(nastran_path, tmpdir)
        print("OK: nastran .bdf convert/validate/hash passed")
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def test_abaqus():
    abaqus_path = resolve("abaqus-cube.inp")
    tmpdir = tempfile.mkdtemp()
    try:
        exl_path = convert_and_verify(abaqus_path, tmpdir)
        print("OK: abaqus .inp convert/validate/hash passed")
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def test_openfoam():
    foam_path = resolve("openfoam-cavity")
    tmpdir = tempfile.mkdtemp()
    try:
        exl_path = convert_and_verify(foam_path, tmpdir)
        print("OK: OpenFOAM case directory convert/validate/hash passed")
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def test_meshio_import_gmsh():
    import numpy as np

    mb = _meshio_bridge()
    points = np.array([[0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1]], dtype=float)
    cells = [__import__("meshio").CellBlock("tetra", np.array([[0, 1, 2, 3]]))]
    mesh = __import__("meshio").Mesh(points, cells)

    tmpdir = tempfile.mkdtemp()
    try:
        msh_path = os.path.join(tmpdir, "test.msh")
        mesh.write(msh_path, file_format="gmsh")

        doc, fid = mb.import_via_meshio(msh_path)
        assert doc is not None, "import_via_meshio returned None"
        assert fid is not None

        part = doc["parts"][0]
        verts = part["geometry"]["mesh"]["vertices"]
        faces = part["geometry"]["mesh"]["faces"]
        assert len(verts) == 4, f"expected 4 vertices, got {len(verts)}"
        assert len(faces) == 4, f"expected 4 faces, got {len(faces)}"

        exl_path = os.path.join(tmpdir, "test.exl")
        save_document(json.dumps(doc), exl_path)
        loaded_json = load_json(exl_path)
        loaded_doc = json.loads(loaded_json)
        assert loaded_doc["parts"][0]["geometry"]["mesh"]["vertices"] == verts
        assert loaded_doc["parts"][0]["geometry"]["mesh"]["faces"] == faces
        assert loaded_doc["parts"][0]["bounding_box"]["min"] is not None
        assert loaded_doc["parts"][0]["bounding_box"]["max"] is not None

        print("OK: meshio import gmsh round-trip passed")
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def test_meshio_import_exodus():
    import numpy as np

    mb = _meshio_bridge()
    meshio_mod = __import__("meshio")
    from meshio._helpers import _writer_map

    if "exodus" not in _writer_map:
        print("SKIP: exodus writer not available")
        return

    points = np.array([[0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1]], dtype=float)
    cells = [meshio_mod.CellBlock("tetra", np.array([[0, 1, 2, 3]]))]
    mesh = meshio_mod.Mesh(points, cells)

    tmpdir = tempfile.mkdtemp()
    try:
        exo_path = os.path.join(tmpdir, "test.exo")
        try:
            mesh.write(exo_path, file_format="exodus")
        except Exception as e:
            print(f"SKIP: exodus write failed: {e}")
            return

        doc, fid = mb.import_via_meshio(exo_path)
        assert doc is not None
        assert len(doc["parts"][0]["geometry"]["mesh"]["vertices"]) == 4
        assert len(doc["parts"][0]["geometry"]["mesh"]["faces"]) == 4

        print("OK: meshio import exodus passed")
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def test_meshio_unknown_cell_dropped():
    import numpy as np

    mb = _meshio_bridge()
    meshio_mod = __import__("meshio")
    points = np.array([[0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1]], dtype=float)
    cells = [
        meshio_mod.CellBlock("tetra", np.array([[0, 1, 2, 3]])),
        meshio_mod.CellBlock("vertex", np.array([[0], [1], [2], [3]])),
    ]
    mesh = meshio_mod.Mesh(points, cells)

    tmpdir = tempfile.mkdtemp()
    try:
        vtk_path = os.path.join(tmpdir, "test.vtk")
        mesh.write(vtk_path, file_format="vtk")

        doc, fid = mb.import_via_meshio(vtk_path)
        assert doc is not None

        dropped = [e for e in fid["entities"] if e["status"] == "dropped"]
        assert any("vertex" in e["entity"] for e in dropped), \
            f"expected vertex entities to be dropped, fid={fid}"

        print("OK: meshio unknown cell type dropped in fidelity")
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def test_meshio_format_detection():
    mb = _meshio_bridge()
    assert "gmsh" in mb.SUPPORTED_IMPORT, f"gmsh not in SUPPORTED_IMPORT: {mb.SUPPORTED_IMPORT}"
    assert "ansys" in mb.SUPPORTED_IMPORT, f"ansys not in SUPPORTED_IMPORT: {mb.SUPPORTED_IMPORT}"
    assert "vtk" in mb.SUPPORTED_EXPORT, f"vtk not in SUPPORTED_EXPORT: {mb.SUPPORTED_EXPORT}"
    assert "nastran" in mb.SUPPORTED_EXPORT, f"nastran not in SUPPORTED_EXPORT: {mb.SUPPORTED_EXPORT}"
    print("OK: meshio format detection passed")


if __name__ == "__main__":
    test()
    test_nastran()
    test_abaqus()
    test_openfoam()
    test_meshio_import_gmsh()
    test_meshio_import_exodus()
    test_meshio_unknown_cell_dropped()
    test_meshio_format_detection()
