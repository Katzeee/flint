"""Package the host bridge and native connection core."""
import argparse
import json
import os
from pathlib import Path
import tomllib
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = ROOT.parents[1]


def build_bundle(destination: Path, native: Path = None) -> None:
    entries = {}
    folder = ROOT / "packages/bridge/src/flint_bridge"
    for source in sorted(folder.rglob("*.py")):
        entries["flint_bridge/" + source.relative_to(folder).as_posix()] = source.read_bytes()
    for name, source in {
        "unity/EditorBridge.cs": REPOSITORY / "bridges/dotnet/unity/EditorBridge.cs",
        "unity/NativeBridge.cs": REPOSITORY / "bridges/dotnet/src/Flint.Bridge/NativeBridge.cs",
    }.items():
        entries[name] = source.read_bytes()
    native = native or Path(os.environ["FLINT_BRIDGE_CORE_LIBRARY"])
    if native.name not in ("flint_bridge_core.dll", "libflint_bridge_core.so", "libflint_bridge_core.dylib"):
        raise ValueError("Unexpected native Bridge core name: " + native.name)
    entries["flint_bridge/native/" + native.name] = native.read_bytes()
    project = tomllib.loads((ROOT / "packages/bridge/pyproject.toml").read_text(encoding="utf-8"))
    entries["flint-bridge.json"] = json.dumps({
        "version": project["project"]["version"], "protocol": 1,
        "hosts": ["maya", "max", "python", "unity"], "mode": "active",
        "python_minimum": "3.7",
        "native_core": native.name,
    }, sort_keys=True).encode("utf-8")

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
