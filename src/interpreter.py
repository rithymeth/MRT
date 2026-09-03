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
    return str(value)


class MRTFunction:
    def __init__(self, declaration: Function, closure: 'Environment'):
        self.declaration = declaration
        self.closure = closure

    def call(self, interpreter: 'Interpreter', arguments: List[Any]) -> Any:
        environment = Environment(self.closure)
        for param, arg in zip(self.declaration.params, arguments):
            environment.define(param.lexeme, arg)

        try:
            interpreter.execute_block(self.declaration.body, environment)
            return None
        except ReturnSignal as return_value:
            return return_value.value

    def __str__(self):
        return f"<function {self.declaration.name.lexeme}>"

class ReturnSignal(Exception):
    def __init__(self, value: Any):
        self.value = value
        super().__init__()

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
        if isinstance(args[0], str):
            return float(len(args[0]))
        if not isinstance(args[0], list):
            raise MRTRuntimeError("len() argument must be an array or string.")
        return float(len(args[0]))

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

        try:
            return float(args[0].index(args[1]))
        except ValueError:
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
                function = MRTFunction(stmt, self.environment)
                self.environment.define(stmt.name.lexeme, function)
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
                array = self.evaluate(expr.array)
                index = self.evaluate(expr.index)
                if not isinstance(array, list):
                    raise MRTRuntimeError("Can only index into arrays.")
                if not isinstance(index, (int, float)) or isinstance(index, bool):
                    raise MRTRuntimeError("Array index must be a number.")
                index = int(index)
                if index < 0 or index >= len(array):
                    raise MRTRuntimeError(f"Array index {index} out of bounds for array of length {len(array)}.")
                return array[index]
            case ArrayAssign():
                array = self.evaluate(expr.array)
                index = self.evaluate(expr.index)
                if not isinstance(array, list):
                    raise MRTRuntimeError("Can only index into arrays.")
                if not isinstance(index, (int, float)) or isinstance(index, bool):
                    raise MRTRuntimeError("Array index must be a number.")
                index = int(index)
                if index < 0 or index >= len(array):
                    raise MRTRuntimeError(f"Array index {index} out of bounds for array of length {len(array)}.")
                value = self.evaluate(expr.value)
                array[index] = value
                return value
            case Assign():
                value = self.evaluate(expr.value)
                self.environment.assign(expr.name, value)
                return value
            case Binary():
                return self.evaluate_binary(expr)
            case Call():
                callee = self.evaluate(expr.callee)
                arguments = [self.evaluate(arg) for arg in expr.arguments]

                if isinstance(callee, MRTFunction):
                    if len(arguments) != len(callee.declaration.params):
                        raise MRTRuntimeError(
                            f"Expected {len(callee.declaration.params)} arguments but got {len(arguments)}.",
                            expr.paren.line)
                    return callee.call(self, arguments)

                if callable(callee):
                    try:
                        return callee(*arguments)
                    except MRTRuntimeError as e:
                        raise MRTRuntimeError(e.message, expr.paren.line) from None
                    except TypeError as e:
                        raise MRTRuntimeError(f"Invalid arguments in call: {e}", expr.paren.line) from None

                raise MRTRuntimeError("Can only call functions.", expr.paren.line)
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
        """Check equality between two values"""
        if a is None and b is None:
            return True
        if a is None or b is None:
            return False
        if isinstance(a, bool) != isinstance(b, bool):
            return False
        return a == b

    def is_truthy(self, obj: Any) -> bool:
        if obj is None:
            return False
        if isinstance(obj, bool):
            return obj
        return True
