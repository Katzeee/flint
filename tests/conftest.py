import asyncio
import socket
import threading
import time


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
    """Runs an async coroutine in a background thread with its own event loop."""

    def __init__(self) -> None:
        self._loop: asyncio.AbstractEventLoop
        self._thread: threading.Thread

    def start(self, coro_fn) -> None:
        ready = threading.Event()

        def _thread_target() -> None:
            self._loop = asyncio.new_event_loop()
            asyncio.set_event_loop(self._loop)
            try:
                self._loop.run_until_complete(self._boot(coro_fn, ready))
            except (asyncio.CancelledError, RuntimeError):
                pass
            finally:
                pending = asyncio.all_tasks(self._loop)
                if pending:
                    for t in pending:
                        t.cancel()
                    self._loop.run_until_complete(
                        asyncio.gather(*pending, return_exceptions=True)
                    )
                self._loop.close()

        self._thread = threading.Thread(target=_thread_target, daemon=True)
        self._thread.start()
        assert ready.wait(timeout=5), "server did not start in time"

    async def _boot(self, coro_fn, ready: threading.Event) -> None:
        task = asyncio.ensure_future(coro_fn())
        await asyncio.sleep(0.05)
        ready.set()
        await task

    def stop(self) -> None:
        self._loop.call_soon_threadsafe(self._loop.stop)
        self._thread.join(timeout=5)

    def run_async(self, coro):
        future = asyncio.run_coroutine_threadsafe(coro, self._loop)
        return future.result(timeout=10)
