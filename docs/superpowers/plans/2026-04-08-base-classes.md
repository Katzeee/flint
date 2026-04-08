# Base Model Classes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `BaseModel`, `WireModel`, `VersionedWireModel`, and `@wire_model` decorator in a single module providing symmetric serde-style serialization.

**Architecture:** Three `@dataclass` base classes with progressive layering — `BaseModel` handles raw dict conversion via dacite/asdict, `WireModel` adds a class-name-keyed registry and type-discriminated dispatch, `VersionedWireModel` adds per-class protocol version validation. A `@wire_model` decorator opts a subclass into the registry. Serialization and deserialization are symmetric: `to_dict()` injects `"type"` / `"version"`, and `parse()` / `parse_versioned()` strip them before passing to dacite.

**Tech Stack:** Python 3.10+, `dataclasses` (stdlib), `dacite>=1.8`, `pytest>=8`

---

## File Structure

| Action | Path | Responsibility |
|---|---|---|
| Modify | `pyproject.toml` | add dacite runtime dep, pytest dev dep, pytest pythonpath config |
| Create | `python-bridge-mcp/shared/__init__.py` | make `shared` importable as a package |
| Modify | `python-bridge-mcp/shared/model_base.py` | all classes: `WireModelError`, `BaseModel`, `WireModel`, `VersionedWireModel`, `wire_model` |
| Create | `tests/__init__.py` | make `tests` a package |
| Create | `tests/shared/__init__.py` | make `tests/shared` a package |
| Create | `tests/shared/test_model_base.py` | all tests |

---

## Task 1: Setup — dependencies, pytest config, package init files

**Files:**
- Modify: `pyproject.toml`
- Create: `python-bridge-mcp/shared/__init__.py`
- Create: `tests/__init__.py`
- Create: `tests/shared/__init__.py`

- [ ] **Step 1: Update pyproject.toml**

Replace the file content with:

```toml
[project]
name = "python-bridge-mcp"
version = "0.0.1"
requires-python = ">=3.10"
dependencies = ["mcp>=1.12.4", "dacite>=1.8"]

[project.optional-dependencies]
build = ["pyinstaller>=6"]
dev = ["pytest>=8"]

[tool.pytest.ini_options]
pythonpath = ["python-bridge-mcp"]
```

- [ ] **Step 2: Create empty `__init__.py` files**

Create `python-bridge-mcp/shared/__init__.py` (empty file).  
Create `tests/__init__.py` (empty file).  
Create `tests/shared/__init__.py` (empty file).

- [ ] **Step 3: Install dependencies**

```bash
pip install dacite pytest
```

Expected: installs without errors.

- [ ] **Step 4: Verify pytest can be found**

```bash
pytest --collect-only
```

Expected: `no tests ran` or `0 items` — no errors.

- [ ] **Step 5: Commit**

```bash
git add pyproject.toml python-bridge-mcp/shared/__init__.py tests/__init__.py tests/shared/__init__.py
git commit -m "chore: add dacite/pytest deps and package init files"
```

---

## Task 2: BaseModel — dict conversion with dacite

**Files:**
- Modify: `python-bridge-mcp/shared/model_base.py`
- Create: `tests/shared/test_model_base.py`

- [ ] **Step 1: Write failing tests for BaseModel**

Create `tests/shared/test_model_base.py`:

```python
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
```

- [ ] **Step 2: Run tests — verify they fail**

```bash
pytest tests/shared/test_model_base.py -v
```

Expected: FAIL — `ImportError` or `cannot import name 'BaseModel'`.

- [ ] **Step 3: Implement BaseModel in model_base.py**

Replace `python-bridge-mcp/shared/model_base.py` with:

```python
from __future__ import annotations

from dataclasses import asdict, dataclass
from typing import Any, ClassVar

import dacite


class WireModelError(Exception):
    pass


@dataclass
class BaseModel:
    def to_dict(self, *, exclude_none: bool = False) -> dict[str, Any]:
        d = asdict(self)
        if exclude_none:
            d = {k: v for k, v in d.items() if v is not None}
        return d

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> BaseModel:
        return dacite.from_dict(cls, data)


@dataclass
class WireModel(BaseModel):
    pass


@dataclass
class VersionedWireModel(WireModel):
    pass


def wire_model(cls: type) -> type:
    return cls
```

