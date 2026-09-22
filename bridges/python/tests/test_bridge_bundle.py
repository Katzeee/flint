"""Verify the portable payload assembled from uv-resolved dependencies."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
from zipfile import ZipFile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("package_bridge", ROOT / "tools/package_bridge.py")
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)


def test_bundle_is_deterministic_and_contains_only_portable_runtime_code(tmp_path):
    first, second = tmp_path / "first.zip", tmp_path / "second.zip"
    packager.build_bundle(first)
    packager.build_bundle(second)
    assert first.read_bytes() == second.read_bytes()
    with ZipFile(first) as archive:
        names = archive.namelist()
        assert "flint_bridge/__init__.py" in names
        assert "flint_protocol/v1/envelope_pb2.py" in names
        assert "licenses/protobuf.txt" in names
        assert not any(name.endswith((".dll", ".pyd", ".so", ".pyc")) for name in names)
        assert json.loads(archive.read("flint-bridge.json"))["protobuf"] == "4.24.4"


def test_bundle_imports_without_site_packages(tmp_path):
    bundle = tmp_path / "bridge.zip"
    packager.build_bundle(bundle)
    script = "\n".join([
        "import sys",
        "sys.path.insert(0, " + repr(str(bundle)) + ")",
        "import flint_bridge",
        "from flint_protocol.v1.envelope_pb2 import Envelope",
        "request = Envelope(protocol_version=1, request_id='portable')",
        "request.heartbeat.instance_id = 'host'",
        "assert Envelope.FromString(request.SerializeToString()) == request",
        "print('BUNDLE_OK')",
    ])
    result = subprocess.run([sys.executable, "-I", "-S", "-c", script], cwd=tmp_path,
                            capture_output=True, text=True, timeout=15)
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "BUNDLE_OK"
