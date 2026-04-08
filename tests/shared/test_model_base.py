from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

import pytest

from shared.model_base import BaseModel, WireModel, VersionedWireModel, WireModelError, wire_model


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
