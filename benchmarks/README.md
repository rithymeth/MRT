# MRT benchmarks

Ten programs, each exercising one cost the interpreter actually pays, plus a
runner that times them on all three implementations.

```bash
python3 scripts/bench.py              # all three, side by side
python3 scripts/bench.py --profile    # where Python's time actually goes
python3 scripts/bench.py fib loops    # just these
```

All three are timed in-process — Python directly, TypeScript through a Node
runner, Rust through `mrt-run --bench` — so none of them is charged for
process start-up while the others are not. The Rust interpreter does not
implement generators yet, so `generators` shows `n/a` for it rather than a
number; the TOTAL ratios only cover the benchmarks an implementation actually
ran.

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
Re-measure before trusting these — they are a snapshot, not a constant, and
they move by several percent between runs.

```
benchmark                 python   typescript       rust   py/ts   py/rs   ts/rs
--------------------------------------------------------------------------------
arrays                 1347.2 ms      58.8 ms    51.9 ms   22.9x   26.0x    1.1x
closures               1024.7 ms     178.9 ms    36.6 ms    5.7x   28.0x    4.9x
fib                    1008.0 ms     246.0 ms    30.8 ms    4.1x   32.7x    8.0x
generators             1018.5 ms     204.9 ms        n/a    5.0x       -       -
loops                  1426.8 ms      46.2 ms    51.1 ms   30.9x   27.9x    0.9x
matching               1177.6 ms     165.1 ms    53.1 ms    7.1x   22.2x    3.1x
scopes_deep            2308.7 ms     120.9 ms    88.1 ms   19.1x   26.2x    1.4x
scopes_shallow         1972.8 ms      66.6 ms    73.0 ms   29.6x   27.0x    0.9x
strings                 744.0 ms      53.6 ms    42.2 ms   13.9x   17.6x    1.3x
structs                1259.2 ms     169.7 ms    82.9 ms    7.4x   15.2x    2.0x
--------------------------------------------------------------------------------
TOTAL                 13287.4 ms    1310.7 ms   509.6 ms   10.1x   24.1x    2.2x
```

## What the numbers say

Four findings, and none of the first three is what the MRT 2.0 proposal
assumed.

**1. Scope depth is not the bottleneck.** Eight extra levels of nesting cost
**17%** in Python. That is real, but it is not the shape of a problem that
justifies a new execution architecture on its own, and typical code sits one
or two levels deep rather than eight.

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
over the same AST shapes, and it is **~10x faster overall** than Python —
31x on the tightest loop, before any architectural change at all.

That finding is what the Rust tree-walker was built to test, and the
`ts/rs` column is the answer.

**4. The Rust tree-walker is 24x faster than Python — and the gap to
TypeScript is entirely about what the benchmark does.** Two tree-walkers,
same AST shapes, same algorithms, differing only in their host:

| where Rust wins big | ts/rs | |
|---|---|---|
| `fib` | 8.0x | recursion: one call frame per unit of work |
| `closures` | 4.9x | closure creation and calls through captures |
| `matching` | 3.1x | pattern dispatch, allocating bindings per arm |
| `structs` | 2.0x | instance construction and method dispatch |

| where it does not | ts/rs | |
|---|---|---|
| `loops` | 0.9x | tight numeric loop — V8's JIT is *faster* |
| `scopes_shallow` | 0.9x | mostly loop, same story |
| `arrays` | 1.1x | index reads and writes in a loop |
| `strings` | 1.3x | mostly time inside built-in string functions |

The split is sharp and it is not noise: where a benchmark is a hot numeric
loop, V8 compiles the interpreter's inner loop to machine code and matches or
beats unoptimised Rust. Where a benchmark allocates an environment, builds a
closure, or pushes a call frame, Rust wins by 2–8x, because that is where
`Rc<RefCell<..>>` and a stack frame beat a GC'd object and a megamorphic call
site.

## What this means for the bytecode VM

The VM's case now has a number to clear, which is the point of having built
the tree-walker first.

The costs the VM is supposed to remove — AST dispatch, environment
allocation per call, name lookup — are exactly the costs in the *first*
table, the ones where Rust already wins 2–8x by removing the host language's
overhead rather than the architecture's. The costs in the *second* table are
the ones a VM would attack directly, and those are the ones where an
unoptimised Rust tree-walker is already at the JIT's level.

So a bytecode VM is not obviously the next 10x. It is plausibly the next 2–3x
on call-heavy code, on top of a tree-walker that is already there. What the
measurement does say clearly is that generators — the one feature this
interpreter cannot implement without a resumable evaluator — argue for the VM
more strongly than the timings do: in Rust the natural way to suspend
execution *is* an explicit instruction pointer over a flat program. The
feature that is hardest to port is the one that most wants the new
architecture.
