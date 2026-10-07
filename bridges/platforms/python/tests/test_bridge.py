import pytest
from flint_bridge.connection import bridge as bridge_module
from flint_bridge.execution.capabilities import ExecutionCapabilities


class Scheduler:
    closed = False

    def close(self):
        self.closed = True


def core(stopped):
    class Core:
        destroyed = False

        def __init__(self, config, capabilities):
            pass

        def stop(self):
            return stopped

        def destroy(self):
            Core.destroyed = True

    return Core


def test_failed_creation_closes_the_scheduler(monkeypatch):
    class Failing:
        def __init__(self, config, capabilities):
            raise RuntimeError("creation failed")

    monkeypatch.setattr(bridge_module, "NativeCore", Failing)
    scheduler = Scheduler()
    with pytest.raises(RuntimeError, match="creation failed"):
        bridge_module.Bridge(ExecutionCapabilities(object(), scheduler), "test", "127.0.0.1", 6321, "test")
    assert scheduler.closed


@pytest.mark.parametrize("idle", [False, True])
def test_resources_are_released_only_after_host_code_finishes(monkeypatch, idle):
    Core = core(idle)
    monkeypatch.setattr(bridge_module, "NativeCore", Core)
    scheduler = Scheduler()
    bridge = bridge_module.Bridge(ExecutionCapabilities(object(), scheduler), "test", "127.0.0.1", 6321, "test")
    assert bridge.stop() is idle
    assert Core.destroyed is idle and scheduler.closed is idle
