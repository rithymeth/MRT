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
    dict, or (read-only) a string.

    `line` is the bracket (or dot) the access was written with. Indexing is
    the one place a runtime error is raised from a helper several frames
    below the expression, so without it a bad index reports no line at all."""
    array: Expr
    index: Expr
    line: int = 0

@dataclass
class ArrayAssign(Expr):
    """Indexing set: `target[index] = value`, and also `target.name = value`.
    `target` may evaluate to an array or a dict."""
    array: Expr
    index: Expr
    value: Expr
    line: int = 0

# -- Binding patterns --------------------------------------------------------
#
# A pattern is the left-hand side of a binding: a plain name, or a shape to
# take apart. The same three nodes serve `var`, function parameters, `for`-`in`
# and `catch`, so destructuring works identically everywhere a name can be
# bound.


class Pattern:
    pass


@dataclass
class NamePattern(Pattern):
    """A plain `name`, optionally `name = default`."""
    name: 'Token'
    default: Optional[Expr] = None


@dataclass
class ArrayPattern(Pattern):
    """`[a, b]`, `[a, ...rest]`, `[a = 1]` -- and nested patterns within."""
    elements: List[Pattern]
    rest: Optional['Token'] = None
    default: Optional[Expr] = None
    token: Optional['Token'] = None


@dataclass
class ObjectPattern(Pattern):
    """`{name, age}`, `{name: local}`, `{name = "anon"}`, `{a, ...others}`.

    `entries` pairs the key to read with the pattern to bind it to; the
    shorthand `{name}` is just `("name", NamePattern(name))`."""
    entries: List[Tuple[str, Pattern]]
    rest: Optional['Token'] = None
    default: Optional[Expr] = None
    token: Optional['Token'] = None


@dataclass
class Param:
    """One declared parameter.

    `pattern` is a NamePattern for an ordinary parameter and an array/object
    pattern for a destructured one; any default lives on the pattern and is
    evaluated *at call time* in the callee's own scope, so a later default
    may refer to an earlier parameter (`func f(a, b = a * 2)`). `rest` marks
    a `...name` parameter, which collects any remaining arguments into an
    array and must come last."""
    pattern: Pattern
    rest: bool = False

    @property
    def default(self) -> Optional[Expr]:
        return self.pattern.default


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
    is_generator: bool = False


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
    line: int = 0

# Statement nodes
@dataclass
class Expression(Stmt):
    expression: Expr

@dataclass
class Function(Stmt):
    name: 'Token'
    params: List[Param]
    body: List[Stmt]
    # True when the body contains a `yield`, which makes calling this
    # function produce a lazy generator instead of running it.
    is_generator: bool = False

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
    pattern: Pattern
    iterable: Expr
    keyword: 'Token'
    body: Stmt


@dataclass
class Yield(Stmt):
    """`yield expr;` inside a function makes that function a generator, and
    `yield* other;` delegates to another iterable, re-yielding every item.

    `yield` is deliberately a *statement*, not a general expression: only
    statement execution then has to be suspendable, which is what makes lazy
    evaluation implementable in a tree-walking interpreter without rewriting
    every expression path. The one exception is `YieldExpr` below, which is
    still confined to a whole statement."""
    keyword: 'Token'
    value: Expr
    delegate: bool = False


@dataclass
class YieldExpr(Expr):
    """`yield expr` in the one position where it produces a value: as the
    entire right-hand side of a declaration or an assignment, i.e.

        var x = yield 1;        x = yield 1;       [a, b] = yield 1;

    Its value is whatever the consumer sends back in with `send()`, or
    `null` when the generator is driven by `for`-`in` or `toArray`. Confining
    it to those shapes is what keeps suspension a statement-level concern:
    the interpreter never has to unwind a half-evaluated expression.

    It is an `Expr` only so that `Var`/`Assign`/`DestructureAssign` can hold
    one without a second set of nodes; evaluating it through the ordinary
    expression path is a runtime error."""
    keyword: 'Token'
    value: Expr


@dataclass
class Throw(Stmt):
    keyword: 'Token'
    value: Expr


@dataclass
class Catch:
    """One `catch (e) { }` clause, optionally guarded by `if (cond)`.

    The guard is evaluated with the pattern already bound, so it can inspect
    the error: `catch (e) if (e.kind == "IndexError") { ... }`. The pattern
    may destructure, e.g. `catch ({message, kind})`."""
    pattern: Pattern
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
    # Set for `import * as name from "..."`, which binds one object holding
    # every export instead of a list of names.
    namespace: Optional['Token'] = None


@dataclass
class Export(Stmt):
    """`export` in front of a `func` or `var` declaration. The declaration
    still binds normally inside its own module; `export` additionally records
    the name in the module's export table."""
    declaration: Stmt
    name: 'Token'


