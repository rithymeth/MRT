# The MRT 2.0 compiler frontend

A new frontend for MRT, written in Rust. This is **Phase 1**: lexer, and next
parser and resolver. There is no evaluator here yet, and the MRT 1.x Python
and TypeScript interpreters remain the implementations that actually run
programs.

## Why this exists

Two things the existing frontend cannot be retrofitted with cheaply:

1. **Source spans.** MRT 1.x tracks one line number per token, so every error
   it can produce reads `[line 17]`. Everything built later — a language
   server, a formatter, a type checker that says "expected number, found
   string" and underlines the string — needs real ranges.
2. **A resolver.** MRT 1.x resolves names at run time by walking a chain of
   `Environment` dictionaries. Resolving each name to a `(depth, slot)` pair
   at compile time is both faster and the analysis any bytecode backend would
   need.

```
$ mrt-check bad.mrt
error: Unexpected character '&' (did you mean '&&'?)
 --> bad.mrt:2:23
  |
2 |     var total = price & tax;
  |                       ^
```

versus MRT 1.x:

```
Syntax Error: Unexpected character '&' (did you mean '&&'?) [line 2]
```

## Conformance is the whole game

This frontend is only useful if it agrees with the reference implementation
about what MRT *is*. It is not a chance to quietly improve the language.

`scripts/check-frontend-conformance.py` feeds every bundled example, every
inline snippet in the interpreter parity checker, every Playground example and
a set of hand-written lexer edge cases through **both** frontends and requires
byte-identical results — including identical rejections, with identical
messages and line numbers.

```bash
cargo build --manifest-path compiler/Cargo.toml
python3 scripts/check-frontend-conformance.py
```

The harness has been mutation-tested: breaking the Rust lexer in three
plausible ways (a bare `.` continuing a number, stamping tokens with their
start line, dropping the backslash on an unrecognised escape) makes 2, 44 and
1 checks fail respectively. A harness that cannot fail is not a harness.

Three inherited behaviours are deliberate, and each looks like a bug until you
try to change it:

* **A token's `line` is the line it *ends* on.** Only observable on a
  multi-line string, which MRT 1.x reports at its closing quote. `span` carries
  the truth; `line` carries the compatibility.
* **There is no exponent notation.** `1e10` is the number `1` followed by the
  identifier `e10`.
* **An unrecognised escape keeps its backslash.** `"\q"` is two characters.

## Layout

```
compiler/
├── crates/
│   ├── mrt-diagnostics/   spans, source maps, two error renderers
│   └── mrt-lexer/         tokens and the scanner
└── cli/                   mrt-check, the frontend driver
```

Offsets count `char`s, not bytes, matching how the Python lexer indexes its
source — so both frontends report the same column in a file with non-ASCII
text.

Zero external crates, on purpose: a hand-written frontend needs none, and a
hermetic build keeps `cargo test` working offline.

## Commands

```bash
cargo build --manifest-path compiler/Cargo.toml
cargo test  --manifest-path compiler/Cargo.toml
cargo clippy --manifest-path compiler/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path compiler/Cargo.toml --all --check

mrt-check FILE                 # rich diagnostics
mrt-check --dump-tokens FILE   # canonical token stream
mrt-check --compat FILE        # MRT 1.x error text, byte for byte
```

## What is not here yet

The parser, the resolver, and everything downstream of them. The 314
behavioural tests and 146 interpreter parity cases are the gate for a backend
that can run programs; they cannot be applied to a frontend that produces no
output, so token- and later AST-level conformance is what Phase 1 is held to.
