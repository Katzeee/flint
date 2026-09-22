from .qt import create_strategy as _qt_strategy


def create_strategy():
    strategy = _qt_strategy()
    try:
        import maya.cmds
        maya.cmds.about(version=True)
        return strategy
    except BaseException:
        strategy.close()
        raise
