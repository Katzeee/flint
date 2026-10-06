"""Package the host bridge and native connection core."""
import argparse
from pathlib import Path
import tomllib
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

ROOT = Path(__file__).resolve().parents[3]


def source_entries() -> dict[str, bytes]:
    """Assemble the declared Python packages from their owning source directories."""
    manifest = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))
    packages = manifest["tool"]["setuptools"]
    entries = {}
    for package in packages["packages"]:
        folder = ROOT / packages["package-dir"][package]
        for source in sorted(folder.glob("*.py")):
            entries[package.replace(".", "/") + "/" + source.name] = source.read_bytes()
    return entries


def build_bundle(destination: Path, native: Path) -> None:
    entries = source_entries()
    if native.name not in ("flint_bridge_core.dll", "libflint_bridge_core.so", "libflint_bridge_core.dylib"):
        raise ValueError("Unexpected native Bridge core name: " + native.name)
    entries["flint_bridge/" + native.name] = native.read_bytes()
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
