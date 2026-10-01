from .qt import create_strategy as _qt_strategy
from .qt import enter_main_thread as enter_main_thread  # noqa: F401


def create_strategy():
    strategy = _qt_strategy()
    try:
        import pymxs
        pymxs.runtime.maxVersion()
        return strategy
    except BaseException:
        strategy.close()
        raise
