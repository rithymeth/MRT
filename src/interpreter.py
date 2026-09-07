import math
from functools import cmp_to_key
from typing import Any, Dict, List, Optional
from .ast import *
from .lexer import Token, TokenType
from .errors import MRTRuntimeError


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
        return "{" + ", ".join(f"{stringify(k)}: {stringify(v)}" for k, v in value.items()) + "}"
    if callable(value):
        # A built-in (a host-language function, not an MRTFunction, which
        # has its own __str__). Without this, Python would render it as
        # "<function MRTBuiltin.len at 0x7f...>" -- leaking the host
        # implementation, embedding a memory address that changes run to
        # run, and disagreeing with the Playground, which would print the
        # JavaScript source text instead.
        return "<builtin>"
    return str(value)


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
    if isinstance(a, list) != isinstance(b, list):
        return False
    if isinstance(a, dict) != isinstance(b, dict):
        return False
    return a == b


class MRTFunction:
    """A callable closure. Built from either a `func name(...)` declaration
    or an anonymous `func(...)` expression -- the two are the same thing at
    runtime, differing only in whether `name` is set."""

    def __init__(self, params: List[Token], body: List[Stmt],
                 closure: 'Environment', name: Optional[str] = None):
        self.params = params
        self.body = body
        self.closure = closure
        self.name = name

    def call(self, interpreter: 'Interpreter', arguments: List[Any]) -> Any:
        environment = Environment(self.closure)
        for param, arg in zip(self.params, arguments):
            environment.define(param.lexeme, arg)

        try:
            interpreter.execute_block(self.body, environment)
            return None
        except ReturnSignal as return_value:
            return return_value.value

    def __str__(self):
        return f"<function {self.name}>" if self.name else "<function>"

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
        super().__init__()


def make_error_value(message: str, line: Optional[int]) -> Dict[str, Any]:
    """The object a `catch` block receives for an interpreter-raised error:
    `{"message": ..., "line": ...}`. Programs can throw any value they like,
    but built-in failures always arrive in this shape."""
    return {"message": message, "line": float(line) if line is not None else None}

class BreakSignal(Exception):
    pass

class ContinueSignal(Exception):
    pass

class Environment:
    def __init__(self, enclosing: Optional['Environment'] = None):
        self.values: Dict[str, Any] = {}
        self.enclosing = enclosing

    def define(self, name: str, value: Any):
        self.values[name] = value

    def get(self, name: Token) -> Any:
        if name.lexeme in self.values:
            return self.values[name.lexeme]

        if self.enclosing:
            return self.enclosing.get(name)

        raise MRTRuntimeError(f"Undefined variable '{name.lexeme}'.", name.line)

    def assign(self, name: Token, value: Any):
        if name.lexeme in self.values:
            self.values[name.lexeme] = value
            return

        if self.enclosing:
            self.enclosing.assign(name, value)
            return

        raise MRTRuntimeError(f"Undefined variable '{name.lexeme}'.", name.line)

