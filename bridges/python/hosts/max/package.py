"""Export a 3ds Max ApplicationPlugins bundle containing Flint Bridge."""
import argparse
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

ROOT = Path(__file__).resolve().parents[2]
BRIDGE = ROOT / "packages/bridge/src/flint_bridge"
HOST = Path(__file__).resolve().parent


def build_bundle(destination: Path, native: Path) -> None:
    if native.name != "flint_bridge_core.dll":
        raise ValueError("3ds Max requires a Windows native Bridge core")
    prefix = "Flint.bundle/"
    entries = {
        prefix + "PackageContents.xml": (HOST / "PackageContents.xml").read_bytes(),
        prefix + "Contents/Scripts/flint_startup.ms": (HOST / "flint_startup.ms").read_bytes(),
        prefix + "Contents/Python/flint_startup.py": (HOST / "flint_startup.py").read_bytes(),
    }
    for source in sorted(BRIDGE.rglob("*.py")):
        entries[prefix + "Contents/Python/flint_bridge/" + source.relative_to(BRIDGE).as_posix()] = source.read_bytes()
    entries[prefix + "Contents/Python/flint_bridge/" + native.name] = native.read_bytes()
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
