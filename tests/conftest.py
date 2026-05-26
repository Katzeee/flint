import asyncio
import socket
import threading
import time
from pathlib import Path

import pytest

from python_bridge_mcp.shared.workflow_persistence import WorkflowPersistence


@pytest.fixture(autouse=True)
def _workflow_tmpdir(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(WorkflowPersistence, "BASE_DIR", tmp_path / "workflows")


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("", 0))
        return s.getsockname()[1]


def wait_for(condition, timeout: float = 3.0, poll: float = 0.05) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if condition():
            return True
        time.sleep(poll)
    return False


class AsyncRunner:
    """Runs an async coroutine in a background thread with proper shutdown.

    Uses asyncio.run() internally so the event loop, transports, and the
    Windows IOCP proactor are cleaned up correctly on all platforms.
    """

    def __init__(self) -> None:
        self._loop: asyncio.AbstractEventLoop
        self._thread: threading.Thread
        self._request_stop = None  # callable, set inside _main

    def start(self, coro_fn) -> None:
        ready = threading.Event()

        def _thread_target() -> None:
            async def _main() -> None:
                self._loop = asyncio.get_running_loop()
                stop_event = asyncio.Event()
                self._request_stop = lambda: self._loop.call_soon_threadsafe(stop_event.set)

                task = asyncio.ensure_future(coro_fn())
                await asyncio.sleep(0.05)
                ready.set()

                stop_task = asyncio.create_task(stop_event.wait())
                await asyncio.wait(
                    [task, stop_task],
                    return_when=asyncio.FIRST_COMPLETED,
                )

            asyncio.run(_main())

        self._thread = threading.Thread(target=_thread_target, daemon=True)
        self._thread.start()
        assert ready.wait(timeout=5), "server did not start in time"

    def stop(self) -> None:
        if self._request_stop is not None:
            self._request_stop()
        self._thread.join(timeout=5)

    def run_async(self, coro):
        future = asyncio.run_coroutine_threadsafe(coro, self._loop)
        return future.result(timeout=10)
