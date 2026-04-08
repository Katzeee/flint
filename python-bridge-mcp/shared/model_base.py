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
            d = {k: v for k, v in d.items() if v is not None}  # top-level only
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
