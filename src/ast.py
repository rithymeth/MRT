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
class Param:
    """One declared parameter.

    `default` is an expression evaluated *at call time* in the callee's own
    scope, so a later default may refer to an earlier parameter
    (`func f(a, b = a * 2)`). `rest` marks a `...name` parameter, which
    collects any remaining arguments into an array and must come last."""
    name: 'Token'
    default: Optional[Expr] = None
    rest: bool = False


@dataclass
class Spread(Expr):
    """`...expr` in a call's argument list or an array literal, expanding an
    array in place. It is not a general-purpose expression: evaluating one
    anywhere else is a runtime error."""
    value: Expr
    token: 'Token'


@dataclass
class FunctionExpr(Expr):
    """An anonymous function used as a value: `func(a, b) { ... }`.

    `name` is normally None; a named function *declaration* is still the
    `Function` statement below. The two share `MRTFunction` at runtime, so a
    closure made here is indistinguishable from a declared one apart from
    how it prints."""
    params: List[Param]
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
    params: List[Param]
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
class Catch:
    """One `catch (e) { }` clause, optionally guarded by `if (cond)`.

    The guard is evaluated with `name` already bound to the error, so it can
    inspect it: `catch (e) if (e.kind == "IndexError") { ... }`."""
    name: 'Token'
    guard: Optional[Expr]
    block: Stmt


@dataclass
class Try(Stmt):
    """`try { } catch (e) { } finally { }`.

    Several `catch` clauses may be given; they are tried in source order and
    the first whose guard passes handles the error. An unguarded clause
    always matches, so it acts as the final `else`. At least one of
    `catches`/`finally_block` must be present."""
    try_block: Stmt
    catches: List[Catch]
    finally_block: Optional[Stmt]


@dataclass
class Import(Stmt):
    """`import { a, b as c } from "./mod.mrt";`

    `names` pairs each exported name with the local name it binds to (the
    same token twice when there is no `as`). Only valid at the top level of
    a file."""
    names: List[Tuple['Token', 'Token']]
    specifier: 'Token'
    keyword: 'Token'


@dataclass
class Export(Stmt):
    """`export` in front of a `func` or `var` declaration. The declaration
    still binds normally inside its own module; `export` additionally records
    the name in the module's export table."""
    declaration: Stmt
    name: 'Token'


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
