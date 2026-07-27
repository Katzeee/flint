import asyncio
import threading
from unittest.mock import Mock

from python_bridge_mcp.server.asyncio_utils import request_server_close

from conftest import AsyncRunner


def test_server_close_is_scheduled_on_its_owner_loop() -> None:
    loop = Mock(spec=asyncio.AbstractEventLoop)
    loop.is_closed.return_value = False
    server = Mock(spec=asyncio.AbstractServer)

    request_server_close(loop, server)

    loop.call_soon_threadsafe.assert_called_once_with(server.close)
    server.close.assert_not_called()


def test_server_close_is_immediate_from_its_owner_loop() -> None:
    server = Mock(spec=asyncio.AbstractServer)

    async def close_server() -> None:
        request_server_close(asyncio.get_running_loop(), server)

    asyncio.run(close_server())

    server.close.assert_called_once_with()


def test_async_runner_drains_service_task_before_stop_returns() -> None:
    started = threading.Event()
    finished = threading.Event()

    async def service() -> None:
        started.set()
        try:
            await asyncio.Event().wait()
        finally:
            finished.set()

    runner = AsyncRunner()
    runner.start(service)
    assert started.wait(timeout=2)

    runner.stop()

    assert finished.is_set()
    assert not runner._thread.is_alive()
