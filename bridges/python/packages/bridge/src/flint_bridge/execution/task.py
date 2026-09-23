import queue
import threading
import traceback

from .output import OUTPUT_CHUNK_SIZE, QueuedTextStream

MAX_EVENTS_PER_DRAIN = 1024


class ExecutionTask:
    """Own one request's worker and ordered output/result queue.

    The worker invokes the host strategy. The Bridge thread drains this queue
    and remains the sole submitter of execution events to the native core.
    """

    def __init__(self, runner, event):
        self._request_id = event["request_id"]
        self._events = queue.Queue()
        self._thread = threading.Thread(
            target=self._run, args=(runner, event), name="flint-execution", daemon=True)

    def start(self):
        self._thread.start()

    def join(self, timeout):
        self._thread.join(timeout)

    def drain(self, submit):
        """Submit queued output before its terminal result; return when complete."""
        stdout, stderr = [], []
        size = 0

        def flush():
            if stdout or stderr:
                submit({
                    "kind": "output", "request_id": self._request_id,
                    "stdout": "".join(stdout), "stderr": "".join(stderr),
                })
                stdout.clear()
                stderr.clear()

        for _ in range(MAX_EVENTS_PER_DRAIN):
            try:
                channel, value = self._events.get_nowait()
            except queue.Empty:
                flush()
                return False
            if channel == "result":
                flush()
                submit(value)
                return True
            if channel == "stdout":
                stdout.append(value)
            else:
                stderr.append(value)
            size += len(value)
            if size >= OUTPUT_CHUNK_SIZE:
                flush()
                size = 0
        flush()
        return False

    def _run(self, runner, event):
        out = QueuedTextStream(self._events, "stdout")
        err = QueuedTextStream(self._events, "stderr")
        try:
            result = runner.execute(event["execution_id"], event["code"], out, err, event.get("filename"))
            command = {
                "kind": "result", "request_id": self._request_id,
                "succeeded": result.status.value == "succeeded",
                "traceback": result.traceback, "error": result.error,
            }
        except BaseException:
            command = {
                "kind": "result", "request_id": self._request_id,
                "succeeded": False, "traceback": traceback.format_exc(), "error": None,
            }
        self._events.put(("result", command))
