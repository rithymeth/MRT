//! Flatten a program and everything it imports into one file.
//!
//! -- What "packaged" has to mean --
//!
//! A game that imports three modules is four files, and handing someone four
//! files and an instruction about where to put them is not a distribution.
//! Packaging turns it into one `.mrt` that runs anywhere MRT does.
//!
//! The tempting shortcut is to paste the modules together. It does not work:
//! two modules may each define `helper`, and concatenating them silently
//! redefines one. Renaming everything instead means rewriting every reference
//! in the program, which means a printer for the whole language.
//!
//! -- The trick --
//!
//! A module becomes a function that returns its exports:
//!
//! ```text
//! var __MRTPKG_0 = (func() {
//!     func helper() { ... }
//!     return {helper: helper};
//! })();
//! ```
//!
//! Every module's names are then local to its own function body, so two
//! `helper`s cannot collide and nothing needs renaming. `import { helper }`
//! becomes `var helper = __MRTPKG_0.helper;`. Only the wrapper names have to
//! be unique, and those are ours to choose.
//!
//! That also means no AST printer: each statement is copied out of its own
//! source by span, comments and all, and only `import` and `export` lines are
//! rewritten.
//!
//! -- The catch, which is the whole reason this is careful --
//!
//! MRT hoists function and struct declarations at the **top level** and
//! nowhere else. Moving a module's top level into a function body would
//! therefore break every forward reference in it -- a `main` calling a `func`
//! written below it stops resolving, in a file that was correct before it was
//! packaged.
//!
//! So the wrapper emits declarations first and everything else after, in
//! source order within each group. That is exactly what top-level hoisting
//! does, reproduced by construction.
//!
//! The entry file is *not* wrapped, for a related reason: MRT calls a
//! top-level `main`, and a `main` inside a function body is not top level.

//! -- What packaging does not preserve --
//!
//! **Line numbers.** A module's line 3 becomes some other line of the
//! flattened file, so a runtime error raised inside a module reports its new
//! position. Fixing that needs a source map and a runtime that reads one;
//! until then the packaged file is for shipping and the original tree is for
//! debugging, which is the usual arrangement and worth stating rather than
//! discovering.
//!
//! Everything else is checked: `scripts/check-package.mjs` packages every
//! multi-module program in the corpus and insists the result prints exactly
//! what the original did, on both engines.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use mrt_ast::{Program, Stmt, StmtKind};

/// The prefix the wrappers use.
///
/// A program already containing it is refused rather than packaged, because
/// the one thing this must never do is quietly shadow a name the author
/// chose.
const PREFIX: &str = "__MRTPKG_";

struct Module {
    path: PathBuf,
    /// Characters, because spans are char offsets rather than byte ones.
    chars: Vec<char>,
    program: Program,
}

impl Module {
    /// The source text a statement came from, as a statement again.
    ///
    /// Two things have to be put back that a span leaves out.
    ///
    /// A declaration's span starts at its **name**: the parser consumes
    /// `func` or `struct` before it begins building the statement, so copying
    /// the span alone yields `circleArea(radius) { ... }`, which is not a
    /// declaration at all. The keyword is recovered from the characters just
    /// before the span rather than reconstructed from the AST, so a generator
    /// -- which MRT spells with no keyword, inferring it from `yield` --
    /// needs no special case, and neither would a new one.
    ///
    /// A statement's span also stops before its `;`. Semicolons are optional
    /// in MRT, but only mostly: a line starting with `(`, `[` or `-` parses
    /// as a continuation of the line above. Putting the separator back costs
    /// nothing and removes the whole hazard.
    fn statement(&self, stmt: &Stmt) -> Result<String, String> {
        let start = match &stmt.kind {
            StmtKind::Function { .. } => self.keyword_before(stmt, "func")?,
            StmtKind::Struct { .. } => self.keyword_before(stmt, "struct")?,
            _ => stmt.span.start as usize,
        };
        let mut text: String = self.chars[start..stmt.span.end as usize].iter().collect();
        if !text.ends_with(';') && !text.ends_with('}') {
            text.push(';');
        }
        Ok(text)
    }

    /// Where the keyword introducing a declaration starts.
    fn keyword_before(&self, stmt: &Stmt, expected: &str) -> Result<usize, String> {
        let mut at = stmt.span.start as usize;
        while at > 0 && self.chars[at - 1].is_whitespace() {
            at -= 1;
        }
        let word_end = at;
        while at > 0 && !self.chars[at - 1].is_whitespace() {
            at -= 1;
        }
        let word: String = self.chars[at..word_end].iter().collect();
        if word == expected {
            Ok(at)
        } else {
            // Refused rather than guessed. Emitting the span alone would
            // produce a file that parses as something else entirely, and the
            // author would be debugging a program they did not write.
            Err(format!(
                "in '{}': expected '{expected}' before a declaration, found {word:?}",
                file_name(&self.path)
            ))
        }
    }
}