(Stubbed `WireModel`, `VersionedWireModel`, `wire_model` so imports don't fail — filled in Task 3 and 4.)

- [ ] **Step 4: Run BaseModel tests — verify they pass**

```bash
pytest tests/shared/test_model_base.py::test_base_model_to_dict_basic tests/shared/test_model_base.py::test_base_model_to_dict_exclude_none_false tests/shared/test_model_base.py::test_base_model_to_dict_exclude_none_true tests/shared/test_model_base.py::test_base_model_from_dict tests/shared/test_model_base.py::test_base_model_roundtrip -v
```

Expected: all 5 PASS.

- [ ] **Step 5: Commit**

```bash
git add python-bridge-mcp/shared/model_base.py tests/shared/test_model_base.py
git commit -m "feat: implement BaseModel with to_dict/from_dict"
```

---

## Task 3: WireModel + @wire_model decorator

**Files:**
- Modify: `python-bridge-mcp/shared/model_base.py`
- Modify: `tests/shared/test_model_base.py`

- [ ] **Step 1: Append WireModel tests to test_model_base.py**

Add to the end of `tests/shared/test_model_base.py`:

```python
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
```

- [ ] **Step 2: Run new WireModel tests — verify they fail**

```bash
pytest tests/shared/test_model_base.py -k "wire_model" -v
```

Expected: FAIL — stubs don't implement registry or parse.

- [ ] **Step 3: Implement WireModel and @wire_model in model_base.py**

Replace the stubbed `WireModel` and `wire_model` in `python-bridge-mcp/shared/model_base.py`:

```python
@dataclass
class WireModel(BaseModel):
    _registry: ClassVar[dict[str, type]] = {}

    def to_dict(self, *, exclude_none: bool = False) -> dict[str, Any]:
        d = super().to_dict(exclude_none=exclude_none)
        d["type"] = type(self).__name__
        return d

    @classmethod
    def parse(cls, data: dict[str, Any]) -> WireModel:
        if "type" not in data:
            raise WireModelError("Missing 'type' field")
        type_name = data["type"]
        if type_name not in cls._registry:
            raise WireModelError(f"Unknown type: '{type_name}'")
        target_cls = cls._registry[type_name]
        stripped = {k: v for k, v in data.items() if k != "type"}
        return target_cls.from_dict(stripped)


def wire_model(cls: type) -> type:
    WireModel._registry[cls.__name__] = cls
    return cls
```

- [ ] **Step 4: Run all tests so far — verify they all pass**

```bash
pytest tests/shared/test_model_base.py -v
```

Expected: all tests PASS.

- [ ] **Step 5: Commit**

```bash
git add python-bridge-mcp/shared/model_base.py tests/shared/test_model_base.py
git commit -m "feat: implement WireModel and @wire_model decorator"
```

---

## Task 4: VersionedWireModel

**Files:**
- Modify: `python-bridge-mcp/shared/model_base.py`
- Modify: `tests/shared/test_model_base.py`

- [ ] **Step 1: Append VersionedWireModel tests to test_model_base.py**

Add to the end of `tests/shared/test_model_base.py`:

```python
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
```

- [ ] **Step 2: Run new VersionedWireModel tests — verify they fail**

```bash
pytest tests/shared/test_model_base.py -k "versioned" -v
```

Expected: FAIL — stub `VersionedWireModel` has no `PROTOCOL_VERSION`, `parse_versioned`, or `to_dict`.

- [ ] **Step 3: Implement VersionedWireModel in model_base.py**

Replace the stubbed `VersionedWireModel` in `python-bridge-mcp/shared/model_base.py`:

```python
@dataclass
class VersionedWireModel(WireModel):
    PROTOCOL_VERSION: ClassVar[int] = 1

    def to_dict(self, *, exclude_none: bool = False) -> dict[str, Any]:
        d = super().to_dict(exclude_none=exclude_none)
        d["version"] = type(self).PROTOCOL_VERSION
        return d

    @classmethod
    def parse_versioned(cls, data: dict[str, Any]) -> VersionedWireModel:
        if "type" not in data:
            raise WireModelError("Missing 'type' field")
        if "version" not in data:
            raise WireModelError("Missing 'version' field")
        type_name = data["type"]
        if type_name not in cls._registry:
            raise WireModelError(f"Unknown type: '{type_name}'")
        target_cls = cls._registry[type_name]
        actual = data["version"]
        expected = target_cls.PROTOCOL_VERSION
        if actual != expected:
            raise WireModelError(
                f"Version mismatch: expected {expected}, got {actual}"
            )
        stripped = {k: v for k, v in data.items() if k not in ("type", "version")}
        return target_cls.from_dict(stripped)
```

- [ ] **Step 4: Run all tests — verify they all pass**

```bash
pytest tests/shared/test_model_base.py -v
```

Expected: all tests PASS.

- [ ] **Step 5: Commit**

```bash
git add python-bridge-mcp/shared/model_base.py tests/shared/test_model_base.py
git commit -m "feat: implement VersionedWireModel with parse_versioned"
```
