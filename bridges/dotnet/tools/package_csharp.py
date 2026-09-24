"""Export the C# binding and native core as a flat ZIP."""

import argparse
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

DOTNET = Path(__file__).resolve().parents[1]


def build_bundle(destination: Path, native: Path) -> None:
    entries = {
        "NativeBridge.cs": (DOTNET / "src/Flint.Bridge/NativeBridge.cs").read_bytes(),
        native.name: native.read_bytes(),
    }
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
