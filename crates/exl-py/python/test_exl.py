import json
import os
import shutil
import sys
import tempfile

from exl import content_hash, convert, diff, load_json, validate, __version__  # noqa: F401


def test():
    assert isinstance(__version__, str)
    assert len(__version__) > 0

    stl_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "corpus", "cube-ascii.stl"
    )
    if not os.path.exists(stl_path):
        stl_path = os.path.join(
            os.path.dirname(__file__), "..", "..", "..", "corpus", "cube-ascii.stl"
        )
    if not os.path.exists(stl_path):
        raise FileNotFoundError(
            f"Cannot find cube-ascii.stl at resolved path: {stl_path}"
        )

    tmpdir = tempfile.mkdtemp()
    try:
        exl_path = os.path.join(tmpdir, "cube.exl")

        reports = convert(stl_path, exl_path)
        reports_parsed = json.loads(reports)
        assert isinstance(reports_parsed, list)

        doc_json = load_json(exl_path)
        doc = json.loads(doc_json)
        assert "parts" in doc
        assert len(doc["parts"]) == 1

        findings = validate(exl_path, "mech")
        findings_list = json.loads(findings)
        assert isinstance(findings_list, list)
        errors = [f for f in findings_list if f.get("severity") == "error"]
        assert len(errors) == 0, f"Unexpected validation errors: {errors}"

        diff_report = diff(exl_path, exl_path)
        diff_obj = json.loads(diff_report)
        assert diff_obj["topology"]["added"] == []
        assert diff_obj["topology"]["removed"] == []
        assert diff_obj["topology"]["modified"] == []
        assert diff_obj["transforms"] == []
        assert diff_obj["metadata"] == []

        h = content_hash(exl_path)
        assert len(h) == 64, f"Expected hash length 64, got {len(h)}"
        assert all(c in "0123456789abcdef" for c in h)

        print("OK: all smoke tests passed")
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


if __name__ == "__main__":
    test()
