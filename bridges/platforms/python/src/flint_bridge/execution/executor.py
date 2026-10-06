import sys
import traceback as tb_mod
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from typing import Any, Dict, Optional

from .models import InstanceExecResult, InstanceExecStatus
from .output import ThreadScopedTextProxy


class CodeExecutor:
    """Prepares a namespace and runs code via exec(), capturing output."""

    def __init__(self, ns: Optional[Dict[str, Any]] = None) -> None:
        self._ns: Dict[str, Any] = ns if ns is not None else {}

    def execute(
        self,
        execution_id: str,
        code: str,
        out=None,
        err=None,
        filename: Optional[str] = None,
    ) -> InstanceExecResult:
        """Run code with writable text streams; direct calls discard output by default."""
        out = out if out is not None else StringIO()
        err = err if err is not None else StringIO()
        stdout = ThreadScopedTextProxy(sys.stdout, out)
        stderr = ThreadScopedTextProxy(sys.stderr, err)
        with redirect_stdout(stdout), redirect_stderr(stderr):
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
