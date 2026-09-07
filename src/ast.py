from dataclasses import dataclass
from typing import List, Any, Optional, Tuple

# Base class for all AST nodes
class Expr:
    pass

class Stmt:
    pass

@dataclass
class Binary(Expr):
    left: Expr
    operator: 'Token'
    right: Expr

@dataclass
class Grouping(Expr):
    expression: Expr

@dataclass
class Literal(Expr):
    value: Any

@dataclass
class Unary(Expr):
    operator: 'Token'
    right: Expr

@dataclass
class Logical(Expr):
    left: Expr
    operator: 'Token'
    right: Expr

@dataclass
class Variable(Expr):
    name: 'Token'

@dataclass
class Assign(Expr):
    name: 'Token'
    value: Expr

@dataclass
class Call(Expr):
    callee: Expr
    paren: 'Token'
    arguments: List[Expr]

@dataclass
class Array(Expr):
    elements: List[Expr]

@dataclass
class ArrayAccess(Expr):
    """Indexing get: `target[index]`, and also `target.name` (desugared to
    `target["name"]` by the parser). `target` may evaluate to an array, a
    dict, or (read-only) a string."""
    array: Expr
    index: Expr

@dataclass
class ArrayAssign(Expr):
    """Indexing set: `target[index] = value`, and also `target.name = value`.
    `target` may evaluate to an array or a dict."""
    array: Expr
    index: Expr
    value: Expr

@dataclass
class FunctionExpr(Expr):
    """An anonymous function used as a value: `func(a, b) { ... }`.

    `name` is normally None; a named function *declaration* is still the
    `Function` statement below. The two share `MRTFunction` at runtime, so a
    closure made here is indistinguishable from a declared one apart from
    how it prints."""
    params: List['Token']
    body: List[Stmt]
    name: Optional['Token'] = None


@dataclass
class Interpolation(Expr):
    """A string template like `"Hi ${name}!"`. `parts` alternates between
    plain `str` (literal text) and `Expr` (an embedded expression), and the
    interpreter renders each expression with the same `stringify` that
    `print` and `toString` use."""
    parts: List[Any]


@dataclass
class DictLiteral(Expr):
    """A `{key: value, ...}` object literal. Keys are arbitrary expressions
    (evaluated at construction time), not bareword shorthand -- so string
    keys must be quoted, e.g. `{"name": "Ada"}`.

    Named `DictLiteral` rather than `Dict` deliberately: interpreter.py
    already imports `typing.Dict` for type hints, and `from .ast import *`
    would otherwise silently shadow it (the same class of bug that used to
    make every `return` statement in this interpreter a no-op -- see
    interpreter.py's history)."""
    pairs: List[Tuple[Expr, Expr]]

# Statement nodes
@dataclass
class Expression(Stmt):
    expression: Expr

@dataclass
class Function(Stmt):
    name: 'Token'
    params: List['Token']
    body: List[Stmt]

@dataclass
class If(Stmt):
    condition: Expr
    then_branch: Stmt
    else_branch: Optional[Stmt]

@dataclass
class Return(Stmt):
    keyword: 'Token'
    value: Optional[Expr]

@dataclass
class While(Stmt):
    condition: Expr
    body: Stmt

@dataclass
class For(Stmt):
    initializer: Optional[Stmt]
    condition: Optional[Expr]
    increment: Optional[Expr]
    body: Stmt

@dataclass
class Break(Stmt):
    keyword: 'Token'

@dataclass
class Continue(Stmt):
    keyword: 'Token'

@dataclass
class ForIn(Stmt):
    """`for (x in iterable) { ... }` -- iterates an array's elements, a
    string's characters, or an object's keys.

    Unlike the C-style `For` above, the loop variable is bound afresh in a
    new scope on every iteration, so a closure created inside the body
    captures that iteration's value rather than sharing one mutable slot."""
    name: 'Token'
    iterable: Expr
    body: Stmt


@dataclass
class Throw(Stmt):
    keyword: 'Token'
    value: Expr


@dataclass
class Try(Stmt):
    """`try { } catch (e) { } finally { }` -- `catch` and `finally` are each
    optional, but at least one must be present."""
    try_block: Stmt
    catch_name: Optional['Token']
    catch_block: Optional[Stmt]
    finally_block: Optional[Stmt]


@dataclass
class Block(Stmt):
    statements: List[Stmt]

@dataclass
class Print(Stmt):
    expressions: List[Expr]

@dataclass
class Var(Stmt):
    name: 'Token'
    initializer: Optional[Expr]
