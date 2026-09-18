#!/usr/bin/env python3
"""Time the MRT interpreters.

    python3 scripts/bench.py              # all three interpreters, side by side
    python3 scripts/bench.py --profile    # where the Python interpreter's time goes
    python3 scripts/bench.py fib loops    # only these

Why this exists: the MRT 2.0 proposal calls tree-walking "the ceiling" and
lists performance as a major limitation. Possibly so -- but until this script
existed the repository had no numbers at all, which means no way to judge
whether a bytecode VM is worth building and no bar for one to clear.

Every benchmark prints a checksum and this runner refuses a result whose
checksum changed, because a benchmark that quietly stopped doing its work
would otherwise post an excellent time. That guard is not hypothetical: this
repository has already shipped a parity case that passed by not running.
"""

import argparse
import contextlib
import cProfile
import io
import pathlib
import pstats
import statistics
import subprocess
import sys
import time

REPO = pathlib.Path(__file__).resolve().parent.parent
BENCH_DIR = REPO / "benchmarks"

sys.path.insert(0, str(REPO))


def run_python(path: pathlib.Path) -> tuple[float, str]:
    """Time one run in-process, so the measurement is the interpreter rather
    than Python's start-up."""
    from mrt.interpreter import Interpreter
    from mrt.lexer import Lexer
    from mrt.parser import Parser

    source = path.read_text()
    statements = Parser(Lexer(source).scan_tokens()).parse()
    interpreter = Interpreter(module_path=str(path))

    # The interpreter both captures output and echoes it to stdout, so the
    # echo is swallowed here -- otherwise every run would spray checksums
    # through the table.
    start = time.perf_counter()
    with contextlib.redirect_stdout(io.StringIO()):
        interpreter.interpret(statements)
    elapsed = time.perf_counter() - start
    output = interpreter.get_output()
    # get_output() already joins the lines; joining it again would iterate
    # characters and quietly turn every checksum into confetti.
    return elapsed, output if isinstance(output, str) else "\n".join(output)


NODE_RUNNER = r"""
const {runMRT} = require(process.argv[2]);
const fs = require('fs');
const src = fs.readFileSync(process.argv[3], 'utf8');
// One untimed warm-up so the comparison is steady-state rather than a
// measurement of V8 deciding whether this code is worth compiling.
runMRT(src, {});
const times = [];
let out = '';
for (let i = 0; i < Number(process.argv[4]); i++) {
  const t = process.hrtime.bigint();
  const r = runMRT(src, {});
  times.push(Number(process.hrtime.bigint() - t) / 1e9);
  out = (r.errors.length ? r.errors : r.output).join('\n');
}
console.log(JSON.stringify({times, out}));
"""


def build_ts_bundle(tmp: pathlib.Path) -> pathlib.Path | None:
    bundle = tmp / "mrtInterpreter.cjs"
    script = (
        "require('esbuild').buildSync({entryPoints:['src/lib/mrtInterpreter.ts'],"
        f"bundle:true,format:'cjs',outfile:{str(bundle)!r},logLevel:'silent'}})"
    )
    try:
        subprocess.run(["node", "-e", script], cwd=REPO, check=True,
                       capture_output=True, text=True)
    except (OSError, subprocess.CalledProcessError):
        return None
    return bundle


def run_typescript(bundle: pathlib.Path, path: pathlib.Path, runs: int):
    import json

    runner = bundle.parent / "run.cjs"
    runner.write_text(NODE_RUNNER)
    proc = subprocess.run(
        ["node", str(runner), str(bundle), str(path), str(runs)],
        cwd=REPO, capture_output=True, text=True,
    )
    if proc.returncode != 0:
        return None, proc.stderr.strip()
    data = json.loads(proc.stdout)
    return data["times"], data["out"]


RUST_BINARY = REPO / "compiler" / "target" / "release" / "mrt-run"


def build_rust() -> pathlib.Path | None:
    """Build the release binary, or give up quietly if there is no cargo."""
    try:
        subprocess.run(["cargo", "build", "--release", "--bin", "mrt-run"],
                       cwd=REPO / "compiler", check=True, capture_output=True, text=True)
    except (OSError, subprocess.CalledProcessError):
        return None
    return RUST_BINARY if RUST_BINARY.exists() else None


def run_rust(binary: pathlib.Path, path: pathlib.Path, runs: int):
    """Time the Rust interpreter in-process via its own --bench mode.

    Spawning it once per run would measure process start-up, which the other
    two are not charged for. `--bench` loops inside the binary and reports the
    same shape of result as the Node runner does."""
    import json

    proc = subprocess.run(
        [str(binary), "--bench", str(runs), str(path)],
        cwd=REPO, capture_output=True, text=True,
    )
    if proc.returncode != 0:
        return None, proc.stderr.strip()
    data = json.loads(proc.stdout)
    return data["times"], data["out"]


