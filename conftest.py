from __future__ import annotations

import os
import sys
import types

# Registers the source subdirectories as subpackages of a synthetic "pbridge"
# parent so that relative imports (from ..shared import ...) work when pytest
# loads client/ and server/ modules.
_SRC = os.path.join(os.path.dirname(__file__), "python-bridge-mcp")


def _reg(name: str, path: str) -> None:
    m = types.ModuleType(name)
    m.__path__ = [path]  # type: ignore[assignment]
    m.__package__ = name
    sys.modules[name] = m


_reg("pbridge", _SRC)
_reg("pbridge.shared", os.path.join(_SRC, "shared"))
_reg("pbridge.client", os.path.join(_SRC, "client"))
_reg("pbridge.server", os.path.join(_SRC, "server"))
_reg("pbridge.backend", os.path.join(_SRC, "backend"))
