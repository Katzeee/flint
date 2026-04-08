# Base Model Classes Design

**Date:** 2026-04-08  
**Topic:** `BaseModel`, `WireModel`, `VersionedWireModel` — serde-style serialization hierarchy

## Overview

Three dataclass base classes providing symmetric serialization/deserialization, wire-format dispatch by class name, and protocol version validation. Analogous to Rust's serde with `#[derive(Serialize, Deserialize)]` and `#[serde(tag = "type")]`.

## Architecture

All code lives in `python-bridge-mcp/shared/model_base.py`. One decorator, one exception type, three classes.

**Dependency:** `dacite` added to `pyproject.toml` for `from_dict`.

## Components

### `WireModelError(Exception)`

Raised for all dispatch and version errors. dacite errors propagate naturally without wrapping.

### `BaseModel`

A `@dataclass` base providing symmetric dict conversion:

- `to_dict(*, exclude_none=False) -> dict` — serializes via `asdict()`; when `exclude_none=True`, strips `None` values
- `from_dict(cls, data: dict) -> Self` — deserializes via `dacite.from_dict(cls, data)`

### `WireModel(BaseModel)`

Adds type-discriminated dispatch. Holds a class-level `_registry: dict[str, type]` shared across all subclasses.

- `to_dict(...)` — calls super, injects `"type": cls.__name__`
- `parse(cls, data: dict) -> Self` — reads `data["type"]`, looks up registry, calls `from_dict` on the resolved class (stripping `"type"` before passing to dacite)
- Raises `WireModelError` if `"type"` is missing or not in registry

### `VersionedWireModel(WireModel)`

Adds protocol version validation.

- `PROTOCOL_VERSION: int = 1` — class-level constant, overridable per subclass
- `to_dict(...)` — calls super, injects `"version": PROTOCOL_VERSION`
- `parse_versioned(cls, data: dict) -> Self` — validates `data["version"] == PROTOCOL_VERSION`, then delegates to `parse` (stripping `"version"` before passing to dacite)
- Raises `WireModelError` if `"version"` is missing or mismatched

### `@wire_model` decorator

Registers a class in `WireModel._registry` using `cls.__name__` as the key. Must be the outermost decorator (applied after `@dataclass`).

```python
@wire_model
@dataclass
class Handshake(VersionedWireModel):
    PROTOCOL_VERSION = 2
    session_id: str
```

## Data Flow

**Serialization:**

```
Handshake(session_id="abc").to_dict()
  → BaseModel.to_dict()  →  {"session_id": "abc"}
  → WireModel.to_dict()  →  {"type": "Handshake", "session_id": "abc"}
  → VersionedWireModel.to_dict()  →  {"type": "Handshake", "version": 2, "session_id": "abc"}
```

**Deserialization:**

```
VersionedWireModel.parse_versioned({"type": "Handshake", "version": 2, "session_id": "abc"})
  → validate version == 2  ✓
  → strip "version", call parse({"type": "Handshake", "session_id": "abc"})
  → lookup registry["Handshake"] → Handshake
  → strip "type", call Handshake.from_dict({"session_id": "abc"})
  → Handshake(session_id="abc")
```

Serialized and deserialized forms are identical — round-trip safe.

## Error Handling

| Situation | Error |
|---|---|
| `"type"` key missing | `WireModelError("Missing 'type' field")` |
| type not in registry | `WireModelError(f"Unknown type: '{name}'")`|
| `"version"` key missing | `WireModelError("Missing 'version' field")` |
| version mismatch | `WireModelError(f"Version mismatch: expected {PROTOCOL_VERSION}, got {actual}")` |
| dacite field errors | propagate as-is |

## Testing (future)

- `BaseModel`: to_dict/from_dict roundtrip, exclude_none
- `WireModel`: decorator registers by class name, parse dispatches correctly, errors on unknown/missing type, to_dict injects `"type"`
- `VersionedWireModel`: parse_versioned passes on correct version, errors on mismatch/missing, to_dict injects both fields, subclass version override works