class MRTBuiltin:
    @staticmethod
    def len(*args):
        if len(args) != 1:
            raise MRTRuntimeError("len() takes exactly one argument.")
        if isinstance(args[0], (str, list, dict)):
            return float(len(args[0]))
        raise MRTRuntimeError("len() argument must be an array, object, or string.")

    @staticmethod
    def push(*args):
        if len(args) != 2:
            raise MRTRuntimeError("push() takes exactly two arguments.")
        if not isinstance(args[0], list):
            raise MRTRuntimeError("First argument to push() must be an array.")
        args[0].append(args[1])
        return args[1]

    @staticmethod
    def pop(*args):
        if len(args) != 1:
            raise MRTRuntimeError("pop() takes exactly one argument.")
        if not isinstance(args[0], list):
            raise MRTRuntimeError("pop() argument must be an array.")
        if not args[0]:
            raise MRTRuntimeError("Cannot pop from empty array.")
        return args[0].pop()

    @staticmethod
    def slice(*args):
        if len(args) not in [2, 3]:
            raise MRTRuntimeError("slice() takes 2 or 3 arguments.")
        if not isinstance(args[0], list):
            raise MRTRuntimeError("First argument to slice() must be an array.")

        arr = args[0]
        start = int(args[1]) if isinstance(args[1], (int, float)) else 0
        end = int(args[2]) if len(args) > 2 and isinstance(args[2], (int, float)) else len(arr)

        if start < 0:
            start = len(arr) + start
        if end < 0:
            end = len(arr) + end

        return arr[start:end]

    @staticmethod
    def join(*args):
        if len(args) not in [1, 2]:
            raise MRTRuntimeError("join() takes 1 or 2 arguments.")
        if not isinstance(args[0], list):
            raise MRTRuntimeError("First argument to join() must be an array.")

        separator = str(args[1]) if len(args) > 1 else ""
        return separator.join(stringify(x) for x in args[0])

    @staticmethod
    def indexOf(*args):
        if len(args) != 2:
            raise MRTRuntimeError("indexOf() takes exactly 2 arguments.")
        if not isinstance(args[0], list):
            raise MRTRuntimeError("First argument to indexOf() must be an array.")

        for i, item in enumerate(args[0]):
            if values_equal(item, args[1]):
                return float(i)
        return -1.0

    @staticmethod
    def split(*args):
        if len(args) not in [1, 2]:
            raise MRTRuntimeError("split() takes 1 or 2 arguments.")
        if not isinstance(args[0], str):
            raise MRTRuntimeError("First argument to split() must be a string.")

        separator = str(args[1]) if len(args) > 1 else " "
        return args[0].split(separator)

    @staticmethod
    def substring(*args):
        if len(args) not in [2, 3]:
            raise MRTRuntimeError("substring() takes 2 or 3 arguments.")
        if not isinstance(args[0], str):
            raise MRTRuntimeError("First argument to substring() must be a string.")

        text = args[0]
        start = int(args[1]) if isinstance(args[1], (int, float)) else 0
        end = int(args[2]) if len(args) > 2 and isinstance(args[2], (int, float)) else len(text)

        if start < 0:
            start = len(text) + start
        if end < 0:
            end = len(text) + end

        return text[start:end]

    @staticmethod
    def toUpper(*args):
        if len(args) != 1:
            raise MRTRuntimeError("toUpper() takes exactly one argument.")
        if not isinstance(args[0], str):
            raise MRTRuntimeError("toUpper() argument must be a string.")
        return args[0].upper()

    @staticmethod
    def toLower(*args):
        if len(args) != 1:
            raise MRTRuntimeError("toLower() takes exactly one argument.")
        if not isinstance(args[0], str):
            raise MRTRuntimeError("toLower() argument must be a string.")
        return args[0].lower()

    @staticmethod
    def trim(*args):
        if len(args) != 1:
            raise MRTRuntimeError("trim() takes exactly one argument.")
        if not isinstance(args[0], str):
            raise MRTRuntimeError("trim() argument must be a string.")
        return args[0].strip()

    @staticmethod
    def replace(*args):
        if len(args) != 3:
            raise MRTRuntimeError("replace() takes exactly 3 arguments.")
        if not isinstance(args[0], str):
            raise MRTRuntimeError("First argument to replace() must be a string.")
        return str(args[0]).replace(str(args[1]), str(args[2]))

    @staticmethod
    def startsWith(*args):
        if len(args) != 2:
            raise MRTRuntimeError("startsWith() takes exactly 2 arguments.")
        if not isinstance(args[0], str):
            raise MRTRuntimeError("First argument to startsWith() must be a string.")
        return args[0].startswith(str(args[1]))

    @staticmethod
    def endsWith(*args):
        if len(args) != 2:
            raise MRTRuntimeError("endsWith() takes exactly 2 arguments.")
        if not isinstance(args[0], str):
            raise MRTRuntimeError("First argument to endsWith() must be a string.")
        return args[0].endswith(str(args[1]))

    @staticmethod
    def contains(*args):
        if len(args) != 2:
            raise MRTRuntimeError("contains() takes exactly 2 arguments.")
        if not isinstance(args[0], str):
            raise MRTRuntimeError("First argument to contains() must be a string.")
        return str(args[1]) in args[0]

    # -- Type / conversion -------------------------------------------------

    @staticmethod
    def type_(*args):
        if len(args) != 1:
            raise MRTRuntimeError("type() takes exactly one argument.")
        value = args[0]
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
        if isinstance(value, MRTFunction) or callable(value):
            return "function"
        return "unknown"

    @staticmethod
    def toNumber(*args):
        if len(args) != 1:
            raise MRTRuntimeError("toNumber() takes exactly one argument.")
        value = args[0]
        if isinstance(value, bool):
            return 1.0 if value else 0.0
        if isinstance(value, (int, float)):
            return float(value)
        if isinstance(value, str):
            try:
                return float(value.strip())
            except ValueError:
                raise MRTRuntimeError(f"Cannot convert '{value}' to a number.")
        raise MRTRuntimeError("toNumber() argument must be a string, number, or boolean.")

    @staticmethod
    def toString(*args):
        if len(args) != 1:
            raise MRTRuntimeError("toString() takes exactly one argument.")
        return stringify(args[0])

    # -- Math ----------------------------------------------------------------

    @staticmethod
    def _num(value, who: str) -> float:
        if not isinstance(value, (int, float)) or isinstance(value, bool):
            raise MRTRuntimeError(f"{who}() argument must be a number.")
        return float(value)

    @staticmethod
    def abs_(*args):
        if len(args) != 1:
            raise MRTRuntimeError("abs() takes exactly one argument.")
        return abs(MRTBuiltin._num(args[0], "abs"))

    @staticmethod
    def min_(*args):
        values = args[0] if len(args) == 1 and isinstance(args[0], list) else list(args)
        if not values:
            raise MRTRuntimeError("min() requires at least one argument.")
        return min(MRTBuiltin._num(v, "min") for v in values)

    @staticmethod
    def max_(*args):
        values = args[0] if len(args) == 1 and isinstance(args[0], list) else list(args)
        if not values:
            raise MRTRuntimeError("max() requires at least one argument.")
        return max(MRTBuiltin._num(v, "max") for v in values)

    @staticmethod
    def round_(*args):
        if len(args) not in (1, 2):
            raise MRTRuntimeError("round() takes 1 or 2 arguments.")
        value = MRTBuiltin._num(args[0], "round")
        digits = int(MRTBuiltin._num(args[1], "round")) if len(args) == 2 else 0
        return float(round(value, digits))

    @staticmethod
    def floor(*args):
        if len(args) != 1:
            raise MRTRuntimeError("floor() takes exactly one argument.")
        return float(math.floor(MRTBuiltin._num(args[0], "floor")))

    @staticmethod
    def ceil(*args):
        if len(args) != 1:
            raise MRTRuntimeError("ceil() takes exactly one argument.")
        return float(math.ceil(MRTBuiltin._num(args[0], "ceil")))

    @staticmethod
    def sqrt(*args):
        if len(args) != 1:
            raise MRTRuntimeError("sqrt() takes exactly one argument.")
        value = MRTBuiltin._num(args[0], "sqrt")
        if value < 0:
            raise MRTRuntimeError("sqrt() argument must not be negative.")
        return math.sqrt(value)

    @staticmethod
    def pow_(*args):
        if len(args) != 2:
            raise MRTRuntimeError("pow() takes exactly 2 arguments.")
        return MRTBuiltin._num(args[0], "pow") ** MRTBuiltin._num(args[1], "pow")

    # -- Objects (dicts) -----------------------------------------------------

    @staticmethod
    def keys(*args):
        if len(args) != 1 or not isinstance(args[0], dict):
            raise MRTRuntimeError("keys() takes exactly one object argument.")
        return list(args[0].keys())

    @staticmethod
    def values(*args):
        if len(args) != 1 or not isinstance(args[0], dict):
            raise MRTRuntimeError("values() takes exactly one object argument.")
        return list(args[0].values())

    @staticmethod
    def has(*args):
        if len(args) != 2:
            raise MRTRuntimeError("has() takes exactly 2 arguments.")
        container, key = args
        if isinstance(container, dict):
            return key in container
        if isinstance(container, list):
            return any(values_equal(item, key) for item in container)
        raise MRTRuntimeError("First argument to has() must be an array or object.")

    @staticmethod
    def get(*args):
        if len(args) not in (2, 3):
            raise MRTRuntimeError("get() takes 2 or 3 arguments.")
        container, key = args[0], args[1]
        default = args[2] if len(args) == 3 else None
        if isinstance(container, dict):
            return container.get(key, default)
        if isinstance(container, list):
            if isinstance(key, (int, float)) and not isinstance(key, bool):
                i = int(key)
                if 0 <= i < len(container):
                    return container[i]
            return default
        raise MRTRuntimeError("First argument to get() must be an array or object.")

    # -- Sequences ---------------------------------------------------------

    @staticmethod
    def reverse(*args):
        if len(args) != 1:
            raise MRTRuntimeError("reverse() takes exactly one argument.")
        if isinstance(args[0], str):
            return args[0][::-1]
        if isinstance(args[0], list):
            return list(reversed(args[0]))
        raise MRTRuntimeError("reverse() argument must be an array or string.")

    @staticmethod
    def unique(*args):
        if len(args) != 1 or not isinstance(args[0], list):
            raise MRTRuntimeError("unique() takes exactly one array argument.")
        result = []
        for item in args[0]:
            if not any(values_equal(item, seen) for seen in result):
                result.append(item)
        return result

    @staticmethod
    def flatten(*args):
        if not 1 <= len(args) <= 2 or not isinstance(args[0], list):
            raise MRTRuntimeError("flatten() takes an array and an optional depth.")
        depth = 1 if len(args) == 1 else int(_number_arg(args[1], "flatten"))
        if depth < 0:
            raise MRTRuntimeError("flatten() depth must not be negative.")

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
        if len(args) != 2 or not isinstance(args[0], list) or not isinstance(args[1], list):
            raise MRTRuntimeError("zip() takes exactly two array arguments.")
        return [[a, b] for a, b in zip(args[0], args[1])]

    @staticmethod
    def enumerate_(*args):
        if len(args) != 1 or not isinstance(args[0], list):
            raise MRTRuntimeError("enumerate() takes exactly one array argument.")
        return [[float(i), v] for i, v in enumerate(args[0])]

    @staticmethod
    def count(*args):
        if len(args) != 2 or not isinstance(args[0], list):
            raise MRTRuntimeError("count() takes an array and a value.")
        return float(sum(1 for item in args[0] if values_equal(item, args[1])))

    @staticmethod
    def sum_(*args):
        if len(args) != 1 or not isinstance(args[0], list):
            raise MRTRuntimeError("sum() takes exactly one array argument.")
        total = 0.0
        for item in args[0]:
            total += _number_arg(item, "sum")
        return total

    @staticmethod
    def range_(*args):
        if not 1 <= len(args) <= 3:
            raise MRTRuntimeError("range() takes one to three number arguments.")
        nums = [_number_arg(a, "range") for a in args]
        if len(nums) == 1:
            start, end, step = 0.0, nums[0], 1.0
        elif len(nums) == 2:
            start, end, step = nums[0], nums[1], 1.0
        else:
            start, end, step = nums
        if step == 0:
            raise MRTRuntimeError("range() step must not be zero.")

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
            raise MRTRuntimeError("repeat() takes a string and a count.")
        n = int(_number_arg(args[1], "repeat"))
        if n < 0:
            raise MRTRuntimeError("repeat() count must not be negative.")
        return args[0] * n

    @staticmethod
    def _pad(args, who, at_start):
        if not 2 <= len(args) <= 3 or not isinstance(args[0], str):
            raise MRTRuntimeError(f"{who}() takes a string, a width, and an optional pad string.")
        text = args[0]
        width = int(_number_arg(args[1], who))
        filler = args[2] if len(args) == 3 else " "
        if not isinstance(filler, str):
            raise MRTRuntimeError(f"{who}() pad argument must be a string.")
        if filler == "":
            raise MRTRuntimeError(f"{who}() pad string must not be empty.")
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
            raise MRTRuntimeError("random() takes exactly one seed argument.")
        seed = int(_number_arg(args[0], "random")) & 0xFFFFFFFF
        state = [seed if seed != 0 else 1]

        def next_value(*call_args):
            if call_args:
                raise MRTRuntimeError("A random generator takes no arguments.")
            x = state[0]
            x ^= (x << 13) & 0xFFFFFFFF
            x ^= x >> 17
            x ^= (x << 5) & 0xFFFFFFFF
            x &= 0xFFFFFFFF
            state[0] = x
            return x / 4294967296.0

        return next_value


