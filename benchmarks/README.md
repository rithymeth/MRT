# MRT benchmarks

Ten programs, each exercising one cost the interpreter actually pays, plus a
runner that times them on both implementations.

```bash
python3 scripts/bench.py              # both interpreters, comparison table
python3 scripts/bench.py --profile    # where Python's time actually goes
python3 scripts/bench.py fib loops    # just these
```

## Why these exist

The MRT 2.0 proposal calls tree-walking "the ceiling" and lists performance as
a major limitation. That may well be true, but until these existed the
repository had no numbers at all — which means no way to tell whether a
bytecode VM is worth building, and no bar for it to clear if it is.

Every benchmark prints a **checksum**, and the runner refuses a result whose
checksum is wrong. A benchmark that silently stopped doing its work would
otherwise report an excellent time; this repository has already shipped one
test that passed by not running (see the `1e10` note in
`scripts/check-playground-parity.mjs`), so the guard is not theoretical.

## What each one measures

| Benchmark | Cost it isolates |
|---|---|
| `fib` | function call and return overhead, via naive recursion |
| `loops` | raw statement/expression dispatch in a tight numeric loop |
| `arrays` | index reads and writes, `push`, and array growth |
| `closures` | closure creation and calls through captured variables |
| `scopes_shallow` / `scopes_deep` | a matched pair isolating variable-lookup depth |
| `strings` | string building and the built-in string functions |
| `generators` | suspension and resumption, and a lazy pipeline |
| `matching` | `match` dispatch over shapes, including struct patterns |
| `structs` | instance construction and method dispatch |

`scopes_shallow` and `scopes_deep` are a matched pair: identical work, same
iteration count, differing only in how far up the scope chain the names live.
The gap between them is the price of resolving names at run time, and
therefore the ceiling on what a resolver could win back.

## Results

Measured on this machine, best of 5 runs, in-process (no start-up cost).
Re-measure before trusting these — they are a snapshot, not a constant.

```
benchmark                 python    typescript    ratio
-------------------------------------------------------
arrays                 1085.0 ms       51.0 ms    21.3x
closures                898.7 ms      170.0 ms     5.3x
fib                     963.1 ms      208.3 ms     4.6x
generators              760.3 ms      190.1 ms     4.0x
loops                  1267.9 ms       38.8 ms    32.7x
matching               1084.5 ms      151.7 ms     7.1x
scopes_deep            2069.1 ms      105.5 ms    19.6x
scopes_shallow         1764.5 ms       61.9 ms    28.5x
strings                 671.2 ms       47.7 ms    14.1x
structs                1140.1 ms      158.8 ms     7.2x
-------------------------------------------------------
TOTAL                 11704.6 ms     1183.8 ms     9.9x
```

## What the numbers say

Three findings, and none of them is the one the MRT 2.0 proposal assumed.

**1. Scope depth is not the bottleneck.** Eight extra levels of nesting cost
**17%** (1764 ms → 2069 ms). That is real, but it is not the shape of a
problem that justifies a new execution architecture on its own, and typical
code sits one or two levels deep rather than eight.

**2. Dispatch dominates, not lookup.** Under `--profile`, on `loops`:

| | share of time |
|---|---|
| `evaluate` + `execute` + `evaluate_binary` — AST dispatch | **~66%** |
| `Environment.get` + `.assign` — name lookup | ~10% |

On `scopes_deep`, lookup rises to ~29% and dispatch is still ~53%. So
resolving names to slots helps, and helps more the deeper the nesting — but
the thing actually consuming the time is walking the tree at all. Only a
different execution model changes that. (cProfile inflates call-heavy code,
so treat these as proportions, not absolutes.)

**3. The implementation language is worth about 10x on its own.** The
TypeScript interpreter is *also* a tree-walker, running the same algorithms
over the same AST shapes, and it is **9.9x faster overall** — 32x on the
tightest loop. That number arrives before any architectural change at all.

The third finding is the one that should inform planning. A Rust *tree-walker*
would plausibly land in the same territory as the TypeScript one, for a small
fraction of the effort of a bytecode VM with a garbage collector — and it
would make the bytecode question answerable with a measurement rather than a
projection, because there would be a fast tree-walker to beat.

None of this says a bytecode VM is a bad idea. It says the ordering matters:
the resolver is worth having (and is a prerequisite either way), the language
change is worth more than expected, and the VM should be justified against a
number rather than against Python.

