from __future__ import annotations

import json
import math
import os
import sys
from functools import cmp_to_key
from typing import Any, Dict, List

from .ast import *
from .errors import MRTRuntimeError
from .lexer import Token, TokenType

# What became of a top-level `main`, recorded on the interpreter for a CLI to
# report. A file that declares only functions and structs runs correctly and
# prints nothing, which looks identical to a broken install -- these let the
# command say which it was.
ENTRY_RAN = "ran"
ENTRY_MISSING = "missing"


def stringify(value: Any) -> str:
    """Render an MRT runtime value the way MRT source code would spell it:
    booleans as `true`/`false`, whole numbers without a trailing `.0`,
    `null` for the absence of a value, and arrays recursively."""
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, float):
        if value.is_integer():
            return str(int(value))
        return str(value)
    if isinstance(value, list):
        return "[" + ", ".join(stringify(v) for v in value) + "]"
    if isinstance(value, dict):
        return (
            "{"
            + ", ".join(
                f"{stringify(load_key(k))}: {stringify(v)}" for k, v in value.items()
            )
            + "}"
        )
    if isinstance(value, (MRTInstance, MRTGenerator)):
        return str(value)
    if callable(value):
        # A built-in (a host-language function, not an MRTFunction, which
        # has its own __str__). Without this, Python would render it as
        # "<function MRTBuiltin.len at 0x7f...>" -- leaking the host
        # implementation, embedding a memory address that changes run to
        # run, and disagreeing with the Playground, which would print the
        # JavaScript source text instead.
        return "<builtin>"
    return str(value)


def type_name(value: Any) -> str:
    """The MRT type of a value, as `type()` reports it -- used in error
    messages so they name the language's types, not Python's."""
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, (int, float)):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    if isinstance(value, dict):
        return "object"
    if isinstance(value, MRTInstance):
        # An instance reports its own struct's name, so `type(p) == "Point"`.
        return value.struct.name
    if isinstance(value, MRTStruct):
        return "struct"
    if isinstance(value, MRTGenerator):
        return "generator"
    return "function"


def object_like(value: Any) -> Dict[str, Any] | None:
    """The field mapping of anything that reads like an object.

    A struct instance answers `keys`/`values`/`has`/`get` and object
    destructuring with its fields (not its methods), so the same code that
    walks a plain object works on a struct without special-casing."""
    if isinstance(value, dict):
        return value
    if isinstance(value, MRTInstance):
        return value.values
    return None


def values_equal(a: Any, b: Any) -> bool:
    """Structural equality between two MRT values (see Interpreter.is_equal
    for why this can't just be Python's `==`: `True == 1` in Python, but not
    in MRT, and that has to hold at every nesting level). Shared by the `==`
    operator and by the indexOf()/has() built-ins, which need the same
    "does this array contain a value equal to X" semantics."""
    if a is None and b is None:
        return True
    if a is None or b is None:
        return False
    if isinstance(a, bool) != isinstance(b, bool):
        return False
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(values_equal(x, y) for x, y in zip(a, b))
    if isinstance(a, dict) and isinstance(b, dict):
        if len(a) != len(b):
            return False
        return all(k in b and values_equal(v, b[k]) for k, v in a.items())
    if isinstance(a, MRTInstance) or isinstance(b, MRTInstance):
        # Two instances are equal when they share a struct and every field
        # matches; an instance never equals a plain object.
        if not (isinstance(a, MRTInstance) and isinstance(b, MRTInstance)):
            return False
        if a.struct is not b.struct:
            return False
        return all(values_equal(v, b.values[k]) for k, v in a.values.items())
    if isinstance(a, list) != isinstance(b, list):
        return False
    if isinstance(a, dict) != isinstance(b, dict):
        return False
    return a == b


class MRTFunction:
    """A callable closure. Built from either a `func name(...)` declaration
    or an anonymous `func(...)` expression -- the two are the same thing at
    runtime, differing only in whether `name` is set."""

    def __init__(
        self,
        params: List[Param],
        body: List[Stmt],
        closure: "Environment",
        name: str | None = None,
        is_generator: bool = False,
    ):
        self.params = params
        self.body = body
        self.closure = closure
        self.name = name
        self.is_generator = is_generator

    def arity_description(self) -> str:
        """How this function's accepted argument count reads in an error."""
        positional = [p for p in self.params if not p.rest]
        required = sum(1 for p in positional if p.default is None)
        if any(p.rest for p in self.params):
            return f"at least {required}"
        if required == len(positional):
            return str(required)
        return f"between {required} and {len(positional)}"

    def accepts(self, count: int) -> bool:
        positional = [p for p in self.params if not p.rest]
        required = sum(1 for p in positional if p.default is None)
        if count < required:
            return False
        return any(p.rest for p in self.params) or count <= len(positional)

    def bind(self, interpreter: "Interpreter", arguments: List[Any]) -> "Environment":
        """Bind arguments to parameters in a fresh scope.

        Defaults are evaluated here, at call time and in the callee's own
        scope, so a default may refer to a parameter to its left
        (`func f(a, b = a * 2)`). A rest parameter always binds -- to an
        empty array when nothing is left over."""
        environment = Environment(self.closure)
        positional = [p for p in self.params if not p.rest]

        previous = interpreter.environment
        try:
            interpreter.environment = environment
            for i, param in enumerate(positional):
                value = arguments[i] if i < len(arguments) else MISSING
                interpreter.bind_pattern(param.pattern, value, environment)
        finally:
            interpreter.environment = previous

        for param in self.params:
            if param.rest:
                environment.define(
                    param.pattern.name.lexeme,  # type: ignore[attr-defined]
                    list(arguments[len(positional) :]),
                )

        return environment

    def call(self, interpreter: "Interpreter", arguments: List[Any]) -> Any:
        environment = self.bind(interpreter, arguments)

        if self.is_generator:
            # Nothing in the body runs yet: calling a generator function only
            # builds the lazy sequence.
            label = f"<generator {self.name}>" if self.name else "<generator>"
            return MRTGenerator(
                label, lambda: interpreter.execute_block_gen(self.body, environment)
            )

        frame = self.name if self.name else "<anonymous>"
        try:
            interpreter.execute_block(self.body, environment)
            return None
        except ReturnSignal as return_value:
            return return_value.value
        except MRTRuntimeError as e:
            # Build the trace on the way out: a raise site knows nothing
            # about who called it, but every frame the error passes through
            # knows its own name. Innermost first.
            e.mrt_stack.append(frame)
            raise
        except MRTThrow as thrown:
            thrown.stack.append(frame)
            raise

    def __str__(self):
        return f"<function {self.name}>" if self.name else "<function>"


class MRTGenerator:
    """A lazy sequence: the result of calling a generator function, or of a
    lazy built-in such as `map` over another generator.

    Nothing runs until something pulls from it, and only as far as it asks --
    which is what makes an endless generator usable. It is *resumable*: a
    loop that stops early leaves it suspended mid-body, and the next consumer
    carries on from there. It is not restartable: once the sequence has run
    out there is no way back to the start, so `for`-`in` over a finished
    generator is an error rather than a silently empty loop.

    `make` builds the host-language generator that actually runs the body,
    and is deliberately a callable rather than the generator itself so that
    nothing is set up until the first pull."""

    def __init__(self, label: str, make: Any):
        self.label = label
        self.make = make
        self.running = None
        self.done = False
        # True while this generator's body is on the stack, so a program
        # that resumes a generator from inside itself gets a real error
        # rather than whichever host-language failure happens first.
        self.active = False

    def step(
        self, interpreter: "Interpreter", sent: Any = None, line: int | None = None
    ):
        """Run to the next `yield`, returning `(done, value)`.

        `sent` becomes the value of the `yield` this generator is suspended
        at -- discarded on the first step, because the body hasn't reached a
        `yield` yet to receive it."""
        if self.done:
            return True, None
        if self.active:
            raise MRTRuntimeError(
                f"{self} is already running; a generator can't be resumed from "
                f"inside itself.",
                line,
                kind="ValueError",
            )

        if self.running is None:
            self.running = self.make()
            sent = None

        frame = str(self)
        # The body and whatever consumes it take turns using the
        # interpreter's current scope, so each hand-off restores the
        # consumer's.
        saved = interpreter.environment
        self.active = True
        try:
            item = self.running.send(sent)  # type: ignore[attr-defined]
        except StopIteration:
            self.done = True
            return True, None
        except ReturnSignal:
            # `return` inside a generator simply ends the sequence.
            self.done = True
            return True, None
        except MRTRuntimeError as e:
            self.done = True
            e.mrt_stack.append(frame)
            raise
        except MRTThrow as thrown:
            self.done = True
            thrown.stack.append(frame)
            raise
        finally:
            self.active = False
            interpreter.environment = saved

        return False, item

    def __str__(self):
        return self.label


class MRTStruct:
    """A declared struct type, and the callable that constructs it.

    Calling it builds an MRTInstance with the declared fields bound
    positionally; field defaults work exactly like parameter defaults."""

    def __init__(
        self, name: str, fields: List[Param], methods: Dict[str, "MRTFunction"]
    ):
        self.name = name
        self.fields = fields
        self.methods = methods

    def field_names(self) -> List[str]:
        return [f.pattern.name.lexeme for f in self.fields]  # type: ignore[attr-defined]

    def arity_description(self) -> str:
        required = sum(1 for f in self.fields if f.default is None)
        if required == len(self.fields):
            return str(required)
        return f"between {required} and {len(self.fields)}"

    def accepts(self, count: int) -> bool:
        required = sum(1 for f in self.fields if f.default is None)
        return required <= count <= len(self.fields)

    def construct(
        self, interpreter: "Interpreter", arguments: List[Any]
    ) -> "MRTInstance":
        values: Dict[str, Any] = {}
        # Defaults are evaluated in a scope where the fields to their left
        # are already bound, mirroring how parameter defaults behave.
        scope = Environment(interpreter.globals)
        previous = interpreter.environment
        try:
            interpreter.environment = scope
            for index, field in enumerate(self.fields):
                key = field.pattern.name.lexeme  # type: ignore[attr-defined]
                if index < len(arguments):
                    value = arguments[index]
                else:
                    value = interpreter.evaluate(field.default)  # type: ignore[arg-type]
                values[key] = value
                scope.define(key, value)
        finally:
            interpreter.environment = previous
        return MRTInstance(self, values)

    def __str__(self):
        return f"<struct {self.name}>"


