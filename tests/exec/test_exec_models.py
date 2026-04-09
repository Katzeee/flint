from __future__ import annotations

import pytest

from pbridge.shared.exec_models import ExecRequest, ExecResult, ExecStatus, ExecWireModel
from pbridge.shared.model_base import VersionedWireModel, WireModelError


def test_exec_request_roundtrip():
    req = ExecRequest(request_id="r1", code="print(1)")
    data = req.to_dict()
    assert data["type"] == "ExecRequest"
    assert data["version"] == 1
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ExecRequest)
    assert parsed == req


def test_exec_result_roundtrip():
    res = ExecResult(
        request_id="r1", status=ExecStatus.SUCCEED, stdout="hello\n", stderr=""
    )
    data = res.to_dict()
    assert data["type"] == "ExecResult"
    assert data["version"] == 1
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ExecResult)
    assert parsed == res


def test_exec_result_exclude_none_omits_optional():
    res = ExecResult(
        request_id="r1", status=ExecStatus.SUCCEED, stdout="", stderr=""
    )
    data = res.to_dict(exclude_none=True)
    assert "traceback" not in data
    assert "error" not in data


def test_exec_result_with_traceback():
    res = ExecResult(
        request_id="r1",
        status=ExecStatus.FAILED,
        stdout="",
        stderr="",
        traceback="Traceback ...\nNameError: name 'x' is not defined\n",
    )
    data = res.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ExecResult)
    assert parsed.traceback == res.traceback


def test_exec_request_version_mismatch():
    data = {
        "type": "ExecRequest",
        "version": 99,
        "request_id": "r1",
        "code": "x",
    }
    with pytest.raises(WireModelError, match="Version mismatch"):
        VersionedWireModel.parse_versioned(data)
