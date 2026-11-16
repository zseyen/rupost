"""WP-T4：手动指南 M7 的 1:1 参数化——复现包 digest 的 Python hashlib 交叉验证
（NIST 思路的再一层：Python 标准库 hashlib 独立于 sha2 crate，消除同错互证）。"""
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile

import pytest

BIN = os.environ.get(
    "RUPOST_BIN",
    str(pathlib.Path(__file__).resolve().parent.parent / "target" / "release" / "rupost"),
)


@pytest.fixture
def failing_incident(tmp_path):
    """必失败套件 → incident 复现包；返回 (manifest dict, bundle dir)。"""
    base = "http://127.0.0.1:1"  # 不可达地址：网络错也是失败语义，零网络依赖
    (tmp_path / "pay.http").write_text(
        "### c\n# @assert status == 200\nPOST http://127.0.0.1:1/x\n"
    )
    incident = tmp_path / "inc"
    r = subprocess.run(
        [
            BIN,
            "t",
            "pay.http",
            "--incident-out",
            str(incident),
        ],
        cwd=tmp_path,
        env={**os.environ, "RUPOST_HISTORY_DIR": str(tmp_path / "h")},
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert r.returncode == 1, f"失败运行必须 exit 1: {r.stderr}"
    manifest = json.loads((incident / "manifest.json").read_text())
    return manifest, incident


def test_digests_match_python_hashlib(failing_incident):
    """Python hashlib（独立实现）逐工件交叉验证 rust 侧 sha256 摘要。"""
    manifest, incident = failing_incident
    digests = manifest["artifact_digests"]
    assert len(digests) == 2
    assert all(d["file"] != "manifest.json" for d in digests)
    for d in digests:
        blob = (incident / d["file"]).read_bytes()
        assert d["sha256"] == hashlib.sha256(blob).hexdigest()
        assert d["bytes"] == len(blob)


def test_manifest_schema_exact_keys(failing_incident):
    """Incident schema 只增不改：顶层键集精确（含 WP-R6 的 artifact_digests）。"""
    manifest, incident = failing_incident
    raw = json.loads((incident / "manifest.json").read_text())
    assert set(raw.keys()) == {
        "schema_version",
        "created_at",
        "source_paths",
        "environment",
        "total",
        "failed",
        "skipped",
        "artifacts",
        "masking",
        "max_snapshot_body_bytes",
        "artifact_digests",
    }
    assert raw["schema_version"] == "1.0"
    assert raw["masking"] == "secrets"
