# The MRT 2.0 compiler frontend

A new frontend for MRT, written in Rust. **Phase 1 is complete**: diagnostics,
lexer, AST, parser and resolver. There is no evaluator here yet, and the MRT 1.x
Python and TypeScript interpreters remain the implementations that actually run
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
a set of hand-written edge cases through **both** frontends and requires
byte-identical results — including identical rejections, with identical
messages and line numbers, and including files that produce *several* errors.

Every input is checked at two levels:

* **Tokens** catch lexer divergence.
* **ASTs** catch the far larger class of parser divergence, where two
  frontends tokenise a program identically and then disagree about what it
  means.

```bash
cargo build --manifest-path compiler/Cargo.toml
python3 scripts/check-frontend-conformance.py
```

The harness has been mutation-tested at both levels, because a harness that
cannot fail is not a harness:

| Broken deliberately | Checks failed |
|---|---|
| A bare `.` continues a number | 2 |
| Tokens stamped with their start line | 44 |
| Unrecognised escape drops its backslash | 1 |
| Binary operands swapped | 84 |
| A leading `{` never opens a pattern | 18 |
| The generator flag is never set | 32 |
| `a.b` not desugared to `a["b"]` | 57 |
| Parent locals never flagged captured | 14 |
| Slot counter advanced twice | 147 |
| Captures never deduplicated | 7 |

That last row is there because the first attempt at it was **not** caught:
duplicate captures are individually well-formed — each names a real parent
local — so nothing noticed. A backend would have allocated one cell per entry
and closures sharing a variable would have quietly stopped sharing it. The
missing invariant was added because the mutation escaped.

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
│   ├── mrt-lexer/         tokens and the scanner
│   ├── mrt-ast/           the tree, and its canonical dump format
│   ├── mrt-parser/        recursive descent, with error recovery
│   └── mrt-resolver/      names to frame slots, captures and globals
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
mrt-check --dump-ast FILE      # canonical AST
mrt-check --dump-scopes FILE   # resolved frame layout
mrt-check --compat FILE        # MRT 1.x error text, byte for byte
```

`--compat` selects the *error format* and composes with either dump, which is
how the harness gets a machine-comparable tree and comparable error text in
one run.

## The resolver

Every name becomes a numbered slot in the current frame, an index into that
frame's capture list, or a global looked up by name. MRT 1.x does this work at
run time by walking a chain of dictionaries, once per read.

```
$ mrt-check --dump-scopes counter.mrt
module
  function "makeCounter" slots=2 captures=0
    slot 0 "start"
    slot 1 "count" captured
    function "<anonymous>" slots=0 captures=1
      capture 0 "count" <- parent local 1
      use "count" 6:9 -> capture 0
```

It **reports no errors, on purpose.** MRT's semantics are dynamic: a name the
resolver cannot find is not a mistake, it is a global, and whether it exists is
decided when the program runs. Reporting "undefined variable" here would reject
programs the reference implementation accepts, which is the exact divergence
the conformance harness exists to prevent.

The behaviours it has to mirror:

* **The module scope is global, not a frame.** A top-level `var` is stored in
  the interpreter's `globals`, so it resolves as a global even from the same
  file.
* **Blocks are scopes but not frames.** A `{ }` block, a `for` header, a
  `catch` clause and each `match` case introduce a scope; all draw slots from
  the enclosing *function's* single slot space.
* **`for`-`in` binds afresh on each iteration**, so its loop variable is
  flagged `per-iteration`: a backend must give each turn its own cell rather
  than reusing one slot. A C-style `for` variable is not.
* **A method's `this`** is modelled as a local of the method.

Since there is no Python counterpart, the resolver cannot be compared — only
exercised. The harness runs it over every corpus program that parses, with its
internal invariants checked, which is how a broken capture chain fails across a
hundred real programs rather than only in unit tests.

## Spans are the part conformance cannot check

The Python parser has no spans, so nothing in the conformance harness can tell
whether a span is *right* — only that the tree shape matches. Span correctness
is therefore covered by Rust unit tests instead, including the awkward case: an
expression inside a `${...}` run is parsed from a detached fragment, and its
spans have to be shifted back onto the real file or every diagnostic inside a
template points at the wrong place.

## What is not here yet

The resolver, and everything downstream of it. The 314 behavioural tests and
146 interpreter parity cases are the gate for a backend that can run programs;
they cannot be applied to a frontend that produces no output, so token- and
AST-level conformance is what Phase 1 is held to.
