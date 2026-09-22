import traceback as tb_mod
from contextlib import redirect_stderr, redirect_stdout
from typing import Any, Dict, Optional

from .models import InstanceExecResult, InstanceExecStatus
from .buffer import ThreadSafeTextBuffer


class CodeExecutor:
    """Prepares a namespace and runs code via exec(), capturing output."""

    def __init__(self, ns: Optional[Dict[str, Any]] = None) -> None:
        self._ns: Dict[str, Any] = ns if ns is not None else {}

    def execute(
        self,
        execution_id: str,
        code: str,
        out: Optional[ThreadSafeTextBuffer] = None,
        err: Optional[ThreadSafeTextBuffer] = None,
        filename: Optional[str] = None,
    ) -> InstanceExecResult:
        out = out or ThreadSafeTextBuffer()
        err = err or ThreadSafeTextBuffer()
        with redirect_stdout(out), redirect_stderr(err):
            try:
                compiled = compile(code, filename or "<string>", "exec")
                exec(compiled, self._ns, self._ns)
                status = InstanceExecStatus.SUCCEEDED
                traceback = None
            except BaseException:
                status = InstanceExecStatus.FAILED
                traceback = tb_mod.format_exc()
        return InstanceExecResult(
            execution_id=execution_id,
            status=status,
            traceback=traceback,
        )
