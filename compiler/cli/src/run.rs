//! `mrt-run` -- execute an MRT program with the Rust interpreter.
//!
//! Output and error text are meant to be byte-identical to `python -m mrt
//! FILE` and to the TypeScript interpreter, so this binary deliberately does
//! no formatting of its own: it prints what the interpreter produced and
//! exits with the reference implementation's codes.

use std::process::ExitCode;

use mrt_diagnostics::SourceFile;
use mrt_interp::{Entry, Outcome};

const USAGE: &str = "usage: mrt-run [--vm] [--bench RUNS] FILE";

/// Exit code MRT uses for a source error, inherited from `sysexits.h`.
const EX_DATAERR: u8 = 65;
/// ...and for an error raised while running.
const EX_SOFTWARE: u8 = 70;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut runs: Option<u32> = None;
    let mut vm = false;
    let mut path: Option<&str> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--vm" => vm = true,
            "--bench" => match rest.next().and_then(|n| n.parse().ok()) {
                Some(n) => runs = Some(n),
                None => {
                    eprintln!("mrt-run: --bench needs a run count");
                    return ExitCode::from(2);
                }
            },
            other if other.starts_with("--") => {
                eprintln!("mrt-run: unknown option '{other}'\n{USAGE}");
                return ExitCode::from(2);
            }
            other => path = Some(other),
        }
    }
    let Some(path) = path else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };

    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("Could not read file '{path}': {e}");
            return ExitCode::from(1);
        }
    };

    let file = SourceFile::new(path, &text);

    // One function, chosen once: everything below is engine-agnostic, so
    // the two paths cannot drift in how they report a result.
    let engine: fn(&SourceFile) -> mrt_interp::Outcome = if vm {
        mrt_interp::run_vm
    } else {
        mrt_interp::run
    };

    if let Some(runs) = runs {
        return bench(&file, runs, engine);
    }

    let outcome = engine(&file);
    for line in &outcome.output {
        println!("{line}");
    }
    match outcome.error {
        None => {
            explain_if_nothing_ran(&outcome, path);
            ExitCode::SUCCESS
        }
        Some(error) => {
            let syntax = error.starts_with("Syntax Error:");
            eprintln!("{error}");
            ExitCode::from(if syntax { EX_DATAERR } else { EX_SOFTWARE })
        }
    }
}

/// Say so when a file ran correctly and did nothing.
///
/// MRT calls a top-level `main` if the file defines one, so a file of only
/// declarations runs to completion, prints nothing and exits 0 -- which is
/// indistinguishable from a broken installation. The program was never
/// wrong, so this is not an error and does not change the exit code; it goes
/// to stderr, which also keeps it out of the printed output that four
/// implementations are held to match byte for byte.
///
/// Worded identically to the Python CLI's note on purpose: the two are one
/// command as far as anyone using them is concerned.
fn explain_if_nothing_ran(outcome: &Outcome, path: &str) {
    if outcome.entry == Entry::Ran || !outcome.output.is_empty() {
        return;
    }
    match &outcome.entry {
        Entry::Ran => unreachable!("returned above"),
        Entry::Missing => {
            eprintln!("mrt: nothing ran in '{path}' -- no top-level 'main' function.");
            eprintln!(
                "hint: add  func main() {{ ... }}  at the top level (not indented \
inside another function), or write statements at file scope."
            );
        }
        // The name is taken by something uncallable, which is a different
        // mistake from not having one and deserves to be named as such.
        Entry::NotCallable(what) => {
            eprintln!(
                "mrt: nothing ran in '{path}' -- the top-level 'main' is a {what}, not a function."
            );
        }
    }
}

/// Time `runs` in-process executions and print `{"times": [...], "out": "..."}`.
///
/// Spawning this binary once per run would time process start-up as much as
/// the interpreter, and the Python and TypeScript sides of the comparison are
/// both measured in-process. This exists so all three are measured the same
/// way.
fn bench(file: &SourceFile, runs: u32, engine: fn(&SourceFile) -> mrt_interp::Outcome) -> ExitCode {
    // One untimed warm-up, matching the JavaScript runner -- not for a JIT
    // here, but so page faults on first touch land outside the measurement.
    let _ = engine(file);

    let mut times = Vec::new();
    let mut out = String::new();
    for _ in 0..runs {
        let start = std::time::Instant::now();
        let outcome = engine(file);
        times.push(start.elapsed().as_secs_f64());
        out = match outcome.error {
            Some(error) => error,
            None => outcome.output.join("\n"),
        };
    }

    let times: Vec<String> = times.iter().map(|t| format!("{t}")).collect();
    println!(
        "{{\"times\":[{}],\"out\":{}}}",
        times.join(","),
        json_string(&out)
    );
    ExitCode::SUCCESS
}

/// Just enough JSON to quote one string; the workspace has no dependencies.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
