"""Build the native core and load its exported ZIP for component tests."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
LIBRARY = (
    "flint_bridge_core.dll" if sys.platform == "win32" else
    "libflint_bridge_core.dylib" if sys.platform == "darwin" else
    "libflint_bridge_core.so"
)
subprocess.run(["cargo", "build", "--locked", "-p", "flint-bridge-core"], cwd=ROOT, check=True)
target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
if not target.is_absolute():
    target = ROOT / target
NATIVE = target / "debug" / LIBRARY
bundle_dir = tempfile.TemporaryDirectory(prefix="flint-python-tests-")
bundle = Path(bundle_dir.name) / "flint-python.zip"
subprocess.run([
    sys.executable, str(ROOT / "bridges/python/tools/package_bridge.py"),
    str(bundle), "--native", str(NATIVE),
], cwd=ROOT, check=True)
sys.path.insert(0, str(bundle))


def pytest_unconfigure(config):
    bundle_dir.cleanup()
