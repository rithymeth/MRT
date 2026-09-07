from typing import Optional


class MRTError(Exception):
    """Base class for all errors produced by the MRT toolchain."""

    def __init__(self, message: str, line: Optional[int] = None):
        self.message = message
        self.line = line
        super().__init__(self._format())

    def _format(self) -> str:
        if self.line is not None:
            return f"{self.message} [line {self.line}]"
        return self.message


class MRTSyntaxError(MRTError):
    """Raised by the lexer or parser when source code is malformed."""


class MRTRuntimeError(MRTError):
    """Raised by the interpreter when a program fails at run time."""
