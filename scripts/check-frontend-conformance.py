#!/usr/bin/env python3
"""Check the Rust frontend against the Python reference implementation.

This is the Phase 1 counterpart to `check-playground-parity.mjs`. That script
compares two *interpreters* by their program output; the Rust frontend has no
evaluator yet, so there is no output to compare. What can be compared is the
token stream and the parsed AST -- and later, resolved scopes.

Every input is checked at both levels. Tokens catch lexer divergence; ASTs
catch the far larger class of parser divergence, where two frontends tokenise
a program identically and then disagree about what it means.

The corpus is every .mrt file in the repository, every inline snippet in the
parity checker, every Playground example, and a set of edge cases written
here specifically to pin down lexer behaviour that the ordinary programs never
exercise (unterminated constructs, stray characters, odd escapes).

    python3 -m scripts.check_frontend_conformance      # via the wrapper below
    python3 scripts/check-frontend-conformance.py
"""

import json
import pathlib
import re
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parent.parent
MRT_CHECK = REPO / "compiler" / "target" / "debug" / "mrt-check"

sys.path.insert(0, str(REPO))

from scripts import dump_ast  # noqa: E402
from scripts.dump_tokens import dump as dump_tokens  # noqa: E402
from src.errors import MRTError  # noqa: E402


# Cases the real programs never produce, each pinning down one behaviour that
# would otherwise be free to drift.
EDGE_CASES = {
    # Numbers: no exponent form, and a dot only continues a number when a
    # digit follows it.
    "number forms": "1 1.5 0.5 10. .5 1e10 1.5e3 007",
    "number then dot": "a.0 1..2 x...y",
    # A token's line is the line it *ends* on, which only shows for strings.
    "multi-line string line number": 'var s = "a\nb\nc"; var t = 1;',
    # Escapes: only the recognised set is special; anything else keeps its
    # backslash.
    "known escapes": r'"a\nb\tc\rd\"e\\f\0g\$h"',
    "unknown escapes keep the backslash": r'"\q\w\e\8"',
    "escaped interpolation is literal": r'"\${not interpolated}"',
    # Interpolation: nesting, strings inside, braces inside.
    "interpolation basics": '"a${b}c${d}e"',
    "interpolation with nested braces": '"${ {a: 1} }"',
    "interpolation with a nested string": '"${ f("}") }"',
    "interpolation with an escaped quote inside": r'"${ f("\"") }"',
    "interpolation spanning lines": '"start${\n  1 + 2\n}end"',
    "adjacent interpolations": '"${a}${b}"',
    "interpolation only": '"${x}"',
    "empty string and empty interpolation": '"" "${}"',
    # Comments.
    "line comment at eof": "var a = 1; // trailing",
    "block comment spanning lines": "/* one\ntwo\nthree */ var a = 1;",
    "block comment not nested": "/* outer /* inner */ var a = 1;",
    # Operators that share a prefix.
    "compound operators": "+= -= *= /= %= == != <= >= && || ! = < >",
    "ellipsis vs dot": "... .. . a.b",
    # Keywords vs identifiers, including the contextual ones.
    "contextual keywords are identifiers": "from as fromage assign",
    "keywords": "func return if else while for print var true false break "
                "continue null try catch finally throw in import export "
                "struct yield match case default",
    "identifier shapes": "_a a_ _ a1 A1 __init__",
    # Non-ASCII: only inside strings and comments, and it must not shift
    # column or line accounting.
    "non-ascii in a string": '"héllo wörld 日本語"',
    "non-ascii in a comment": "// héllo\nvar a = 1;",
    # Whitespace handling.
    "crlf and tabs": "var\ta\r\n=\t1;",
    "empty file": "",
    "only whitespace": "   \n\t\n  ",
    # -- parser-only behaviour ------------------------------------------
    # Precedence and associativity.
    "precedence": "func f() { return 1 + 2 * 3 - 4 / 5 % 6 == 7 != 8 < 9 && a || !b; }",
    "unary chains": "func f() { return - - -1; } func g() { return !!!true; }",
    "left associativity": "func f() { return 1 - 2 - 3; }",
    "grouping changes the tree": "func f() { return (1 + 2) * 3; }",
    # The speculative parses: a leading brace/bracket, and for-in vs C-style.
    "block vs destructuring assignment": "func f() { var x = 0; { x = 1; } [x] = [2]; { print(x); } }",
    "object destructuring assignment vs block": "func f() { var x = 0; var y = 0; {x, y} = {x: 1, y: 2}; }",
    "array literal statement is not a pattern": "func f() { [1, 2][0]; var a = [1]; [a][0][0]; }",
    "for-in vs c-style": "func f() { for (var i = 0; i < 2; i += 1) { } for (x in [1]) { } for (var y in [1]) { } for ([a, b] in p) { } }",
    "empty c-style for": "func f() { for (;;) { break; } }",
    # `func` as declaration vs expression, decided by one token of lookahead.
    "func declaration vs expression": "func named() { } func f() { var g = func() { }; var h = func inner() { }; }",
    # yield in each legal shape, and the generator flag it sets.
    "yield shapes": 'func g() { yield 1; var a = yield 2; a = yield 3; var o = {k: 0}; o.k = yield 4; var p = 0; var q = 0; [p, q] = yield 5; yield* [1]; }',
    "yield in a nested function belongs to it": "func outer() { var inner = func() { yield 1; }; return inner; }",
    # match in both positions, with every pattern kind.
    "match statement every pattern": 'func f(v) { match (v) { case 0: print(1); case -1: print(2); case "s": print(3); case true: print(4); case null: print(5); case [a, ...r]: print(6); case {k: x}: print(7); case P(a, b): print(8); case n if (n > 0): print(9); default: print(10); } }',
    "match expression with trailing comma": 'func f(v) { return match (v) { case 1: "a", default: "b", }; }',
    "nested match expression": 'func f(v) { return match (v) { case 1: match (v) { case 1: "x", default: "y" }, default: "z" }; }',
    # Structs, imports, exports.
    "struct with defaults and methods": "struct S { a, b = 1; func m() { return this.a; } func n() { yield 1; } }",
    "export forms": 'export var a = 1; export func f() { } export struct S { x; } export { a }; export { a as b } from "./m.mrt";',
    "import forms": 'import { a, b as c } from "./m.mrt"; import * as ns from "./m.mrt";',
    # Patterns in every binding position.
    "patterns everywhere": 'func f([a, b], {c, d = 1}, ...rest) { } func g() { try { } catch ({message, kind}) { } for ([x, y] in p) { } var [q, ...r] = [1]; }',
    "nested pattern defaults": "func f() { var [a = 1, [b = 2], {c = 3}] = x; }",
    # Interpolation sub-parsing.
    "interpolation sub-expressions": 'func f() { print("${1 + 2} ${ {a: 1} } ${ g(h(1)) } ${ match (1) { case 1: "x", default: "y" } }"); }',
    "interpolation nested template": 'func f() { print("outer ${ "inner ${1}" }"); }',
    # Calls, indexing, spread.
    "call chains and indexing": "func f() { return a.b.c[0](1)(2)[x].y; }",
    "spread positions": "func f() { g(...a, b); var arr = [...a, ...b]; print(...a); }",
    "compound assignment desugaring": "func f() { var x = 1; x += 1; x -= 1; x *= 2; x /= 2; x %= 2; a[i] += 1; o.k *= 2; }",
    # return's same-line rule.
    "bare return vs value on next line": "func f() { return\n  1; }",
    "return with value": "func f() { return 1; }",
    # Optional semicolons.
    "no semicolons anywhere": "func f() { var a = 1 var b = 2 print(a) }",

    # Parser error cases, including multi-error recovery.
    "missing paren in func declaration": "func main( { print(1); }",
    "invalid assignment target": "func f() { 1 = 2; }",
    "yield outside a function": "yield 1;",
    "yield in a nested expression": "func f() { print(1 + yield 2); }",
    "yield star as an expression": "func f() { var x = yield* [1]; }",
    "import not at top level": 'func f() { import { a } from "./m.mrt"; }',
    "export not at top level": "func f() { export var a = 1; }",
    "duplicate struct field": "struct S { a, a; }",
    "field and method clash": "struct S { a; func a() { } }",
    "required param after default": "func f(a = 1, b) { }",
    "rest param not last": "func f(...a, b) { }",
    "rest param with default": "func f(...a = 1) { }",
    "two defaults in a match": "func f() { match (1) { default: print(1); default: print(2); } }",
    "case after default": "func f() { match (1) { default: print(1); case 1: print(2); } }",
    "empty match": "func f() { match (1) { } }",
    "try with no catch or finally": "func f() { try { } }",
    "destructuring declaration with no initializer": "func f() { var [a, b]; }",
    "export a destructuring var": "export var [a, b] = x;",
    "multiple errors recover": "func f() { var = 1; }\nfunc g() { print(); }\nfunc h() { var = 2; }",
    "unterminated block": "func f() { print(1);",
    "empty interpolation": 'func f() { print("${}"); }',
    "trailing tokens in interpolation": 'func f() { print("${1 2}"); }',

    # Error cases: both sides must reject these identically.
    "unterminated string": '"abc',
    "unterminated string with newline": '"abc\ndef',
    "unterminated interpolation": '"${1 + 2"',
    "unterminated interpolation no close": '"${1 + 2',
    "unterminated block comment": "/* never closed",
    "stray ampersand": "a & b",
    "stray pipe": "a | b",
    "stray character": "a @ b",
    "stray character non-ascii": "a § b",
    "unterminated string inside interpolation": '"${ f("unclosed }"',
}


