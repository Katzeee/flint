from dataclasses import asdict, dataclass
from enum import Enum
from typing import Any, ClassVar, Dict, Type, TypeVar, Callable, List, Tuple

import dacite

_T = TypeVar("_T", bound="BaseModel")
_W = TypeVar("_W", bound="WireModel")
_VW = TypeVar("_VW", bound="VersionedWireModel")


class WireModelError(Exception):
    pass


@dataclass
class BaseModel:
    @staticmethod
    def _dict_factory(exclude_none: bool = False) -> Callable[[List[Tuple[str, Any]]], Dict[str, Any]]:
        def factory(items: List[Tuple[str, Any]]) -> Dict[str, Any]:
            d = dict(items)
            if exclude_none:
                d = {k: v for k, v in d.items() if v is not None}
            return d

        return factory

    def to_dict(self, *, exclude_none: bool = False) -> Dict[str, Any]:
        return asdict(self, dict_factory=self._dict_factory(exclude_none=exclude_none))

    @classmethod
    def from_dict(cls: Type[_T], data: Dict[str, Any]) -> _T:
        return dacite.from_dict(cls, data, config=dacite.Config(cast=[Enum]))


@dataclass
class WireModel(BaseModel):
    _registry: ClassVar[Dict[str, Type["WireModel"]]] = {}

    def to_dict(self, *, exclude_none: bool = False) -> Dict[str, Any]:
        d = super().to_dict(exclude_none=exclude_none)
        d["type"] = type(self).__name__
        return d

    @classmethod
    def _resolve_type(cls, data: Dict[str, Any]) -> Type["WireModel"]:
        if "type" not in data:
            raise WireModelError("Missing 'type' field")
        type_name = data["type"]
        if type_name not in cls._registry:
            raise WireModelError(f"Unknown type: '{type_name}'")
        return cls._registry[type_name]

    @classmethod
    def parse(cls: Type[_W], data: Dict[str, Any]) -> _W:
        target_cls = cls._resolve_type(data)
        stripped = {k: v for k, v in data.items() if k != "type"}
        return target_cls.from_dict(stripped)  # type: ignore[return-value]


@dataclass
class VersionedWireModel(WireModel):
    PROTOCOL_VERSION: ClassVar[int] = 1

    def to_dict(self, *, exclude_none: bool = False) -> Dict[str, Any]:
        d = super().to_dict(exclude_none=exclude_none)
        d["version"] = type(self).PROTOCOL_VERSION
        return d

    @classmethod
    def parse_versioned(cls: Type[_VW], data: Dict[str, Any]) -> _VW:
        if "version" not in data:
            raise WireModelError("Missing 'version' field")
        target_cls = cls._resolve_type(data)
        assert issubclass(target_cls, VersionedWireModel)
        actual = data["version"]
        expected = target_cls.PROTOCOL_VERSION
        if actual != expected:
            raise WireModelError(f"Version mismatch: expected {expected}, got {actual}")
        return cls.parse({k: v for k, v in data.items() if k != "version"})  # type: ignore[return-value]


def wire_model(cls: Type[_W]) -> Type[_W]:
    WireModel._registry[cls.__name__] = cls
    return cls
