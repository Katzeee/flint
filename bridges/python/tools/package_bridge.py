"""Package the active uv environment's runtime dependency and workspace sources."""
import argparse
from importlib.metadata import distribution
import json
from pathlib import Path
import tomllib
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

ROOT = Path(__file__).resolve().parents[1]


def build_bundle(destination: Path) -> None:
    entries = {}
    for folder, package in (
        (ROOT / "packages/bridge/src/flint_bridge", "flint_bridge"),
        (ROOT / "packages/protocol/src/flint_protocol", "flint_protocol"),
    ):
        for source in sorted(folder.rglob("*.py")):
            entries[package + "/" + source.relative_to(folder).as_posix()] = source.read_bytes()

    protobuf = distribution("protobuf")
    for source in protobuf.files or ():
        if source.parts[:2] == ("google", "protobuf") and source.suffix == ".py":
            entries[source.as_posix()] = Path(protobuf.locate_file(source)).read_bytes()
        elif source.name == "LICENSE" and source.parts[0].endswith(".dist-info"):
            entries["licenses/protobuf.txt"] = Path(protobuf.locate_file(source)).read_bytes()
    for required in ("google/protobuf/__init__.py", "licenses/protobuf.txt"):
        if required not in entries:
            raise RuntimeError("Incomplete Protobuf installation: " + required)

    entries["google/__init__.py"] = b"from pkgutil import extend_path\n__path__ = extend_path(__path__, __name__)\n"
    project = tomllib.loads((ROOT / "packages/bridge/pyproject.toml").read_text(encoding="utf-8"))
    entries["flint-bridge.json"] = json.dumps({
        "version": project["project"]["version"], "protocol": 1,
        "hosts": ["maya", "max", "python"], "mode": "active",
        "python_minimum": "3.7", "protobuf": protobuf.version,
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
    build_bundle(parser.parse_args().output)
