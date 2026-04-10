import asyncio
import uuid
from typing import Iterator

import pytest

from pbridge.client.code_executor import CodeExecutor
from pbridge.client.code_runner import DirectRunner
from pbridge.client.exec_listener import ExecListener
from pbridge.shared.discovery_models import RegisterDiscovery
from pbridge.shared.exec_models import ExecRequest, ExecResult, ExecStatus
from pbridge.shared.jsonline import AsyncJsonLineCodec
from pbridge.shared.model_base import VersionedWireModel

from conftest import AsyncRunner, free_port


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture
def port() -> int:
    return free_port()


@pytest.fixture
def listener_runner(port: int) -> Iterator[AsyncRunner]:
    executor = CodeExecutor()
    code_runner = DirectRunner(executor)
    listener = ExecListener("localhost", port, code_runner)
    lr = AsyncRunner()
    lr.start(listener.run)
    yield lr
    listener.stop()
    lr.stop()


async def _exec_call(host: str, port: int, code: str, connect_timeout: float = 10.0) -> ExecResult:
    request_id = str(uuid.uuid4())
    reader, writer = await asyncio.wait_for(
        asyncio.open_connection(host, port), timeout=connect_timeout,
    )
    try:
        await AsyncJsonLineCodec.send(writer, ExecRequest(request_id=request_id, code=code).to_dict())
        data = await AsyncJsonLineCodec.recv(reader)
        result = VersionedWireModel.parse_versioned(data)
        if not isinstance(result, ExecResult):
            raise RuntimeError(f"unexpected response: {type(result).__name__}")
        return result
    finally:
        writer.close()


def _call(port: int, code: str) -> ExecResult:
    return asyncio.run(_exec_call("localhost", port, code))


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_exec_hello_world(listener_runner: AsyncRunner, port: int) -> None:
    result = _call(port, 'print("hello")')
    assert result.status == ExecStatus.SUCCEED
    assert result.stdout == "hello\n"
    assert result.stderr == ""
    assert result.traceback is None


def test_exec_exception(listener_runner: AsyncRunner, port: int) -> None:
    result = _call(port, "raise ValueError('boom')")
    assert result.status == ExecStatus.FAILED
    assert result.traceback is not None
    assert "ValueError" in result.traceback
    assert "boom" in result.traceback


def test_exec_stderr(listener_runner: AsyncRunner, port: int) -> None:
    result = _call(port, 'import sys; sys.stderr.write("err\\n")')
    assert result.status == ExecStatus.SUCCEED
    assert "err" in result.stderr


def test_exec_namespace_persists(port: int) -> None:
    """Two sequential calls share the same executor namespace."""
    executor = CodeExecutor()
    code_runner = DirectRunner(executor)
    listener = ExecListener("localhost", port, code_runner)
    lr = AsyncRunner()
    lr.start(listener.run)
    try:
        r1 = _call(port, "x = 42")
        assert r1.status == ExecStatus.SUCCEED

        r2 = _call(port, "print(x)")
        assert r2.status == ExecStatus.SUCCEED
        assert r2.stdout == "42\n"
    finally:
        listener.stop()
        lr.stop()


def test_exec_wrong_message_type(listener_runner: AsyncRunner, port: int) -> None:
    """Sending a non-ExecRequest returns an error result."""
    async def _send_wrong():
        reader, writer = await asyncio.open_connection("localhost", port)
        try:
            msg = RegisterDiscovery(
                pid="1", instance_id="x", instance_name="x",
                exec_host="localhost", exec_port=0,
            )
            await AsyncJsonLineCodec.send(writer, msg.to_dict())
            data = await AsyncJsonLineCodec.recv(reader)
            return VersionedWireModel.parse_versioned(data)
        finally:
            writer.close()

    result = asyncio.run(_send_wrong())
    assert result.status == ExecStatus.FAILED
    assert result.error is not None
    assert "unexpected" in result.error.lower() or "RegisterDiscovery" in result.error


def test_exec_concurrent(listener_runner: AsyncRunner, port: int) -> None:
    """Two concurrent requests both return correct results."""
    async def _run():
        t1 = asyncio.create_task(_exec_call("localhost", port, 'print("a")'))
        t2 = asyncio.create_task(_exec_call("localhost", port, 'print("b")'))
        r1, r2 = await asyncio.gather(t1, t2)
        return r1, r2

    r1, r2 = asyncio.run(_run())
    assert r1.status == ExecStatus.SUCCEED and r2.status == ExecStatus.SUCCEED
    outputs = {r1.stdout.strip(), r2.stdout.strip()}
    assert outputs == {"a", "b"}
