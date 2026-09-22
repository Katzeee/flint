from typing import Optional

from .models import InstanceExecResult
from .buffer import ThreadSafeTextBuffer
from .executor import CodeExecutor
from .strategies import ExecutionStrategy


class CodeRunner(object):
    """Executes code using a replaceable host-thread strategy."""

    def __init__(
        self,
        executor: CodeExecutor,
        strategy: ExecutionStrategy,
    ) -> None:
        if strategy is None:
            raise TypeError("strategy is required")
        self._executor = executor
        self._strategy = strategy

    def execute(
        self,
        execution_id: str,
        code: str,
        out: Optional[ThreadSafeTextBuffer] = None,
        err: Optional[ThreadSafeTextBuffer] = None,
        filename: Optional[str] = None,
    ) -> InstanceExecResult:
        return self._strategy.run(
            lambda: self._executor.execute(
                execution_id,
                code,
                out=out,
                err=err,
                filename=filename,
            )
        )

    def close(self) -> None:
        self._strategy.close()
