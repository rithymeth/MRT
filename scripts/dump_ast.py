"""Emit the canonical AST dump for an MRT source file.

The other half of this format lives in `compiler/crates/mrt-ast/src/dump.rs`.
Both sides must produce byte-identical output for the same input; that
equality is the only evidence that the Rust parser and the reference parser
agree about what MRT *means*, as opposed to merely agreeing about how it is
tokenised.

One node per line, two spaces per level. A node line is its name plus scalar
attributes; a child group is a `.name` line followed by that group's children,
indented one further. Groups are always emitted, even when empty, so the shape
of the output never depends on whether an optional part happens to be present.

    python3 -m scripts.dump_ast FILE
"""

import struct
import sys

from mrt import ast as A
from mrt.errors import MRTError
from mrt.lexer import Lexer
from mrt.parser import Parser
from scripts.dump_tokens import quote


def lit(value) -> str:
    if value is None:
        return "null"
    # bool before float: bool is a subclass of int in Python.
    if isinstance(value, bool):
        return f"B:{'true' if value else 'false'}"
    if isinstance(value, float):
        return f"N:{struct.unpack('<Q', struct.pack('<d', value))[0]:016x}"
    if isinstance(value, str):
        return f"S:{quote(value)}"
    raise AssertionError(f"unexpected literal {value!r}")


def flag(b: bool) -> str:
    return "true" if b else "false"


def opt_name(token) -> str:
    return quote(token.lexeme) if token is not None else "-"


