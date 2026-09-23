import threading


OUTPUT_CHUNK_SIZE = 65536


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


class QueuedTextStream:
    """Queue writes from the host execution thread for the Bridge thread."""

    def __init__(self, events, channel):
        self._events = events
        self._channel = channel

    def write(self, text):
        if not isinstance(text, str):
            raise TypeError("write() argument must be str")
        for start in range(0, len(text), OUTPUT_CHUNK_SIZE):
            self._events.put((self._channel, text[start:start + OUTPUT_CHUNK_SIZE]))
        return len(text)

    def flush(self):
        pass

    def isatty(self):
        return False