# -- Match patterns ----------------------------------------------------------
#
# Deliberately separate from the binding patterns above: a binding pattern
# only takes a value apart (and fails loudly if it can't), whereas a match
# pattern *tests* a value and binds only if it fits. Sharing one node set
# would mean every node carrying both a default and a literal, so the two
# families stay distinct and the interpreter has one function for each job.


class MatchPattern:
    pass


@dataclass
class LiteralMatch(MatchPattern):
    """`case 0:` / `case "x":` -- matches by structural equality."""
    value: Any


@dataclass
class BindMatch(MatchPattern):
    """`case n:` -- matches anything and binds it to `n`."""
    name: 'Token'


@dataclass
class ArrayMatch(MatchPattern):
    """`case [a, b]:` / `case [head, ...tail]:`.

    Unlike destructuring, the length must match exactly unless a rest is
    given -- a pattern that silently ignored extra elements would make
    `case [x]:` swallow every non-empty array."""
    elements: List[MatchPattern]
    rest: Optional['Token'] = None
    token: Optional['Token'] = None


@dataclass
class ObjectMatch(MatchPattern):
    """`case {kind: "circle", radius: r}:` -- a *partial* match: the listed
    keys must be present and match, and any other keys are ignored."""
    entries: List[Tuple[str, MatchPattern]]
    token: Optional['Token'] = None


@dataclass
class StructMatch(MatchPattern):
    """`case Point(x, y):` -- matches an instance of that exact struct, with
    sub-patterns bound to its fields in declaration order."""
    name: 'Token'
    elements: List[MatchPattern]


@dataclass
class MatchCase:
    pattern: Optional[MatchPattern]  # None for `default:`
    guard: Optional[Expr]
    body: List[Stmt]
    keyword: 'Token'


@dataclass
class Match(Stmt):
    """`match (subject) { case <pattern>: ... default: ... }`.

    Cases are tried in order and the first whose pattern fits (and whose
    guard passes) runs; there is no fall-through. If nothing matches and
    there is no `default`, that is a runtime error rather than a silent
    no-op."""
    subject: Expr
    cases: List[MatchCase]
    keyword: 'Token'


@dataclass
class MatchArm:
    """One arm of a `match` *expression*: a pattern and the expression its
    value is."""
    pattern: Optional[MatchPattern]  # None for `default:`
    guard: Optional[Expr]
    value: Expr
    keyword: 'Token'


@dataclass
class MatchExpr(Expr):
    """`match (subject) { case <pattern>: <expr>, default: <expr> }` used as
    a value.

    The same patterns and guards as the `Match` statement, but each arm is a
    single expression and the whole thing evaluates to the matching arm's
    value -- so a match can be assigned, returned or passed straight to a
    call without a mutable temporary."""
    subject: Expr
    arms: List[MatchArm]
    keyword: 'Token'


@dataclass
class StructDecl(Stmt):
    """`struct Point { x, y; func mag() { ... } }`.

    `fields` reuses `Param` so a field may carry a default and so the
    constructor's arity logic is the same one function calls use. `methods`
    are ordinary functions that additionally see `this` bound to the
    instance they were reached through."""
    name: 'Token'
    fields: List[Param]
    methods: List['Function']


@dataclass
class ExportNames(Stmt):
    """`export { a, b as c };` re-exports names already declared in this
    module, and `export { a } from "./m.mrt";` forwards another module's
    exports without binding them locally."""
    names: List[Tuple['Token', 'Token']]
    specifier: Optional['Token']
    keyword: 'Token'


@dataclass
class DestructureAssign(Stmt):
    """`[a, b] = pair;` / `{x, y} = point;` -- assignment through a pattern
    to variables that already exist.

    Deliberately a statement rather than an expression: `{...}` in expression
    position is an object literal, and only at the start of a statement can
    the parser tell the two apart without the parenthesis dance other
    languages need (`({x} = o)`)."""
    pattern: Pattern
    value: Expr
    token: 'Token'


@dataclass
class Block(Stmt):
    statements: List[Stmt]

@dataclass
class Print(Stmt):
    expressions: List[Expr]

@dataclass
class Var(Stmt):
    """`var <pattern> = expr;` -- `pattern` is usually a plain name, but may
    destructure an array or object."""
    pattern: Pattern
    initializer: Optional[Expr]
