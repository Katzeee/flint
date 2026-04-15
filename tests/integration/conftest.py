from pathlib import Path

import pytest

from python_bridge_mcp.shared.workflow_persistence import WorkflowPersistence


@pytest.fixture(autouse=True)
def _workflow_tmpdir(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(WorkflowPersistence, "BASE_DIR", tmp_path / "workflows")
