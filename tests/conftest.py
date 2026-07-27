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
        self._thread_error = None

    def start(self, coro_fn) -> None:
        ready = threading.Event()

        def _thread_target() -> None:
            async def _main() -> None:
                self._loop = asyncio.get_running_loop()
                stop_event = asyncio.Event()

                def _request_stop() -> None:
                    if self._loop.is_closed():
                        return
                    try:
                        self._loop.call_soon_threadsafe(stop_event.set)
                    except RuntimeError:
                        pass

                self._request_stop = _request_stop

                task = asyncio.ensure_future(coro_fn())
                await asyncio.sleep(0.05)
                ready.set()

                stop_task = asyncio.create_task(stop_event.wait())
                await asyncio.wait(
                    [task, stop_task],
                    return_when=asyncio.FIRST_COMPLETED,
                )

                current_task = asyncio.current_task()
                owned_tasks = [
                    pending_task
                    for pending_task in asyncio.all_tasks()
                    if pending_task is not current_task
                ]
                for pending_task in owned_tasks:
                    if not pending_task.done():
                        pending_task.cancel()
                await asyncio.gather(*owned_tasks, return_exceptions=True)

                if task.cancelled():
                    return
                task_result = task.exception()
                if (
                    isinstance(task_result, BaseException)
                    and not isinstance(task_result, asyncio.CancelledError)
                ):
                    raise task_result

            try:
                asyncio.run(_main())
            except BaseException as exc:
                self._thread_error = exc
            finally:
                ready.set()

        self._thread = threading.Thread(target=_thread_target, daemon=True)
        self._thread.start()
        assert ready.wait(timeout=5), "server did not start in time"
        if self._thread_error is not None:
            raise RuntimeError("async service failed during startup") from self._thread_error

    def stop(self) -> None:
        if self._request_stop is not None:
            self._request_stop()
        self._thread.join(timeout=5)
        if self._thread.is_alive():
            raise RuntimeError("async service did not stop within 5 seconds")
        if self._thread_error is not None:
            raise RuntimeError("async service failed") from self._thread_error

    def run_async(self, coro):
        future = asyncio.run_coroutine_threadsafe(coro, self._loop)
        return future.result(timeout=10)
