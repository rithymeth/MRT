from typing import Any, Dict, List, Optional
from .ast import *
from .lexer import Token, TokenType

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
        except Return as return_value:
            return return_value.value

    def __str__(self):
        return f"<function {self.declaration.name.lexeme}>"

class Return(Exception):
    def __init__(self, value: Any):
        self.value = value
        super().__init__()

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

        raise RuntimeError(f"Undefined variable '{name.lexeme}'.")

    def assign(self, name: Token, value: Any):
        if name.lexeme in self.values:
            self.values[name.lexeme] = value
            return

        if self.enclosing:
            self.enclosing.assign(name, value)
            return

        raise RuntimeError(f"Undefined variable '{name.lexeme}'.")

class MRTBuiltin:
    @staticmethod
    def len(*args):
        if len(args) != 1:
            raise RuntimeError("len() takes exactly one argument.")
        if not isinstance(args[0], list):
            raise RuntimeError("len() argument must be an array.")
        return float(len(args[0]))

    @staticmethod
    def push(*args):
        if len(args) != 2:
            raise RuntimeError("push() takes exactly two arguments.")
        if not isinstance(args[0], list):
            raise RuntimeError("First argument to push() must be an array.")
        args[0].append(args[1])
        return args[1]

    @staticmethod
    def pop(*args):
        if len(args) != 1:
            raise RuntimeError("pop() takes exactly one argument.")
        if not isinstance(args[0], list):
            raise RuntimeError("pop() argument must be an array.")
        if not args[0]:
            raise RuntimeError("Cannot pop from empty array.")
        return args[0].pop()

    @staticmethod
    def slice(*args):
        if len(args) not in [2, 3]:
            raise RuntimeError("slice() takes 2 or 3 arguments.")
        if not isinstance(args[0], list):
            raise RuntimeError("First argument to slice() must be an array.")
        
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
            raise RuntimeError("join() takes 1 or 2 arguments.")
        if not isinstance(args[0], list):
            raise RuntimeError("First argument to join() must be an array.")
            
        separator = str(args[1]) if len(args) > 1 else ""
        return separator.join(str(x) for x in args[0])

    @staticmethod
    def indexOf(*args):
        if len(args) != 2:
            raise RuntimeError("indexOf() takes exactly 2 arguments.")
        if not isinstance(args[0], list):
            raise RuntimeError("First argument to indexOf() must be an array.")
            
        try:
            return float(args[0].index(args[1]))
        except ValueError:
            return -1.0

class Interpreter:
    def __init__(self):
        self.globals = Environment()
        self.environment = self.globals
        
        # Add built-in functions
        self.globals.define("print", lambda x: print(x))
        self.globals.define("len", MRTBuiltin.len)
        self.globals.define("push", MRTBuiltin.push)
        self.globals.define("pop", MRTBuiltin.pop)
        self.globals.define("slice", MRTBuiltin.slice)
        self.globals.define("join", MRTBuiltin.join)
        self.globals.define("indexOf", MRTBuiltin.indexOf)

    def interpret(self, statements: List[Stmt]):
        try:
            print("Starting interpretation...")
            for statement in statements:
                self.execute(statement)
                if isinstance(statement, Function) and statement.name.lexeme == "main":
                    # Call main function if found
                    main_func = self.environment.get(statement.name)
                    main_func.call(self, [])
        except Exception as e:
            print(f"Runtime Error: {str(e)}")

    def execute(self, stmt: Stmt):
        match stmt:
            case Block():
                self.execute_block(stmt.statements, Environment(self.environment))
            case Expression():
                self.evaluate(stmt.expression)
            case Function():
                function = MRTFunction(stmt, self.environment)
                self.environment.define(stmt.name.lexeme, function)
            case If():
                if self.is_truthy(self.evaluate(stmt.condition)):
                    self.execute(stmt.then_branch)
                elif stmt.else_branch:
                    self.execute(stmt.else_branch)
            case Print():
                value = self.evaluate(stmt.expression)
                print(str(value))
            case Return():
                value = None
                if stmt.value:
                    value = self.evaluate(stmt.value)
                raise Return(value)
            case Var():
                value = None
                if stmt.initializer:
                    value = self.evaluate(stmt.initializer)
                self.environment.define(stmt.name.lexeme, value)
            case While():
                while self.is_truthy(self.evaluate(stmt.condition)):
                    self.execute(stmt.body)

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
                    raise RuntimeError("Can only index into arrays.")
                if not isinstance(index, (int, float)):
                    raise RuntimeError("Array index must be a number.")
                index = int(index)
                if index < 0 or index >= len(array):
                    raise RuntimeError("Array index out of bounds.")
                return array[index]
            case ArrayAssign():
                array = self.evaluate(expr.array)
                index = self.evaluate(expr.index)
                if not isinstance(array, list):
                    raise RuntimeError("Can only index into arrays.")
                if not isinstance(index, (int, float)):
                    raise RuntimeError("Array index must be a number.")
                index = int(index)
                if index < 0 or index >= len(array):
                    raise RuntimeError("Array index out of bounds.")
                value = self.evaluate(expr.value)
                array[index] = value
                return value
            case Assign():
                value = self.evaluate(expr.value)
                self.environment.assign(expr.name, value)
                return value
            case Binary():
                left = self.evaluate(expr.left)
                right = self.evaluate(expr.right)

                match expr.operator.type:
                    case TokenType.PLUS:
                        return float(left) + float(right)
                    case TokenType.MINUS:
                        return float(left) - float(right)
                    case TokenType.MULTIPLY:
                        return float(left) * float(right)
                    case TokenType.DIVIDE:
                        return float(left) / float(right)
                    case TokenType.EQUALS:
                        return left == right
                    case TokenType.GREATER:
                        return float(left) > float(right)
                    case TokenType.LESS:
                        return float(left) < float(right)
            case Call():
                callee = self.evaluate(expr.callee)
                arguments = [self.evaluate(arg) for arg in expr.arguments]
                
                if isinstance(callee, MRTFunction):
                    if len(arguments) != len(callee.declaration.params):
                        raise RuntimeError(f"Expected {len(callee.declaration.params)} arguments but got {len(arguments)}.")
                    return callee.call(self, arguments)
                
                if callable(callee):
                    return callee(*arguments)
                    
                raise RuntimeError("Can only call functions.")
            case Grouping():
                return self.evaluate(expr.expression)
            case Literal():
                return expr.value
            case Unary():
                right = self.evaluate(expr.right)
                
                if expr.operator.type == TokenType.MINUS:
                    return -float(right)
            case Variable():
                return self.environment.get(expr.name)

    def is_truthy(self, obj: Any) -> bool:
        if obj is None:
            return False
        if isinstance(obj, bool):
            return obj
        return True
