import os
from contextlib import contextmanager
from pathlib import Path
from typing import Iterator, Union

from filelock import BaseFileLock, FileLock


class FileWriter:
    """Read/write files with cross-process file locking.

    Usage:
        with FileWriter.locked(path) as f:
            data = f.read()
            f.write(data + "appended")
    """

    def __init__(self, path: Path) -> None:
        self._path = path

    @property
    def path(self) -> Path:
        return self._path

    @staticmethod
    def _lock_for(path: Path, timeout: float) -> BaseFileLock:
        return FileLock(str(path) + ".lock", timeout=timeout)

    def write(self, data: str) -> None:
        """Write string to file. Atomic: writes .tmp then replaces."""
        self._path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self._path.with_suffix(".tmp")
        with tmp.open("w", encoding="utf-8", newline="\n") as handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        tmp.replace(self._path)

    def read(self) -> str:
        """Read file contents as string."""
        return self._path.read_text(encoding="utf-8")

    def exists(self) -> bool:
        return self._path.exists()

    @staticmethod
    @contextmanager
    def locked(path: Union[str, Path], *, timeout: float = 10.0) -> Iterator["FileWriter"]:
        """Context manager that holds the file lock for the duration of the block."""
        p = Path(path)
        with FileWriter._lock_for(p, timeout):
            yield FileWriter(p)
