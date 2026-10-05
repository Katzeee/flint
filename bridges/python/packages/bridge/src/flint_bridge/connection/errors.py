"""Failures the Bridge reports to the code that asked for an operation."""


class BridgeCreationError(RuntimeError):
    """No Bridge was created; ``kind`` names why.

    The kinds are ``invalid_configuration``, ``claimed`` (another Bridge owns
    this process), ``system``, ``library_unavailable``, and ``abi_mismatch``.
    """

    def __init__(self, kind, message):
        super().__init__(message)
        self.kind = kind


class BridgeBusyError(RuntimeError):
    """The Bridge refused a settings change while host code is executing."""
