//! `mrt-check` -- the MRT 2.0 frontend driver.
//!
//! Phase 1 has no evaluator, so this binary exists to *inspect* the frontend
//! and to be diffed against the Python reference implementation. It is the
//! harness's other half, not a language runtime.
//!
//! ```text
//! mrt-check FILE                 rich diagnostics (default)
//! mrt-check --dump-tokens FILE   canonical token stream
//! mrt-check --compat FILE        render errors as MRT 1.x does, byte for
//!                                byte. Independent of what is dumped, so
//!                                the conformance harness can ask for a
//!                                token dump and comparable error text in
//!                                the same run.
//! ```

use std::process::ExitCode;

use mrt_diagnostics::SourceFile;
use mrt_lexer::{lex, Literal, TemplatePart};

/// Exit code MRT 1.x uses for a source error, inherited from `sysexits.h`.
const EX_DATAERR: u8 = 65;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut mode = Mode::Diagnose;
    let mut compat = false;
    let mut path: Option<String> = None;
    for arg in &args {
        match arg.as_str() {
            "--compat" => compat = true,
            "--dump-tokens" => mode = Mode::DumpTokens,
            "-h" | "--help" => {
                eprintln!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other if other.starts_with('-') => {
                eprintln!("mrt-check: unknown option '{other}'\n{USAGE}");
                return ExitCode::from(2);
            }
            other => path = Some(other.to_string()),
        }
    }

    let Some(path) = path else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("Could not read file '{path}': {e}");
            return ExitCode::from(1);
        }
    };

    // The name shown in diagnostics is the path as given, matching how the
    // Python CLI is invoked.
    let file = SourceFile::new(&path, &text);

    let tokens = match lex(&file) {
        Ok(tokens) => tokens,
        Err(diagnostic) => {
            if compat {
                eprintln!("{}", diagnostic.render_compat());
            } else {
                eprint!("{}", diagnostic.render(&file));
            }
            return ExitCode::from(EX_DATAERR);
        }
    };

    match mode {
        Mode::DumpTokens => {
            let mut out = String::new();
            for token in &tokens {
                out.push_str(&format!(
                    "{}\t{}\t{}\t{}\n",
                    token.kind.python_name(),
                    quote(&token.lexeme),
                    literal_repr(&token.literal),
                    token.line
                ));
            }
            print!("{out}");
        }
        // With no parser yet, a file that lexes cleanly is as far as this
        // phase can take it. Phase 1 continues in the parser crate.
        Mode::Diagnose => {}
    }

    ExitCode::SUCCESS
}

const USAGE: &str = "usage: mrt-check [--dump-tokens] [--compat] FILE";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Diagnose,
    DumpTokens,
}

/// A literal in the canonical dump format shared with the Python dumper.
///
/// Numbers are written as the raw IEEE-754 bit pattern rather than as text.
/// Formatting a float is where two languages quietly disagree -- Python
/// renders 1e20 as `1e+20` and Rust as `100000000000000000000` -- and the
/// comparison is meant to be about lexing, not about float printing.
fn literal_repr(literal: &Literal) -> String {
    match literal {
        Literal::None => "-".to_string(),
        Literal::Bool(b) => format!("B:{b}"),
        Literal::Number(n) => format!("N:{:016x}", n.to_bits()),
        Literal::Str(s) => format!("S:{}", quote(s)),
        Literal::Template(parts) => {
            let rendered: Vec<String> = parts
                .iter()
                .map(|part| match part {
                    TemplatePart::Text(t) => format!("T:{}", quote(t)),
                    TemplatePart::Expr { source, line, .. } => {
                        format!("E:{}@{}", quote(source), line)
                    }
                })
                .collect();
            format!("[{}]", rendered.join(","))
        }
    }
}

/// JSON-style quoting, so a lexeme containing a newline, tab or quote stays
/// on one line of the dump. Hand-rolled to keep the crate dependency-free.
fn quote(s: &str) -> String {
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
