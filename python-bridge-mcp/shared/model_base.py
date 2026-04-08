from __future__ import annotations

from dataclasses import asdict, dataclass
from typing import Any, ClassVar, TypeVar

_T = TypeVar("_T", bound="BaseModel")

import dacite


class WireModelError(Exception):
    pass


@dataclass
class BaseModel:
    def to_dict(self, *, exclude_none: bool = False) -> dict[str, Any]:
        d = asdict(self)
        if exclude_none:
            d = {k: v for k, v in d.items() if v is not None}  # top-level only
        return d

    @classmethod
    def from_dict(cls: type[_T], data: dict[str, Any]) -> _T:
        return dacite.from_dict(cls, data)


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


def wire_model(cls: type) -> type:
    WireModel._registry[cls.__name__] = cls
    return cls