def _number_arg(value: Any, who: str) -> float:
    """Validate a numeric built-in argument. The message is worded to match
    the Playground interpreter's `numArg` exactly -- a caught error's
    `.message` is program-visible, so the two must agree verbatim."""
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        raise MRTRuntimeError(f"{who}() argument must be a number.")
    return float(value)


class Interpreter:
    def __init__(self):
        self.globals = Environment()
        self.environment = self.globals
        self.output = []  # Capture output for web playground

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

    # -- Higher-order built-ins -------------------------------------------
    #
    # These live on the interpreter rather than in MRTBuiltin because they
    # have to call back into user code (`self.call_value`), which a plain
    # static helper has no handle on.

    def _array_arg(self, value: Any, who: str) -> List[Any]:
        if not isinstance(value, list):
            raise MRTRuntimeError(f"First argument to {who}() must be an array.")
        return value

    def builtin_map(self, *args):
        if len(args) != 2:
            raise MRTRuntimeError("map() takes an array and a function.")
        items = self._array_arg(args[0], "map")
        return [self.call_value(args[1], [item]) for item in items]

    def builtin_filter(self, *args):
        if len(args) != 2:
            raise MRTRuntimeError("filter() takes an array and a function.")
        items = self._array_arg(args[0], "filter")
        return [item for item in items if self.is_truthy(self.call_value(args[1], [item]))]

    def builtin_reduce(self, *args):
        if not 2 <= len(args) <= 3:
            raise MRTRuntimeError("reduce() takes an array, a function, and an optional initial value.")
        items = self._array_arg(args[0], "reduce")
        function = args[1]

        if len(args) == 3:
            accumulator = args[2]
            rest = items
        else:
            if not items:
                raise MRTRuntimeError("reduce() of an empty array needs an initial value.")
            accumulator = items[0]
            rest = items[1:]

        for item in rest:
            accumulator = self.call_value(function, [accumulator, item])
        return accumulator

    def builtin_find(self, *args):
        if len(args) != 2:
            raise MRTRuntimeError("find() takes an array and a function.")
        items = self._array_arg(args[0], "find")
        for item in items:
            if self.is_truthy(self.call_value(args[1], [item])):
                return item
        return None

    def builtin_some(self, *args):
        if len(args) != 2:
            raise MRTRuntimeError("some() takes an array and a function.")
        items = self._array_arg(args[0], "some")
        return any(self.is_truthy(self.call_value(args[1], [item])) for item in items)

    def builtin_every(self, *args):
        if len(args) != 2:
            raise MRTRuntimeError("every() takes an array and a function.")
        items = self._array_arg(args[0], "every")
        return all(self.is_truthy(self.call_value(args[1], [item])) for item in items)

    def builtin_sort(self, *args):
        """`sort(arr)` or `sort(arr, compare)`. Returns a new array; the
        input is left alone. Without a comparator the array must be all
        numbers or all strings -- there is no cross-type default order.

        The comparator follows the usual convention: negative if `a` sorts
        first, positive if `b` does, zero for a tie. Sorting is stable in
        both interpreters, so equal elements keep their original order."""
        if not 1 <= len(args) <= 2:
            raise MRTRuntimeError("sort() takes an array and an optional compare function.")
        items = list(self._array_arg(args[0], "sort"))

        if len(args) == 2:
            comparator = args[1]

            def compare(a, b):
                result = self.call_value(comparator, [a, b])
                if not isinstance(result, (int, float)) or isinstance(result, bool):
                    raise MRTRuntimeError("sort() compare function must return a number.")
                return -1 if result < 0 else (1 if result > 0 else 0)

            return sorted(items, key=cmp_to_key(compare))

        if all(isinstance(i, (int, float)) and not isinstance(i, bool) for i in items):
            return sorted(items)
        if all(isinstance(i, str) for i in items):
            return sorted(items)
        raise MRTRuntimeError(
            "sort() without a compare function needs an array of all numbers or all strings.")

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
            # First pass: define all functions
            for statement in statements:
                if isinstance(statement, Function):
                    self.execute(statement)

            # Second pass: look for and execute main function
            main_func = None
            try:
                main_token = Token(TokenType.IDENTIFIER, "main", None, 1)
                main_func = self.environment.get(main_token)
            except MRTRuntimeError:
                pass

            if main_func and isinstance(main_func, MRTFunction):
                main_func.call(self, [])
            else:
                # If no main function, execute all non-function statements
                for statement in statements:
                    if not isinstance(statement, Function):
                        self.execute(statement)

        except MRTThrow as thrown:
            # Nothing caught it, so it halts the program like any other
            # runtime failure -- but reports the thrown value, since that's
            # what the program chose to say.
            error_msg = f"Runtime Error: Uncaught {stringify(thrown.value)}"
            self.output.append(error_msg)
            print(error_msg)
        except MRTRuntimeError as e:
            error_msg = f"Runtime Error: {e}"
            self.output.append(error_msg)
            print(error_msg)
        except Exception as e:
            error_msg = f"Runtime Error: {str(e)}"
            self.output.append(error_msg)
            print(error_msg)

    def execute(self, stmt: Stmt):
        match stmt:
            case Block():
                self.execute_block(stmt.statements, Environment(self.environment))
            case Break():
                raise BreakSignal()
            case Continue():
                raise ContinueSignal()
            case Expression():
                self.evaluate(stmt.expression)
            case For():
                self.execute_for(stmt)
            case Function():
                function = MRTFunction(stmt.params, stmt.body, self.environment,
                                       stmt.name.lexeme)
                self.environment.define(stmt.name.lexeme, function)
            case ForIn():
                self.execute_for_in(stmt)
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
                values = [self.evaluate(e) for e in stmt.expressions]
                self.print_function(*values)
            case Return():
                value = None
                if stmt.value:
                    value = self.evaluate(stmt.value)
                raise ReturnSignal(value)
            case Var():
                value = None
                if stmt.initializer:
                    value = self.evaluate(stmt.initializer)
                self.environment.define(stmt.name.lexeme, value)
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

            while stmt.condition is None or self.is_truthy(self.evaluate(stmt.condition)):
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

    def execute_for_in(self, stmt: ForIn):
        iterable = self.evaluate(stmt.iterable)

        if isinstance(iterable, list):
            items = list(iterable)
        elif isinstance(iterable, str):
            items = list(iterable)
        elif isinstance(iterable, dict):
            items = list(iterable.keys())
        else:
            raise MRTRuntimeError(
                "Can only iterate over an array, string, or object.", stmt.name.line)

        previous = self.environment
        try:
            for item in items:
                # A fresh scope per iteration, so a closure made in the body
                # captures this item rather than sharing one slot with every
                # other iteration.
                self.environment = Environment(previous)
                self.environment.define(stmt.name.lexeme, item)
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
                if stmt.catch_block is None:
                    raise
                self.run_catch(stmt, thrown.value)
            except MRTRuntimeError as e:
                # An interpreter-raised failure (bad index, division by zero,
                # ...) is catchable too: it reaches the program as the
                # standard {"message", "line"} error object.
                if stmt.catch_block is None:
                    raise
                self.run_catch(stmt, make_error_value(e.message, e.line))
        finally:
            # Runs on every path out of the try -- normal completion, a
            # caught or uncaught throw, and a return/break/continue
            # unwinding through it.
            if stmt.finally_block is not None:
                self.execute(stmt.finally_block)

    def run_catch(self, stmt: Try, value: Any):
        environment = Environment(self.environment)
        environment.define(stmt.catch_name.lexeme, value)
        self.execute_block(stmt.catch_block.statements, environment)

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
                return [self.evaluate(element) for element in expr.elements]
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
                    self._check_hashable_key(key)
                    result[key] = self.evaluate(value_expr)
                return result
            case Call():
                callee = self.evaluate(expr.callee)
                arguments = [self.evaluate(arg) for arg in expr.arguments]
                return self.call_value(callee, arguments, expr.paren.line)
            case FunctionExpr():
                return MRTFunction(expr.params, expr.body, self.environment,
                                   expr.name.lexeme if expr.name else None)
            case Interpolation():
                out = []
                for part in expr.parts:
                    out.append(part if isinstance(part, str)
                               else stringify(self.evaluate(part)))
                return "".join(out)
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
                        raise MRTRuntimeError("Operand of '-' must be a number.", expr.operator.line)
                    return -float(right)
                if expr.operator.type == TokenType.NOT:
                    return not self.is_truthy(right)
            case Variable():
                return self.environment.get(expr.name)

    def call_value(self, callee: Any, arguments: List[Any], line: Optional[int] = None) -> Any:
        """Invoke an MRT value with arguments. Shared by the `Call`
        expression and by the higher-order built-ins (map, filter, sort,
        ...), which need to call back into user code."""
        if isinstance(callee, MRTFunction):
            if len(arguments) != len(callee.params):
                raise MRTRuntimeError(
                    f"Expected {len(callee.params)} arguments but got {len(arguments)}.",
                    line)
            return callee.call(self, arguments)

        if callable(callee):
            try:
                return callee(*arguments)
            except MRTRuntimeError as e:
                raise MRTRuntimeError(e.message, e.line if e.line is not None else line) from None
            except TypeError as e:
                raise MRTRuntimeError(f"Invalid arguments in call: {e}", line) from None

        raise MRTRuntimeError("Can only call functions.", line)

    def evaluate_index_get(self, expr: ArrayAccess) -> Any:
        target = self.evaluate(expr.array)
        index = self.evaluate(expr.index)

        if isinstance(target, dict):
            self._check_hashable_key(index)
            if index not in target:
                raise MRTRuntimeError(f"Key {stringify(index)!r} not found in object.")
            return target[index]

        if isinstance(target, list):
            i = self._require_array_index(index, len(target))
            return target[i]

        if isinstance(target, str):
            i = self._require_array_index(index, len(target))
            return target[i]

        raise MRTRuntimeError("Can only index into arrays, objects, or strings.")

    def evaluate_index_set(self, expr: ArrayAssign) -> Any:
        target = self.evaluate(expr.array)
        index = self.evaluate(expr.index)
        value = self.evaluate(expr.value)

        if isinstance(target, dict):
            self._check_hashable_key(index)
            target[index] = value
            return value

        if isinstance(target, list):
            i = self._require_array_index(index, len(target))
            target[i] = value
            return value

        if isinstance(target, str):
            raise MRTRuntimeError("Strings are immutable; cannot assign to a character index.")

        raise MRTRuntimeError("Can only assign into arrays or objects.")

    def _require_array_index(self, index: Any, length: int) -> int:
        if not isinstance(index, (int, float)) or isinstance(index, bool):
            raise MRTRuntimeError("Array index must be a number.")
        i = int(index)
        if i < 0 or i >= length:
            raise MRTRuntimeError(f"Array index {i} out of bounds for array of length {length}.")
        return i

    def _check_hashable_key(self, key: Any):
        if isinstance(key, (list, dict)):
            raise MRTRuntimeError("Object keys must be numbers, strings, or booleans (not arrays or objects).")

    def evaluate_binary(self, expr: Binary) -> Any:
        left = self.evaluate(expr.left)
        right = self.evaluate(expr.right)
        op = expr.operator.type
        line = expr.operator.line

        if op == TokenType.PLUS:
            # Handle string concatenation
            if isinstance(left, str) or isinstance(right, str):
                return stringify(left) + stringify(right)
            self._check_numbers(left, right, '+', line)
            return float(left) + float(right)
        if op == TokenType.MINUS:
            self._check_numbers(left, right, '-', line)
            return float(left) - float(right)
        if op == TokenType.MULTIPLY:
            self._check_numbers(left, right, '*', line)
            return float(left) * float(right)
        if op == TokenType.DIVIDE:
            self._check_numbers(left, right, '/', line)
            if float(right) == 0:
                raise MRTRuntimeError("Division by zero.", line)
            return float(left) / float(right)
        if op == TokenType.MODULO:
            self._check_numbers(left, right, '%', line)
            if float(right) == 0:
                raise MRTRuntimeError("Modulo by zero.", line)
            return float(left) % float(right)
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

        raise MRTRuntimeError(f"Unknown binary operator '{expr.operator.lexeme}'.", line)

    def _check_numbers(self, left: Any, right: Any, op: str, line: int):
        def is_number(v):
            return isinstance(v, (int, float)) and not isinstance(v, bool)
        if not is_number(left) or not is_number(right):
            raise MRTRuntimeError(f"Operands of '{op}' must be numbers.", line)

    def _compare(self, left: Any, right: Any, line: int) -> int:
        """Return a negative/zero/positive int comparing left to right.
        Numbers compare numerically; strings compare lexicographically."""
        if isinstance(left, str) and isinstance(right, str):
            return -1 if left < right else (1 if left > right else 0)
        if isinstance(left, (int, float)) and isinstance(right, (int, float)) \
                and not isinstance(left, bool) and not isinstance(right, bool):
            fl, fr = float(left), float(right)
            return -1 if fl < fr else (1 if fl > fr else 0)
        raise MRTRuntimeError(
            "Comparison operators require two numbers or two strings.", line)

    def is_equal(self, a: Any, b: Any) -> bool:
        return values_equal(a, b)

    def is_truthy(self, obj: Any) -> bool:
        if obj is None:
            return False
        if isinstance(obj, bool):
            return obj
        return True
