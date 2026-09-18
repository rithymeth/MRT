# The MRT 2.0 compiler

A new implementation of MRT, written in Rust: diagnostics, lexer, AST, parser,
resolver, and a tree-walking interpreter that runs programs. It is the third
implementation of the language, alongside the Python reference and the
Playground's TypeScript one, and it is held to the same conformance corpus as
they are.

Generators are the one thing it does not implement — see [What is not here
yet](#what-is-not-here-yet).

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
│   ├── mrt-resolver/      names to frame slots, captures and globals
│   └── mrt-interp/        values, environments, and the tree-walker
└── cli/                   mrt-check (frontend driver), mrt-run (runtime)
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

mrt-run FILE                   # run a program
mrt-run --bench 5 FILE         # time 5 in-process runs, as JSON
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

## The interpreter

`mrt-interp` is a tree-walker, deliberately: the benchmarks showed the
TypeScript tree-walker running ~10x faster than the Python one over the same
AST shapes, which said the implementation language was worth more than the
architecture — and the only way to find out was to build one and measure it.

It is held to the same corpus the Playground's interpreter is:

```bash
node scripts/check-interp-conformance.mjs   # or: npm run check:rust-interp
```

Every bundled example and every shared regression case in
`scripts/parity-cases.mjs` is run through the Python reference and through
`mrt-run`, and the two outputs — stdout and error text alike — must be
byte-identical. Cases that stop on a feature this interpreter does not have
yet are recorded as *unsupported* rather than skipped, and the unsupported set
is a **ratchet**: the harness fails both when a case newly stops working and
when a listed case starts working. Implementing generators is expected to make
it fail once, on purpose, until the list is shortened.

### Modules

`import` / `export` work, including namespace imports, re-exports, module
caching and cycle detection, and the resolution rules are the reference
implementation's: only `./` and `../` specifiers resolve, so an `import`
always names exactly one file and reading the source tells you which.

Two details are load-bearing rather than incidental:

* **Paths are normalised lexically, not canonicalised.** `canonicalize`
  resolves symlinks and requires the file to exist, so a missing module would
  fail with the OS's error instead of MRT's `Cannot find module`, and two
  importers reaching one file through different symlinked paths would get two
  evaluations instead of the cache hit the language promises.
* **An export table is an ordered `Vec`, not a map.** A namespace import binds
  it as an object, and `keys()` on that object is observable — so export order
  is part of the language. Declarations hoist, which is why `export var PI`
  followed by `export func square` yields `[square, PI]`.

The decisions it had to make on its own — the ones where agreeing with the
other two implementations is not automatic — are pinned by unit tests in
`crates/mrt-interp/src/lib.rs`: reference semantics for aggregates, Python's
exponent form for large and small numbers, `true` and `1` as distinct object
keys, `%` taking the sign of its dividend, a `for`-`in` variable rebound each
iteration.

Building it found a defect in the other two. Indexing raises from a helper
several frames below the expression, and nothing put the line back: every
`IndexError`, `KeyError` and bad-key `TypeError` arrived with `line` null and
printed with no `[line N]` suffix at all. Both existing implementations agreed
on it, so parity said nothing. The third one reported the line and gave it
away; all three now do, and a parity case pins the values.

Results are in [`benchmarks/README.md`](../benchmarks/README.md). The short
version: **24x faster than Python overall, and 2.2x faster than TypeScript** —
but that second number splits sharply by workload, from 8x on recursion down
to 0.9x on a tight numeric loop, where V8's JIT beats an unoptimised Rust
tree-walker.

## What is not here yet

**Generators.** Both existing implementations suspend one by delegating to a
host coroutine — Python's `yield from`, JavaScript's `yield*` — and stable
Rust has no equivalent. The options are all expensive: a thread per generator
forces `Arc<Mutex<..>>` through the whole interpreter and gives back the
performance this was built to measure; async-as-generators fights the borrow
checker for a tree-walker holding `&mut self` across a yield; and an explicit
resumable evaluator is most of a bytecode VM already.

That last point is why generators are deferred rather than hacked around: in
Rust the natural way to suspend execution *is* an instruction pointer over a
flat program. The feature that is hardest to port is also the one that argues
hardest for the VM — a better case for it than the timings make.

**Everything downstream of the tree-walker**: HIR, MIR, bytecode, WASM. The
interpreter exists partly to give those a number to beat.
