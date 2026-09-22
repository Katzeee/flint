import asyncio
import logging
import os
import platform
import threading
from uuid import uuid4

from flint_protocol.v1 import host_pb2 as host_messages, common_pb2 as common
from ..execution.buffer import ThreadSafeTextBuffer
from .wire import message, receive, send

log = logging.getLogger(__name__)


class Bridge:
    def __init__(self, runner, host, address, port, name, heartbeat_interval=5.0):
        self.runner, self.host, self.address, self.port, self.name = runner, host, address, port, name
        self.heartbeat_interval = heartbeat_interval
        self.bridge_id = uuid4().hex
        self.instance_id = None
        self._stop = threading.Event()
        self._online = threading.Event()
        self._busy = threading.Lock()
        self._loop = None
        self._writers = []
        self._cancel = None
        self.thread = threading.Thread(target=self._run, name="flint-bridge", daemon=True)

    def start(self):
        self.thread.start()
        return self

    def wait_until_connected(self, timeout=10):
        return self._online.wait(timeout)

    @property
    def connected(self):
        return self._online.is_set()

    @property
    def busy(self):
        return self._busy.locked()

    def stop(self, timeout=5):
        self._stop.set()
        self._online.clear()
        self.runner.close()
        loop, cancel = self._loop, self._cancel
        if loop is not None:
            try:
                if cancel is not None:
                    loop.call_soon_threadsafe(cancel.set)
                for writer in list(self._writers):
                    loop.call_soon_threadsafe(writer.close)
            except RuntimeError:
                pass
        if threading.current_thread() is not self.thread:
            self.thread.join(timeout)
        return not self.thread.is_alive() and not self.busy

    def _run(self):
        delay = 0
        while not self._stop.is_set():
            loop = asyncio.new_event_loop()
            self._loop = loop
            try:
                loop.run_until_complete(self._session())
                delay = 0
            except Exception as exc:
                if not self._stop.is_set():
                    delay = min(delay * 2 + 1, 10)
                    log.debug("Bridge disconnected: %s", exc)
            finally:
                self._online.clear()
                self._loop = self._cancel = None
                self._writers = []
                loop.close()
            self._stop.wait(delay)

    async def _open(self):
        reader, writer = await asyncio.wait_for(asyncio.open_connection(self.address, self.port), 10)
        self._writers.append(writer)
        return reader, writer

    async def _ack(self, reader, request):
        response = await asyncio.wait_for(receive(reader), 10)
        if response.request_id != request.request_id or response.WhichOneof("payload") != "instance_ack" or not response.instance_ack.success:
            raise RuntimeError("Bridge handshake rejected")
        return response.instance_ack

    async def _session(self):
        tasks = set()
        executions = set()
        self._cancel = asyncio.Event()
        try:
            control_reader, control_writer = await self._open()
            registration = message("register_instance", host_messages.RegisterInstance(
                pid=os.getpid(), name_hint=self.host, instance_name=self.name, instance_type=self.host,
                bridge_id=self.bridge_id, runtime_version="CPython " + platform.python_version(), bridge_version="0.1.0"))
            await send(control_writer, registration)
            ack = await self._ack(control_reader, registration)
            self.instance_id = ack.instance_id
            execution_reader, execution_writer = await self._open()
            registration = message("register_execution_channel", host_messages.RegisterExecutionChannel(
                instance_id=self.instance_id, pid=os.getpid(), session_token=ack.session_token))
            await send(execution_writer, registration)
            await self._ack(execution_reader, registration)
            write_lock = asyncio.Lock()
            self._online.set()
            tasks.add(asyncio.create_task(self._heartbeat(control_reader, control_writer)))
            tasks.add(asyncio.create_task(self._read_executions(execution_reader, execution_writer, write_lock, executions)))
            tasks.add(asyncio.create_task(self._cancel.wait()))
            done, _ = await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
            for task in done:
                task.result()
        finally:
            self._online.clear()
            for task in tasks | executions:
                task.cancel()
            await asyncio.gather(*(tasks | executions), return_exceptions=True)
            for writer in self._writers:
                writer.close()
                try:
                    await writer.wait_closed()
                except Exception:
                    pass

    async def _heartbeat(self, reader, writer):
        while True:
            request = message("heartbeat", host_messages.Heartbeat(instance_id=self.instance_id))
            await send(writer, request)
            await self._ack(reader, request)
            await asyncio.sleep(self.heartbeat_interval)

    async def _read_executions(self, reader, writer, write_lock, executions):
        while True:
            request = await receive(reader)
            if request.WhichOneof("payload") != "host_execute_request":
                raise ValueError("Unexpected execution message")
            task = asyncio.create_task(self._execute(request, writer, write_lock))
            executions.add(task)
            task.add_done_callback(executions.discard)
            task.add_done_callback(self._consume)

    @staticmethod
    def _consume(task):
        if not task.cancelled():
            task.exception()

    async def _execute(self, envelope, writer, write_lock):
        request = envelope.host_execute_request
        async def reply(kind, payload):
            async with write_lock:
                await send(writer, message(kind, payload, envelope.request_id))
        if not self._busy.acquire(False):
            await reply("execution_result", common.ExecutionResult(execution_id=request.execution_id,
                status=common.EXECUTION_STATUS_FAILED, error="instance_busy"))
            return
        out, err = ThreadSafeTextBuffer(), ThreadSafeTextBuffer()
        def execute():
            try:
                return self.runner.execute(request.execution_id, request.code, out, err,
                    request.filename if request.HasField("filename") else None)
            finally:
                self._busy.release()
        # The lock is released by the actual worker, not by a cancelled socket task.
        future = asyncio.get_running_loop().run_in_executor(None, execute)
        positions = [0, 0]
        sequence = [0]
        async def flush():
            values = [out.getvalue(), err.getvalue()]
            while any(len(v) > p for v, p in zip(values, positions)):
                chunks = [v[p:p + 65536] for v, p in zip(values, positions)]
                sequence[0] += 1
                await reply("execution_output_update", host_messages.ExecutionOutputUpdate(
                    workflow_id=request.workflow_id, execution_id=request.execution_id, sequence=sequence[0],
                    stdout_delta=chunks[0], stderr_delta=chunks[1]))
                positions[:] = [p + len(c) for p, c in zip(positions, chunks)]
        while not future.done():
            done, _ = await asyncio.wait({future}, timeout=2)
            await flush()
            if done:
                break
        result = future.result()
        await flush()
        response = common.ExecutionResult(execution_id=request.execution_id,
            status=common.EXECUTION_STATUS_SUCCEEDED if result.status.value == "succeeded" else common.EXECUTION_STATUS_FAILED)
        if result.traceback is not None:
            response.traceback = result.traceback
        if result.error is not None:
            response.error = result.error
        await reply("execution_result", response)
