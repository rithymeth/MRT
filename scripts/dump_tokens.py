"""Emit the canonical token dump for an MRT source file.

The other half of this format lives in `compiler/cli/src/main.rs`. Both sides
must produce byte-identical output for the same input; that equality is what
`scripts/check-frontend-conformance.py` checks, and it is the only evidence
that the Rust lexer really is a drop-in replacement for this one.

Numbers are written as the raw IEEE-754 bit pattern rather than as text,
because formatting a float is exactly where two languages quietly disagree
(Python renders 1e20 as `1e+20`, Rust as `100000000000000000000`) and this
comparison is about lexing, not float printing.

    python3 -m scripts.dump_tokens FILE
"""

import struct
import sys

from src.errors import MRTError
from src.lexer import Lexer


def quote(s: str) -> str:
    """JSON-style quoting, matching `quote` in the Rust CLI, so a lexeme
    containing a newline or tab stays on one line of the dump."""
    out = ['"']
    for c in s:
        if c == '"':
            out.append('\\"')
        elif c == '\\':
            out.append('\\\\')
        elif c == '\n':
            out.append('\\n')
        elif c == '\r':
            out.append('\\r')
        elif c == '\t':
            out.append('\\t')
        elif ord(c) < 0x20:
            out.append(f'\\u{ord(c):04x}')
        else:
            out.append(c)
    out.append('"')
    return "".join(out)


def literal_repr(literal) -> str:
    if literal is None:
        return "-"
    # bool is a subclass of int in Python, so it must be tested first or
    # `True` would be reported as the number 1.
    if isinstance(literal, bool):
        return f"B:{'true' if literal else 'false'}"
    if isinstance(literal, float):
        bits = struct.unpack("<Q", struct.pack("<d", literal))[0]
        return f"N:{bits:016x}"
    if isinstance(literal, str):
        return f"S:{quote(literal)}"
    if isinstance(literal, list):
        parts = []
        for part in literal:
            if part[0] == "str":
                parts.append(f"T:{quote(part[1])}")
            else:
                _, source, line = part
                parts.append(f"E:{quote(source)}@{line}")
        return "[" + ",".join(parts) + "]"
    raise AssertionError(f"unexpected literal {literal!r}")


def dump(source: str) -> str:
    tokens = Lexer(source).scan_tokens()
    return "".join(
        f"{t.type.name}\t{quote(t.lexeme)}\t{literal_repr(t.literal)}\t{t.line}\n"
        for t in tokens
    )


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: python3 -m scripts.dump_tokens FILE", file=sys.stderr)
        return 2
    with open(sys.argv[1], "r") as f:
        source = f.read()
    try:
        sys.stdout.write(dump(source))
    except MRTError as e:
        # Same shape as `python -m src`, so an error case can be compared too.
        print(f"Syntax Error: {e}", file=sys.stderr)
        return 65
    return 0


if __name__ == "__main__":
    sys.exit(main())
