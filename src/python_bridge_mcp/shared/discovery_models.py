from __future__ import annotations

from dataclasses import dataclass
from typing import ClassVar, Optional

from .model_base import VersionedWireModel, wire_model


@dataclass
class DiscoveryWireModel(VersionedWireModel):
    PROTOCOL_VERSION: ClassVar[int] = 1


@wire_model
@dataclass
class RegisterDiscovery(DiscoveryWireModel):
    pid: int
    instance_id: str
    instance_name: str
    alias: Optional[str] = None
    instance_type: str = ""


@wire_model
@dataclass
class HeartbeatDiscovery(DiscoveryWireModel):
    instance_id: str


@wire_model
@dataclass
class AckDiscovery(DiscoveryWireModel):
    success: bool
    error: Optional[str] = None