def python_tokens(source: str):
    """(ok, text) -- the token dump, or the MRT 1.x error line."""
    try:
        return True, dump_tokens(source)
    except MRTError as e:
        return False, f"Syntax Error: {e}"


def python_ast(source: str):
    """(ok, text) -- the AST dump, or every MRT 1.x parse error."""
    return dump_ast.dump(source)


def rust_dump(source: str, flag: str):
    with tempfile.NamedTemporaryFile("w", suffix=".mrt", delete=False) as f:
        f.write(source)
        path = f.name
    try:
        proc = subprocess.run(
            [str(MRT_CHECK), flag, "--compat", path],
            capture_output=True, text=True,
        )
        if proc.returncode == 0:
            return True, proc.stdout
        return False, proc.stderr.strip()
    finally:
        pathlib.Path(path).unlink(missing_ok=True)


def corpus():
    """(name, source) for everything worth lexing."""
    for path in sorted(REPO.glob("examples/**/*.mrt")):
        yield f"example: {path.relative_to(REPO)}", path.read_text()

    parity = (REPO / "scripts" / "check-playground-parity.mjs").read_text()
    for name, src in re.findall(r"name: '([^']*)',\n\s*src: '(.*)',\n", parity):
        yield f"parity snippet: {name}", src.replace("\\'", "'").replace("\\\\", "\\")

    tsx = (REPO / "src" / "pages" / "Playground.tsx").read_text()
    for name, code in re.findall(r"name: '([^']*)',\n\s*code: `(.*?)`\n", tsx, re.S):
        source = (code.replace("\\`", "`").replace("\\${", "${").replace("\\\\", "\\"))
        yield f"playground: {name}", source

    for name, src in EDGE_CASES.items():
        yield f"edge case: {name}", src


