import traceback as tb_mod
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from typing import Any, Dict, Optional

from ..shared.exec_models import ExecResult, ExecStatus


class CodeExecutor:
    """Prepares a namespace and runs code via exec(), capturing output."""

    def __init__(self, ns: Optional[Dict[str, Any]] = None) -> None:
        self._ns: Dict[str, Any] = ns if ns is not None else {}

    def execute(self, request_id: str, code: str) -> ExecResult:
        out = StringIO()
        err = StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            try:
                exec(code, self._ns, self._ns)
                status = ExecStatus.SUCCEED
                traceback = None
            except Exception:
                status = ExecStatus.FAILED
                traceback = tb_mod.format_exc()
        return ExecResult(
            request_id=request_id,
            status=status,
            stdout=out.getvalue(),
            stderr=err.getvalue(),
            traceback=traceback,
        )
