import threading


class ThreadScopedTextProxy:
    """Capture the execution thread while preserving other threads' host output.

    redirect_stdout replaces sys.stdout process-wide. The proxy must therefore
    forward writes from unrelated host threads to the original stream.
    """

    def __init__(self, original, capture):
        self._original = original
        self._capture = capture
        self._owner = threading.get_ident()

    def _stream(self):
        return self._capture if threading.get_ident() == self._owner else self._original

    def write(self, text):
        return self._stream().write(text)

    def flush(self):
        return self._stream().flush()

    def isatty(self):
        return self._stream().isatty()
