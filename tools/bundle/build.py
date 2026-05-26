from pathlib import Path
import shutil
import sys
import textwrap


def ensure_removed(path: Path) -> None:
    shutil.rmtree(path, ignore_errors=True)
    if path.exists():
        raise RuntimeError(
            f"Could not clean `{path}`. Close any running shim/backend processes that use this directory and retry."
        )


def generate_spec(project_root: Path) -> str:
    src_path = str(project_root / "src").replace("\\", "/")
    shim_entry = str(project_root / "tools" / "bundle" / "entrypoints" / "shim_entry.py").replace("\\", "/")
    backend_entry = str(project_root / "tools" / "bundle" / "entrypoints" / "backend_entry.py").replace("\\", "/")

    return textwrap.dedent(f"""\
        # -*- mode: python ; coding: utf-8 -*-
        from PyInstaller.utils.hooks import collect_data_files

        _schema_pkg_datas = collect_data_files("jsonschema_specifications")


        def merge_toc(*collections):
            merged = []
            seen = set()
            for collection in collections:
                for entry in collection:
                    key = entry[0]
                    if key in seen:
                        continue
                    seen.add(key)
                    merged.append(entry)
            return merged


        analysis_kwargs = dict(
            pathex=[{src_path!r}],
            binaries=[],
            datas=_schema_pkg_datas,
            hiddenimports=[],
            hookspath=[],
            hooksconfig={{}},
            runtime_hooks=[],
            excludes=[],
            noarchive=False,
        )

        shim_analysis = Analysis([{shim_entry!r}], **analysis_kwargs)
        backend_analysis = Analysis([{backend_entry!r}], **analysis_kwargs)

        shim_pyz = PYZ(shim_analysis.pure)
        backend_pyz = PYZ(backend_analysis.pure)

        shim_exe = EXE(
            shim_pyz,
            shim_analysis.scripts,
            [],
            exclude_binaries=True,
            name="shim",
            console=True,
        )

        backend_exe = EXE(
            backend_pyz,
            backend_analysis.scripts,
            [],
            exclude_binaries=True,
            name="backend",
            console=True,
        )

        COLLECT(
            shim_exe,
            backend_exe,
            merge_toc(shim_analysis.binaries, backend_analysis.binaries),
            merge_toc(shim_analysis.zipfiles, backend_analysis.zipfiles),
            merge_toc(shim_analysis.datas, backend_analysis.datas),
            strip=False,
            upx=True,
            upx_exclude=[],
            name="python-bridge-mcp",
        )
    """)


def main() -> int:
    try:
        from PyInstaller.__main__ import run as pyinstaller_run
    except ImportError:
        print("PyInstaller is not installed. Run `python -m pip install -e .[build]` first.", file=sys.stderr)
        return 1

    bundle_dir = Path(__file__).resolve().parent
    project_root = bundle_dir.parents[1]
    dist_dir = project_root / "dist"
    output_dir = dist_dir / "python-bridge-mcp"
    work_dir = project_root / "build" / "pyinstaller"
    spec_path = work_dir / "windows_bundle.spec"

    try:
        ensure_removed(output_dir)
        ensure_removed(work_dir)

        work_dir.mkdir(parents=True, exist_ok=True)
        spec_path.write_text(generate_spec(project_root), encoding="utf-8")

        pyinstaller_run([
            "--noconfirm",
            "--clean",
            "--distpath",
            str(dist_dir),
            "--workpath",
            str(work_dir),
            str(spec_path),
        ])
    except RuntimeError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
