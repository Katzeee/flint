"""Export a Maya module containing the Flint plug-in and Python Bridge."""
import argparse
from pathlib import Path
import sys
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "platforms/python/tools"))
from package_bridge import source_entries
HOST = Path(__file__).resolve().parent
NATIVE_NAMES = {"flint_bridge_core.dll", "libflint_bridge_core.so", "libflint_bridge_core.dylib"}


def build_bundle(destination: Path, native: Path) -> None:
    if native.name not in NATIVE_NAMES:
        raise ValueError("Unexpected native Bridge core name: " + native.name)
    entries = {
        "flint.mod": (HOST / "flint.mod").read_bytes(),
        "flint/plug-ins/flint_plugin.py": (HOST / "flint_plugin.py").read_bytes(),
        "flint/scripts/flint_maya.py": (HOST / "flint_maya.py").read_bytes(),
    }
    for name, content in source_entries().items():
        entries["flint/scripts/" + name] = content
    entries["flint/scripts/flint_bridge/" + native.name] = native.read_bytes()
    destination.parent.mkdir(parents=True, exist_ok=True)
    with ZipFile(destination, "w") as archive:
        for name, content in sorted(entries.items()):
            entry = ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            entry.external_attr = 0o644 << 16
            archive.writestr(entry, content, compress_type=ZIP_DEFLATED)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--native", required=True, type=Path)
    args = parser.parse_args()
    build_bundle(args.output, args.native)
