"""Verify the ZIP contains the Python adapter and native core."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
from zipfile import ZipFile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("package_bridge", ROOT / "tools/package_bridge.py")
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)


def test_bundle_is_deterministic_and_contains_native_core(tmp_path):
    first, second = tmp_path / "first.zip", tmp_path / "second.zip"
    packager.build_bundle(first)
    packager.build_bundle(second)
    assert first.read_bytes() == second.read_bytes()
    with ZipFile(first) as archive:
        names = archive.namelist()
        assert "flint_bridge/__init__.py" in names
        assert "unity/EditorBridge.cs" in names
        assert "unity/NativeBridge.cs" in names
        assert "flint_bridge/native/" + Path(os.environ["FLINT_BRIDGE_CORE_LIBRARY"]).name in names
        assert not any(name.startswith(("flint_protocol/", "google/")) for name in names)
        assert not any(name.endswith(".pyc") for name in names)
        assert json.loads(archive.read("flint-bridge.json"))["native_core"] == Path(os.environ["FLINT_BRIDGE_CORE_LIBRARY"]).name


def test_bundle_imports_without_site_packages(tmp_path):
    bundle = tmp_path / "bridge.zip"
    packager.build_bundle(bundle)
    script = "\n".join([
        "import sys",
        "sys.path.insert(0, " + repr(str(bundle)) + ")",
        "import flint_bridge",
        "from flint_bridge.connection.native import NativeCore",
        "core = NativeCore({'host':'python','address':'127.0.0.1','port':1,'name':'bundle',",
        "    'runtime_version':'CPython'})",
        "assert not core.connected",
        "core.stop()",
        "core.close()",
        "print('BUNDLE_OK')",
    ])
    result = subprocess.run([sys.executable, "-I", "-S", "-c", script], cwd=tmp_path,
                            capture_output=True, text=True, timeout=15)
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "BUNDLE_OK"
