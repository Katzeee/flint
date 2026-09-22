import threading
from io import StringIO


class ThreadSafeTextBuffer:
    """Thread-safe text stream for concurrent write + read.

    Compatible with redirect_stdout/redirect_stderr.
    """

    def __init__(self) -> None:
        self._buf = StringIO()
        self._lock = threading.Lock()

    def write(self, s: str) -> int:
        with self._lock:
            return self._buf.write(s)

    def getvalue(self) -> str:
        with self._lock:
            return self._buf.getvalue()

    def flush(self) -> None:
        pass

    def isatty(self) -> bool:
        return False
