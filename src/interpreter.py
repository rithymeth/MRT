from typing import Any, Dict, List
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

class Interpreter:
    def __init__(self):
        self.globals = Environment()
        self.environment = self.globals
        
        # Add built-in functions
        self.globals.define("print", lambda x: print(x))

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
