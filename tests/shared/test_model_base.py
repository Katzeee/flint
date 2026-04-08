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
