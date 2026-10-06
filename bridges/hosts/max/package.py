"""Export a 3ds Max ApplicationPlugins bundle containing Flint Bridge."""
import argparse
from pathlib import Path
import sys
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "platforms/python/tools"))
from package_bridge import source_entries
HOST = Path(__file__).resolve().parent


def build_bundle(destination: Path, native: Path) -> None:
    if native.name != "flint_bridge_core.dll":
        raise ValueError("3ds Max requires a Windows native Bridge core")
    prefix = "Flint.bundle/"
    entries = {
        prefix + "PackageContents.xml": (HOST / "PackageContents.xml").read_bytes(),
        prefix + "Contents/Scripts/flint_startup.ms": (HOST / "flint_startup.ms").read_bytes(),
        prefix + "Contents/Scripts/flint_settings.mcr": (HOST / "flint_settings.mcr").read_bytes(),
        prefix + "Contents/Python/flint_startup.py": (HOST / "flint_startup.py").read_bytes(),
        prefix + "Contents/Python/flint_max.py": (HOST / "flint_max.py").read_bytes(),
    }
    for name, content in source_entries().items():
        entries[prefix + "Contents/Python/" + name] = content
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