def checksum_of(output: str) -> str:
    for line in output.splitlines():
        if line.startswith("checksum"):
            return line
    return output.strip()[:40] or "<no output>"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("only", nargs="*", help="benchmark names to run")
    parser.add_argument("--runs", type=int, default=5, help="timed runs each (default 5)")
    parser.add_argument("--profile", action="store_true",
                        help="profile the Python interpreter instead of timing")
    parser.add_argument("--python-only", action="store_true")
    args = parser.parse_args()

    names = sorted(p.stem for p in BENCH_DIR.glob("*.mrt"))
    if args.only:
        unknown = set(args.only) - set(names)
        if unknown:
            print(f"unknown benchmark(s): {', '.join(sorted(unknown))}", file=sys.stderr)
            print(f"available: {', '.join(names)}", file=sys.stderr)
            return 2
        names = [n for n in names if n in args.only]

    if args.profile:
        return profile(names)

    import tempfile

    tmp = pathlib.Path(tempfile.mkdtemp(prefix="mrt-bench-"))
    bundle = None if args.python_only else build_ts_bundle(tmp)
    if bundle is None and not args.python_only:
        print("note: could not build the TypeScript bundle; skipping it\n")
    rust = None if args.python_only else build_rust()
    if rust is None and not args.python_only:
        print("note: could not build the Rust interpreter; skipping it\n")

    header = (f"{'benchmark':<20}{'python':>12}{'typescript':>13}{'rust':>11}"
              f"{'py/ts':>8}{'py/rs':>8}{'ts/rs':>8}")
    print(header)
    print("-" * len(header))

    failures = 0
    # Ratios are only meaningful over benchmarks both sides ran, so each
    # comparison keeps its own Python total rather than sharing one.
    py_total = ts_total = rs_total = 0.0
    py_vs_ts = py_vs_rs = ts_vs_rs = 0.0
    for name in names:
        path = BENCH_DIR / f"{name}.mrt"

        py_times, py_out = [], ""
        for _ in range(args.runs):
            elapsed, py_out = run_python(path)
            py_times.append(elapsed)
        py = min(py_times)
        py_total += py

        expected = checksum_of(py_out)
        if not expected.startswith("checksum"):
            print(f"{name:<20}  FAILED: {expected}")
            failures += 1
            continue

        def other(label, times, out):
            """One competing implementation's cell, and the time to total."""
            nonlocal failures
            if times is None:
                failures += 1
                return "error", None
            if checksum_of(out) != expected:
                # A faster time that computes something else is not a result.
                print(f"{name:<20}  CHECKSUM MISMATCH ({label})\n"
                      f"    python: {expected}\n"
                      f"    {label}: {checksum_of(out)}")
                failures += 1
                return "wrong", None
            best = min(times)
            return f"{best * 1000:.1f} ms", best

        cell_ts, ts = "-", None
        if bundle is not None:
            cell_ts, ts = other("typescript", *run_typescript(bundle, path, args.runs))

        cell_rs, rs = "-", None
        if rust is not None:
            times, out = run_rust(rust, path, args.runs)
            # The Rust interpreter does not implement every feature yet, so a
            # benchmark it cannot run is reported as such rather than counted
            # as a failure -- the other two still have a result worth printing.
            if times is not None and "not implemented in this interpreter yet" in out:
                cell_rs = "n/a"
            else:
                cell_rs, rs = other("rust", times, out)

        if ts is not None:
            ts_total += ts
            py_vs_ts += py
        if rs is not None:
            rs_total += rs
            py_vs_rs += py
            if ts is not None:
                ts_vs_rs += ts

        ratio_ts = f"{py / ts:.1f}x" if ts else "-"
        ratio_rs = f"{py / rs:.1f}x" if rs else "-"
        # The one that decides whether a bytecode VM is the next step: two
        # tree-walkers over the same AST, differing only in their host.
        ratio_both = f"{ts / rs:.1f}x" if ts and rs else "-"
        print(f"{name:<20}{py * 1000:>9.1f} ms{cell_ts:>13}{cell_rs:>11}"
              f"{ratio_ts:>8}{ratio_rs:>8}{ratio_both:>8}")

    print("-" * len(header))
    ts_cell = f"{ts_total * 1000:.1f} ms" if ts_total else "-"
    rs_cell = f"{rs_total * 1000:.1f} ms" if rs_total else "-"
    total_ts = f"{py_vs_ts / ts_total:.1f}x" if ts_total else "-"
    total_rs = f"{py_vs_rs / rs_total:.1f}x" if rs_total else "-"
    total_both = f"{ts_vs_rs / rs_total:.1f}x" if rs_total and ts_vs_rs else "-"
    print(f"{'TOTAL':<20}{py_total * 1000:>9.1f} ms{ts_cell:>13}{rs_cell:>11}"
          f"{total_ts:>8}{total_rs:>8}{total_both:>8}")
    print(f"\nBest of {args.runs} runs, measured in-process (no start-up cost).")
    if rs_total and py_vs_rs != py_total:
        print("Each ratio covers only the benchmarks that implementation ran, so the\n"
              "TOTAL times and the TOTAL ratios are over different sets.")

    if failures:
        print(f"\n{failures} benchmark(s) did not produce a usable result.", file=sys.stderr)
        return 1
    return 0


def profile(names: list[str]) -> int:
    """Where the Python interpreter's time actually goes.

    This is the measurement that decides whether the resolver and a bytecode
    VM are worth their cost: if name lookup dominates, resolving to slots is
    the win; if dispatch dominates, only a different execution model helps.
    """
    from mrt.interpreter import Interpreter
    from mrt.lexer import Lexer
    from mrt.parser import Parser

    for name in names:
        path = BENCH_DIR / f"{name}.mrt"
        source = path.read_text()
        statements = Parser(Lexer(source).scan_tokens()).parse()
        interpreter = Interpreter(module_path=str(path))

        profiler = cProfile.Profile()
        with contextlib.redirect_stdout(io.StringIO()):
            profiler.enable()
            interpreter.interpret(statements)
            profiler.disable()

        print(f"\n=== {name} " + "=" * (60 - len(name)))
        stats = pstats.Stats(profiler)
        stats.sort_stats("tottime").print_stats(8)
    return 0


if __name__ == "__main__":
    sys.exit(main())
