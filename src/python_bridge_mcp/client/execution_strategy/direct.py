from typing import Callable

from .base import ExecutionStrategy, ResultT


class DirectExecutionStrategy(ExecutionStrategy):
    def run(self, func: Callable[[], ResultT]) -> ResultT:
        with self._admit():
            pass
        return func()
