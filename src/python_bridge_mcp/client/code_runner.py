import asyncio
import functools
from typing import Optional

from ..shared.instance_control_models import InstanceExecResult
from ..shared.text_buffer import ThreadSafeTextBuffer
from .code_executor import CodeExecutor
from .execution_strategy import ExecutionStrategy


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

    async def async_execute(
        self,
        execution_id: str,
        code: str,
        out: Optional[ThreadSafeTextBuffer] = None,
        err: Optional[ThreadSafeTextBuffer] = None,
        filename: Optional[str] = None,
    ) -> InstanceExecResult:
        loop = asyncio.get_event_loop()
        return await loop.run_in_executor(
            None, functools.partial(self.execute, execution_id, code, out=out, err=err, filename=filename),
        )

    def close(self) -> None:
        self._strategy.close()
