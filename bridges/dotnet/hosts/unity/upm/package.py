"""Export a Unity Package Manager tarball for the Editor Bridge."""

import argparse
import gzip
import io
import json
from pathlib import Path
import tarfile
import tomllib
import uuid

UPM = Path(__file__).resolve().parent
UNITY = UPM.parent
DOTNET = UNITY.parents[1]
REPOSITORY = DOTNET.parents[1]

IMPORTERS = {
    ".cs": "MonoImporter:\n  externalObjects: {}\n  serializedVersion: 2\n  defaultReferences: []\n  executionOrder: 0\n  icon: {instanceID: 0}\n",
    ".asmdef": "AssemblyDefinitionImporter:\n  externalObjects: {}\n",
    ".json": "PackageManifestImporter:\n  externalObjects: {}\n",
}


def meta(path: str, folder: bool = False) -> bytes:
    guid = uuid.uuid5(uuid.NAMESPACE_URL, "com.flint.bridge/" + path).hex
    importer = "DefaultImporter:\n  externalObjects: {}\n" if folder else IMPORTERS[Path(path).suffix]
    prefix = "folderAsset: yes\n" if folder else ""
    return (f"fileFormatVersion: 2\nguid: {guid}\n{prefix}{importer}"
            "  userData:\n  assetBundleName:\n  assetBundleVariant:\n").encode("utf-8")


def build_bundle(destination: Path, native: Path) -> None:
    if native.name != "flint_bridge_core.dll":
        raise ValueError("Unity export requires the Windows x64 Bridge core")
    image = native.read_bytes()
    if len(image) < 0x40 or image[:2] != b"MZ":
        raise ValueError("Unity export requires a Windows x64 DLL")
    pe_offset = int.from_bytes(image[0x3c:0x40], "little")
    machine = int.from_bytes(image[pe_offset + 4:pe_offset + 6], "little")
    if image[pe_offset:pe_offset + 4] != b"PE\0\0" or machine != 0x8664:
        raise ValueError("Unity export requires a Windows x64 DLL")
    manifest = json.loads((UPM / "package.json").read_text(encoding="utf-8"))
    workspace = tomllib.loads((REPOSITORY / "Cargo.toml").read_text(encoding="utf-8"))
    manifest["version"] = workspace["workspace"]["package"]["version"]
    entries = {
        "package/package.json": (json.dumps(manifest, indent=2) + "\n").encode("utf-8"),
        "package/Editor/Flint.Unity.Editor.asmdef":
            (UPM / "Flint.Unity.Editor.asmdef").read_bytes(),
        "package/Editor/EditorBootstrap.cs": (UPM / "EditorBootstrap.cs").read_bytes(),
        "package/Editor/EditorConnectionSettings.cs": (UPM / "EditorConnectionSettings.cs").read_bytes(),
        "package/Editor/EditorBridge.cs": (UNITY / "EditorBridge.cs").read_bytes(),
        "package/Editor/NativeBridge.cs":
            (DOTNET / "src/Flint.Bridge/NativeBridge.cs").read_bytes(),
        "package/Editor/Plugins/flint_bridge_core.dll": image,
        "package/Editor/Plugins/flint_bridge_core.dll.meta":
            (UPM / "flint_bridge_core.dll.meta").read_bytes(),
    }
    for folder in ("Editor", "Editor/Plugins"):
        entries["package/" + folder + ".meta"] = meta(folder, folder=True)
    for name in list(entries):
        if not name.endswith((".meta", ".dll")):
            entries[name + ".meta"] = meta(name.removeprefix("package/"))
    destination.parent.mkdir(parents=True, exist_ok=True)
    with destination.open("wb") as output:
        with gzip.GzipFile(fileobj=output, mode="wb", filename="", mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
                for name, content in sorted(entries.items()):
                    item = tarfile.TarInfo(name)
                    item.size = len(content)
                    item.mode = 0o644
                    item.mtime = 0
                    archive.addfile(item, io.BytesIO(content))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--native", required=True, type=Path)
    args = parser.parse_args()
    build_bundle(args.output, args.native)