def main() -> int:
    if not MRT_CHECK.exists():
        print(f"error: {MRT_CHECK} not built. Run: cargo build --manifest-path "
              f"compiler/Cargo.toml", file=sys.stderr)
        return 2

    # Tokens catch lexer divergence; ASTs catch the much larger class where
    # two frontends tokenise identically and then disagree about meaning.
    levels = (
        ("tokens", python_tokens, "--dump-tokens"),
        ("ast", python_ast, "--dump-ast"),
    )

    checked = failures = 0
    for name, source in corpus():
        for level, py_fn, flag in levels:
            checked += 1
            label = f"{name} [{level}]"
            py_ok, py_out = py_fn(source)
            rs_ok, rs_out = rust_dump(source, flag)

            if py_ok == rs_ok and py_out.rstrip("\n") == rs_out.rstrip("\n"):
                print(f"\u2713 {label}")
                continue

            failures += 1
            print(f"\n\u2717 MISMATCH: {label}")
            print(f"  source: {json.dumps(source[:200])}")
            if py_ok != rs_ok:
                print(f"  python {'accepted' if py_ok else 'rejected'}, "
                      f"rust {'accepted' if rs_ok else 'rejected'}")
            py_lines = py_out.splitlines()
            rs_lines = rs_out.splitlines()
            shown = 0
            for i in range(max(len(py_lines), len(rs_lines))):
                a = py_lines[i] if i < len(py_lines) else "<missing>"
                b = rs_lines[i] if i < len(rs_lines) else "<missing>"
                if a != b:
                    print(f"  line {i + 1}:\n    python: {a}\n    rust:   {b}")
                    shown += 1
                    if shown >= 6:
                        print("  ... (further differences suppressed)")
                        break

    print(f"\n{checked - failures}/{checked} frontend conformance checks matched.")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
