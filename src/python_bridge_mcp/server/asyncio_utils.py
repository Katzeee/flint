import asyncio
from typing import Optional


def request_server_close(
    loop: Optional[asyncio.AbstractEventLoop],
    server: Optional[asyncio.AbstractServer],
) -> None:
    """Schedule an asyncio server close on the event loop that owns it."""
    if loop is None or server is None or loop.is_closed():
        return
    try:
        if asyncio.get_running_loop() is loop:
            server.close()
            return
    except RuntimeError:
        pass
    try:
        loop.call_soon_threadsafe(server.close)
    except RuntimeError:
        # The loop can close between is_closed() and call_soon_threadsafe().
        pass