/// Package `entry` and everything it imports. Returns the flattened source.
pub fn package(entry: &Path) -> Result<String, String> {
    let mut modules = Vec::new();
    let mut ids = HashMap::new();
    collect(&normalize(entry), &mut Vec::new(), &mut modules, &mut ids)?;

    // Every module's export table, before any of them is rendered: an
    // import can only be checked against a table that already exists.
    let mut tables = HashMap::new();
    for (index, module) in modules.iter().enumerate() {
        tables.insert(index, exports_of(module, &ids)?);
    }

    let entry_id = modules.len() - 1;
    let mut out = String::new();
    out.push_str(&format!(
        "// Packaged by mrt-game from {} and {} module{}.\n\
         // Every import is inlined; this file needs nothing beside it.\n\n",
        entry
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| entry.display().to_string()),
        entry_id,
        if entry_id == 1 { "" } else { "s" }
    ));

    for (index, module) in modules.iter().enumerate() {
        if index == entry_id {
            // The entry's own top level, unwrapped, so its `main` stays a
            // top-level `main`.
            out.push_str(&render(module, &ids, &tables, None)?);
        } else {
            out.push_str(&format!(
                "// -- {} --\nvar {PREFIX}{index} = (func() {{\n",
                module
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
            out.push_str(&render(module, &ids, &tables, Some(index))?);
            out.push_str("})();\n\n");
        }
    }
    Ok(out)
}

/// Depth-first, dependencies before dependents, with cycles refused.
fn collect(
    path: &Path,
    visiting: &mut Vec<PathBuf>,
    modules: &mut Vec<Module>,
    ids: &mut HashMap<PathBuf, usize>,
) -> Result<(), String> {
    if ids.contains_key(path) {
        return Ok(());
    }
    if visiting.contains(&path.to_path_buf()) {
        let names: Vec<String> = visiting
            .iter()
            .chain(std::iter::once(&path.to_path_buf()))
            .map(|p| file_name(p))
            .collect();
        return Err(format!("import cycle: {}", names.join(" -> ")));
    }

    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read '{}': {e}", path.display()))?;
    if text.contains(PREFIX) {
        return Err(format!(
            "'{}' contains the name '{PREFIX}', which packaging needs for itself",
            path.display()
        ));
    }
    let source = mrt_diagnostics::SourceFile::new(path.display().to_string(), &text);
    let parsed = mrt_parser::parse(&source);
    if !parsed.errors.is_empty() {
        let mut report = String::new();
        for diagnostic in &parsed.errors {
            report.push_str(&diagnostic.render(&source));
        }
        return Err(report);
    }

    visiting.push(path.to_path_buf());
    let base = path.parent().unwrap_or(Path::new("."));
    for specifier in specifiers(&parsed.program) {
        if !(specifier.starts_with("./") || specifier.starts_with("../")) {
            return Err(format!(
                "in '{}': module path \"{specifier}\" must start with './' or '../'",
                file_name(path)
            ));
        }
        collect(&normalize(&base.join(&specifier)), visiting, modules, ids)?;
    }
    visiting.pop();

    ids.insert(path.to_path_buf(), modules.len());
    modules.push(Module {
        path: path.to_path_buf(),
        chars: text.chars().collect(),
        program: parsed.program,
    });
    Ok(())
}

/// Every module a program imports or re-exports from.
fn specifiers(program: &Program) -> Vec<String> {
    let mut out = Vec::new();
    for stmt in &program.statements {
        match &stmt.kind {
            StmtKind::Import { specifier, .. } => out.push(specifier.clone()),
            StmtKind::ExportNames {
                specifier: Some(specifier),
                ..
            } => out.push(specifier.clone()),
            _ => {}
        }
    }
    out
}

/// A module's exports, in the order the interpreter records them.
///
/// The order is observable: `import * as m` binds an object and `keys(m)`
/// reports it. And it is *hoisted* order, not source order -- declarations
/// are defined before the top level runs, so `export func square` is recorded
/// before `export var PI` even when PI is written first. Getting this wrong
/// changes what a packaged program prints, which is exactly the kind of
/// difference packaging must not introduce.
fn exports_of(
    module: &Module,
    ids: &HashMap<PathBuf, usize>,
) -> Result<Vec<(String, String)>, String> {
    let base = module.path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut first = Vec::new();
    let mut rest = Vec::new();
    for stmt in &module.program.statements {
        let out = if is_hoisted(stmt) {
            &mut first
        } else {
            &mut rest
        };
        match &stmt.kind {
            StmtKind::Export { name, .. } => out.push((name.text.clone(), name.text.clone())),
            StmtKind::ExportNames { names, specifier } => match specifier {
                Some(specifier) => {
                    let id = ids
                        .get(&normalize(&base.join(specifier)))
                        .ok_or_else(|| format!("no module for \"{specifier}\""))?;
                    for (local, exported) in names {
                        out.push((
                            exported.text.clone(),
                            format!("{PREFIX}{id}.{}", local.text),
                        ));
                    }
                }
                None => {
                    for (local, exported) in names {
                        out.push((exported.text.clone(), local.text.clone()));
                    }
                }
            },
            _ => {}
        }
    }
    first.extend(rest);
    Ok(first)
}

/// Whether a statement is one MRT hoists at the top level.
fn is_hoisted(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Function { .. } | StmtKind::Struct { .. } => true,
        // `export func f()` hoists because the function inside it does --
        // otherwise a name's visibility would depend on whether it is
        // exported.
        StmtKind::Export { declaration, .. } => is_hoisted(declaration),
        _ => false,
    }
}

