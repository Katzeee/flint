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
