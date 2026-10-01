"""Host adapters select the owning application's execution thread."""


def strategy_for(host):
    if host == "maya":
        from .maya import create_strategy
    elif host == "max":
        from .max import create_strategy
    elif host == "blender":
        from .blender import create_strategy
    elif host == "python":
        from ..execution.strategies import DirectExecutionStrategy
        return DirectExecutionStrategy()
    else:
        raise ValueError("Unsupported host: " + host)
    return create_strategy()


def enter_main_thread(host, callback):
    """Schedule `callback` on `host`'s main thread; run it inline for plain Python."""
    if host == "maya":
        from .maya import enter_main_thread as entry
    elif host == "max":
        from .max import enter_main_thread as entry
    elif host == "blender":
        from .blender import enter_main_thread as entry
    elif host == "python":
        callback()
        return
    else:
        raise ValueError("Unsupported host: " + host)
    entry(callback)
