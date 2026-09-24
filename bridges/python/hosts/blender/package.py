"""Export an installable Blender Add-on ZIP with its native Bridge core."""
import argparse
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

ROOT = Path(__file__).resolve().parents[2]
BRIDGE = ROOT / "packages/bridge/src/flint_bridge"
ADDON = Path(__file__).resolve().parent / "addon"


def build_bundle(destination: Path, native: Path) -> None:
    if native.name not in ("flint_bridge_core.dll", "libflint_bridge_core.so", "libflint_bridge_core.dylib"):
        raise ValueError("Unexpected native Bridge core name: " + native.name)
    entries = {"flint_blender/__init__.py": (ADDON / "__init__.py").read_bytes()}
    for source in sorted(BRIDGE.rglob("*.py")):
        entries["flint_blender/flint_bridge/" + source.relative_to(BRIDGE).as_posix()] = source.read_bytes()
    entries["flint_blender/flint_bridge/" + native.name] = native.read_bytes()
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
