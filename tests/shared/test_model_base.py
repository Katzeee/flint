from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

import pytest

from python_bridge_mcp.shared.model_base import BaseModel, WireModel, VersionedWireModel, WireModelError, wire_model


@pytest.fixture(autouse=True)
def _isolated_registry():
    snapshot = dict(WireModel._registry)
    yield
    WireModel._registry.clear()
    WireModel._registry.update(snapshot)


# ---------------------------------------------------------------------------
# BaseModel
# ---------------------------------------------------------------------------

def test_base_model_to_dict_basic():
    @dataclass
    class Point(BaseModel):
        x: int
        y: int

    assert Point(x=1, y=2).to_dict() == {"x": 1, "y": 2}


def test_base_model_to_dict_exclude_none_false():
    @dataclass
    class Item(BaseModel):
        name: str
        value: Optional[int] = None

    assert Item(name="foo").to_dict(exclude_none=False) == {"name": "foo", "value": None}


def test_base_model_to_dict_exclude_none_true():
    @dataclass
    class Item(BaseModel):
        name: str
        value: Optional[int] = None

    assert Item(name="foo").to_dict(exclude_none=True) == {"name": "foo"}


def test_base_model_from_dict():
    @dataclass
    class Point(BaseModel):
        x: int
        y: int

    assert Point.from_dict({"x": 3, "y": 4}) == Point(x=3, y=4)


def test_base_model_roundtrip():
    @dataclass
    class Point(BaseModel):
        x: int
        y: int

    original = Point(x=5, y=6)
    assert Point.from_dict(original.to_dict()) == original


# ---------------------------------------------------------------------------
# WireModel + @wire_model
# ---------------------------------------------------------------------------

def test_wire_model_decorator_registers_by_class_name():
    @wire_model
    @dataclass
    class Ping(WireModel):
        pass

    assert "Ping" in WireModel._registry
    assert WireModel._registry["Ping"] is Ping


def test_wire_model_to_dict_injects_type():
    @wire_model
    @dataclass
    class Pong(WireModel):
        msg: str

    assert Pong(msg="hi").to_dict() == {"type": "Pong", "msg": "hi"}


def test_wire_model_to_dict_exclude_none_with_type():
    @wire_model
    @dataclass
    class Greet(WireModel):
        name: str
        extra: Optional[str] = None

    assert Greet(name="x").to_dict(exclude_none=True) == {"type": "Greet", "name": "x"}


def test_wire_model_parse_dispatches_to_correct_class():
    @wire_model
    @dataclass
    class Echo(WireModel):
        text: str

    result = WireModel.parse({"type": "Echo", "text": "hello"})
    assert result == Echo(text="hello")


def test_wire_model_roundtrip():
    @wire_model
    @dataclass
    class Reply(WireModel):
        content: str

    original = Reply(content="world")
    assert WireModel.parse(original.to_dict()) == original


def test_wire_model_parse_missing_type_raises():
    with pytest.raises(WireModelError, match="Missing 'type' field"):
        WireModel.parse({"text": "hello"})


def test_wire_model_parse_unknown_type_raises():
    with pytest.raises(WireModelError, match="Unknown type: 'Nonexistent'"):
        WireModel.parse({"type": "Nonexistent"})


# ---------------------------------------------------------------------------
# VersionedWireModel
# ---------------------------------------------------------------------------

def test_versioned_wire_model_default_protocol_version():
    @wire_model
    @dataclass
    class MsgA(VersionedWireModel):
        x: int

    assert MsgA.PROTOCOL_VERSION == 1


def test_versioned_wire_model_override_protocol_version():
    @wire_model
    @dataclass
    class MsgB(VersionedWireModel):
        PROTOCOL_VERSION = 3
        x: int

    assert MsgB.PROTOCOL_VERSION == 3


def test_versioned_wire_model_to_dict_injects_type_and_version():
    @wire_model
    @dataclass
    class MsgC(VersionedWireModel):
        PROTOCOL_VERSION = 2
        val: str

    assert MsgC(val="x").to_dict() == {"type": "MsgC", "version": 2, "val": "x"}


def test_versioned_wire_model_parse_versioned_dispatches():
    @wire_model
    @dataclass
    class MsgD(VersionedWireModel):
        PROTOCOL_VERSION = 2
        val: str

    result = VersionedWireModel.parse_versioned({"type": "MsgD", "version": 2, "val": "x"})
    assert result == MsgD(val="x")


def test_versioned_wire_model_roundtrip():
    @wire_model
    @dataclass
    class MsgE(VersionedWireModel):
        PROTOCOL_VERSION = 2
        val: str

    original = MsgE(val="test")
    assert VersionedWireModel.parse_versioned(original.to_dict()) == original


def test_versioned_wire_model_version_mismatch_raises():
    @wire_model
    @dataclass
    class MsgF(VersionedWireModel):
        PROTOCOL_VERSION = 2
        val: str

    with pytest.raises(WireModelError, match="Version mismatch: expected 2, got 1"):
        VersionedWireModel.parse_versioned({"type": "MsgF", "version": 1, "val": "x"})


def test_versioned_wire_model_missing_version_raises():
    @wire_model
    @dataclass
    class MsgG(VersionedWireModel):
        val: str

    with pytest.raises(WireModelError, match="Missing 'version' field"):
        VersionedWireModel.parse_versioned({"type": "MsgG", "val": "x"})


def test_versioned_wire_model_missing_type_raises():
    with pytest.raises(WireModelError, match="Missing 'type' field"):
        VersionedWireModel.parse_versioned({"version": 1, "val": "x"})


def test_versioned_wire_model_unknown_type_raises():
    with pytest.raises(WireModelError, match="Unknown type: 'DoesNotExist'"):
        VersionedWireModel.parse_versioned({"type": "DoesNotExist", "version": 1})


# ---------------------------------------------------------------------------
# Protocol validation (Section B)
# ---------------------------------------------------------------------------

def test_parse_versioned_rejects_far_future_version():
    @wire_model
    @dataclass
    class MsgV1(VersionedWireModel):
        PROTOCOL_VERSION = 1
        val: str

    with pytest.raises(WireModelError, match="9999"):
        VersionedWireModel.parse_versioned({"type": "MsgV1", "version": 9999, "val": "x"})


def test_cross_protocol_version_rejected():
    """exec (v2) and discovery (v1) messages must not be interchangeable."""
    from python_bridge_mcp.shared.exec_models import ExecRequest
    from python_bridge_mcp.shared.discovery_models import RegisterDiscovery

    with pytest.raises(WireModelError, match="Version mismatch"):
        VersionedWireModel.parse_versioned({
            "type": "ExecRequest", "version": 1,
            "execution_id": "001", "code": "x", "workflow_id": "wf",
        })

    with pytest.raises(WireModelError, match="Version mismatch"):
        VersionedWireModel.parse_versioned({
            "type": "RegisterDiscovery", "version": 2,
            "pid": 1, "instance_id": "c1", "instance_name": "t",
        })
