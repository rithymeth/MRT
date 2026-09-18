//! `mrt-check` -- the MRT 2.0 frontend driver.
//!
//! Phase 1 has no evaluator, so this binary exists to *inspect* the frontend
//! and to be diffed against the Python reference implementation. It is the
//! harness's other half, not a language runtime.
//!
//! ```text
//! mrt-check FILE                 rich diagnostics (default)
//! mrt-check --dump-tokens FILE   canonical token stream
//! mrt-check --dump-ast FILE      canonical AST
//! mrt-check --dump-scopes FILE   resolved frame layout
//! mrt-check --compat FILE        render errors as MRT 1.x does, byte for
//!                                byte. Independent of what is dumped, so
//!                                the conformance harness can ask for a
//!                                token dump and comparable error text in
//!                                the same run.
//! ```

use std::process::ExitCode;

use mrt_ast::dump::dump_program;
use mrt_diagnostics::{quote, SourceFile};
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
            "--dump-ast" => mode = Mode::DumpAst,
            "--dump-scopes" => mode = Mode::DumpScopes,
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
        Mode::DumpAst | Mode::DumpScopes | Mode::Diagnose => {
            let parsed = mrt_parser::Parser::new(tokens).parse();
            if !parsed.errors.is_empty() {
                for diagnostic in &parsed.errors {
                    if compat {
                        eprintln!("{}", diagnostic.render_compat());
                    } else {
                        eprint!("{}", diagnostic.render(&file));
                    }
                }
                return ExitCode::from(EX_DATAERR);
            }
            match mode {
                Mode::DumpAst => print!("{}", dump_program(&parsed.program)),
                Mode::DumpScopes => {
                    let resolved = mrt_resolver::resolve(&parsed.program);
                    // Checked on every dump rather than only in tests: the
                    // conformance harness runs this over the whole corpus, so
                    // a broken capture chain fails there rather than silently
                    // printing nonsense.
                    if let Err(problem) = mrt_resolver::validate(&resolved) {
                        eprintln!("resolver invariant violated: {problem}");
                        return ExitCode::from(EX_DATAERR);
                    }
                    print!("{}", mrt_resolver::dump(&resolved, &file));
                }
                _ => {}
            }
        }
    }

    ExitCode::SUCCESS
}

const USAGE: &str = "usage: mrt-check [--dump-tokens | --dump-ast | --dump-scopes] [--compat] FILE";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Diagnose,
    DumpTokens,
    DumpAst,
    DumpScopes,
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
