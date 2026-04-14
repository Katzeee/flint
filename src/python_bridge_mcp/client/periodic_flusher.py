import threading

from ..shared.text_buffer import ThreadSafeTextBuffer
from ..shared.workflow_persistence import WorkflowPersistence


class PeriodicFlusher:
    """Daemon thread that flushes stdout/stderr to the workflow file periodically."""

    def __init__(
        self,
        interval: float,
        workflow_id: str,
        execution_id: str,
        out: ThreadSafeTextBuffer,
        err: ThreadSafeTextBuffer,
    ) -> None:
        self._interval = interval
        self._workflow_id = workflow_id
        self._execution_id = execution_id
        self._out = out
        self._err = err
        self._stop_event = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)

    def start(self) -> None:
        self._thread.start()

    def stop(self) -> None:
        self._stop_event.set()
        self._thread.join()

    def _run(self) -> None:
        while not self._stop_event.wait(self._interval):
            WorkflowPersistence.update_execution_output(
                self._workflow_id,
                self._execution_id,
                self._out.getvalue(),
                self._err.getvalue(),
            )