/// Render one module's statements, rewriting its imports and exports.
///
/// `wrapper` is the module's own id when it is being wrapped, and `None` for
/// the entry file, which gets no export table because nothing imports it.
fn render(
    module: &Module,
    ids: &HashMap<PathBuf, usize>,
    tables: &HashMap<usize, Vec<(String, String)>>,
    wrapper: Option<usize>,
) -> Result<String, String> {
    let indent = if wrapper.is_some() { "    " } else { "" };
    let base = module.path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let id_of = |specifier: &str| -> Result<usize, String> {
        let path = normalize(&base.join(specifier));
        ids.get(&path)
            .copied()
            .ok_or_else(|| format!("no module for \"{specifier}\""))
    };

    // A name taken from another module has to be one that module actually
    // exports. The interpreter raises a clear error for this at run time;
    // once flattened there is no module left to name, and the program would
    // instead fail with a key error against an anonymous object. Refusing
    // here keeps the diagnosis where it belongs -- and finds it earlier.
    let has = |id: usize, name: &str| -> bool {
        tables
            .get(&id)
            .is_some_and(|table| table.iter().any(|(exported, _)| exported == name))
    };
    let mut declarations = String::new();
    let mut body = String::new();

    for stmt in &module.program.statements {
        let out = if is_hoisted(stmt) {
            &mut declarations
        } else {
            &mut body
        };
        match &stmt.kind {
            StmtKind::Import {
                names,
                namespace,
                specifier,
            } => {
                let id = id_of(specifier)?;
                match namespace {
                    // A namespace import is the module's export object, which
                    // is exactly what the wrapper already returns.
                    Some(ns) => out.push_str(&format!("{indent}var {} = {PREFIX}{id};\n", ns.text)),
                    None => {
                        for (exported, local) in names {
                            if !has(id, &exported.text) {
                                return Err(format!(
                                    "in '{}': module \"{specifier}\" has no export named '{}'",
                                    file_name(&module.path),
                                    exported.text
                                ));
                            }
                            out.push_str(&format!(
                                "{indent}var {} = {PREFIX}{id}.{};\n",
                                local.text, exported.text
                            ));
                        }
                    }
                }
            }
            StmtKind::Export { declaration, .. } => {
                // The declaration keeps its own text; only the `export `
                // keyword in front of it goes away. What it exports is
                // recorded by `exports_of`, so the two cannot disagree.
                out.push_str(&format!("{indent}{}\n", module.statement(declaration)?));
            }
            StmtKind::ExportNames { names, specifier } => {
                // `export { a } from "./m.mrt"` forwards without binding
                // locally, so it emits nothing at all -- the export table
                // references that module's object directly.
                if let Some(specifier) = specifier {
                    let id = id_of(specifier)?;
                    for (local, _) in names {
                        if !has(id, &local.text) {
                            return Err(format!(
                                "in '{}': module \"{specifier}\" has no export named '{}'",
                                file_name(&module.path),
                                local.text
                            ));
                        }
                    }
                }
            }
            _ => out.push_str(&format!("{indent}{}\n", module.statement(stmt)?)),
        }
    }

    // Declarations first: that is what top-level hoisting does, and a module
    // moved into a function body would otherwise lose every forward
    // reference in it.
    let mut rendered = declarations;
    rendered.push_str(&body);

    if let Some(id) = wrapper {
        let fields: Vec<String> = tables
            .get(&id)
            .map(|table| {
                table
                    .iter()
                    .map(|(name, value)| format!("{name}: {value}"))
                    .collect()
            })
            .unwrap_or_default();
        rendered.push_str(&format!("{indent}return {{{}}};\n", fields.join(", ")));
    }
    Ok(rendered)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Resolve `.` and `..` without touching the filesystem, matching how the
/// interpreter resolves a specifier.
fn normalize(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut out = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}
