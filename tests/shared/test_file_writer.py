import threading
from pathlib import Path

import pytest

from pbridge.shared.file_writer import FileWriter


def test_write_and_read_string(tmp_path: Path) -> None:
    with FileWriter.locked(tmp_path / "f.txt") as f:
        f.write("hello world")
        assert f.read() == "hello world"


def test_write_multiline_string(tmp_path: Path) -> None:
    text = "line1\nline2\nline3"
    with FileWriter.locked(tmp_path / "f.txt") as f:
        f.write(text)
        assert f.read() == text


def test_read_modify_write(tmp_path: Path) -> None:
    p = tmp_path / "f.txt"
    with FileWriter.locked(p) as f:
        f.write("v1")
    with FileWriter.locked(p) as f:
        data = f.read()
        f.write(data + "+v2")
    with FileWriter.locked(p) as f:
        assert f.read() == "v1+v2"


def test_exists_false_then_true(tmp_path: Path) -> None:
    with FileWriter.locked(tmp_path / "f.txt") as f:
        assert not f.exists()
        f.write("data")
        assert f.exists()


def test_write_creates_parent_dirs(tmp_path: Path) -> None:
    nested = tmp_path / "a" / "b" / "c" / "f.txt"
    with FileWriter.locked(nested) as f:
        f.write("data")
    assert nested.exists()


def test_atomic_write_cleans_tmp(tmp_path: Path) -> None:
    with FileWriter.locked(tmp_path / "f.txt") as f:
        f.write("data")
    assert not (tmp_path / "f.tmp").exists()


def test_read_nonexistent_raises(tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError):
        with FileWriter.locked(tmp_path / "missing.txt") as f:
            f.read()


# ---------------------------------------------------------------------------
# Concurrency
# ---------------------------------------------------------------------------

def test_concurrent_writes_are_serialized(tmp_path: Path) -> None:
    """Multiple threads incrementing a counter — final value must equal thread count."""
    p = tmp_path / "counter.txt"
    with FileWriter.locked(p) as f:
        f.write("0")

    n_threads = 20
    barrier = threading.Barrier(n_threads)

    def _increment() -> None:
        barrier.wait()
        with FileWriter.locked(p) as f:
            val = int(f.read())
            f.write(str(val + 1))

    threads = [threading.Thread(target=_increment) for _ in range(n_threads)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    with FileWriter.locked(p) as f:
        assert f.read() == str(n_threads)


def test_concurrent_read_write_no_partial(tmp_path: Path) -> None:
    """Readers never see a partial or empty write."""
    p = tmp_path / "data.txt"
    content = "A" * 10000
    with FileWriter.locked(p) as f:
        f.write(content)

    errors = []
    stop = threading.Event()

    def _writer() -> None:
        while not stop.is_set():
            with FileWriter.locked(p) as f:
                f.write(content)

    def _reader() -> None:
        while not stop.is_set():
            with FileWriter.locked(p) as f:
                data = f.read()
                if data != content:
                    errors.append(data)

    threads = [threading.Thread(target=_writer) for _ in range(3)]
    threads += [threading.Thread(target=_reader) for _ in range(3)]
    for t in threads:
        t.start()

    stop.wait(timeout=0.5)
    stop.set()
    for t in threads:
        t.join()

    assert errors == []
