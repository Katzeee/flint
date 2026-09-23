import platform
import threading
import time
import traceback

from ..execution.buffer import ThreadSafeTextBuffer
from .native import NativeCore


class Bridge:
    def __init__(self, runner, host, address, port, name):
        self.runner, self.host, self.address, self.port, self.name = runner, host, address, port, name
        self._core = NativeCore({
            "host": host,
            "address": address,
            "port": port,
            "name": name,
            "runtime_version": "CPython " + platform.python_version(),
        })
        self._stop = threading.Event()
        self._workers = set()
        self._workers_lock = threading.Lock()
        self.thread = threading.Thread(target=self._run, name="flint-bridge", daemon=True)

    def start(self):
        self.thread.start()
        return self

    @property
    def instance_id(self):
        return self._core.instance_id

    @property
    def connected(self):
        return self._core.connected

    @property
    def busy(self):
        return self._core.busy

    def wait_until_connected(self, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.connected:
                return True
            time.sleep(0.02)
        return self.connected

    def stop(self, timeout=5):
        self._stop.set()
        self.runner.close()
        self._core.stop()
        if threading.current_thread() is not self.thread:
            self.thread.join(timeout)
        with self._workers_lock:
            workers = list(self._workers)
        for worker in workers:
            if worker is not threading.current_thread():
                worker.join(max(0, timeout))
        if not workers and self.busy:
            pending = self._core.poll(0)
            if pending is not None:
                self._core.submit({
                    "kind": "result", "request_id": pending["request_id"],
                    "succeeded": False, "traceback": None,
                    "error": "Bridge stopped before host execution",
                })
        if self.thread.is_alive() or self.busy:
            return False
        self._core.close()
        return True

    def _force_reconnect(self):
        self._core.reconnect()

    def _run(self):
        while not self._stop.is_set():
            event = self._core.poll(200)
            if event is None:
                continue
            worker = threading.Thread(target=self._execute, args=(event,), daemon=True)
            with self._workers_lock:
                self._workers.add(worker)
            worker.start()

    def _execute(self, event):
        request_id = event["request_id"]
        out, err = ThreadSafeTextBuffer(), ThreadSafeTextBuffer()
        positions = [0, 0]
        outcome = {}

        def run():
            try:
                outcome["result"] = self.runner.execute(
                    event["execution_id"], event["code"], out, err, event.get("filename"))
            except BaseException:
                outcome["error"] = traceback.format_exc()

        def flush():
            values = [out.getvalue(), err.getvalue()]
            while any(len(value) > position for value, position in zip(values, positions)):
                chunks = [value[position:position + 65536] for value, position in zip(values, positions)]
                self._core.submit({
                    "kind": "output", "request_id": request_id,
                    "stdout": chunks[0], "stderr": chunks[1],
                })
                positions[:] = [position + len(chunk) for position, chunk in zip(positions, chunks)]

        try:
            execution = threading.Thread(target=run, name="flint-execution", daemon=True)
            execution.start()
            while execution.is_alive():
                execution.join(2)
                flush()
            flush()
            result = outcome.get("result")
            self._core.submit({
                "kind": "result", "request_id": request_id,
                "succeeded": result is not None and result.status.value == "succeeded",
                "traceback": result.traceback if result is not None else outcome.get("error"),
                "error": result.error if result is not None else None,
            })
        finally:
            with self._workers_lock:
                self._workers.discard(threading.current_thread())