class MRTInstance:
    """One value of a struct type: a fixed set of named fields, plus the
    methods its struct declares."""

    def __init__(self, struct: MRTStruct, values: Dict[str, Any]):
        self.struct = struct
        self.values = values

    def get(
        self, name: str, interpreter: "Interpreter", line: int | None = None
    ) -> Any:
        if name in self.values:
            return self.values[name]
        method = self.struct.methods.get(name)
        if method is not None:
            # Bind `this` by wrapping the method's closure, so a method
            # pulled off an instance still knows its receiver later.
            bound = Environment(method.closure)
            bound.define("this", self)
            return MRTFunction(
                method.params, method.body, bound, method.name, method.is_generator
            )
        raise MRTRuntimeError(
            f"Struct {self.struct.name} has no field or method {json.dumps(name)}.",
            line,
            kind="KeyError",
        )

    def set(self, name: str, value: Any, line: int | None = None):
        if name not in self.values:
            raise MRTRuntimeError(
                f"Struct {self.struct.name} has no field {json.dumps(name)}.",
                line,
                kind="KeyError",
            )
        self.values[name] = value

    def __str__(self):
        inner = ", ".join(f"{k}: {stringify(v)}" for k, v in self.values.items())
        return f"{self.struct.name}({inner})"


class ReturnSignal(Exception):
    def __init__(self, value: Any):
        self.value = value
        super().__init__()


class MRTThrow(Exception):
    """A value thrown by `throw`, unwinding until a `try`/`catch` catches it.

    Distinct from MRTRuntimeError: this carries an arbitrary MRT *value*
    (whatever the program threw), whereas MRTRuntimeError is the
    interpreter's own failure. Built-in runtime errors become catchable by
    being converted into one of these -- see Interpreter.execute_try."""

    def __init__(self, value: Any):
        self.value = value
        self.stack: List[str] = []
        super().__init__()


def make_error_value(error: MRTRuntimeError) -> Dict[str, Any]:
    """The object a `catch` block receives for an interpreter-raised error.

    Programs can throw any value they like, but built-in failures always
    arrive in this shape: a message, the line it happened on, a coarse
    `kind` to branch on, and the MRT call stack innermost-first."""
    return {
        "message": error.message,
        "line": float(error.line) if error.line is not None else None,
        "kind": error.kind,
        "stack": list(error.mrt_stack),
    }


class BoolKey:
    """How a boolean object key is stored.

    MRT's `==` says `true != 1` at every nesting level, and there is a parity
    case pinning it. Python's dict disagrees: it hashes `True` and `1`
    identically, so `{1: "a"}[true]` used to find `"a"` here while the
    Playground -- whose Map keeps them apart -- reported a missing key. The
    same program gave two different answers depending on where it ran.

    Wrapping booleans on the way in makes object keys agree with the equality
    the rest of the language already uses. Strings and numbers are stored as
    themselves, so nothing else changes.
    """

    __slots__ = ("value",)

    def __init__(self, value: bool):
        self.value = value

    def __hash__(self) -> int:
        return hash(("mrt-bool", self.value))

    def __eq__(self, other: Any) -> bool:
        return isinstance(other, BoolKey) and other.value is self.value

    def __repr__(self) -> str:
        return f"BoolKey({self.value})"


def store_key(key: Any) -> Any:
    """The dict key an MRT object key is stored under."""
    return BoolKey(key) if isinstance(key, bool) else key


def load_key(stored: Any) -> Any:
    """The MRT value a stored dict key stands for."""
    return stored.value if isinstance(stored, BoolKey) else stored


class _Missing:
    """Sentinel for "this pattern slot had no corresponding value", which is
    distinct from a value that is genuinely `null`."""

    def __repr__(self):
        return "<missing>"


MISSING = _Missing()


class GeneratorScope:
    """Manages the interpreter's current scope for a block inside a generator
    body, and does nothing when that generator is being *closed*.

    A suspended generator that nothing holds a reference to any more is
    garbage; CPython disposes of it by throwing `GeneratorExit` at whatever
    `yield` it stopped at. That happens at a collection point -- an arbitrary
    moment during completely unrelated execution -- so ordinary
    `try`/`finally` cleanup inside a generator body does not run "when the
    generator finishes", it runs *in the middle of someone else's work*.
    Restoring `interpreter.environment` there silently replaces the live scope
    with a stale one, and the program fails later with a nonsense error such
    as an undefined variable that is plainly in scope.

    So the previous scope is restored on normal completion and on a real
    error, and deliberately not on `GeneratorExit`. JavaScript never closes an
    abandoned generator at all, which is the other half of the reason: this is
    what keeps the two implementations agreeing about a program that drops a
    generator on the floor.
    """

    def __init__(self, interpreter: "Interpreter", environment: "Environment"):
        self.interpreter = interpreter
        self.previous = interpreter.environment
        interpreter.environment = environment

    def __enter__(self) -> "GeneratorScope":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        if exc_type is not GeneratorExit:
            self.interpreter.environment = self.previous


class BreakSignal(Exception):
    pass


class ContinueSignal(Exception):
    pass


class Environment:
    def __init__(self, enclosing: "Environment" | None = None):
        self.values: Dict[str, Any] = {}
        self.enclosing = enclosing

    def define(self, name: str, value: Any):
        self.values[name] = value

    def get(self, name: Token) -> Any:
        if name.lexeme in self.values:
            return self.values[name.lexeme]

        if self.enclosing:
            return self.enclosing.get(name)

        raise MRTRuntimeError(
            f"Undefined variable '{name.lexeme}'.", name.line, kind="NameError"
        )

    def assign(self, name: Token, value: Any):
        if name.lexeme in self.values:
            self.values[name.lexeme] = value
            return

        if self.enclosing:
            self.enclosing.assign(name, value)
            return

        raise MRTRuntimeError(
            f"Undefined variable '{name.lexeme}'.", name.line, kind="NameError"
        )