class Dumper:
    def __init__(self):
        self.out = []
        self.depth = 0

    def node(self, kind, **attrs):
        pairs = "".join(f" {k}={v}" for k, v in attrs.items())
        self.out.append(f"{'  ' * self.depth}{kind}{pairs}")

    def label(self, group):
        self.out.append(f"{'  ' * self.depth}.{group}")

    def group(self, name, items, emit):
        self.label(name)
        self.depth += 1
        for item in items:
            emit(item)
        self.depth -= 1

    def single(self, name, item, emit):
        self.label(name)
        self.depth += 1
        if item is not None:
            emit(item)
        self.depth -= 1

    # -- expressions -------------------------------------------------------

    def expr(self, e):
        match e:
            case A.Literal():
                self.node("Literal", value=lit(e.value))
            case A.Variable():
                self.node("Variable", name=quote(e.name.lexeme))
            case A.Assign():
                self.node("Assign", name=quote(e.name.lexeme))
                self.depth += 1
                self.single("value", e.value, self.expr)
                self.depth -= 1
            case A.Binary():
                self.node("Binary", op=quote(e.operator.lexeme))
                self.depth += 1
                self.single("left", e.left, self.expr)
                self.single("right", e.right, self.expr)
                self.depth -= 1
            case A.Logical():
                self.node("Logical", op=quote(e.operator.lexeme))
                self.depth += 1
                self.single("left", e.left, self.expr)
                self.single("right", e.right, self.expr)
                self.depth -= 1
            case A.Unary():
                self.node("Unary", op=quote(e.operator.lexeme))
                self.depth += 1
                self.single("right", e.right, self.expr)
                self.depth -= 1
            case A.Grouping():
                self.node("Grouping")
                self.depth += 1
                self.single("expression", e.expression, self.expr)
                self.depth -= 1
            case A.Call():
                self.node("Call")
                self.depth += 1
                self.single("callee", e.callee, self.expr)
                self.group("arguments", e.arguments, self.expr)
                self.depth -= 1
            case A.Array():
                self.node("Array")
                self.depth += 1
                self.group("elements", e.elements, self.expr)
                self.depth -= 1
            case A.ArrayAccess():
                self.node("Index")
                self.depth += 1
                self.single("target", e.array, self.expr)
                self.single("index", e.index, self.expr)
                self.depth -= 1
            case A.ArrayAssign():
                self.node("IndexAssign")
                self.depth += 1
                self.single("target", e.array, self.expr)
                self.single("index", e.index, self.expr)
                self.single("value", e.value, self.expr)
                self.depth -= 1
            case A.DictLiteral():
                self.node("Dict")
                self.depth += 1
                self.group("pairs", e.pairs, self.pair)
                self.depth -= 1
            case A.Spread():
                self.node("Spread")
                self.depth += 1
                self.single("value", e.value, self.expr)
                self.depth -= 1
            case A.FunctionExpr():
                self.node("FunctionExpr", name=opt_name(e.name),
                          generator=flag(e.is_generator))
                self.depth += 1
                self.group("params", e.params, self.param)
                self.group("body", e.body, self.stmt)
                self.depth -= 1
            case A.Interpolation():
                self.node("Interpolation")
                self.depth += 1
                self.group("parts", e.parts, self.interp_part)
                self.depth -= 1
            case A.MatchExpr():
                self.node("MatchExpr")
                self.depth += 1
                self.single("subject", e.subject, self.expr)
                self.group("arms", e.arms, self.arm)
                self.depth -= 1
            case A.YieldExpr():
                self.node("YieldExpr")
                self.depth += 1
                self.single("value", e.value, self.expr)
                self.depth -= 1
            case _:
                raise AssertionError(f"unhandled expression {type(e).__name__}")

    def pair(self, kv):
        key, value = kv
        self.node("Pair")
        self.depth += 1
        self.single("key", key, self.expr)
        self.single("value", value, self.expr)
        self.depth -= 1

    def interp_part(self, part):
        if isinstance(part, str):
            self.node("Text", value=quote(part))
        else:
            self.expr(part)

    def arm(self, a):
        self.node("Arm")
        self.depth += 1
        self.single("pattern", a.pattern, self.match_pattern)
        self.single("guard", a.guard, self.expr)
        self.single("value", a.value, self.expr)
        self.depth -= 1

    # -- statements --------------------------------------------------------

    def stmt(self, s):
        match s:
            case A.Expression():
                self.node("Expression")
                self.depth += 1
                self.single("expression", s.expression, self.expr)
                self.depth -= 1
            case A.Print():
                self.node("Print")
                self.depth += 1
                self.group("values", s.expressions, self.expr)
                self.depth -= 1
            case A.Var():
                self.node("Var")
                self.depth += 1
                self.single("pattern", s.pattern, self.pattern)
                self.single("initializer", s.initializer, self.expr)
                self.depth -= 1
            case A.DestructureAssign():
                self.node("DestructureAssign")
                self.depth += 1
                self.single("pattern", s.pattern, self.pattern)
                self.single("value", s.value, self.expr)
                self.depth -= 1
            case A.Block():
                self.node("Block")
                self.depth += 1
                self.group("statements", s.statements, self.stmt)
                self.depth -= 1
            case A.If():
                self.node("If")
                self.depth += 1
                self.single("condition", s.condition, self.expr)
                self.single("then", s.then_branch, self.stmt)
                self.single("else", s.else_branch, self.stmt)
                self.depth -= 1
            case A.While():
                self.node("While")
                self.depth += 1
                self.single("condition", s.condition, self.expr)
                self.single("body", s.body, self.stmt)
                self.depth -= 1
            case A.For():
                self.node("For")
                self.depth += 1
                self.single("initializer", s.initializer, self.stmt)
                self.single("condition", s.condition, self.expr)
                self.single("increment", s.increment, self.expr)
                self.single("body", s.body, self.stmt)
                self.depth -= 1
            case A.ForIn():
                self.node("ForIn")
                self.depth += 1
                self.single("pattern", s.pattern, self.pattern)
                self.single("iterable", s.iterable, self.expr)
                self.single("body", s.body, self.stmt)
                self.depth -= 1
            case A.Function():
                self.node("Function", name=quote(s.name.lexeme),
                          generator=flag(s.is_generator))
                self.depth += 1
                self.group("params", s.params, self.param)
                self.group("body", s.body, self.stmt)
                self.depth -= 1
            case A.Return():
                self.node("Return")
                self.depth += 1
                self.single("value", s.value, self.expr)
                self.depth -= 1
            case A.Break():
                self.node("Break")
            case A.Continue():
                self.node("Continue")
            case A.Throw():
                self.node("Throw")
                self.depth += 1
                self.single("value", s.value, self.expr)
                self.depth -= 1
            case A.Yield():
                self.node("Yield", delegate=flag(s.delegate))
                self.depth += 1
                self.single("value", s.value, self.expr)
                self.depth -= 1
            case A.Try():
                self.node("Try")
                self.depth += 1
                # The Rust AST stores blocks as statement lists, so unwrap the
                # Block nodes the Python parser builds.
                self.group("body", s.try_block.statements, self.stmt)
                self.group("catches", s.catches, self.catch)
                self.group(
                    "finally",
                    s.finally_block.statements if s.finally_block is not None else [],
                    self.stmt,
                )
                self.depth -= 1
            case A.Match():
                self.node("Match")
                self.depth += 1
                self.single("subject", s.subject, self.expr)
                self.group("cases", s.cases, self.case)
                self.depth -= 1
            case A.StructDecl():
                self.node("Struct", name=quote(s.name.lexeme))
                self.depth += 1
                self.group("fields", s.fields, self.param)
                self.group("methods", s.methods, self.stmt)
                self.depth -= 1
            case A.Import():
                self.node("Import", specifier=quote(s.specifier.literal),
                          namespace=opt_name(s.namespace))
                self.depth += 1
                self.group("names", s.names, self.import_name)
                self.depth -= 1
            case A.Export():
                self.node("Export", name=quote(s.name.lexeme))
                self.depth += 1
                self.single("declaration", s.declaration, self.stmt)
                self.depth -= 1
            case A.ExportNames():
                self.node(
                    "ExportNames",
                    specifier=quote(s.specifier.literal) if s.specifier is not None else "-",
                )
                self.depth += 1
                self.group("names", s.names, self.export_name)
                self.depth -= 1
            case _:
                raise AssertionError(f"unhandled statement {type(s).__name__}")

    def catch(self, c):
        self.node("Catch")
        self.depth += 1
        self.single("pattern", c.pattern, self.pattern)
        self.single("guard", c.guard, self.expr)
        self.group("body", c.block.statements, self.stmt)
        self.depth -= 1

    def case(self, c):
        self.node("Case")
        self.depth += 1
        self.single("pattern", c.pattern, self.match_pattern)
        self.single("guard", c.guard, self.expr)
        self.group("body", c.body, self.stmt)
        self.depth -= 1

    def import_name(self, pair):
        exported, local = pair
        self.node("ImportName", exported=quote(exported.lexeme), local=quote(local.lexeme))

    def export_name(self, pair):
        local, exported = pair
        self.node("ExportName", local=quote(local.lexeme), exported=quote(exported.lexeme))

    # -- patterns ----------------------------------------------------------

    def param(self, p):
        self.node("Param", rest=flag(p.rest))
        self.depth += 1
        self.single("pattern", p.pattern, self.pattern)
        self.depth -= 1

    def pattern(self, p):
        match p:
            case A.NamePattern():
                self.node("NamePattern", name=quote(p.name.lexeme))
                self.depth += 1
                self.single("default", p.default, self.expr)
                self.depth -= 1
            case A.ArrayPattern():
                self.node("ArrayPattern", rest=opt_name(p.rest))
                self.depth += 1
                self.group("elements", p.elements, self.pattern)
                self.single("default", p.default, self.expr)
                self.depth -= 1
            case A.ObjectPattern():
                self.node("ObjectPattern", rest=opt_name(p.rest))
                self.depth += 1
                self.group("entries", p.entries, self.pattern_entry)
                self.single("default", p.default, self.expr)
                self.depth -= 1
            case _:
                raise AssertionError(f"unhandled pattern {type(p).__name__}")

    def pattern_entry(self, entry):
        key, sub = entry
        self.node("Entry", key=quote(key))
        self.depth += 1
        self.single("pattern", sub, self.pattern)
        self.depth -= 1

    def match_pattern(self, p):
        match p:
            case A.LiteralMatch():
                self.node("LiteralMatch", value=lit(p.value))
            case A.BindMatch():
                self.node("BindMatch", name=quote(p.name.lexeme))
            case A.ArrayMatch():
                self.node("ArrayMatch", rest=opt_name(p.rest))
                self.depth += 1
                self.group("elements", p.elements, self.match_pattern)
                self.depth -= 1
            case A.ObjectMatch():
                self.node("ObjectMatch")
                self.depth += 1
                self.group("entries", p.entries, self.match_entry)
                self.depth -= 1
            case A.StructMatch():
                self.node("StructMatch", name=quote(p.name.lexeme))
                self.depth += 1
                self.group("elements", p.elements, self.match_pattern)
                self.depth -= 1
            case _:
                raise AssertionError(f"unhandled match pattern {type(p).__name__}")

    def match_entry(self, entry):
        key, sub = entry
        self.node("Entry", key=quote(key))
        self.depth += 1
        self.single("pattern", sub, self.match_pattern)
        self.depth -= 1


def dump(source: str):
    """(ok, text) -- the AST dump, or the first MRT 1.x error line."""
    try:
        tokens = Lexer(source).scan_tokens()
    except MRTError as e:
        return False, f"Syntax Error: {e}"

    parser = Parser(tokens)
    statements = parser.parse()
    if parser.errors:
        return False, "\n".join(f"Syntax Error: {e}" for e in parser.errors)

    d = Dumper()
    d.node("Program")
    d.depth += 1
    d.group("statements", statements, d.stmt)
    return True, "\n".join(d.out) + "\n"


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: python3 -m scripts.dump_ast FILE", file=sys.stderr)
        return 2
    with open(sys.argv[1], "r") as f:
        ok, text = dump(f.read())
    if ok:
        sys.stdout.write(text)
        return 0
    print(text, file=sys.stderr)
    return 65


if __name__ == "__main__":
    sys.exit(main())
