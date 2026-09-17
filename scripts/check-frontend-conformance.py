#!/usr/bin/env python3
"""Check the Rust frontend against the Python reference implementation.

This is the Phase 1 counterpart to `check-playground-parity.mjs`. That script
compares two *interpreters* by their program output; the Rust frontend has no
evaluator yet, so there is no output to compare. What can be compared is the
token stream, and later the AST and resolved scopes.

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

from scripts.dump_tokens import dump  # noqa: E402
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


def python_dump(source: str):
    """(ok, text) -- the dump, or the MRT 1.x error line."""
    try:
        return True, dump(source)
    except MRTError as e:
        return False, f"Syntax Error: {e}"


def rust_dump(source: str):
    with tempfile.NamedTemporaryFile("w", suffix=".mrt", delete=False) as f:
        f.write(source)
        path = f.name
    try:
        proc = subprocess.run(
            [str(MRT_CHECK), "--dump-tokens", "--compat", path],
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

    checked = failures = 0
    for name, source in corpus():
        checked += 1
        py_ok, py_out = python_dump(source)
        rs_ok, rs_out = rust_dump(source)

        if py_ok == rs_ok and py_out.rstrip("\n") == rs_out.rstrip("\n"):
            print(f"✓ {name}")
            continue

        failures += 1
        print(f"\n✗ MISMATCH: {name}")
        print(f"  source: {json.dumps(source[:200])}")
        if py_ok != rs_ok:
            print(f"  python {'accepted' if py_ok else 'rejected'}, "
                  f"rust {'accepted' if rs_ok else 'rejected'}")
        py_lines = py_out.splitlines()
        rs_lines = rs_out.splitlines()
        for i in range(max(len(py_lines), len(rs_lines))):
            p = py_lines[i] if i < len(py_lines) else "<missing>"
            r = rs_lines[i] if i < len(rs_lines) else "<missing>"
            if p != r:
                print(f"  line {i + 1}:\n    python: {p}\n    rust:   {r}")

    print(f"\n{checked - failures}/{checked} frontend conformance checks matched.")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
