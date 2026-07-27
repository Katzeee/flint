import sys
import threading

import pytest

import python_bridge_mcp.client.bootstrap as bootstrap


@pytest.fixture(autouse=True)
def _clean_service_reference():
    previous = getattr(sys, bootstrap._SERVICE_ATTR, None)
    if previous is not None:
        delattr(sys, bootstrap._SERVICE_ATTR)
    yield
    current = getattr(sys, bootstrap._SERVICE_ATTR, None)
    if current is not None:
        current.client.stop()
        if current.thread.is_alive():
            current.thread.join(timeout=1)
        if getattr(sys, bootstrap._SERVICE_ATTR, None) is current:
            delattr(sys, bootstrap._SERVICE_ATTR)
    if previous is not None:
        setattr(sys, bootstrap._SERVICE_ATTR, previous)


class _Client:
    def __init__(self, stop_event=None):
        self.stop_event = stop_event
        self.stop_calls = 0

    def stop(self):
        self.stop_calls += 1
        if self.stop_event is not None:
            self.stop_event.set()


def test_failed_stop_retains_service_reference() -> None:
    release = threading.Event()
    thread = threading.Thread(target=release.wait, daemon=True)
    thread.start()
    service = bootstrap.ControlClientService(client=_Client(), thread=thread)
    setattr(sys, bootstrap._SERVICE_ATTR, service)

    try:
        assert service.stop(timeout=0.01) is False
        assert bootstrap.get_control_client_service() is service
        assert service.is_running()
    finally:
        release.set()
        thread.join(timeout=1)

    assert service.stop(timeout=0.1) is True
    assert bootstrap.get_control_client_service() is None


def test_start_refuses_second_listener_if_existing_thread_will_not_stop(
    monkeypatch,
) -> None:
    release = threading.Event()
    thread = threading.Thread(target=release.wait, daemon=True)
    thread.start()
    service = bootstrap.ControlClientService(client=_Client(), thread=thread)
    setattr(sys, bootstrap._SERVICE_ATTR, service)
    monkeypatch.setattr(bootstrap, "_STOP_TIMEOUT", 0.01)

    # ControlClientService.stop has its default bound at definition time, so
    # provide a fast failing stop for this lifecycle-level test.
    monkeypatch.setattr(service, "stop", lambda timeout=0.01: False)

    try:
        with pytest.raises(RuntimeError, match="refusing to start"):
            bootstrap.start_control_client_service(
                "new",
                "new",
                runner=object(),
            )
        assert bootstrap.get_control_client_service() is service
    finally:
        release.set()
        thread.join(timeout=1)


def test_reload_replacement_stops_old_thread_before_starting_new(
    monkeypatch,
) -> None:
    created = []

    class FakeDiscoveryClient:
        HEARTBEAT_INTERVAL = 5

        def __init__(self, **kwargs):
            self.stop_event = threading.Event()
            created.append(self)

        def run(self):
            self.stop_event.wait()

        def stop(self):
            self.stop_event.set()

    monkeypatch.setattr(bootstrap, "DiscoveryClient", FakeDiscoveryClient)

    first = bootstrap.start_control_client_service(
        "first",
        "first",
        runner=object(),
    )
    second = bootstrap.start_control_client_service(
        "second",
        "second",
        runner=object(),
    )

    assert not first.thread.is_alive()
    assert second.thread.is_alive()
    assert bootstrap.get_control_client_service() is second
    assert len(created) == 2
    assert bootstrap.stop_control_client_service() is True
    assert not second.thread.is_alive()
    assert bootstrap.get_control_client_service() is None


def test_lifecycle_lock_is_process_wide() -> None:
    assert bootstrap._get_lifecycle_lock() is bootstrap._get_lifecycle_lock()


def test_concurrent_starts_leave_exactly_one_listener(monkeypatch) -> None:
    class FakeDiscoveryClient:
        HEARTBEAT_INTERVAL = 5

        def __init__(self, **kwargs):
            self.stop_event = threading.Event()

        def run(self):
            self.stop_event.wait()

        def stop(self):
            self.stop_event.set()

    monkeypatch.setattr(bootstrap, "DiscoveryClient", FakeDiscoveryClient)
    services = []
    errors = []

    def start(name):
        try:
            services.append(
                bootstrap.start_control_client_service(
                    name,
                    name,
                    runner=object(),
                )
            )
        except Exception as exc:
            errors.append(exc)

    callers = [
        threading.Thread(target=start, args=("first",)),
        threading.Thread(target=start, args=("second",)),
    ]
    for caller in callers:
        caller.start()
    for caller in callers:
        caller.join(timeout=1)

    assert errors == []
    assert len(services) == 2
    assert sum(service.thread.is_alive() for service in services) == 1
    assert bootstrap.get_control_client_service().thread.is_alive()
    assert bootstrap.stop_control_client_service() is True
