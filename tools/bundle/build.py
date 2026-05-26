from pathlib import Path
import shutil
import sys


def ensure_removed(path: Path) -> None:
    shutil.rmtree(path, ignore_errors=True)
    if path.exists():
        raise RuntimeError(
            f"Could not clean `{path}`. Close any running shim/backend processes that use this directory and retry."
        )


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
    spec_path = bundle_dir / "windows_bundle.spec"

    try:
        ensure_removed(output_dir)
        ensure_removed(work_dir)

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
