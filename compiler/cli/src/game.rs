//! `mrt-game` -- make and run games written in MRT.
//!
//! -- Why this is a separate command --
//!
//! MRT-Game is an *engine extension*: the Rust engines have it and the Python
//! reference and TypeScript playground deliberately do not. The `mrt` command
//! that PyPI installs is the Python one, so it could never run a game -- and a
//! command that exists but fails on the thing it is named for is worse than
//! one that is not there. So the game tooling lives on the side that can
//! actually do it.
//!
//! -- Why there is no `build` --
//!
//! MRT is interpreted; there is no compilation step that produces an artifact,
//! so a `build` command would either be a lie or a synonym for `check`. It is
//! called `check`, which is what it does: parse and resolve the whole program
//! and report what is wrong, without running it.
//!
//! `package` is real rather than a zip: it flattens a game's imports into one
//! file that runs anywhere MRT does. See `game/package.rs` for how, and for
//! the hoisting rule that makes it delicate.

// `game.rs` is a binary root, so a plain `mod package;` would look in src/.
// The path keeps this binary's two extra files -- the packager and the
// starter game -- together in one directory.
#[path = "game/package.rs"]
mod package;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mrt_diagnostics::SourceFile;

const USAGE: &str = "\
usage: mrt-game <command> [path]

  new <name>     start a game in a new directory of that name
  run [dir]      run the game in dir (default: the current directory)
  check [dir]    parse and resolve it without running it
  package [dir]  flatten it and its imports into one file

A game is a directory with a main.mrt in it.";

/// The game a `new` scaffolds: a complete, playable one rather than a stub.
///
/// A scaffold that prints "hello" teaches nothing about the thing being
/// scaffolded. This is breakout -- a loop, real elapsed time, input, swept
/// collision and text -- so the first edit anyone makes is to a program that
/// already works, which is a much better place to start than a blank file.
const STARTER: &str = include_str!("game/starter.mrt");

const README: &str = "\
# {NAME}

A game written in MRT.

    mrt-game run      play it
    mrt-game check    look for mistakes without running it

Arrow keys or A/D move the paddle. Escape quits.

`main.mrt` is the whole game. It uses MRT-Game, an engine extension that only
the Rust engines provide -- see \"Engine extensions\" in the MRT language
specification for what that means.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut rest = args.iter().map(String::as_str);

    match rest.next() {
        Some("new") => match rest.next() {
            Some(name) => new(name),
            None => {
                eprintln!("mrt-game: 'new' needs a name\n{USAGE}");
                ExitCode::from(2)
            }
        },
        Some("run") => run(rest.next().unwrap_or(".")),
        Some("check") => check(rest.next().unwrap_or(".")),
        Some("package") => pack(rest.next().unwrap_or(".")),
        Some("-h") | Some("--help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("mrt-game: unknown command '{other}'\n{USAGE}");
            ExitCode::from(2)
        }
        None => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Start a game in a new directory.
fn new(name: &str) -> ExitCode {
    let dir = Path::new(name);
    // Refused rather than merged into: someone typing `new` at an existing
    // game would otherwise overwrite the main.mrt they have been editing.
    if dir.exists() {
        eprintln!("mrt-game: '{name}' already exists.");
        return ExitCode::from(1);
    }
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("mrt-game: could not create '{name}': {e}");
        return ExitCode::from(1);
    }
    let files = [
        (dir.join("main.mrt"), STARTER.to_string()),
        (dir.join("README.md"), README.replace("{NAME}", name)),
    ];
    for (path, contents) in files {
        if let Err(e) = std::fs::write(&path, contents) {
            eprintln!("mrt-game: could not write '{}': {e}", path.display());
            return ExitCode::from(1);
        }
    }
    println!("Created {name}/ with a playable game in it.");
    println!();
    println!("    cd {name}");
    println!("    mrt-game run");
    ExitCode::SUCCESS
}