class MRTBuiltin:
    @staticmethod
    def len(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "len() takes exactly one argument.", kind="ArityError"
            )
        source = object_like(args[0])
        if source is not None:
            return float(len(source))
        if isinstance(args[0], (str, list)):
            return float(len(args[0]))
        raise MRTRuntimeError(
            "len() argument must be an array, object, or string.", kind="TypeError"
        )

    @staticmethod
    def push(*args):
        if len(args) != 2:
            raise MRTRuntimeError(
                "push() takes exactly two arguments.", kind="ArityError"
            )
        if not isinstance(args[0], list):
            raise MRTRuntimeError(
                "First argument to push() must be an array.", kind="TypeError"
            )
        args[0].append(args[1])
        return args[1]

    @staticmethod
    def pop(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "pop() takes exactly one argument.", kind="ArityError"
            )
        if not isinstance(args[0], list):
            raise MRTRuntimeError("pop() argument must be an array.", kind="TypeError")
        if not args[0]:
            raise MRTRuntimeError("Cannot pop from empty array.", kind="ValueError")
        return args[0].pop()

    @staticmethod
    def slice(*args):
        if len(args) not in [2, 3]:
            raise MRTRuntimeError("slice() takes 2 or 3 arguments.", kind="ArityError")
        if not isinstance(args[0], list):
            raise MRTRuntimeError(
                "First argument to slice() must be an array.", kind="TypeError"
            )

        arr = args[0]
        start = int(args[1]) if isinstance(args[1], (int, float)) else 0
        end = (
            int(args[2])
            if len(args) > 2 and isinstance(args[2], (int, float))
            else len(arr)
        )

        if start < 0:
            start = len(arr) + start
        if end < 0:
            end = len(arr) + end

        return arr[start:end]

    @staticmethod
    def join(*args):
        if len(args) not in [1, 2]:
            raise MRTRuntimeError("join() takes 1 or 2 arguments.", kind="ArityError")
        if not isinstance(args[0], list):
            raise MRTRuntimeError(
                "First argument to join() must be an array.", kind="TypeError"
            )

        separator = str(args[1]) if len(args) > 1 else ""
        return separator.join(stringify(x) for x in args[0])

    @staticmethod
    def indexOf(*args):
        if len(args) != 2:
            raise MRTRuntimeError(
                "indexOf() takes exactly 2 arguments.", kind="ArityError"
            )
        if not isinstance(args[0], list):
            raise MRTRuntimeError(
                "First argument to indexOf() must be an array.", kind="TypeError"
            )

        for i, item in enumerate(args[0]):
            if values_equal(item, args[1]):
                return float(i)
        return -1.0

    @staticmethod
    def split(*args):
        if len(args) not in [1, 2]:
            raise MRTRuntimeError("split() takes 1 or 2 arguments.", kind="ArityError")
        if not isinstance(args[0], str):
            raise MRTRuntimeError(
                "First argument to split() must be a string.", kind="TypeError"
            )

        separator = str(args[1]) if len(args) > 1 else " "
        return args[0].split(separator)

    @staticmethod
    def substring(*args):
        if len(args) not in [2, 3]:
            raise MRTRuntimeError(
                "substring() takes 2 or 3 arguments.", kind="ArityError"
            )
        if not isinstance(args[0], str):
            raise MRTRuntimeError(
                "First argument to substring() must be a string.", kind="TypeError"
            )

        text = args[0]
        start = int(args[1]) if isinstance(args[1], (int, float)) else 0
        end = (
            int(args[2])
            if len(args) > 2 and isinstance(args[2], (int, float))
            else len(text)
        )

        if start < 0:
            start = len(text) + start
        if end < 0:
            end = len(text) + end

        return text[start:end]

    @staticmethod
    def toUpper(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "toUpper() takes exactly one argument.", kind="ArityError"
            )
        if not isinstance(args[0], str):
            raise MRTRuntimeError(
                "toUpper() argument must be a string.", kind="TypeError"
            )
        return args[0].upper()

    @staticmethod
    def toLower(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "toLower() takes exactly one argument.", kind="ArityError"
            )
        if not isinstance(args[0], str):
            raise MRTRuntimeError(
                "toLower() argument must be a string.", kind="TypeError"
            )
        return args[0].lower()

    @staticmethod
    def trim(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "trim() takes exactly one argument.", kind="ArityError"
            )
        if not isinstance(args[0], str):
            raise MRTRuntimeError("trim() argument must be a string.", kind="TypeError")
        return args[0].strip()

    @staticmethod
    def replace(*args):
        if len(args) != 3:
            raise MRTRuntimeError(
                "replace() takes exactly 3 arguments.", kind="ArityError"
            )
        if not isinstance(args[0], str):
            raise MRTRuntimeError(
                "First argument to replace() must be a string.", kind="TypeError"
            )
        return str(args[0]).replace(str(args[1]), str(args[2]))

    @staticmethod
    def startsWith(*args):
        if len(args) != 2:
            raise MRTRuntimeError(
                "startsWith() takes exactly 2 arguments.", kind="ArityError"
            )
        if not isinstance(args[0], str):
            raise MRTRuntimeError(
                "First argument to startsWith() must be a string.", kind="TypeError"
            )
        return args[0].startswith(str(args[1]))

    @staticmethod
    def endsWith(*args):
        if len(args) != 2:
            raise MRTRuntimeError(
                "endsWith() takes exactly 2 arguments.", kind="ArityError"
            )
        if not isinstance(args[0], str):
            raise MRTRuntimeError(
                "First argument to endsWith() must be a string.", kind="TypeError"
            )
        return args[0].endswith(str(args[1]))

    @staticmethod
    def contains(*args):
        if len(args) != 2:
            raise MRTRuntimeError(
                "contains() takes exactly 2 arguments.", kind="ArityError"
            )
        if not isinstance(args[0], str):
            raise MRTRuntimeError(
                "First argument to contains() must be a string.", kind="TypeError"
            )
        return str(args[1]) in args[0]

    # -- Type / conversion -------------------------------------------------

    @staticmethod
    def type_(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "type() takes exactly one argument.", kind="ArityError"
            )
        # One source of truth, shared with the error messages that name a
        # value's type -- otherwise the two drift, and a new kind of value
        # shows up as "unknown" in one place and correctly in the other.
        return type_name(args[0])

    @staticmethod
    def toNumber(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "toNumber() takes exactly one argument.", kind="ArityError"
            )
        value = args[0]
        if isinstance(value, bool):
            return 1.0 if value else 0.0
        if isinstance(value, (int, float)):
            return float(value)
        if isinstance(value, str):
            try:
                return float(value.strip())
            except ValueError:
                raise MRTRuntimeError(
                    f"Cannot convert '{value}' to a number.", kind="ValueError"
                )
        raise MRTRuntimeError(
            "toNumber() argument must be a string, number, or boolean.",
            kind="TypeError",
        )

    @staticmethod
    def toString(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "toString() takes exactly one argument.", kind="ArityError"
            )
        return stringify(args[0])

    # -- Math ----------------------------------------------------------------

    @staticmethod
    def _num(value, who: str) -> float:
        if not isinstance(value, (int, float)) or isinstance(value, bool):
            raise MRTRuntimeError(
                f"{who}() argument must be a number.", kind="TypeError"
            )
        return float(value)

    @staticmethod
    def abs_(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "abs() takes exactly one argument.", kind="ArityError"
            )
        return abs(MRTBuiltin._num(args[0], "abs"))

    @staticmethod
    def min_(*args):
        values = args[0] if len(args) == 1 and isinstance(args[0], list) else list(args)
        if not values:
            raise MRTRuntimeError(
                "min() requires at least one argument.", kind="ArityError"
            )
        return min(MRTBuiltin._num(v, "min") for v in values)

    @staticmethod
    def max_(*args):
        values = args[0] if len(args) == 1 and isinstance(args[0], list) else list(args)
        if not values:
            raise MRTRuntimeError(
                "max() requires at least one argument.", kind="ArityError"
            )
        return max(MRTBuiltin._num(v, "max") for v in values)

    @staticmethod
    def round_(*args):
        if len(args) not in (1, 2):
            raise MRTRuntimeError("round() takes 1 or 2 arguments.", kind="ArityError")
        value = MRTBuiltin._num(args[0], "round")
        digits = int(MRTBuiltin._num(args[1], "round")) if len(args) == 2 else 0
        return _round_half_away(value, digits)

    @staticmethod
    def floor(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "floor() takes exactly one argument.", kind="ArityError"
            )
        return float(math.floor(MRTBuiltin._num(args[0], "floor")))

    @staticmethod
    def ceil(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "ceil() takes exactly one argument.", kind="ArityError"
            )
        return float(math.ceil(MRTBuiltin._num(args[0], "ceil")))

    @staticmethod
    def sqrt(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "sqrt() takes exactly one argument.", kind="ArityError"
            )
        value = MRTBuiltin._num(args[0], "sqrt")
        if value < 0:
            raise MRTRuntimeError(
                "sqrt() argument must not be negative.", kind="ValueError"
            )
        return math.sqrt(value)

    @staticmethod
    def pow_(*args):
        if len(args) != 2:
            raise MRTRuntimeError("pow() takes exactly 2 arguments.", kind="ArityError")
        return MRTBuiltin._num(args[0], "pow") ** MRTBuiltin._num(args[1], "pow")

    # -- Objects (dicts) -----------------------------------------------------

    @staticmethod
    def keys(*args):
        source = object_like(args[0]) if len(args) == 1 else None
        if source is None:
            raise MRTRuntimeError(
                "keys() takes exactly one object argument.", kind="ArityError"
            )
        return [load_key(k) for k in source]

    @staticmethod
    def values(*args):
        source = object_like(args[0]) if len(args) == 1 else None
        if source is None:
            raise MRTRuntimeError(
                "values() takes exactly one object argument.", kind="ArityError"
            )
        return list(source.values())

    @staticmethod
    def has(*args):
        if len(args) != 2:
            raise MRTRuntimeError("has() takes exactly 2 arguments.", kind="ArityError")
        container, key = args
        source = object_like(container)
        if source is not None:
            return store_key(key) in source
        if isinstance(container, list):
            return any(values_equal(item, key) for item in container)
        raise MRTRuntimeError(
            "First argument to has() must be an array or object.", kind="TypeError"
        )

    @staticmethod
    def get(*args):
        if len(args) not in (2, 3):
            raise MRTRuntimeError("get() takes 2 or 3 arguments.", kind="ArityError")
        container, key = args[0], args[1]
        default = args[2] if len(args) == 3 else None
        source = object_like(container)
        if source is not None:
            return source.get(store_key(key), default)
        if isinstance(container, list):
            if isinstance(key, (int, float)) and not isinstance(key, bool):
                i = int(key)
                if 0 <= i < len(container):
                    return container[i]
            return default
        if isinstance(container, str):
            if isinstance(key, (int, float)) and not isinstance(key, bool):
                i = int(key)
                if 0 <= i < len(container):
                    return container[i]
            return default
        # Anything else has no keys at all, so the default is the answer.
        # get() is the total, never-raising accessor -- raising here would
        # break its whole contract, and would make the natural `catch`
        # guard `get(e, "kind", "")` blow up on a thrown string or number.
        return default

    # -- Sequences ---------------------------------------------------------

    @staticmethod
    def reverse(*args):
        if len(args) != 1:
            raise MRTRuntimeError(
                "reverse() takes exactly one argument.", kind="ArityError"
            )
        if isinstance(args[0], str):
            return args[0][::-1]
        if isinstance(args[0], list):
            return list(reversed(args[0]))
        raise MRTRuntimeError(
            "reverse() argument must be an array or string.", kind="TypeError"
        )

    @staticmethod
    def unique(*args):
        if len(args) != 1 or not isinstance(args[0], list):
            raise MRTRuntimeError(
                "unique() takes exactly one array argument.", kind="ArityError"
            )
        result = []
        for item in args[0]:
            if not any(values_equal(item, seen) for seen in result):
                result.append(item)
        return result

    @staticmethod
    def flatten(*args):
        if not 1 <= len(args) <= 2 or not isinstance(args[0], list):
            raise MRTRuntimeError(
                "flatten() takes an array and an optional depth.", kind="ArityError"
            )
        depth = 1 if len(args) == 1 else int(_number_arg(args[1], "flatten"))
        if depth < 0:
            raise MRTRuntimeError(
                "flatten() depth must not be negative.", kind="ValueError"
            )

        def go(items, d):
            out = []
            for item in items:
                if isinstance(item, list) and d > 0:
                    out.extend(go(item, d - 1))
                else:
                    out.append(item)
            return out

        return go(args[0], depth)

    @staticmethod
    def zip_(*args):
        if (
            len(args) != 2
            or not isinstance(args[0], list)
            or not isinstance(args[1], list)
        ):
            raise MRTRuntimeError(
                "zip() takes exactly two array arguments.", kind="ArityError"
            )
        return [[a, b] for a, b in zip(args[0], args[1])]

    @staticmethod
    def enumerate_(*args):
        if len(args) != 1 or not isinstance(args[0], list):
            raise MRTRuntimeError(
                "enumerate() takes exactly one array argument.", kind="ArityError"
            )
        return [[float(i), v] for i, v in enumerate(args[0])]

    @staticmethod
    def count(*args):
        if len(args) != 2 or not isinstance(args[0], list):
            raise MRTRuntimeError(
                "count() takes an array and a value.", kind="ArityError"
            )
        return float(sum(1 for item in args[0] if values_equal(item, args[1])))

    @staticmethod
    def sum_(*args):
        if len(args) != 1 or not isinstance(args[0], list):
            raise MRTRuntimeError(
                "sum() takes exactly one array argument.", kind="ArityError"
            )
        total = 0.0
        for item in args[0]:
            total += _number_arg(item, "sum")
        return total

    @staticmethod
    def range_(*args):
        if not 1 <= len(args) <= 3:
            raise MRTRuntimeError(
                "range() takes one to three number arguments.", kind="ArityError"
            )
        nums = [_number_arg(a, "range") for a in args]
        if len(nums) == 1:
            start, end, step = 0.0, nums[0], 1.0
        elif len(nums) == 2:
            start, end, step = nums[0], nums[1], 1.0
        else:
            start, end, step = nums
        if step == 0:
            raise MRTRuntimeError("range() step must not be zero.", kind="ValueError")

        out = []
        current = start
        while (step > 0 and current < end) or (step < 0 and current > end):
            out.append(current)
            current += step
        return out

    # -- Strings -----------------------------------------------------------

    @staticmethod
    def repeat(*args):
        if len(args) != 2 or not isinstance(args[0], str):
            raise MRTRuntimeError(
                "repeat() takes a string and a count.", kind="ArityError"
            )
        n = int(_number_arg(args[1], "repeat"))
        if n < 0:
            raise MRTRuntimeError(
                "repeat() count must not be negative.", kind="ValueError"
            )
        return args[0] * n

    @staticmethod
    def _pad(args, who, at_start):
        if not 2 <= len(args) <= 3 or not isinstance(args[0], str):
            raise MRTRuntimeError(
                f"{who}() takes a string, a width, and an optional pad string.",
                kind="ArityError",
            )
        text = args[0]
        width = int(_number_arg(args[1], who))
        filler = args[2] if len(args) == 3 else " "
        if not isinstance(filler, str):
            raise MRTRuntimeError(
                f"{who}() pad argument must be a string.", kind="TypeError"
            )
        if filler == "":
            raise MRTRuntimeError(
                f"{who}() pad string must not be empty.", kind="ValueError"
            )
        if len(text) >= width:
            return text
        needed = width - len(text)
        padding = (filler * (needed // len(filler) + 1))[:needed]
        return padding + text if at_start else text + padding

    @staticmethod
    def padStart(*args):
        return MRTBuiltin._pad(args, "padStart", True)

    @staticmethod
    def padEnd(*args):
        return MRTBuiltin._pad(args, "padEnd", False)

    # -- Randomness --------------------------------------------------------

    @staticmethod
    def random(*args):
        """`random(seed)` returns a *function* producing the next value in a
        deterministic stream in [0, 1).

        Deterministic on purpose: the Playground's TypeScript interpreter
        runs the identical xorshift32 over uint32 state, so a seeded program
        prints the same numbers in the browser as it does here (and the
        parity checker can compare them). There is no unseeded/global
        random.

        Note that the raw values are fractions with a 2^32 denominator; for
        whole numbers use `floor(rng() * n)`."""
        if len(args) != 1:
            raise MRTRuntimeError(
                "random() takes exactly one seed argument.", kind="ArityError"
            )
        seed = int(_number_arg(args[0], "random")) & 0xFFFFFFFF
        state = [seed if seed != 0 else 1]

        def next_value(*call_args):
            if call_args:
                raise MRTRuntimeError(
                    "A random generator takes no arguments.", kind="ArityError"
                )
            x = state[0]
            x ^= (x << 13) & 0xFFFFFFFF
            x ^= x >> 17
            x ^= (x << 5) & 0xFFFFFFFF
            x &= 0xFFFFFFFF
            state[0] = x
            return x / 4294967296.0

        return next_value


def _round_half_away(value: float, digits: int) -> float:
    """Round half away from zero, scaling first.

    Deliberately *not* Python's built-in `round`, which rounds half to even
    on the exact binary value. The Playground has no equivalent, so the two
    interpreters would disagree on any value landing near a .5 boundary --
    `round(3.14159 * 2500, 2)` gave 7853.97 here and 7853.98 there. Doing
    the same double arithmetic in both is what keeps them identical, and
    half-away-from-zero is the behaviour most people expect."""
    factor = 10.0**digits
    scaled = value * factor
    rounded = math.floor(abs(scaled) + 0.5)
    return (-rounded if scaled < 0 else rounded) / factor


def _number_arg(value: Any, who: str) -> float:
    """Validate a numeric built-in argument. The message is worded to match
    the Playground interpreter's `numArg` exactly -- a caught error's
    `.message` is program-visible, so the two must agree verbatim."""
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        raise MRTRuntimeError(f"{who}() argument must be a number.", kind="TypeError")
    return float(value)


class Interpreter:
    def __init__(
        self, module_path: str | None = None, module_loader: Any | None = None
    ):
        self.globals = Environment()
        self.environment = self.globals
        self.output: List[str] = []  # Capture output for web playground
        # What became of a top-level `main`: ENTRY_RAN, ENTRY_MISSING, or the
        # type name of a non-callable `main`. A CLI reads this to explain a
        # file that ran correctly and did nothing, which is otherwise
        # indistinguishable from a broken installation. Kept off `output` so
        # that conformance, which compares printed text across four
        # implementations, is unaffected.
        self.entry = ENTRY_MISSING
        # Whether the program stopped on a runtime error (as opposed to
        # running to completion, however little it printed). A CLI needs
        # this to choose an exit code -- `interpret()` used to swallow every
        # runtime error internally and this stayed `False` no matter what
        # went wrong, which meant `mrt program.mrt` exited 0 on a crash. A
        # shell script or CI step checking `$?` after running a program this
        # way would see success on one that had actually stopped part way
        # through.
        self.had_error = False

        # -- Modules --
        # `module_path` is the absolute path of the file being run; imports
        # resolve relative to it. `module_loader(path) -> source` is
        # injectable so tests (and the Playground, which has no filesystem)
        # can supply modules without touching disk.
        self.module_path = os.path.abspath(module_path) if module_path else None
        self.module_loader = module_loader
        self.module_exports: Dict[str, Dict[str, Any]] = {}
        self.module_loading: List[str] = []
        self.current_exports: Dict[str, Any] = {}

        # Add built-in functions
        self.globals.define("print", self.print_function)
        self.globals.define("len", MRTBuiltin.len)
        self.globals.define("push", MRTBuiltin.push)
        self.globals.define("pop", MRTBuiltin.pop)
        self.globals.define("slice", MRTBuiltin.slice)
        self.globals.define("join", MRTBuiltin.join)
        self.globals.define("indexOf", MRTBuiltin.indexOf)
        # Add string functions
        self.globals.define("split", MRTBuiltin.split)
        self.globals.define("substring", MRTBuiltin.substring)
        self.globals.define("toUpper", MRTBuiltin.toUpper)
        self.globals.define("toLower", MRTBuiltin.toLower)
        self.globals.define("trim", MRTBuiltin.trim)
        self.globals.define("replace", MRTBuiltin.replace)
        self.globals.define("startsWith", MRTBuiltin.startsWith)
        self.globals.define("endsWith", MRTBuiltin.endsWith)
        self.globals.define("contains", MRTBuiltin.contains)
        # Type / conversion
        self.globals.define("type", MRTBuiltin.type_)
        self.globals.define("toNumber", MRTBuiltin.toNumber)
        self.globals.define("toString", MRTBuiltin.toString)
        # Math
        self.globals.define("abs", MRTBuiltin.abs_)
        self.globals.define("min", MRTBuiltin.min_)
        self.globals.define("max", MRTBuiltin.max_)
        self.globals.define("round", MRTBuiltin.round_)
        self.globals.define("floor", MRTBuiltin.floor)
        self.globals.define("ceil", MRTBuiltin.ceil)
        self.globals.define("sqrt", MRTBuiltin.sqrt)
        self.globals.define("pow", MRTBuiltin.pow_)
        # Objects (dicts)
        self.globals.define("keys", MRTBuiltin.keys)
        self.globals.define("values", MRTBuiltin.values)
        self.globals.define("has", MRTBuiltin.has)
        self.globals.define("get", MRTBuiltin.get)
        # Sequences
        self.globals.define("reverse", MRTBuiltin.reverse)
        self.globals.define("unique", MRTBuiltin.unique)
        self.globals.define("flatten", MRTBuiltin.flatten)
        self.globals.define("zip", MRTBuiltin.zip_)
        self.globals.define("enumerate", MRTBuiltin.enumerate_)
        self.globals.define("count", MRTBuiltin.count)
        self.globals.define("sum", MRTBuiltin.sum_)
        self.globals.define("range", MRTBuiltin.range_)
        # More strings
        self.globals.define("repeat", MRTBuiltin.repeat)
        self.globals.define("padStart", MRTBuiltin.padStart)
        self.globals.define("padEnd", MRTBuiltin.padEnd)
        # Deterministic, seeded randomness
        self.globals.define("random", MRTBuiltin.random)
        # Higher-order (need to call back into user code)
        self.globals.define("map", self.builtin_map)
        self.globals.define("filter", self.builtin_filter)
        self.globals.define("reduce", self.builtin_reduce)
        self.globals.define("find", self.builtin_find)
        self.globals.define("some", self.builtin_some)
        self.globals.define("every", self.builtin_every)
        self.globals.define("sort", self.builtin_sort)
        # Generators
        self.globals.define("toArray", self.builtin_toArray)
        self.globals.define("take", self.builtin_take)
        self.globals.define("next", self.builtin_next)
        self.globals.define("send", self.builtin_send)

    # -- Higher-order built-ins -------------------------------------------
    #
    # These live on the interpreter rather than in MRTBuiltin because they
    # have to call back into user code (`self.call_value`), which a plain
    # static helper has no handle on.

    def _array_arg(self, value: Any, who: str) -> List[Any]:
        if not isinstance(value, list):
            raise MRTRuntimeError(
                f"First argument to {who}() must be an array.", kind="TypeError"
            )
        return value

    def _items_arg(self, value: Any, who: str) -> List[Any]:
        """The first argument of a built-in that walks a sequence.

        Anything `for`-`in` accepts is accepted here too -- including a struct
        that implements `iter()` -- so the iterator protocol reaches the
        standard library rather than stopping at the loop keyword. The items
        are materialised, which is why the built-ins that use this are the
        eager ones."""
        if not self.is_iterable(value):
            raise MRTRuntimeError(
                f"{who}() needs something iterable, not {type_name(value)}.",
                kind="TypeError",
            )
        return list(self.iterate(value))

    def builtin_map(self, *args):
        """`map(seq, f)` maps anything iterable; `map(gen, f)` stays lazy.

        Matching the input's laziness is what keeps a pipeline over an
        endless generator from hanging: `take(map(naturals(), square), 5)`
        calls `square` five times. On an array, eagerly returning an array
        keeps the common case a plain value you can index and print."""
        if len(args) != 2:
            raise MRTRuntimeError(
                "map() takes a sequence and a function.", kind="ArityError"
            )
        source, function = args
        if isinstance(source, MRTGenerator):
            return MRTGenerator(
                "<generator map>", lambda: self._lazy_map(source, function)
            )
        items = self._items_arg(source, "map")
        return [self.call_value(function, [item]) for item in items]

    def _lazy_map(self, source: Any, function: Any):
        for item in self.iterate(source):
            value = self.call_value(function, [item])
            mine = self.environment
            yield value
            self.environment = mine

    def builtin_filter(self, *args):
        """`filter(seq, pred)` filters anything iterable; `filter(gen, pred)`
        stays lazy (see `map` above).

        A lazy filter over an endless generator only terminates if matches
        keep coming: `take(filter(naturals(), isEven), 3)` is fine, but
        filtering for something that never occurs runs forever -- the same
        bargain any lazy sequence makes."""
        if len(args) != 2:
            raise MRTRuntimeError(
                "filter() takes a sequence and a function.", kind="ArityError"
            )
        source, predicate = args
        if isinstance(source, MRTGenerator):
            return MRTGenerator(
                "<generator filter>", lambda: self._lazy_filter(source, predicate)
            )
        items = self._items_arg(source, "filter")
        return [
            item for item in items if self.is_truthy(self.call_value(predicate, [item]))
        ]

    def _lazy_filter(self, source: Any, predicate: Any):
        for item in self.iterate(source):
            if not self.is_truthy(self.call_value(predicate, [item])):
                continue
            mine = self.environment
            yield item
            self.environment = mine

    def builtin_reduce(self, *args):
        if not 2 <= len(args) <= 3:
            raise MRTRuntimeError(
                "reduce() takes a sequence, a function, and an optional initial value.",
                kind="ArityError",
            )
        items = self._items_arg(args[0], "reduce")
        function = args[1]

        if len(args) == 3:
            accumulator = args[2]
            rest = items
        else:
            if not items:
                raise MRTRuntimeError(
                    "reduce() of an empty sequence needs an initial value.",
                    kind="ValueError",
                )
            accumulator = items[0]
            rest = items[1:]

        for item in rest:
            accumulator = self.call_value(function, [accumulator, item])
        return accumulator

    def builtin_find(self, *args):
        if len(args) != 2:
            raise MRTRuntimeError(
                "find() takes a sequence and a function.", kind="ArityError"
            )
        items = self._items_arg(args[0], "find")
        for item in items:
            if self.is_truthy(self.call_value(args[1], [item])):
                return item
        return None

    def builtin_some(self, *args):
        if len(args) != 2:
            raise MRTRuntimeError(
                "some() takes a sequence and a function.", kind="ArityError"
            )
        items = self._items_arg(args[0], "some")
        return any(self.is_truthy(self.call_value(args[1], [item])) for item in items)

    def builtin_every(self, *args):
        if len(args) != 2:
            raise MRTRuntimeError(
                "every() takes a sequence and a function.", kind="ArityError"
            )
        items = self._items_arg(args[0], "every")
        return all(self.is_truthy(self.call_value(args[1], [item])) for item in items)

    def builtin_toArray(self, *args):
        """Materialise any iterable -- array, string, object keys, or a
        generator -- into an array. On an endless generator this never
        returns, which is why `take` exists."""
        if len(args) != 1:
            raise MRTRuntimeError(
                "toArray() takes exactly one argument.", kind="ArityError"
            )
        return list(self.iterate(args[0]))

    def builtin_take(self, *args):
        """The first `n` items of any iterable, as an array. Safe on an
        endless generator: it stops pulling once it has `n`, and pulls
        exactly `n` -- which matters now that what is left of a generator
        can still be consumed afterwards."""
        if len(args) != 2:
            raise MRTRuntimeError(
                "take() takes an iterable and a count.", kind="ArityError"
            )
        count = int(_number_arg(args[1], "take"))
        if count < 0:
            raise MRTRuntimeError(
                "take() count must not be negative.", kind="ValueError"
            )
        source = self.iterate(args[0])
        out = []
        while len(out) < count:
            try:
                out.append(next(source))
            except StopIteration:
                break
        return out

    def builtin_next(self, *args):
        """One step of a generator, as `{done, value}`."""
        if len(args) != 1:
            raise MRTRuntimeError("next() takes a generator.", kind="ArityError")
        return self._step_result(args[0], None, "next")

    def builtin_send(self, *args):
        """One step of a generator, with `value` becoming the result of the
        `yield` it is suspended at."""
        if len(args) != 2:
            raise MRTRuntimeError(
                "send() takes a generator and a value.", kind="ArityError"
            )
        return self._step_result(args[0], args[1], "send")

    def _step_result(self, generator: Any, sent: Any, who: str) -> Dict[str, Any]:
        """Drive a generator by hand.

        Unlike `for`-`in`, a finished generator is not an error here: it keeps
        answering `{done: true, value: null}`, so the obvious drive loop
        needs no special case at the end.

        The value sent on the very first step is dropped, because the body
        hasn't reached a `yield` yet to receive it."""
        if not isinstance(generator, MRTGenerator):
            raise MRTRuntimeError(
                f"{who}() needs a generator, not {type_name(generator)}.",
                kind="TypeError",
            )
        done, value = generator.step(self, sent)
        return {"done": done, "value": None if done else value}

    def builtin_sort(self, *args):
        """`sort(arr)` or `sort(arr, compare)`. Returns a new array; the
        input is left alone. Without a comparator the array must be all
        numbers or all strings -- there is no cross-type default order.

        The comparator follows the usual convention: negative if `a` sorts
        first, positive if `b` does, zero for a tie. Sorting is stable in
        both interpreters, so equal elements keep their original order."""
        if not 1 <= len(args) <= 2:
            raise MRTRuntimeError(
                "sort() takes an array and an optional compare function.",
                kind="ArityError",
            )
        items = list(self._array_arg(args[0], "sort"))

        if len(args) == 2:
            comparator = args[1]

            def compare(a, b):
                result = self.call_value(comparator, [a, b])
                if not isinstance(result, (int, float)) or isinstance(result, bool):
                    raise MRTRuntimeError(
                        "sort() compare function must return a number.",
                        kind="TypeError",
                    )
                return -1 if result < 0 else (1 if result > 0 else 0)

            return sorted(items, key=cmp_to_key(compare))

        if all(isinstance(i, (int, float)) and not isinstance(i, bool) for i in items):
            return sorted(items)
        if all(isinstance(i, str) for i in items):
            return sorted(items)
        raise MRTRuntimeError(
            "sort() without a compare function needs an array of all numbers or all strings.",
            kind="ValueError",
        )

    def print_function(self, *args):
        """Custom print function that captures output"""
        output_str = " ".join(stringify(arg) for arg in args)
        self.output.append(output_str)
        print(output_str)  # Also print to console for local execution

    def get_output(self):
        """Get captured output for web playground"""
        return "\n".join(self.output)

    def clear_output(self):
        """Clear captured output"""
        self.output = []

    def interpret(self, statements: List[Stmt]):
        try:
            self.clear_output()
            # Reset for this run: an Interpreter can be reused (the web
            # playground keeps one alive across requests), and a stale
            # `True` from a previous program would report every later one
            # as having failed too.
            self.had_error = False
            # Function declarations are registered first (so they can refer
            # to each other in any order), then the remaining top-level
            # statements run in source order -- imports among them, which is
            # why they can no longer be skipped when a main() exists.
            self.run_top_level(statements)

            main_func = None
            try:
                main_token = Token(TokenType.IDENTIFIER, "main", None, 1)
                main_func = self.environment.get(main_token)
            except MRTRuntimeError:
                pass

            if isinstance(main_func, MRTFunction):
                self.entry = ENTRY_RAN
                main_func.call(self, [])
            elif main_func is None:
                self.entry = ENTRY_MISSING
            else:
                # A `main` that is not callable is a different mistake from
                # not having one, and worth saying so: the name is taken.
                self.entry = type_name(main_func)

        except MRTThrow as thrown:
            # Nothing caught it, so it halts the program like any other
            # runtime failure -- but reports the thrown value, since that's
            # what the program chose to say.
            error_msg = f"Runtime Error: Uncaught {stringify(thrown.value)}"
            self.output.append(error_msg)
            self.had_error = True
            print(error_msg, file=sys.stderr)
        except MRTRuntimeError as e:
            error_msg = f"Runtime Error: {e}"
            self.output.append(error_msg)
            self.had_error = True
            print(error_msg, file=sys.stderr)
        except Exception as e:
            error_msg = f"Runtime Error: {str(e)}"
            self.output.append(error_msg)
            self.had_error = True
            print(error_msg, file=sys.stderr)

    def execute(self, stmt: Stmt):
        match stmt:
            case Block():
                self.execute_block(stmt.statements, Environment(self.environment))
            case Break():
                raise BreakSignal()
            case Continue():
                raise ContinueSignal()
            case DestructureAssign():
                self.bind_pattern(
                    stmt.pattern,
                    self.evaluate(stmt.value),
                    self.environment,
                    stmt.token.line,
                    declare=False,
                )
            case Expression():
                self.evaluate(stmt.expression)
            case For():
                self.execute_for(stmt)
            case Function():
                function = MRTFunction(
                    stmt.params,
                    stmt.body,
                    self.environment,
                    stmt.name.lexeme,
                    stmt.is_generator,
                )
                self.environment.define(stmt.name.lexeme, function)
            case Match():
                self.execute_match(stmt)
            case StructDecl():
                methods = {
                    m.name.lexeme: MRTFunction(
                        m.params,
                        m.body,
                        self.environment,
                        m.name.lexeme,
                        m.is_generator,
                    )
                    for m in stmt.methods
                }
                self.environment.define(
                    stmt.name.lexeme, MRTStruct(stmt.name.lexeme, stmt.fields, methods)
                )
            case ForIn():
                self.execute_for_in(stmt)
            case Import():
                self.execute_import(stmt)
            case ExportNames():
                self.execute_export_names(stmt)
            case Export():
                self.execute(stmt.declaration)
                self.current_exports[stmt.name.lexeme] = self.environment.get(stmt.name)
            case Throw():
                raise MRTThrow(self.evaluate(stmt.value))
            case Try():
                self.execute_try(stmt)
            case If():
                if self.is_truthy(self.evaluate(stmt.condition)):
                    self.execute(stmt.then_branch)
                elif stmt.else_branch:
                    self.execute(stmt.else_branch)
            case Print():
                self.print_function(*self.evaluate_spread_list(stmt.expressions))
            case Return():
                value = None
                if stmt.value:
                    value = self.evaluate(stmt.value)
                raise ReturnSignal(value)
            case Var():
                value = None
                if stmt.initializer:
                    value = self.evaluate(stmt.initializer)
                self.bind_pattern(stmt.pattern, value, self.environment)
            case While():
                while self.is_truthy(self.evaluate(stmt.condition)):
                    try:
                        self.execute(stmt.body)
                    except BreakSignal:
                        break
                    except ContinueSignal:
                        continue

    def execute_for(self, stmt: For):
        # Each `for` loop gets its own scope so a `var` in the initializer
        # doesn't leak into the surrounding block.
        previous = self.environment
        self.environment = Environment(previous)
        try:
            if stmt.initializer:
                self.execute(stmt.initializer)

            while stmt.condition is None or self.is_truthy(
                self.evaluate(stmt.condition)
            ):
                try:
                    self.execute(stmt.body)
                except BreakSignal:
                    break
                except ContinueSignal:
                    pass  # fall through to the increment step below

                if stmt.increment is not None:
                    self.evaluate(stmt.increment)
        finally:
            self.environment = previous

    # -- Modules -----------------------------------------------------------

    def resolve_module(self, specifier: str, line: int | None) -> str:
        """Turn an import specifier into an absolute path.

        Only explicitly relative specifiers are supported: there is no search
        path, no implicit extension, and no package directory, so a given
        `import` always names exactly one file and reading the source tells
        you which."""
        if not (specifier.startswith("./") or specifier.startswith("../")):
            raise MRTRuntimeError(
                f"Module path {json.dumps(specifier)} must start with './' or '../'.",
                line,
                kind="ValueError",
            )
        if self.module_path is None:
            raise MRTRuntimeError(
                "Imports need a file to resolve against; run this program from a file.",
                line,
                kind="RuntimeError",
            )
        base = os.path.dirname(self.module_path)
        return os.path.normpath(os.path.join(base, specifier))

    def read_module(self, path: str, specifier: str, line: int | None) -> str:
        if self.module_loader is not None:
            source = self.module_loader(path)
            if source is None:
                raise MRTRuntimeError(
                    f"Cannot find module {json.dumps(specifier)}.",
                    line,
                    kind="ValueError",
                )
            return source
        try:
            with open(path, "r") as handle:
                return handle.read()
        except OSError:
            raise MRTRuntimeError(
                f"Cannot find module {json.dumps(specifier)}.", line, kind="ValueError"
            ) from None

    def load_module(self, specifier: str, line: int | None) -> Dict[str, Any]:
        """Evaluate a module once and return its export table.

        Modules are cached by resolved path, so importing the same file from
        two places runs it once and shares the result -- which matters, since
        a module's top-level code can have side effects."""
        path = self.resolve_module(specifier, line)
        if path in self.module_exports:
            return self.module_exports[path]

        if path in self.module_loading:
            cycle = " -> ".join(
                os.path.basename(p) for p in self.module_loading + [path]
            )
            raise MRTRuntimeError(
                f"Circular import: {cycle}.", line, kind="RuntimeError"
            )

        source = self.read_module(path, specifier, line)

        from .lexer import Lexer as _Lexer
        from .parser import Parser as _Parser

        tokens = _Lexer(source).scan_tokens()
        parser = _Parser(tokens)
        statements = parser.parse()
        if parser.errors:
            raise MRTRuntimeError(
                f"Module {json.dumps(specifier)} has syntax errors: "
                f"{parser.errors[0].message}",
                line,
                kind="RuntimeError",
            )

        previous_env = self.environment
        previous_path = self.module_path
        previous_exports = self.current_exports

        self.module_loading.append(path)
        self.environment = Environment(self.globals)
        self.module_path = path
        self.current_exports = {}
        try:
            self.run_top_level(statements)
            exports = self.current_exports
        finally:
            self.module_loading.pop()
            self.environment = previous_env
            self.module_path = previous_path
            self.current_exports = previous_exports

        self.module_exports[path] = exports
        return exports

    def execute_export_names(self, stmt: ExportNames):
        """`export { a };` re-exports a local name; `export { a } from "..."`
        forwards another module's export without binding it here."""
        if stmt.specifier is not None:
            source = self.load_module(str(stmt.specifier.literal), stmt.keyword.line)
            for local, exported in stmt.names:
                if local.lexeme not in source:
                    raise MRTRuntimeError(
                        f"Module {json.dumps(stmt.specifier.literal)} has no export named "
                        f"'{local.lexeme}'.",
                        local.line,
                        kind="NameError",
                    )
                self.current_exports[exported.lexeme] = source[local.lexeme]
            return

        for local, exported in stmt.names:
            self.current_exports[exported.lexeme] = self.environment.get(local)

    def execute_import(self, stmt: Import):
        exports = self.load_module(str(stmt.specifier.literal), stmt.keyword.line)

        if stmt.namespace is not None:
            # A namespace import binds one ordinary MRT object, so the usual
            # dot access and `keys()` work on it with no new machinery.
            self.environment.define(stmt.namespace.lexeme, dict(exports))
            return

        for exported, local in stmt.names:
            if exported.lexeme not in exports:
                raise MRTRuntimeError(
                    f"Module {json.dumps(stmt.specifier.literal)} has no export named "
                    f"'{exported.lexeme}'.",
                    exported.line,
                    kind="NameError",
                )
            self.environment.define(local.lexeme, exports[exported.lexeme])

    def run_top_level(self, statements: List[Stmt]):
        """Run a file's top-level statements: function and struct
        declarations first, so they can refer to each other regardless of
        order, then everything else in source order."""

        def is_hoisted(statement) -> bool:
            inner = (
                statement.declaration if isinstance(statement, Export) else statement
            )
            return isinstance(inner, (Function, StructDecl))

        for statement in statements:
            if is_hoisted(statement):
                self.execute(statement)

        for statement in statements:
            if not is_hoisted(statement):
                self.execute(statement)

    # -- Generators ----------------------------------------------------------
    #
    # Only *statements* can suspend, because `yield` is a statement. So there
    # is a second, generator-flavoured execution path that mirrors `execute`
    # for the compound statements a `yield` can sit inside, and hands
    # everything else straight to the ordinary `execute`.

    def execute_block_gen(self, statements: List[Stmt], environment: "Environment"):
        with GeneratorScope(self, environment):
            for statement in statements:
                yield from self.execute_gen(statement)

    def execute_gen(self, stmt: Stmt):
        match stmt:
            case Yield():
                yield from self.execute_yield_gen(stmt)
            # The three shapes that can receive a value sent back in. They are
            # matched before the general `Var`/`Expression` handling below,
            # which cannot suspend.
            case Var() if isinstance(stmt.initializer, YieldExpr):
                sent = yield from self.suspend(stmt.initializer)
                self.bind_pattern(stmt.pattern, sent, self.environment)
            case DestructureAssign() if isinstance(stmt.value, YieldExpr):
                sent = yield from self.suspend(stmt.value)
                self.bind_pattern(
                    stmt.pattern, sent, self.environment, stmt.token.line, declare=False
                )
            case Expression(expression=Assign(value=YieldExpr())):
                target = stmt.expression
                sent = yield from self.suspend(target.value)  # type: ignore[attr-defined]
                self.environment.assign(target.name, sent)  # type: ignore[attr-defined]
            case Expression(expression=ArrayAssign(value=YieldExpr())):
                target = stmt.expression
                sent = yield from self.suspend(target.value)  # type: ignore[attr-defined]
                # Re-enter the ordinary index-assignment path with the
                # received value standing in for the `yield`, so the index
                # checks and error messages stay in exactly one place.
                self.evaluate(
                    ArrayAssign(
                        target.array,  # type: ignore[attr-defined]
                        target.index,  # type: ignore[attr-defined]
                        Literal(sent),
                        target.line,  # type: ignore[attr-defined]
                    )
                )
            case Block():
                yield from self.execute_block_gen(
                    stmt.statements, Environment(self.environment)
                )
            case If():
                if self.is_truthy(self.evaluate(stmt.condition)):
                    yield from self.execute_gen(stmt.then_branch)
                elif stmt.else_branch:
                    yield from self.execute_gen(stmt.else_branch)
            case While():
                while self.is_truthy(self.evaluate(stmt.condition)):
                    try:
                        yield from self.execute_gen(stmt.body)
                    except BreakSignal:
                        break
                    except ContinueSignal:
                        continue
            case For():
                with GeneratorScope(self, Environment(self.environment)):
                    if stmt.initializer:
                        self.execute(stmt.initializer)
                    while stmt.condition is None or self.is_truthy(
                        self.evaluate(stmt.condition)
                    ):
                        try:
                            yield from self.execute_gen(stmt.body)
                        except BreakSignal:
                            break
                        except ContinueSignal:
                            pass
                        if stmt.increment is not None:
                            self.evaluate(stmt.increment)
            case ForIn():
                with GeneratorScope(self, self.environment) as scope:
                    for item in self.iterate(
                        self.evaluate(stmt.iterable), stmt.keyword.line
                    ):
                        self.environment = Environment(scope.previous)
                        self.bind_pattern(
                            stmt.pattern, item, self.environment, stmt.keyword.line
                        )
                        try:
                            yield from self.execute_gen(stmt.body)
                        except BreakSignal:
                            break
                        except ContinueSignal:
                            continue
            case Try():
                yield from self.execute_try_gen(stmt)
            case Match():
                yield from self.execute_match_gen(stmt)
            case _:
                # No `yield` can occur here, so ordinary execution is enough.
                self.execute(stmt)

    def execute_yield_gen(self, stmt: Yield):
        """`yield v;` and `yield* other;`."""
        if not stmt.delegate:
            value = self.evaluate(stmt.value)
            # The consumer runs arbitrary code while we are suspended and
            # will leave `self.environment` pointing somewhere else, so the
            # generator re-establishes its own scope on resume.
            mine = self.environment
            yield value
            self.environment = mine
            return

        # `yield* other` re-yields another iterable's items as if they were
        # ours, and passes anything sent in straight through to it, so a
        # chain of delegating generators still behaves like one.
        source = self.iterate(self.evaluate(stmt.value), stmt.keyword.line)
        sent = None
        while True:
            mine = self.environment
            try:
                item = source.send(sent)
            except StopIteration:
                # Caught rather than allowed to escape: a StopIteration that
                # leaves a generator body is a host-language error, not the
                # end of this sequence.
                self.environment = mine
                return
            sent = yield item
            self.environment = mine

    def suspend(self, expr: YieldExpr):
        """Hand `expr`'s value to the consumer and return what it sends back.

        `null` when the consumer is a `for`-`in` loop or `toArray`, which have
        nothing to send -- so a generator written for `send()` still works
        when it is merely iterated."""
        value = self.evaluate(expr.value)
        mine = self.environment
        sent = yield value
        self.environment = mine
        return sent

    def execute_try_gen(self, stmt: Try):
        # `closing` distinguishes "this try block finished" from "this
        # generator was abandoned and is being disposed of". A `finally` runs
        # in the first case and not the second: see GeneratorScope for why
        # running user code at a garbage-collection point is not something a
        # program can rely on, and not something the JavaScript
        # implementation can do at all.
        closing = False
        try:
            try:
                yield from self.execute_gen(stmt.try_block)
            except GeneratorExit:
                closing = True
                raise
            except MRTThrow as thrown:
                handled = yield from self.run_catch_gen(stmt, thrown.value)
                if not handled:
                    raise
            except MRTRuntimeError as e:
                handled = yield from self.run_catch_gen(stmt, make_error_value(e))
                if not handled:
                    raise
        finally:
            if not closing and stmt.finally_block is not None:
                yield from self.execute_gen(stmt.finally_block)

    def run_catch_gen(self, stmt: Try, value: Any):
        for clause in stmt.catches:
            environment = Environment(self.environment)
            self.bind_pattern(clause.pattern, value, environment)

            if clause.guard is not None:
                previous = self.environment
                try:
                    self.environment = environment
                    if not self.is_truthy(self.evaluate(clause.guard)):
                        continue
                finally:
                    self.environment = previous

            yield from self.execute_block_gen(
                clause.block.statements, environment  # type: ignore[attr-defined]
            )
            return True
        return False

    def execute_match_gen(self, stmt: Match):
        subject = self.evaluate(stmt.subject)
        for case in stmt.cases:
            environment = Environment(self.environment)
            if case.pattern is not None:
                if not self.match_pattern(case.pattern, subject, environment):
                    continue
            if case.guard is not None:
                previous = self.environment
                try:
                    self.environment = environment
                    if not self.is_truthy(self.evaluate(case.guard)):
                        continue
                finally:
                    self.environment = previous
            yield from self.execute_block_gen(case.body, environment)
            return
        raise MRTRuntimeError(
            f"No case matched {stringify(subject)} in this match, and there is no 'default'.",
            stmt.keyword.line,
            kind="ValueError",
        )

    def iterate(self, value: Any, line: int | None = None):
        """An iterator over a value's items -- lazily for a generator, from a
        snapshot for the eager containers (so mutating an array mid-loop
        can't shift the iteration underneath it).

        Deliberately an ordinary method returning a generator, rather than a
        generator function itself: "that isn't iterable" is then reported
        when `iterate` is *called*, not on the first item pulled, so
        `take(5, 0)` still says 5 isn't iterable instead of quietly
        answering `[]`.

        Every branch returns a host generator, so callers can `.send()` into
        whatever comes back without checking what it was."""
        if isinstance(value, MRTGenerator):
            if value.done:
                raise MRTRuntimeError(
                    f"{value} has already been iterated; a generator can only be "
                    f"used once.",
                    line,
                    kind="ValueError",
                )
            return self._drive_generator(value, line)

        if isinstance(value, MRTInstance) and "iter" in value.struct.methods:
            # The iterator protocol: a struct says how to iterate itself by
            # declaring `iter()`, which returns anything else iterable --
            # usually a generator, but an array works just as well.
            sequence = self.call_value(value.get("iter", self, line), [], line)
            if sequence is value:
                raise MRTRuntimeError(
                    f"{value.struct.name}.iter() returned the struct itself, "
                    f"which would iterate forever.",
                    line,
                    kind="ValueError",
                )
            if not self.is_iterable(sequence):
                # Named rather than left to the generic message below: the
                # mistake is in the struct's `iter`, which may be a long way
                # from the `for` loop that tripped over it.
                raise MRTRuntimeError(
                    f"{value.struct.name}.iter() returned {type_name(sequence)}, "
                    f"which isn't iterable.",
                    line,
                    kind="TypeError",
                )
            return self.iterate(sequence, line)

        if isinstance(value, list):
            return self._drive_items(list(value))
        if isinstance(value, str):
            return self._drive_items(list(value))
        source = object_like(value)
        if source is not None:
            return self._drive_items([load_key(k) for k in source])

        raise MRTRuntimeError(
            "Can only iterate over an array, string, object, or generator.",
            line,
            kind="TypeError",
        )

    def is_iterable(self, value: Any) -> bool:
        return (
            isinstance(value, (MRTGenerator, list, str))
            or object_like(value) is not None
        )

    def _drive_generator(self, generator: MRTGenerator, line: int | None = None):
        """Pull from an MRT generator, forwarding values sent in by whoever
        is consuming this iterator (that's how `yield*` stays two-way)."""
        sent = None
        while True:
            done, item = generator.step(self, sent, line)
            if done:
                return
            sent = yield item

    def _drive_items(self, items: List[Any]):
        """The same shape for an already-materialised sequence. Anything sent
        in has nowhere to go and is dropped."""
        for item in items:
            yield item

    # -- match --------------------------------------------------------------

    def execute_match(self, stmt: Match):
        subject = self.evaluate(stmt.subject)

        for case in stmt.cases:
            environment = Environment(self.environment)

            if case.pattern is not None:
                if not self.match_pattern(case.pattern, subject, environment):
                    continue

            if case.guard is not None:
                previous = self.environment
                try:
                    self.environment = environment
                    if not self.is_truthy(self.evaluate(case.guard)):
                        continue
                finally:
                    self.environment = previous

            self.execute_block(case.body, environment)
            return

        raise MRTRuntimeError(
            f"No case matched {stringify(subject)} in this match, and there is no 'default'.",
            stmt.keyword.line,
            kind="ValueError",
        )

    def evaluate_match_expr(self, expr: MatchExpr) -> Any:
        """The expression form of `match`: the first arm that fits decides the
        value. Shares `match_pattern` with the statement form, so the two
        spellings can never disagree about what a pattern means."""
        subject = self.evaluate(expr.subject)

        for arm in expr.arms:
            environment = Environment(self.environment)

            if arm.pattern is not None:
                if not self.match_pattern(arm.pattern, subject, environment):
                    continue

            previous = self.environment
            try:
                self.environment = environment
                if arm.guard is not None and not self.is_truthy(
                    self.evaluate(arm.guard)
                ):
                    continue
                return self.evaluate(arm.value)
            finally:
                self.environment = previous

        raise MRTRuntimeError(
            f"No case matched {stringify(subject)} in this match, and there is "
            f"no 'default'.",
            expr.keyword.line,
            kind="ValueError",
        )

    def match_pattern(
        self, pattern: MatchPattern, value: Any, environment: "Environment"
    ) -> bool:
        """Test `value` against `pattern`, binding names into `environment`.

        Returns False instead of raising when the shape doesn't fit -- that's
        the whole point of matching. Bindings made by a partially successful
        match are left in `environment`, which is discarded by the caller when
        the overall match fails."""
        if isinstance(pattern, LiteralMatch):
            return values_equal(value, pattern.value)

        if isinstance(pattern, BindMatch):
            environment.define(pattern.name.lexeme, value)
            return True

        if isinstance(pattern, ArrayMatch):
            if not isinstance(value, list):
                return False
            if pattern.rest is None:
                if len(value) != len(pattern.elements):
                    return False
            elif len(value) < len(pattern.elements):
                return False
            for element, item in zip(pattern.elements, value):
                if not self.match_pattern(element, item, environment):
                    return False
            if pattern.rest is not None:
                environment.define(
                    pattern.rest.lexeme, list(value[len(pattern.elements) :])
                )
            return True

        if isinstance(pattern, ObjectMatch):
            source = object_like(value)
            if source is None:
                return False
            for key, sub in pattern.entries:
                if key not in source:
                    return False
                if not self.match_pattern(sub, source[key], environment):
                    return False
            return True

        if isinstance(pattern, StructMatch):
            struct = self.environment.get(pattern.name)
            if not isinstance(struct, MRTStruct):
                raise MRTRuntimeError(
                    f"'{pattern.name.lexeme}' is not a struct, so it can't be used as a "
                    f"pattern.",
                    pattern.name.line,
                    kind="TypeError",
                )
            if len(pattern.elements) != len(struct.fields):
                raise MRTRuntimeError(
                    f"Pattern for struct {struct.name} has {len(pattern.elements)} field(s) "
                    f"but the struct declares {len(struct.fields)}.",
                    pattern.name.line,
                    kind="ArityError",
                )
            if not isinstance(value, MRTInstance) or value.struct is not struct:
                return False
            for element, field in zip(pattern.elements, struct.field_names()):
                if not self.match_pattern(element, value.values[field], environment):
                    return False
            return True

        raise MRTRuntimeError("Unknown match pattern.", kind="RuntimeError")

    # -- Binding patterns ---------------------------------------------------

    def bind_pattern(
        self,
        pattern: Pattern,
        value: Any,
        environment: "Environment",
        line: int | None = None,
        declare: bool = True,
    ):
        """Bind `value` to `pattern` inside `environment`.

        Destructuring is strict, like the rest of the language: a missing
        element or key is an error unless that slot has a default, rather
        than quietly binding `null`.

        With `declare=False` the leaves are *assigned* to variables that must
        already exist, which is what `[a, b] = pair;` means. Everything else
        about the pattern -- nesting, rest, defaults, the error messages --
        is identical, so the two forms can never drift apart."""
        if value is MISSING:
            if pattern.default is None:
                raise MRTRuntimeError(
                    "Cannot destructure: no value for this part of the pattern.",
                    line,
                    kind="ValueError",
                )
            value = self.evaluate(pattern.default)

        if isinstance(pattern, NamePattern):
            self._bind_name(pattern.name, value, environment, declare)
            return

        if isinstance(pattern, ArrayPattern):
            where = pattern.token.line if pattern.token else line
            if not isinstance(value, list):
                raise MRTRuntimeError(
                    f"Cannot destructure {type_name(value)} with an array pattern.",
                    where,
                    kind="TypeError",
                )
            for index, element in enumerate(pattern.elements):
                slot = value[index] if index < len(value) else MISSING
                if slot is MISSING and element.default is None:
                    raise MRTRuntimeError(
                        f"Cannot destructure: the array has {len(value)} element(s) "
                        f"but the pattern needs at least {len(pattern.elements)}.",
                        where,
                        kind="IndexError",
                    )
                self.bind_pattern(element, slot, environment, where, declare)
            if pattern.rest is not None:
                self._bind_name(
                    pattern.rest,
                    list(value[len(pattern.elements) :]),
                    environment,
                    declare,
                )
            return

        if isinstance(pattern, ObjectPattern):
            where = pattern.token.line if pattern.token else line
            source = object_like(value)
            if source is None:
                raise MRTRuntimeError(
                    f"Cannot destructure {type_name(value)} with an object pattern.",
                    where,
                    kind="TypeError",
                )
            taken = set()
            for key, sub in pattern.entries:
                taken.add(key)
                slot = source[key] if key in source else MISSING
                if slot is MISSING and sub.default is None:
                    raise MRTRuntimeError(
                        f"Cannot destructure: no key {json.dumps(key)} in the object.",
                        where,
                        kind="KeyError",
                    )
                self.bind_pattern(sub, slot, environment, where, declare)
            if pattern.rest is not None:
                self._bind_name(
                    pattern.rest,
                    {k: v for k, v in source.items() if k not in taken},
                    environment,
                    declare,
                )
            return

        raise MRTRuntimeError("Unknown binding pattern.", line, kind="RuntimeError")

    def _bind_name(
        self, name: Token, value: Any, environment: "Environment", declare: bool
    ):
        if declare:
            environment.define(name.lexeme, value)
        else:
            environment.assign(name, value)

    def execute_for_in(self, stmt: ForIn):
        iterable = self.evaluate(stmt.iterable)

        previous = self.environment
        try:
            for item in self.iterate(iterable, stmt.keyword.line):
                # A fresh scope per iteration, so a closure made in the body
                # captures this item rather than sharing one slot with every
                # other iteration.
                self.environment = Environment(previous)
                self.bind_pattern(
                    stmt.pattern, item, self.environment, stmt.keyword.line
                )
                try:
                    self.execute(stmt.body)
                except BreakSignal:
                    break
                except ContinueSignal:
                    continue
        finally:
            self.environment = previous

    def execute_try(self, stmt: Try):
        try:
            try:
                self.execute(stmt.try_block)
            except MRTThrow as thrown:
                if not self.run_catch(stmt, thrown.value):
                    raise
            except MRTRuntimeError as e:
                # An interpreter-raised failure (bad index, division by
                # zero, ...) is catchable too: it reaches the program as the
                # standard error object, carrying `kind` for guards to
                # branch on.
                if not self.run_catch(stmt, make_error_value(e)):
                    raise
        finally:
            # Runs on every path out of the try -- normal completion, a
            # caught or uncaught throw, and a return/break/continue
            # unwinding through it.
            if stmt.finally_block is not None:
                self.execute(stmt.finally_block)

    def run_catch(self, stmt: Try, value: Any) -> bool:
        """Run the first `catch` clause that matches, returning whether one
        did. A clause with no guard always matches; a guarded one is tried
        with the error already bound, so the guard can inspect it. If none
        match, the error keeps propagating (and `finally` still runs)."""
        for clause in stmt.catches:
            environment = Environment(self.environment)
            self.bind_pattern(clause.pattern, value, environment)

            if clause.guard is not None:
                previous = self.environment
                try:
                    self.environment = environment
                    if not self.is_truthy(self.evaluate(clause.guard)):
                        continue
                finally:
                    self.environment = previous

            self.execute_block(clause.block.statements, environment)  # type: ignore[attr-defined]
            return True
        return False

    def execute_block(self, statements: List[Stmt], environment: Environment):
        previous = self.environment
        try:
            self.environment = environment
            for statement in statements:
                self.execute(statement)
        finally:
            self.environment = previous

    def evaluate(self, expr: Expr) -> Any:
        match expr:
            case Array():
                return self.evaluate_spread_list(expr.elements)
            case ArrayAccess():
                return self.evaluate_index_get(expr)
            case ArrayAssign():
                return self.evaluate_index_set(expr)
            case Assign():
                value = self.evaluate(expr.value)
                self.environment.assign(expr.name, value)
                return value
            case Binary():
                return self.evaluate_binary(expr)
            case DictLiteral():
                result: Dict[Any, Any] = {}
                for key_expr, value_expr in expr.pairs:
                    key = self.evaluate(key_expr)
                    self._check_hashable_key(key, expr.line)
                    result[store_key(key)] = self.evaluate(value_expr)
                return result
            case Call():
                callee = self.evaluate(expr.callee)
                arguments = self.evaluate_spread_list(expr.arguments)
                return self.call_value(callee, arguments, expr.paren.line)
            case Spread():
                raise MRTRuntimeError(
                    "'...' is only allowed in a call's arguments or an array literal.",
                    expr.token.line,
                    kind="TypeError",
                )
            case FunctionExpr():
                return MRTFunction(
                    expr.params,
                    expr.body,
                    self.environment,
                    expr.name.lexeme if expr.name else None,
                    expr.is_generator,
                )
            case Interpolation():
                out = []
                for part in expr.parts:
                    out.append(
                        part
                        if isinstance(part, str)
                        else stringify(self.evaluate(part))
                    )
                return "".join(out)
            case MatchExpr():
                return self.evaluate_match_expr(expr)
            case YieldExpr():
                # Unreachable through the parser, which only builds one where
                # `execute_gen` handles it. Kept so a future node that holds
                # an expression without knowing about yield fails loudly
                # instead of returning None.
                raise MRTRuntimeError(
                    "'yield' can only be a statement of its own, or the entire "
                    "right-hand side of a declaration or an assignment.",
                    expr.keyword.line,
                    kind="RuntimeError",
                )
            case Grouping():
                return self.evaluate(expr.expression)
            case Literal():
                return expr.value
            case Logical():
                left = self.evaluate(expr.left)
                if expr.operator.type == TokenType.OR:
                    if self.is_truthy(left):
                        return left
                else:  # AND
                    if not self.is_truthy(left):
                        return left
                return self.evaluate(expr.right)
            case Unary():
                right = self.evaluate(expr.right)

                if expr.operator.type == TokenType.MINUS:
                    if not isinstance(right, (int, float)) or isinstance(right, bool):
                        raise MRTRuntimeError(
                            "Operand of '-' must be a number.",
                            expr.operator.line,
                            kind="TypeError",
                        )
                    return -float(right)
                if expr.operator.type == TokenType.NOT:
                    return not self.is_truthy(right)
            case Variable():
                return self.environment.get(expr.name)

    def evaluate_spread_list(self, items: List[Expr]) -> List[Any]:
        """Evaluate an argument list or array literal, splicing `...expr`
        elements in place. Spreading anything but an array is an error --
        there is no implicit iteration of strings or objects here, which
        keeps `f(...x)` from silently meaning something different depending
        on what `x` happens to hold."""
        values: List[Any] = []
        for item in items:
            if isinstance(item, Spread):
                spread = self.evaluate(item.value)
                if not isinstance(spread, list):
                    raise MRTRuntimeError(
                        "Can only spread an array with '...'.",
                        item.token.line,
                        kind="TypeError",
                    )
                values.extend(spread)
            else:
                values.append(self.evaluate(item))
        return values

    def call_value(
        self, callee: Any, arguments: List[Any], line: int | None = None
    ) -> Any:
        """Invoke an MRT value with arguments. Shared by the `Call`
        expression and by the higher-order built-ins (map, filter, sort,
        ...), which need to call back into user code."""
        if isinstance(callee, MRTStruct):
            if not callee.accepts(len(arguments)):
                raise MRTRuntimeError(
                    f"Struct {callee.name} takes {callee.arity_description()} "
                    f"field values but got {len(arguments)}.",
                    line,
                    kind="ArityError",
                )
            return callee.construct(self, arguments)

        if isinstance(callee, MRTFunction):
            if not callee.accepts(len(arguments)):
                raise MRTRuntimeError(
                    f"Expected {callee.arity_description()} arguments "
                    f"but got {len(arguments)}.",
                    line,
                    kind="ArityError",
                )
            return callee.call(self, arguments)

        if callable(callee):
            try:
                return callee(*arguments)
            except MRTRuntimeError as e:
                # Re-raised only to attach a call-site line; the original
                # classification and any stack collected so far are the
                # error's own and must survive.
                located = MRTRuntimeError(
                    e.message, e.line if e.line is not None else line, kind=e.kind
                )
                located.mrt_stack = e.mrt_stack
                raise located from None
            except TypeError as e:
                raise MRTRuntimeError(
                    f"Invalid arguments in call: {e}", line, kind="ArityError"
                ) from None

        raise MRTRuntimeError("Can only call functions.", line, kind="TypeError")

    def evaluate_index_get(self, expr: ArrayAccess) -> Any:
        target = self.evaluate(expr.array)
        index = self.evaluate(expr.index)
        line = expr.line

        if isinstance(target, MRTInstance):
            if not isinstance(index, str):
                raise MRTRuntimeError(
                    "A struct field name must be a string.", line, kind="TypeError"
                )
            return target.get(index, self)

        if isinstance(target, dict):
            self._check_hashable_key(index, line)
            key = store_key(index)
            if key not in target:
                # json.dumps, not !r: the Playground formats this message with
                # JSON.stringify, and Python's repr would quote with
                # apostrophes (and escape differently), so a program that
                # prints a caught e.message would see two different texts.
                raise MRTRuntimeError(
                    f"Key {json.dumps(stringify(index))} not found in object.",
                    line,
                    kind="KeyError",
                )
            return target[key]

        if isinstance(target, list):
            i = self._require_array_index(index, len(target), line)
            return target[i]

        if isinstance(target, str):
            i = self._require_array_index(index, len(target), line)
            return target[i]

        raise MRTRuntimeError(
            "Can only index into arrays, objects, or strings.", line, kind="TypeError"
        )

    def evaluate_index_set(self, expr: ArrayAssign) -> Any:
        target = self.evaluate(expr.array)
        index = self.evaluate(expr.index)
        value = self.evaluate(expr.value)

        line = expr.line

        if isinstance(target, MRTInstance):
            if not isinstance(index, str):
                raise MRTRuntimeError(
                    "A struct field name must be a string.", line, kind="TypeError"
                )
            target.set(index, value)
            return value

        if isinstance(target, dict):
            self._check_hashable_key(index, line)
            target[store_key(index)] = value
            return value

        if isinstance(target, list):
            i = self._require_array_index(index, len(target), line)
            target[i] = value
            return value

        if isinstance(target, str):
            raise MRTRuntimeError(
                "Strings are immutable; cannot assign to a character index.",
                line,
                kind="TypeError",
            )

        raise MRTRuntimeError(
            "Can only assign into arrays or objects.", line, kind="TypeError"
        )

    def _require_array_index(self, index: Any, length: int, line: int) -> int:
        if not isinstance(index, (int, float)) or isinstance(index, bool):
            raise MRTRuntimeError(
                "Array index must be a number.", line, kind="IndexError"
            )
        i = int(index)
        if i < 0 or i >= length:
            raise MRTRuntimeError(
                f"Array index {i} out of bounds for array of length {length}.",
                line,
                kind="IndexError",
            )
        return i

    def _check_hashable_key(self, key: Any, line: int):
        if isinstance(key, (list, dict)):
            raise MRTRuntimeError(
                "Object keys must be numbers, strings, or booleans (not arrays or objects).",
                line,
                kind="TypeError",
            )

    def evaluate_binary(self, expr: Binary) -> Any:
        left = self.evaluate(expr.left)
        right = self.evaluate(expr.right)
        op = expr.operator.type
        line = expr.operator.line

        if op == TokenType.PLUS:
            # Handle string concatenation
            if isinstance(left, str) or isinstance(right, str):
                return stringify(left) + stringify(right)
            self._check_numbers(left, right, "+", line)
            return float(left) + float(right)
        if op == TokenType.MINUS:
            self._check_numbers(left, right, "-", line)
            return float(left) - float(right)
        if op == TokenType.MULTIPLY:
            self._check_numbers(left, right, "*", line)
            return float(left) * float(right)
        if op == TokenType.DIVIDE:
            self._check_numbers(left, right, "/", line)
            if float(right) == 0:
                raise MRTRuntimeError("Division by zero.", line, kind="ArithmeticError")
            return float(left) / float(right)
        if op == TokenType.MODULO:
            self._check_numbers(left, right, "%", line)
            if float(right) == 0:
                raise MRTRuntimeError("Modulo by zero.", line, kind="ArithmeticError")
            # The result takes the sign of the *dividend*, as `%` does in C,
            # Java, JavaScript, Rust and Go. Python's own `%` takes the sign
            # of the divisor instead, which made `-7 % 3` evaluate to 2 here
            # and -1 in the Playground -- a silent disagreement about a core
            # operator. MRT's stated design goal is C/JS-family syntax, so
            # the C/JS answer is the right one and this is the odd one out.
            return math.fmod(float(left), float(right))
        if op == TokenType.EQUALS:
            return self.is_equal(left, right)
        if op == TokenType.NOT_EQUALS:
            return not self.is_equal(left, right)
        if op == TokenType.GREATER:
            return self._compare(left, right, line) > 0
        if op == TokenType.GREATER_EQUAL:
            return self._compare(left, right, line) >= 0
        if op == TokenType.LESS:
            return self._compare(left, right, line) < 0
        if op == TokenType.LESS_EQUAL:
            return self._compare(left, right, line) <= 0

        raise MRTRuntimeError(
            f"Unknown binary operator '{expr.operator.lexeme}'.",
            line,
            kind="RuntimeError",
        )

    def _check_numbers(self, left: Any, right: Any, op: str, line: int):
        def is_number(v):
            return isinstance(v, (int, float)) and not isinstance(v, bool)

        if not is_number(left) or not is_number(right):
            raise MRTRuntimeError(
                f"Operands of '{op}' must be numbers.", line, kind="TypeError"
            )

    def _compare(self, left: Any, right: Any, line: int) -> int:
        """Return a negative/zero/positive int comparing left to right.
        Numbers compare numerically; strings compare lexicographically."""
        if isinstance(left, str) and isinstance(right, str):
            return -1 if left < right else (1 if left > right else 0)
        if (
            isinstance(left, (int, float))
            and isinstance(right, (int, float))
            and not isinstance(left, bool)
            and not isinstance(right, bool)
        ):
            fl, fr = float(left), float(right)
            return -1 if fl < fr else (1 if fl > fr else 0)
        raise MRTRuntimeError(
            "Comparison operators require two numbers or two strings.",
            line,
            kind="TypeError",
        )

    def is_equal(self, a: Any, b: Any) -> bool:
        return values_equal(a, b)

    def is_truthy(self, obj: Any) -> bool:
        if obj is None:
            return False
        if isinstance(obj, bool):
            return obj
        return True
