"""What a host supplies so the core can execute its code."""
from dataclasses import dataclass


@dataclass(frozen=True)
class ExecutionCapabilities:
    """``executor.run(prepared_result, out, err)`` executes code; an optional
    ``executor.prepare(request)`` turns the request into the prepared result,
    which defaults to the request itself. Either may return a
    ``concurrent.futures.Future`` to complete later. ``scheduler.post(callback)``
    runs callbacks on the host's execution thread, and ``scheduler.close()``
    ends it after the Bridge is released."""

    executor: object
    scheduler: object