/// Find `main.mrt`, whether given the directory or the file itself.
///
/// Both, because `mrt-game run` in a game directory and `mrt-game run
/// mygame/main.mrt` are each the obvious thing to type, and guessing wrong
/// about which one someone meant costs nothing to avoid.
fn entry_file(path: &str) -> Result<PathBuf, String> {
    let given = Path::new(path);
    if given.is_file() {
        return Ok(given.to_path_buf());
    }
    if !given.is_dir() {
        return Err(format!("there is no '{path}'"));
    }
    let main = given.join("main.mrt");
    if main.is_file() {
        Ok(main)
    } else {
        Err(format!(
            "'{path}' has no main.mrt -- is it a game directory? \
             'mrt-game new NAME' makes one"
        ))
    }
}

fn read(path: &str) -> Result<(PathBuf, String), ExitCode> {
    let file = match entry_file(path) {
        Ok(file) => file,
        Err(why) => {
            eprintln!("mrt-game: {why}");
            return Err(ExitCode::from(1));
        }
    };
    match std::fs::read_to_string(&file) {
        Ok(text) => Ok((file, text)),
        Err(e) => {
            eprintln!("mrt-game: could not read '{}': {e}", file.display());
            Err(ExitCode::from(1))
        }
    }
}

/// Run the game.
fn run(path: &str) -> ExitCode {
    let (file, text) = match read(path) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let name = file.display().to_string();
    let source = SourceFile::new(&name, &text);
    let outcome = mrt_interp::run(&source);

    for line in &outcome.output {
        println!("{line}");
    }
    match outcome.error {
        None => ExitCode::SUCCESS,
        Some(error) => {
            eprintln!("{error}");
            // The same codes mrt-run uses, so a script driving either gets
            // one answer rather than two.
            ExitCode::from(if error.starts_with("Syntax Error:") {
                65
            } else {
                70
            })
        }
    }
}

/// Parse and resolve without running.
fn check(path: &str) -> ExitCode {
    let (file, text) = match read(path) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let name = file.display().to_string();
    let source = SourceFile::new(&name, &text);

    // `parse` lexes too, so a lexical error arrives here as an ordinary
    // parse error and there is one path for reporting both.
    let parsed = mrt_parser::parse(&source);
    if !parsed.errors.is_empty() {
        for diagnostic in &parsed.errors {
            eprint!("{}", diagnostic.render(&source));
        }
        let count = parsed.errors.len();
        eprintln!(
            "{count} problem{} in {}.",
            if count == 1 { "" } else { "s" },
            file.display()
        );
        return ExitCode::from(65);
    }

    // Resolution has no error list of its own -- a program that parsed has
    // scopes -- but it does have invariants, and a broken capture chain is
    // exactly the kind of thing a game author would meet as nonsense at
    // runtime rather than as a message here.
    let resolved = mrt_resolver::resolve(&parsed.program);
    if let Err(problem) = mrt_resolver::validate(&resolved) {
        eprintln!("mrt-game: resolver invariant violated: {problem}");
        return ExitCode::from(70);
    }

    println!("{}: no problems found.", file.display());
    ExitCode::SUCCESS
}

/// Flatten a game and its imports into one file.
fn pack(path: &str) -> ExitCode {
    let file = match entry_file(path) {
        Ok(file) => file,
        Err(why) => {
            eprintln!("mrt-game: {why}");
            return ExitCode::from(1);
        }
    };
    let flattened = match package::package(&file) {
        Ok(text) => text,
        Err(why) => {
            eprintln!("mrt-game: {why}");
            return ExitCode::from(65);
        }
    };
    // Beside the entry rather than over it: packaging something onto the file
    // it was made from is a mistake nobody recovers from.
    let out = file.with_file_name("game.packaged.mrt");
    if let Err(e) = std::fs::write(&out, &flattened) {
        eprintln!("mrt-game: could not write '{}': {e}", out.display());
        return ExitCode::from(1);
    }
    println!(
        "Wrote {} ({} lines).",
        out.display(),
        flattened.lines().count()
    );
    ExitCode::SUCCESS
}
