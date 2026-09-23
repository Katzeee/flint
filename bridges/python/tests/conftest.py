"""Build the native core used by Bridge component and bundle tests."""
import os
from pathlib import Path
import subprocess
import sys


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
os.environ["FLINT_BRIDGE_CORE_LIBRARY"] = str(target / "debug" / LIBRARY)
