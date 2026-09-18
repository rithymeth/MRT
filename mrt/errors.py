from typing import List, Optional


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
    """Raised by the interpreter when a program fails at run time.

    `kind` is a coarse category a program can branch on from a `catch`
    guard (see docs/LANGUAGE_SPEC.md); it is deliberately a small closed
    set rather than one label per message, so that catching "any bad
    index" doesn't mean enumerating every built-in that can produce one.

    `mrt_stack` is the MRT-level call stack, innermost first. It is filled
    in as the error propagates outward through MRTFunction.call rather
    than captured at the raise site, because only the callers know their
    own names.
    """

    def __init__(self, message: str, line: Optional[int] = None,
                 kind: str = "RuntimeError"):
        self.kind = kind
        self.mrt_stack: List[str] = []
        super().__init__(message, line)


# The closed set of error kinds. Kept here (rather than as bare strings at
# each raise site) so the Playground interpreter can be checked against the
# same list, and so `docs/LANGUAGE_SPEC.md` has one place to mirror.
ERROR_KINDS = (
    "TypeError",        # a value of the wrong type
    "ArityError",       # wrong number of arguments
    "IndexError",       # index outside an array/string, or a non-numeric index
    "KeyError",         # object key that isn't present
    "NameError",        # undefined variable
    "ValueError",       # right type, unusable value (sqrt(-1), a zero step)
    "ArithmeticError",  # division or modulo by zero
    "RuntimeError",     # anything not covered above
)
