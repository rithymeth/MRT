//! A tree-walking interpreter for MRT, in Rust.
//!
//! Its job is to be *the same language* as `mrt/interpreter.py` and
//! `src/lib/mrtInterpreter.ts`, not a better one: the conformance harness
//! runs real programs through all three and requires byte-identical output,
//! so a tidier decision here is a bug.
//!
//! # Why this exists
//!
//! The benchmark suite measured the two existing implementations and found
//! that the TypeScript tree-walker is ~10x faster than the Python one while
//! running the same algorithms over the same AST shapes. That is a large
//! number arriving before any architectural change, and it suggests the
//! cheapest real speed-up available is the implementation language rather
//! than a bytecode VM -- which this exists to confirm or refute with a
//! measurement rather than a projection.
//!
//! # What is not here yet: generators
//!
//! Both existing implementations suspend a generator by delegating to a
//! native coroutine in their host language -- Python's `yield from`,
//! JavaScript's `yield*`. Rust has neither on stable. The options are all
//! expensive: a thread per generator forces `Arc<Mutex<..>>` through the
//! whole interpreter and gives back the performance this is trying to
//! measure; async-as-generators fights the borrow checker for a tree-walker
//! that holds `&mut self` across a yield; and an explicit resumable
//! evaluator is most of a bytecode VM already.
//!
//! That last point is the interesting one, and it is why generators are
//! deferred rather than hacked around: in Rust the natural way to suspend
//! execution *is* an explicit instruction pointer over a flat program, so the
//! feature that is hardest to port is also the one that argues hardest for
//! the VM. Calling a generator function raises a clear error meanwhile.
//!
//! Everything else the language has is here, modules included.

pub mod builtins;
pub mod env;
pub mod error;
pub mod generator;
pub mod value;
pub mod vm;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mrt_ast::*;
use mrt_diagnostics::SourceFile;

use crate::env::Env;
use crate::error::{
    arity_error, type_error, value_error, Eval, Exec, Kind, RuntimeError, Signal, Thrown,
};
use crate::generator::{already_iterated, already_running, Cursor, GenState, Generator, Transform};
use crate::value::*;

/// Which engine is running a program.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Engine {
    TreeWalker,
    Vm,
}

pub struct Interpreter {
    pub globals: Env,
    pub env: Env,
    pub output: Vec<String>,
    /// What became of a top-level `main`. Read by a CLI to explain a file
    /// that ran correctly and printed nothing.
    pub entry: Entry,
    /// Which engine is driving this program.
    ///
    /// Only the module loader reads it, and only to decide how to evaluate an
    /// imported file's top level. Without it `mrt-run --vm` would compile the
    /// entry file and then quietly walk every module it imports, so "the VM
    /// ran this" would be true of one file out of however many.
    pub(crate) engine: Engine,

    /// The file currently being evaluated. `import` specifiers resolve
    /// against its directory, so this is swapped while a module runs and
    /// restored afterwards. `None` means the program did not come from a
    /// file, and any `import` is an error rather than a guess.
    module_path: Option<PathBuf>,
    /// Every module evaluated so far, by resolved path. A module's top-level
    /// code can have side effects, so importing one file from two places has
    /// to run it once and share the result.
    module_exports: HashMap<PathBuf, Rc<Exports>>,
    /// The modules currently part-way through evaluation, outermost first.
    /// A specifier that resolves to one of these is a cycle.
    module_loading: Vec<PathBuf>,
    /// What the file being evaluated has exported so far. Ordered, because a
    /// namespace import turns it into an object and `keys()` on that object
    /// is observable.
    current_exports: Exports,
}

/// A module's export table: names in the order they were exported.
///
/// A `Vec` rather than a map because the order is part of the language --
/// `import * as m` binds an object and `keys(m)` reports it -- and an export
/// table is small enough that a linear lookup is not worth avoiding.
pub type Exports = Vec<(String, Value)>;

fn exports_get(exports: &Exports, name: &str) -> Option<Value> {
    exports
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.clone())
}

/// What running a program produced.
pub struct Outcome {
    pub output: Vec<String>,
    /// The error text, if the program stopped early. Rendered exactly as
    /// `python -m mrt` prints it.
    pub error: Option<String>,
    /// What became of a top-level `main`.
    ///
    /// Carried out so a CLI can explain a file that ran correctly and did
    /// nothing, which is otherwise indistinguishable from a broken install.
    /// Deliberately not part of `output`: that is the printed stream four
    /// implementations are held to match byte for byte.
    pub entry: Entry,
}

/// What became of a top-level `main`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// Declared, and called.
    Ran,
    /// The file defines none, so only its top-level statements ran.
    Missing,
    /// The name is taken by something that cannot be called. A different
    /// mistake from having no `main`, and worth telling apart.
    NotCallable(String),
}

pub fn run(file: &SourceFile) -> Outcome {
    let parsed = mrt_parser::parse(file);
    if !parsed.errors.is_empty() {
        return Outcome {
            output: Vec::new(),
            error: Some(
                parsed
                    .errors
                    .iter()
                    .map(|e| format!("Syntax Error: {}", strip_prefix(&e.render_compat())))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            entry: Entry::Missing,
        };
    }
    // The SourceFile's name is the path the program was loaded from, which
    // is what `import "./x.mrt"` resolves against.
    let mut interp = Interpreter::with_module_path(Some(PathBuf::from(file.name())));
    let error = interp.interpret(&parsed.program);
    Outcome {
        output: interp.output,
        error,
        entry: interp.entry,
    }
}

/// Run a program on the bytecode VM instead of the tree-walker.
///
/// A program the compiler cannot yet handle comes back as an error naming the
/// construct, not as a wrong answer -- which is what lets the VM conformance
/// harness tell "not implemented" apart from "implemented wrongly".
pub fn run_vm(file: &SourceFile) -> Outcome {
    let parsed = mrt_parser::parse(file);
    if !parsed.errors.is_empty() {
        return Outcome {
            output: Vec::new(),
            error: Some(
                parsed
                    .errors
                    .iter()
                    .map(|e| format!("Syntax Error: {}", strip_prefix(&e.render_compat())))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            entry: Entry::Missing,
        };
    }

    let chunk = match vm::compile_program(&parsed.program.statements) {
        Ok(chunk) => chunk,
        Err(vm::Unsupported(what)) => {
            return Outcome {
                output: Vec::new(),
                error: Some(format!("Runtime Error: {what}")),
                entry: Entry::Missing,
            }
        }
    };

    let mut interp = Interpreter::with_module_path(Some(std::path::PathBuf::from(file.name())));
    interp.engine = Engine::Vm;
    let result = (|| -> Exec {
        let mut machine = vm::Vm::new(&mut interp);
        machine.run(chunk)?;
        // `main` is called if one was declared, exactly as the tree-walker
        // does it.
        match interp.globals.get("main", None) {
            Ok(main) if matches!(main, Value::Function(_) | Value::Compiled(_)) => {
                interp.entry = Entry::Ran;
                let mut machine = vm::Vm::new(&mut interp);
                machine.call_and_run(main, Vec::new())?;
            }
            Ok(other) => interp.entry = Entry::NotCallable(type_name(&other)),
            Err(_) => interp.entry = Entry::Missing,
        }
        Ok(())
    })();

    let error = match result {
        Ok(()) => None,
        Err(Signal::Error(e)) => Some(e.render()),
        Err(Signal::Throw(t)) => Some(format!("Runtime Error: Uncaught {}", stringify(&t.value))),
        Err(Signal::Break) => Some(fail("'break' outside a loop.")),
        Err(Signal::Continue) => Some(fail("'continue' outside a loop.")),
        Err(Signal::Return(_)) => Some(fail("'return' outside a function.")),
    };

    Outcome {
        output: interp.output,
        error,
        entry: interp.entry,
    }
}

fn fail(message: &str) -> String {
    format!("Runtime Error: {message}")
}

fn strip_prefix(rendered: &str) -> String {
    rendered
        .strip_prefix("Syntax Error: ")
        .unwrap_or(rendered)
        .to_string()
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Interpreter {
        Interpreter::with_module_path(None)
    }

    /// An interpreter whose `import`s resolve against `path`.
    ///
    /// Without one, importing is an error rather than a resolution against
    /// the process's working directory -- which would make the same program
    /// mean different things depending on where it was run from.
    pub fn with_module_path(path: Option<PathBuf>) -> Interpreter {
        let globals = Env::new();
        builtins::install(&globals);
        Interpreter {
            env: globals.clone(),
            globals,
            output: Vec::new(),
            entry: Entry::Missing,
            engine: Engine::TreeWalker,
            module_path: path.map(|p| absolute(&p)),
            module_exports: HashMap::new(),
            module_loading: Vec::new(),
            current_exports: Vec::new(),
        }
    }

    /// Run a whole program. Returns the error text if it stopped early.
    pub fn interpret(&mut self, program: &Program) -> Option<String> {
        let result = (|| -> Exec {
            // The entry file runs by the same rules as any module: functions
            // and structs hoisted so a top-level `main` can call anything
            // declared below it, then everything else in source order.
            self.run_top_level(&program.statements)?;
            // A `main` is called if one was declared, matching the reference
            // implementation's entry convention.
            match self.globals.get("main", None) {
                Ok(main) if matches!(main, Value::Function(_)) => {
                    self.entry = Entry::Ran;
                    self.call_value(main, Vec::new(), None)?;
                }
                Ok(other) => self.entry = Entry::NotCallable(type_name(&other)),
                Err(_) => self.entry = Entry::Missing,
            }
            Ok(())
        })();

        // `output` stays the printed stream and the error is returned
        // separately, so a caller can place it where its host puts errors --
        // stderr for the CLI, an error pane for a playground -- rather than
        // having it already spliced into the output it is about to print.
        match result {
            Ok(()) => None,
            Err(Signal::Error(e)) => Some(e.render()),
            Err(Signal::Throw(t)) => {
                Some(format!("Runtime Error: Uncaught {}", stringify(&t.value)))
            }
            // `break`/`continue`/`return` outside their construct. The
            // reference implementation lets the signal escape and prints an
            // empty message; this says what happened.
            Err(Signal::Break) => Some(fail("'break' outside a loop.")),
            Err(Signal::Continue) => Some(fail("'continue' outside a loop.")),
            Err(Signal::Return(_)) => Some(fail("'return' outside a function.")),
        }
    }

    pub fn print_values(&mut self, values: &[Value]) {
        let line = values.iter().map(stringify).collect::<Vec<_>>().join(" ");
        self.output.push(line);
    }

    // -- statements --------------------------------------------------------

    pub fn execute(&mut self, stmt: &Stmt) -> Exec {
        match &stmt.kind {
            StmtKind::Expression(e) => {
                self.evaluate(e)?;
                Ok(())
            }
            StmtKind::Print(values) => {
                let values = self.evaluate_spread_list(values)?;
                self.print_values(&values);
                Ok(())
            }
            StmtKind::Var {
                pattern,
                initializer,
            } => {
                let value = match initializer {
                    Some(e) => self.evaluate(e)?,
                    None => Value::Null,
                };
                let env = self.env.clone();
                self.bind_pattern(pattern, Some(value), &env, Some(stmt.line), true)
            }
            StmtKind::DestructureAssign { pattern, value } => {
                let value = self.evaluate(value)?;
                let env = self.env.clone();
                self.bind_pattern(pattern, Some(value), &env, Some(stmt.line), false)
            }
            StmtKind::Block(statements) => {
                let scope = self.env.child();
                self.execute_block(statements, scope)
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                if self.evaluate(condition)?.is_truthy() {
                    self.execute(then_branch)
                } else if let Some(branch) = else_branch {
                    self.execute(branch)
                } else {
                    Ok(())
                }
            }
            StmtKind::While { condition, body } => {
                while self.evaluate(condition)?.is_truthy() {
                    match self.execute(body) {
                        Err(Signal::Break) => break,
                        Err(Signal::Continue) | Ok(()) => {}
                        Err(other) => return Err(other),
                    }
                }
                Ok(())
            }
            StmtKind::For {
                initializer,
                condition,
                increment,
                body,
            } => {
                // The header gets its own scope so a `var` in the initializer
                // does not leak into the surrounding block.
                let previous = self.env.clone();
                self.env = previous.child();
                let result = (|| -> Exec {
                    if let Some(init) = initializer {
                        self.execute(init)?;
                    }
                    loop {
                        if let Some(c) = condition {
                            if !self.evaluate(c)?.is_truthy() {
                                break;
                            }
                        }
                        match self.execute(body) {
                            Err(Signal::Break) => break,
                            // `continue` still runs the increment, which is
                            // why this is a dedicated node rather than sugar
                            // for `while`.
                            Err(Signal::Continue) | Ok(()) => {}
                            Err(other) => return Err(other),
                        }
                        if let Some(inc) = increment {
                            self.evaluate(inc)?;
                        }
                    }
                    Ok(())
                })();
                self.env = previous;
                result
            }
            StmtKind::ForIn {
                pattern,
                iterable,
                body,
            } => {
                let iterable = self.evaluate(iterable)?;
                // Pulled one at a time, not collected: the loop has to be able
                // to walk an endless generator and to stop early, leaving it
                // parked where it stopped for the next consumer.
                let mut cursor = self.cursor(&iterable, Some(stmt.line))?;
                let previous = self.env.clone();
                let result = (|| -> Exec {
                    while let Some(item) = self.advance(&mut cursor, Some(stmt.line))? {
                        // A fresh scope per iteration, so a closure made in
                        // the body captures this item rather than sharing one
                        // slot with every other iteration.
                        self.env = previous.child();
                        let scope = self.env.clone();
                        self.bind_pattern(pattern, Some(item), &scope, Some(stmt.line), true)?;
                        match self.execute(body) {
                            Err(Signal::Break) => break,
                            Err(Signal::Continue) | Ok(()) => {}
                            Err(other) => return Err(other),
                        }
                    }
                    Ok(())
                })();
                self.env = previous;
                result
            }
            StmtKind::Function {
                name,
                params,
                body,
                is_generator,
            } => {
                let function = Rc::new(Function {
                    name: Some(name.text.clone()),
                    params: params.clone(),
                    body: body.clone(),
                    closure: self.env.clone(),
                    is_generator: *is_generator,
                });
                self.env.define(&name.text, Value::Function(function));
                Ok(())
            }
            StmtKind::Return(value) => {
                let value = match value {
                    Some(e) => self.evaluate(e)?,
                    None => Value::Null,
                };
                Err(Signal::Return(value))
            }
            StmtKind::Break => Err(Signal::Break),
            StmtKind::Continue => Err(Signal::Continue),
            StmtKind::Throw(e) => {
                let value = self.evaluate(e)?;
                Err(Signal::Throw(Thrown {
                    value,
                    stack: Vec::new(),
                }))
            }
            StmtKind::Yield { .. } => Err(Signal::error(
                Kind::RuntimeError,
                "Generators are not implemented in this interpreter yet.",
            )
            .at(Some(stmt.line))),
            StmtKind::Try {
                body,
                catches,
                finally,
            } => self.execute_try(body, catches, finally.as_deref()),
            StmtKind::Match { subject, cases } => self.execute_match(stmt, subject, cases),
            StmtKind::Struct {
                name,
                fields,
                methods,
            } => {
                let methods = methods
                    .iter()
                    .filter_map(|m| match &m.kind {
                        StmtKind::Function {
                            name,
                            params,
                            body,
                            is_generator,
                        } => Some((
                            name.text.clone(),
                            Rc::new(Function {
                                name: Some(name.text.clone()),
                                params: params.clone(),
                                body: body.clone(),
                                closure: self.env.clone(),
                                is_generator: *is_generator,
                            }),
                        )),
                        _ => None,
                    })
                    .collect();
                let struct_type = Rc::new(StructType {
                    name: name.text.clone(),
                    fields: fields.clone(),
                    methods,
                });
                self.env.define(&name.text, Value::Struct(struct_type));
                Ok(())
            }
            StmtKind::Export { declaration, name } => {
                self.execute(declaration)?;
                // Read the value back out of the environment rather than
                // capturing it during the declaration: `export var x = f();`
                // must export whatever `x` ended up bound to.
                let value = self.env.get(&name.text, Some(name.line))?;
                self.record_export(&name.text, value);
                Ok(())
            }
            StmtKind::Import {
                names,
                namespace,
                specifier,
            } => self.execute_import(names, namespace.as_ref(), specifier, stmt.line),
            StmtKind::ExportNames { names, specifier } => {
                self.execute_export_names(names, specifier.as_deref(), stmt.line)
            }
        }
    }

    pub fn execute_block(&mut self, statements: &[Stmt], scope: Env) -> Exec {
        let previous = std::mem::replace(&mut self.env, scope);
        let mut result = Ok(());
        for stmt in statements {
            result = self.execute(stmt);
            if result.is_err() {
                break;
            }
        }
        self.env = previous;
        result
    }

    fn execute_try(
        &mut self,
        body: &[Stmt],
        catches: &[CatchClause],
        finally: Option<&[Stmt]>,
    ) -> Exec {
        let scope = self.env.child();
        let mut result = self.execute_block(body, scope);

        if let Err(signal) = &result {
            let caught = match signal {
                Signal::Throw(t) => Some(t.value.clone()),
                // An interpreter failure is catchable too: it reaches the
                // program as the standard error object, carrying `kind` for
                // guards to branch on.
                Signal::Error(e) => Some(error_value(e)),
                _ => None,
            };
            if let Some(value) = caught {
                match self.run_catch(catches, value) {
                    Ok(true) => result = Ok(()),
                    Ok(false) => {}
                    Err(e) => result = Err(e),
                }
            }
        }

        // Runs on every path out of the try -- normal completion, a caught or
        // uncaught throw, and a return/break/continue unwinding through it.
        // A signal raised by the `finally` block replaces whatever the try
        // was already carrying, which is why it is propagated before
        // `result`.
        if let Some(block) = finally {
            let scope = self.env.child();
            self.execute_block(block, scope)?;
        }
        result
    }

    fn run_catch(&mut self, catches: &[CatchClause], value: Value) -> Result<bool, Signal> {
        for clause in catches {
            let scope = self.env.child();
            self.bind_pattern(&clause.pattern, Some(value.clone()), &scope, None, true)?;

            if let Some(guard) = &clause.guard {
                let previous = std::mem::replace(&mut self.env, scope.clone());
                let passed = self.evaluate(guard);
                self.env = previous;
                if !passed?.is_truthy() {
                    continue;
                }
            }

            self.execute_block(&clause.body, scope)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn execute_match(&mut self, stmt: &Stmt, subject: &Expr, cases: &[MatchCase]) -> Exec {
        let subject = self.evaluate(subject)?;
        for case in cases {
            let scope = self.env.child();
            if let Some(pattern) = &case.pattern {
                if !self.match_pattern(pattern, &subject, &scope)? {
                    continue;
                }
            }
            if let Some(guard) = &case.guard {
                let previous = std::mem::replace(&mut self.env, scope.clone());
                let passed = self.evaluate(guard);
                self.env = previous;
                if !passed?.is_truthy() {
                    continue;
                }
            }
            return self.execute_block(&case.body, scope);
        }
        Err(value_error(format!(
            "No case matched {} in this match, and there is no 'default'.",
            stringify(&subject)
        ))
        .at(Some(stmt.line)))
    }

    // -- expressions -------------------------------------------------------

    pub fn evaluate(&mut self, expr: &Expr) -> Eval {
        let line = Some(expr.line);
        match &expr.kind {
            ExprKind::Literal(v) => Ok(match v {
                LitValue::Null => Value::Null,
                LitValue::Bool(b) => Value::Bool(*b),
                LitValue::Number(n) => Value::Number(*n),
                LitValue::Str(s) => Value::str(s.as_str()),
            }),
            ExprKind::Variable(name) => self.env.get(&name.text, Some(name.line)),
            ExprKind::Assign { name, value } => {
                let value = self.evaluate(value)?;
                self.env
                    .assign(&name.text, value.clone(), Some(name.line))?;
                Ok(value)
            }
            ExprKind::Binary { left, op, right } => {
                let left = self.evaluate(left)?;
                let right = self.evaluate(right)?;
                self.binary(left, *op, right, expr.line)
            }
            ExprKind::Logical { left, op, right } => {
                let left = self.evaluate(left)?;
                match op {
                    LogicalOp::Or if left.is_truthy() => Ok(left),
                    LogicalOp::And if !left.is_truthy() => Ok(left),
                    _ => self.evaluate(right),
                }
            }
            ExprKind::Unary { op, right } => {
                let right = self.evaluate(right)?;
                match op {
                    UnOp::Neg => match right {
                        Value::Number(n) => Ok(Value::Number(-n)),
                        _ => Err(type_error("Operand of '-' must be a number.").at(line)),
                    },
                    UnOp::Not => Ok(Value::Bool(!right.is_truthy())),
                }
            }
            ExprKind::Grouping(inner) => self.evaluate(inner),
            ExprKind::Call { callee, args } => {
                let callee = self.evaluate(callee)?;
                let args = self.evaluate_spread_list(args)?;
                self.call_value(callee, args, line)
            }
            ExprKind::Array(elements) => Ok(Value::array(self.evaluate_spread_list(elements)?)),
            ExprKind::Index { target, index } => {
                let target = self.evaluate(target)?;
                let index = self.evaluate(index)?;
                self.index_get(&target, &index, line)
            }
            ExprKind::IndexAssign {
                target,
                index,
                value,
            } => {
                let target = self.evaluate(target)?;
                let index = self.evaluate(index)?;
                let value = self.evaluate(value)?;
                self.index_set(&target, &index, value, line)
            }
            ExprKind::Dict(pairs) => {
                let mut map = ObjMap::new();
                for (key_expr, value_expr) in pairs {
                    let key = self.evaluate(key_expr)?;
                    let Some(key) = ObjKey::from_value(&key) else {
                        return Err(type_error(
                            "Object keys must be numbers, strings, or booleans (not arrays or objects).",
                        )
                        .at(line));
                    };
                    let value = self.evaluate(value_expr)?;
                    map.insert(key, value);
                }
                Ok(Value::object(map))
            }
            ExprKind::Spread(_) => Err(type_error(
                "'...' is only allowed in a call's arguments or an array literal.",
            )
            .at(line)),
            ExprKind::Function {
                name,
                params,
                body,
                is_generator,
            } => Ok(Value::Function(Rc::new(Function {
                name: name.as_ref().map(|n| n.text.clone()),
                params: params.clone(),
                body: body.clone(),
                closure: self.env.clone(),
                is_generator: *is_generator,
            }))),
            ExprKind::Interpolation(parts) => {
                let mut out = String::new();
                for part in parts {
                    match part {
                        InterpPart::Text(text) => out.push_str(text),
                        InterpPart::Expr(e) => {
                            let value = self.evaluate(e)?;
                            out.push_str(&stringify(&value));
                        }
                    }
                }
                Ok(Value::str(out))
            }
            ExprKind::Match { subject, arms } => {
                let subject = self.evaluate(subject)?;
                for arm in arms {
                    let scope = self.env.child();
                    if let Some(pattern) = &arm.pattern {
                        if !self.match_pattern(pattern, &subject, &scope)? {
                            continue;
                        }
                    }
                    // The guard and the arm's value both see the pattern's
                    // bindings, so both run in the arm's scope.
                    let previous = std::mem::replace(&mut self.env, scope);
                    let taken = match &arm.guard {
                        Some(guard) => self.evaluate(guard).map(|v| v.is_truthy()),
                        None => Ok(true),
                    };
                    let outcome = match taken {
                        Ok(true) => Some(self.evaluate(&arm.value)),
                        Ok(false) => None,
                        Err(e) => Some(Err(e)),
                    };
                    self.env = previous;
                    if let Some(result) = outcome {
                        return result;
                    }
                }
                Err(value_error(format!(
                    "No case matched {} in this match, and there is no 'default'.",
                    stringify(&subject)
                ))
                .at(line))
            }
            ExprKind::Yield(_) => Err(Signal::error(
                Kind::RuntimeError,
                "Generators are not implemented in this interpreter yet.",
            )
            .at(line)),
        }
    }

    pub fn evaluate_spread_list(&mut self, items: &[Expr]) -> Result<Vec<Value>, Signal> {
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            if let ExprKind::Spread(inner) = &item.kind {
                let spread = self.evaluate(inner)?;
                match spread {
                    Value::Array(a) => values.extend(a.borrow().iter().cloned()),
                    _ => {
                        return Err(
                            type_error("Can only spread an array with '...'.").at(Some(item.line))
                        )
                    }
                }
            } else {
                values.push(self.evaluate(item)?);
            }
        }
        Ok(values)
    }

    pub fn call_value(&mut self, callee: Value, args: Vec<Value>, line: Option<u32>) -> Eval {
        match callee {
            Value::Struct(struct_type) => {
                if !struct_type.accepts(args.len()) {
                    return Err(Signal::error(
                        Kind::ArityError,
                        format!(
                            "Struct {} takes {} field values but got {}.",
                            struct_type.name,
                            struct_type.arity_description(),
                            args.len()
                        ),
                    )
                    .at(line));
                }
                self.construct(&struct_type, args, line)
            }
            Value::Function(function) => {
                // Only the machine can suspend, so an AST generator -- a
                // struct's generator method, or any generator reached while
                // the tree-walker is driving -- is compiled here and handed
                // back as a parked frame. The alternative is a language whose
                // generators depend on which engine happened to run the file,
                // which is exactly the drift the corpus exists to prevent.
                if function.is_generator {
                    if !function.accepts(args.len()) {
                        return Err(Signal::error(
                            Kind::ArityError,
                            format!(
                                "Expected {} arguments but got {}.",
                                function.arity_description(),
                                args.len()
                            ),
                        )
                        .at(line));
                    }
                    return self.make_generator(&function, args, line);
                }
                if !function.accepts(args.len()) {
                    return Err(Signal::error(
                        Kind::ArityError,
                        format!(
                            "Expected {} arguments but got {}.",
                            function.arity_description(),
                            args.len()
                        ),
                    )
                    .at(line));
                }
                self.call_function(&function, args, line)
            }
            // A compiled closure handed to a builtin -- `map(xs, func(x){..})`
            // under the VM -- comes back here, because the builtin library is
            // shared. Running it means starting a machine, so that a value
            // never becomes uncallable merely by having been made by the
            // other engine.
            Value::Compiled(_) => {
                let mut machine = vm::Vm::new(self);
                machine.call_and_run(callee, args).map_err(|e| e.at(line))
            }
            Value::Builtin(builtin) => match &builtin.rng {
                // A value handed back by `random(seed)` is a closure over
                // PRNG state, not a named builtin, so it never reaches the
                // name table.
                Some(state) => builtins::next_random(state, &args).map_err(|e| e.at(line)),
                None => builtins::call(self, builtin.name, args).map_err(|e| e.at(line)),
            },
            _ => Err(type_error("Can only call functions.").at(line)),
        }
    }

    fn call_function(
        &mut self,
        function: &Rc<Function>,
        args: Vec<Value>,
        line: Option<u32>,
    ) -> Eval {
        let scope = function.closure.child();
        self.bind_params(&function.params, args, &scope, line)?;

        let frame = function
            .name
            .clone()
            .unwrap_or_else(|| "<anonymous>".into());
        let previous = std::mem::replace(&mut self.env, scope);
        let mut result = Ok(Value::Null);
        for stmt in &function.body {
            match self.execute(stmt) {
                Ok(()) => {}
                Err(Signal::Return(value)) => {
                    result = Ok(value);
                    break;
                }
                // Build the trace on the way out: a raise site knows nothing
                // about who called it, but every frame the error passes
                // through knows its own name. Innermost first.
                Err(other) => {
                    result = Err(other.with_frame(&frame));
                    break;
                }
            }
        }
        self.env = previous;
        result
    }

    pub(crate) fn bind_params(
        &mut self,
        params: &[Param],
        args: Vec<Value>,
        scope: &Env,
        line: Option<u32>,
    ) -> Exec {
        let positional: Vec<&Param> = params.iter().filter(|p| !p.rest).collect();

        // Defaults are evaluated at call time in the callee's own scope, so a
        // later default may refer to a parameter to its left.
        let previous = std::mem::replace(&mut self.env, scope.clone());
        let mut result = Ok(());
        for (i, param) in positional.iter().enumerate() {
            let value = args.get(i).cloned();
            if let Err(e) = self.bind_pattern(&param.pattern, value, scope, line, true) {
                result = Err(e);
                break;
            }
        }
        self.env = previous;
        result?;

        // A rest parameter always binds -- to an empty array when nothing is
        // left over.
        for param in params.iter().filter(|p| p.rest) {
            if let Pattern::Name { name, .. } = &param.pattern {
                let rest: Vec<Value> = args.iter().skip(positional.len()).cloned().collect();
                scope.define(&name.text, Value::array(rest));
            }
        }
        Ok(())
    }

    fn construct(
        &mut self,
        struct_type: &Rc<StructType>,
        args: Vec<Value>,
        line: Option<u32>,
    ) -> Eval {
        let scope = self.env.child();
        let previous = std::mem::replace(&mut self.env, scope.clone());
        let mut fields: Vec<(String, Value)> = Vec::new();
        let mut result = Ok(());
        for (i, field) in struct_type.fields.iter().enumerate() {
            let Pattern::Name { name, default } = &field.pattern else {
                continue;
            };
            let value = match args.get(i) {
                Some(v) => Ok(v.clone()),
                None => match default {
                    Some(d) => self.evaluate(d),
                    None => Err(arity_error(format!(
                        "Struct {} is missing a value for field '{}'.",
                        struct_type.name, name.text
                    ))),
                },
            };
            match value {
                Ok(v) => {
                    // Field defaults see the fields to their left.
                    scope.define(&name.text, v.clone());
                    fields.push((name.text.clone(), v));
                }
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        self.env = previous;
        result?;
        let _ = line;
        Ok(Value::Instance(Rc::new(Instance {
            struct_type: struct_type.clone(),
            fields: RefCell::new(fields),
        })))
    }

    /// A method pulled off an instance keeps its receiver, so `var m = p.mag;
    /// m()` still works.
    fn bind_method(&self, instance: &Rc<Instance>, function: &Rc<Function>) -> Value {
        let bound = function.closure.child();
        bound.define("this", Value::Instance(instance.clone()));
        Value::Function(Rc::new(Function {
            name: function.name.clone(),
            params: function.params.clone(),
            body: function.body.clone(),
            closure: bound,
            is_generator: function.is_generator,
        }))
    }
}

/// The object a `catch` block receives for an interpreter-raised error.
pub fn error_value(error: &RuntimeError) -> Value {
    let mut map = ObjMap::new();
    map.insert(
        ObjKey::Str("message".into()),
        Value::str(error.message.as_str()),
    );
    map.insert(
        ObjKey::Str("line".into()),
        match error.line {
            Some(line) => Value::Number(line as f64),
            None => Value::Null,
        },
    );
    map.insert(ObjKey::Str("kind".into()), Value::str(error.kind.as_str()));
    map.insert(
        ObjKey::Str("stack".into()),
        Value::array(error.stack.iter().map(|f| Value::str(f.as_str())).collect()),
    );
    Value::object(map)
}

// -- modules -----------------------------------------------------------------

/// `std::path::absolute` without the MSRV bump: make `path` absolute by
/// joining it onto the working directory, without touching the filesystem.
fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path),
        Err(_) => path.to_path_buf(),
    }
}

/// Resolve `.` and `..` lexically, the way `os.path.normpath` does.
///
/// Deliberately not `canonicalize`: that resolves symlinks and requires the
/// file to exist, so a missing module would fail with the OS's error instead
/// of MRT's own "Cannot find module", and two importers reaching one file by
/// different symlinked paths would get two evaluations instead of the cache
/// hit the language promises.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

impl Interpreter {
    pub(crate) fn record_export(&mut self, name: &str, value: Value) {
        // Re-exporting a name replaces the earlier entry in place, so the
        // table keeps first-export order rather than last.
        if let Some(slot) = self.current_exports.iter_mut().find(|(n, _)| n == name) {
            slot.1 = value;
        } else {
            self.current_exports.push((name.to_string(), value));
        }
    }

    /// Turn an import specifier into an absolute, normalized path.
    ///
    /// Only explicitly relative specifiers resolve: there is no search path,
    /// no implicit extension and no package directory, so an `import` always
    /// names exactly one file and reading the source tells you which.
    fn resolve_module(&self, specifier: &str, line: u32) -> Result<PathBuf, Signal> {
        if !(specifier.starts_with("./") || specifier.starts_with("../")) {
            return Err(value_error(format!(
                "Module path {} must start with './' or '../'.",
                json_quote(specifier)
            ))
            .at(Some(line)));
        }
        let Some(base) = self.module_path.as_ref().and_then(|p| p.parent()) else {
            return Err(Signal::error(
                Kind::RuntimeError,
                "Imports need a file to resolve against; run this program from a file.",
            )
            .at(Some(line)));
        };
        Ok(normalize(&base.join(specifier)))
    }

    /// Evaluate a module once and return its export table.
    fn load_module(&mut self, specifier: &str, line: u32) -> Result<Rc<Exports>, Signal> {
        let path = self.resolve_module(specifier, line)?;

        if let Some(exports) = self.module_exports.get(&path) {
            return Ok(exports.clone());
        }

        if self.module_loading.contains(&path) {
            // Named by basename, innermost path first, the way the reference
            // implementation reports it: the file names are what a reader
            // needs, and absolute paths would bury them.
            let cycle: Vec<String> = self
                .module_loading
                .iter()
                .chain(std::iter::once(&path))
                .map(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.to_string_lossy().into_owned())
                })
                .collect();
            return Err(Signal::error(
                Kind::RuntimeError,
                format!("Circular import: {}.", cycle.join(" -> ")),
            )
            .at(Some(line)));
        }

        let Ok(source) = std::fs::read_to_string(&path) else {
            return Err(
                value_error(format!("Cannot find module {}.", json_quote(specifier)))
                    .at(Some(line)),
            );
        };

        let file = SourceFile::new(path.to_string_lossy(), &source);
        let parsed = mrt_parser::parse(&file);
        if let Some(first) = parsed.errors.first() {
            return Err(Signal::error(
                Kind::RuntimeError,
                format!(
                    "Module {} has syntax errors: {}",
                    json_quote(specifier),
                    first.message
                ),
            )
            .at(Some(line)));
        }

        // A module gets a fresh scope off globals -- not off the importer's
        // scope, which would leak the importer's locals into it -- and its
        // own export table and path while it runs.
        let previous_env = std::mem::replace(&mut self.env, self.globals.child());
        let previous_path = self.module_path.replace(path.clone());
        let previous_exports = std::mem::take(&mut self.current_exports);
        self.module_loading.push(path.clone());

        let result = self.run_module_body(&parsed.program.statements, specifier, line);

        self.module_loading.pop();
        let exports = std::mem::replace(&mut self.current_exports, previous_exports);
        self.module_path = previous_path;
        self.env = previous_env;
        result?;

        let exports = Rc::new(exports);
        self.module_exports.insert(path, exports.clone());
        Ok(exports)
    }

    /// Evaluate an imported module's top level on the engine that is
    /// driving.
    ///
    /// A module is MRT source like any other, so the engine running the
    /// program should run it: otherwise `--vm` compiles the entry file and
    /// walks everything it imports. Both engines are held to the same corpus,
    /// so this changes no answer -- but "which engine ran this program" should
    /// have one answer rather than one per file.
    fn run_module_body(&mut self, statements: &[Stmt], specifier: &str, line: u32) -> Exec {
        if self.engine == Engine::TreeWalker {
            return self.run_top_level(statements);
        }
        let chunk = vm::compile_program(statements).map_err(|vm::Unsupported(what)| {
            // Named the same way the entry file's refusal is named, with the
            // module said out loud: a gap is a gap wherever the source lives.
            Signal::error(
                Kind::RuntimeError,
                format!("Module {}: {what}", json_quote(specifier)),
            )
            .at(Some(line))
        })?;
        let mut machine = vm::Vm::new(self);
        machine.run(chunk)?;
        Ok(())
    }

    /// Run a file's top-level statements: declarations first so they can
    /// refer to each other regardless of order, then everything else in
    /// source order.
    pub fn run_top_level(&mut self, statements: &[Stmt]) -> Exec {
        let hoisted = |s: &Stmt| {
            let inner = match &s.kind {
                StmtKind::Export { declaration, .. } => &declaration.kind,
                other => other,
            };
            matches!(inner, StmtKind::Function { .. } | StmtKind::Struct { .. })
        };
        for stmt in statements.iter().filter(|s| hoisted(s)) {
            self.execute(stmt)?;
        }
        for stmt in statements.iter().filter(|s| !hoisted(s)) {
            self.execute(stmt)?;
        }
        Ok(())
    }

    pub(crate) fn execute_import(
        &mut self,
        names: &[(Name, Name)],
        namespace: Option<&Name>,
        specifier: &str,
        line: u32,
    ) -> Exec {
        let exports = self.load_module(specifier, line)?;

        if let Some(namespace) = namespace {
            // A namespace import binds one ordinary MRT object, so dot access
            // and `keys()` work on it with no new machinery.
            let mut map = ObjMap::new();
            for (name, value) in exports.iter() {
                map.insert(ObjKey::Str(name.as_str().into()), value.clone());
            }
            self.env.define(&namespace.text, Value::object(map));
            return Ok(());
        }

        for (exported, local) in names {
            let Some(value) = exports_get(&exports, &exported.text) else {
                return Err(self.no_such_export(specifier, &exported.text, exported.line));
            };
            self.env.define(&local.text, value);
        }
        Ok(())
    }

    /// `export { a };` re-exports a local name; `export { a } from "./m.mrt";`
    /// forwards another module's export without binding it here.
    pub(crate) fn execute_export_names(
        &mut self,
        names: &[(Name, Name)],
        specifier: Option<&str>,
        line: u32,
    ) -> Exec {
        if let Some(specifier) = specifier {
            let source = self.load_module(specifier, line)?;
            for (local, exported) in names {
                let Some(value) = exports_get(&source, &local.text) else {
                    return Err(self.no_such_export(specifier, &local.text, local.line));
                };
                self.record_export(&exported.text, value);
            }
            return Ok(());
        }

        for (local, exported) in names {
            let value = self.env.get(&local.text, Some(local.line))?;
            self.record_export(&exported.text, value);
        }
        Ok(())
    }

    fn no_such_export(&self, specifier: &str, name: &str, line: u32) -> Signal {
        Signal::error(
            Kind::NameError,
            format!(
                "Module {} has no export named '{}'.",
                json_quote(specifier),
                name
            ),
        )
        .at(Some(line))
    }
}

// -- operators, indexing, iteration, patterns --------------------------------

impl Interpreter {
    pub(crate) fn binary(&mut self, left: Value, op: BinOp, right: Value, line: u32) -> Eval {
        let line = Some(line);
        match op {
            BinOp::Add => {
                // `+` concatenates when either side is a string, which is why
                // it is the one arithmetic operator that does not demand two
                // numbers.
                if matches!(left, Value::Str(_)) || matches!(right, Value::Str(_)) {
                    return Ok(Value::str(format!(
                        "{}{}",
                        stringify(&left),
                        stringify(&right)
                    )));
                }
                let (a, b) = numbers(&left, &right, "+", line)?;
                Ok(Value::Number(a + b))
            }
            BinOp::Sub => {
                let (a, b) = numbers(&left, &right, "-", line)?;
                Ok(Value::Number(a - b))
            }
            BinOp::Mul => {
                let (a, b) = numbers(&left, &right, "*", line)?;
                Ok(Value::Number(a * b))
            }
            BinOp::Div => {
                let (a, b) = numbers(&left, &right, "/", line)?;
                if b == 0.0 {
                    return Err(Signal::error(Kind::ArithmeticError, "Division by zero.").at(line));
                }
                Ok(Value::Number(a / b))
            }
            BinOp::Mod => {
                let (a, b) = numbers(&left, &right, "%", line)?;
                if b == 0.0 {
                    return Err(Signal::error(Kind::ArithmeticError, "Modulo by zero.").at(line));
                }
                // Takes the sign of the dividend, as in C and JavaScript.
                // Rust's `%` on f64 already does this.
                Ok(Value::Number(a % b))
            }
            BinOp::Equal => Ok(Value::Bool(values_equal(&left, &right))),
            BinOp::NotEqual => Ok(Value::Bool(!values_equal(&left, &right))),
            BinOp::Less => Ok(Value::Bool(compare(&left, &right, line)? < 0)),
            BinOp::LessEqual => Ok(Value::Bool(compare(&left, &right, line)? <= 0)),
            BinOp::Greater => Ok(Value::Bool(compare(&left, &right, line)? > 0)),
            BinOp::GreaterEqual => Ok(Value::Bool(compare(&left, &right, line)? >= 0)),
        }
    }

    pub(crate) fn index_get(&mut self, target: &Value, index: &Value, line: Option<u32>) -> Eval {
        match target {
            Value::Instance(instance) => {
                let Some(name) = index.as_str() else {
                    return Err(type_error("A struct field name must be a string.").at(line));
                };
                if let Some(value) = instance.get_field(name) {
                    return Ok(value);
                }
                if let Some(method) = instance.struct_type.method(name) {
                    return Ok(self.bind_method(instance, method));
                }
                Err(Signal::error(
                    Kind::KeyError,
                    format!(
                        "Struct {} has no field or method {}.",
                        instance.struct_type.name,
                        json_quote(name)
                    ),
                )
                .at(line))
            }
            Value::Object(map) => {
                let Some(key) = ObjKey::from_value(index) else {
                    return Err(type_error(
                        "Object keys must be numbers, strings, or booleans (not arrays or objects).",
                    )
                    .at(line));
                };
                match map.borrow().get(&key) {
                    Some(value) => Ok(value.clone()),
                    None => Err(Signal::error(
                        Kind::KeyError,
                        format!("Key {} not found in object.", json_quote(&stringify(index))),
                    )
                    .at(line)),
                }
            }
            Value::Array(items) => {
                let items = items.borrow();
                let i = array_index(index, items.len(), line)?;
                Ok(items[i].clone())
            }
            Value::Str(s) => {
                let chars: Vec<char> = s.chars().collect();
                let i = array_index(index, chars.len(), line)?;
                Ok(Value::str(chars[i].to_string()))
            }
            _ => Err(type_error("Can only index into arrays, objects, or strings.").at(line)),
        }
    }

    pub(crate) fn index_set(
        &mut self,
        target: &Value,
        index: &Value,
        value: Value,
        line: Option<u32>,
    ) -> Eval {
        match target {
            Value::Instance(instance) => {
                let Some(name) = index.as_str() else {
                    return Err(type_error("A struct field name must be a string.").at(line));
                };
                if instance.set_field(name, value.clone()) {
                    return Ok(value);
                }
                Err(Signal::error(
                    Kind::KeyError,
                    format!(
                        "Struct {} has no field {}.",
                        instance.struct_type.name,
                        json_quote(name)
                    ),
                )
                .at(line))
            }
            Value::Object(map) => {
                let Some(key) = ObjKey::from_value(index) else {
                    return Err(type_error(
                        "Object keys must be numbers, strings, or booleans (not arrays or objects).",
                    )
                    .at(line));
                };
                map.borrow_mut().insert(key, value.clone());
                Ok(value)
            }
            Value::Array(items) => {
                let len = items.borrow().len();
                let i = array_index(index, len, line)?;
                items.borrow_mut()[i] = value.clone();
                Ok(value)
            }
            _ => Err(type_error("Can only assign into arrays or objects.").at(line)),
        }
    }

    /// A value's items, materialised.
    ///
    /// Only for consumers that genuinely need all of them -- `list()`,
    /// `sorted()`, spread. Anything that consumes in order should take a
    /// `cursor` instead, because this one does not return on an endless
    /// generator.
    pub fn iterate(&mut self, value: &Value, line: Option<u32>) -> Result<Vec<Value>, Signal> {
        let mut cursor = self.cursor(value, line)?;
        let mut items = Vec::new();
        while let Some(item) = self.advance(&mut cursor, line)? {
            items.push(item);
        }
        Ok(items)
    }

    /// A cursor over a value's items, pulling only as far as it is asked.
    ///
    /// Laziness is not an optimisation here, it is the feature: `for (n in
    /// naturals())` over an endless generator has to run, and materialising
    /// first would simply hang. Eager sources keep the representation they
    /// already had, so nothing pays for laziness that has none to offer.
    pub fn cursor(&mut self, value: &Value, line: Option<u32>) -> Result<Cursor, Signal> {
        match value {
            Value::Generator(generator) => {
                // Single use, and the message has to distinguish "used up"
                // from "never started" -- a finished generator is an error to
                // iterate rather than a silently empty loop.
                if generator.is_done() {
                    return Err(already_iterated(&generator.label, line));
                }
                Ok(Cursor::Gen(generator.clone()))
            }
            // The iterator protocol resolves here rather than in the eager
            // collector, so a struct whose `iter()` hands back a generator is
            // as lazy as the generator is -- which is the whole point of
            // letting a struct supply one.
            Value::Instance(instance) => match instance.struct_type.method("iter") {
                Some(method) => {
                    let sequence = self.call_iter(instance, method, line)?;
                    self.cursor(&sequence, line)
                }
                // A struct without `iter()` iterates its field names, exactly
                // as a plain object does.
                None => Ok(Cursor::items(
                    instance
                        .fields
                        .borrow()
                        .iter()
                        .map(|(k, _)| Value::str(k.as_str()))
                        .collect(),
                )),
            },
            other => Ok(Cursor::items(self.collect_eager(other, line)?)),
        }
    }

    /// Call a struct's `iter()` and check what it handed back.
    ///
    /// The two rejections are the language's, not defensive coding: a struct
    /// returning *itself* would iterate forever, and one returning something
    /// that is not a sequence has to say so against the struct rather than
    /// failing somewhere further along.
    fn call_iter(
        &mut self,
        instance: &Rc<Instance>,
        method: &Rc<Function>,
        line: Option<u32>,
    ) -> Result<Value, Signal> {
        let bound = self.bind_method(instance, method);
        let sequence = self.call_value(bound, Vec::new(), line)?;
        if let Value::Instance(other) = &sequence {
            if Rc::ptr_eq(other, instance) {
                return Err(value_error(format!(
                    "{}.iter() returned the struct itself, which would iterate forever.",
                    instance.struct_type.name
                ))
                .at(line));
            }
        }
        if !is_iterable(&sequence) {
            return Err(type_error(format!(
                "{}.iter() returned {}, which isn't iterable.",
                instance.struct_type.name,
                type_name(&sequence)
            ))
            .at(line));
        }
        Ok(sequence)
    }

    /// Pull the next item, or `None` once the sequence has run out.
    pub fn advance(
        &mut self,
        cursor: &mut Cursor,
        line: Option<u32>,
    ) -> Result<Option<Value>, Signal> {
        self.advance_with(cursor, Value::Null, line)
    }

    /// Pull the next item, passing a sent value into the sequence.
    ///
    /// Only `yield*` sends anything: delegation has to forward what was sent
    /// into the *outer* generator on to the inner one, or a two-way protocol
    /// stops working the moment it is factored into a delegate. An eager
    /// sequence has nothing to receive it, so it is dropped there.
    pub fn advance_with(
        &mut self,
        cursor: &mut Cursor,
        sent: Value,
        line: Option<u32>,
    ) -> Result<Option<Value>, Signal> {
        match cursor {
            Cursor::Items { items, next } => {
                let item = items.get(*next).cloned();
                if item.is_some() {
                    *next += 1;
                }
                Ok(item)
            }
            Cursor::Gen(generator) => {
                let generator = generator.clone();
                self.step_generator(&generator, sent, line)
            }
        }
    }

    /// Compile an AST generator function's body and park it in a value.
    ///
    /// Nothing of the body runs: calling a generator function hands back a
    /// sequence, and the first instruction waits for the first pull. The
    /// compiled body is not cached on the function, so two calls of one
    /// generator function compile twice -- correct, and the cost is paid once
    /// per call rather than once per step.
    fn make_generator(
        &mut self,
        function: &Rc<Function>,
        args: Vec<Value>,
        line: Option<u32>,
    ) -> Result<Value, Signal> {
        let proto = vm::compile_function(&function.params, &function.body, function.name.clone())
            .map_err(|unsupported| {
            // A generator body using something the compiler does not
            // handle yet fails here, by name, rather than somewhere
            // downstream wearing a misleading message.
            // `Unsupported` already carries the whole sentence.
            Signal::error(Kind::RuntimeError, unsupported.0).at(line)
        })?;
        let compiled = Rc::new(Compiled {
            proto,
            closure: function.closure.clone(),
        });
        Ok(Value::Generator(Rc::new(Generator::starting(
            vm::generator_label(&function.name),
            compiled,
            args,
        ))))
    }

    /// Pull one value from a generator, running its body until it yields.
    ///
    /// `Ok(None)` means the sequence has run out. `sent` becomes the value of
    /// the `yield` the body is parked at -- discarded on the first step,
    /// because the body has not reached a `yield` yet to receive it.
    pub fn step_generator(
        &mut self,
        generator: &Rc<Generator>,
        sent: Value,
        line: Option<u32>,
    ) -> Result<Option<Value>, Signal> {
        // Take the state out for the duration of the step. That is what makes
        // `Running` observable: a body that resumes its own generator finds
        // it, and gets the language's error instead of a borrow panic from
        // the host.
        let state = generator.state.replace(GenState::Running);
        match state {
            GenState::Done => {
                generator.state.replace(GenState::Done);
                Ok(None)
            }
            GenState::Running => {
                generator.state.replace(GenState::Running);
                Err(already_running(&generator.label, line))
            }
            GenState::Transform { kind, source, f } => {
                let outcome = self.step_transform(kind, source, f, generator, line);
                if matches!(outcome, Ok(None) | Err(_)) {
                    generator.state.replace(GenState::Done);
                }
                outcome
            }
            started => {
                // The body runs on the machine: suspending is the one thing
                // only it can do. Whatever the step leaves behind -- parked
                // at the next `yield`, or finished -- is written back by the
                // resumer.
                vm::resume_generator(self, generator, started, sent, line)
            }
        }
    }

    /// One step of a lazy `map` or `filter`.
    ///
    /// Only the pulling is lazy: the function is the language's, so it runs
    /// through `call_value` exactly as an eager `map` would, and a transform
    /// over an eager source is still a generator that nothing has to
    /// materialise.
    fn step_transform(
        &mut self,
        kind: Transform,
        mut source: Box<Cursor>,
        f: Value,
        generator: &Rc<Generator>,
        line: Option<u32>,
    ) -> Result<Option<Value>, Signal> {
        loop {
            let Some(item) = self.advance(&mut source, line)? else {
                return Ok(None);
            };
            match kind {
                Transform::Map => {
                    let mapped = self.call_value(f.clone(), vec![item], line)?;
                    generator.state.replace(GenState::Transform {
                        kind,
                        source,
                        f: f.clone(),
                    });
                    return Ok(Some(mapped));
                }
                Transform::Filter => {
                    let keep = self.call_value(f.clone(), vec![item.clone()], line)?;
                    if keep.is_truthy() {
                        generator.state.replace(GenState::Transform {
                            kind,
                            source,
                            f: f.clone(),
                        });
                        return Ok(Some(item));
                    }
                }
            }
        }
    }

    /// The items of an eager sequence: nothing here can be endless, so
    /// collecting is safe. Generators and the `iter()` protocol are resolved
    /// by `cursor` before reaching this.
    fn collect_eager(&mut self, value: &Value, line: Option<u32>) -> Result<Vec<Value>, Signal> {
        match value {
            Value::Array(items) => Ok(items.borrow().clone()),
            Value::Str(s) => Ok(s.chars().map(|c| Value::str(c.to_string())).collect()),
            Value::Object(map) => Ok(map.borrow().keys().map(|k| k.to_value()).collect()),
            _ => Err(
                type_error("Can only iterate over an array, string, object, or generator.")
                    .at(line),
            ),
        }
    }

    // -- binding patterns --------------------------------------------------

    /// Bind `value` to `pattern`. With `declare = false` the leaves are
    /// *assigned* to variables that must already exist, which is what
    /// `[a, b] = pair;` means -- the same code path either way, so the two
    /// forms cannot drift apart.
    pub fn bind_pattern(
        &mut self,
        pattern: &Pattern,
        value: Option<Value>,
        scope: &Env,
        line: Option<u32>,
        declare: bool,
    ) -> Exec {
        let value = match value {
            Some(v) => v,
            None => match pattern.default() {
                Some(default) => self.evaluate(default)?,
                None => {
                    return Err(value_error(
                        "Cannot destructure: no value for this part of the pattern.",
                    )
                    .at(line))
                }
            },
        };

        match pattern {
            Pattern::Name { name, .. } => self.bind_name(&name.text, value, scope, line, declare),
            Pattern::Array {
                elements,
                rest,
                line: at,
                ..
            } => {
                let where_ = Some(*at).or(line);
                let Value::Array(items) = &value else {
                    return Err(type_error(format!(
                        "Cannot destructure {} with an array pattern.",
                        type_name(&value)
                    ))
                    .at(where_));
                };
                let items = items.borrow().clone();
                for (i, element) in elements.iter().enumerate() {
                    let slot = items.get(i).cloned();
                    if slot.is_none() && element.default().is_none() {
                        return Err(Signal::error(
                            Kind::IndexError,
                            format!(
                                "Cannot destructure: the array has {} element(s) but the pattern \
                                 needs at least {}.",
                                items.len(),
                                elements.len()
                            ),
                        )
                        .at(where_));
                    }
                    self.bind_pattern(element, slot, scope, where_, declare)?;
                }
                if let Some(rest) = rest {
                    let remaining: Vec<Value> =
                        items.iter().skip(elements.len()).cloned().collect();
                    self.bind_name(&rest.text, Value::array(remaining), scope, where_, declare)?;
                }
                Ok(())
            }
            Pattern::Object {
                entries,
                rest,
                line: at,
                ..
            } => {
                let where_ = Some(*at).or(line);
                let source = object_fields(&value).ok_or_else(|| {
                    type_error(format!(
                        "Cannot destructure {} with an object pattern.",
                        type_name(&value)
                    ))
                    .at(where_)
                })?;
                let mut taken: Vec<&str> = Vec::new();
                for (key, sub) in entries {
                    taken.push(key);
                    let slot = source
                        .iter()
                        .find(|(k, _)| k == key)
                        .map(|(_, v)| v.clone());
                    if slot.is_none() && sub.default().is_none() {
                        return Err(Signal::error(
                            Kind::KeyError,
                            format!(
                                "Cannot destructure: no key {} in the object.",
                                json_quote(key)
                            ),
                        )
                        .at(where_));
                    }
                    self.bind_pattern(sub, slot, scope, where_, declare)?;
                }
                if let Some(rest) = rest {
                    let remaining: ObjMap = source
                        .iter()
                        .filter(|(k, _)| !taken.contains(&k.as_str()))
                        .map(|(k, v)| (ObjKey::Str(k.as_str().into()), v.clone()))
                        .collect();
                    self.bind_name(&rest.text, Value::object(remaining), scope, where_, declare)?;
                }
                Ok(())
            }
        }
    }

    fn bind_name(
        &mut self,
        name: &str,
        value: Value,
        scope: &Env,
        line: Option<u32>,
        declare: bool,
    ) -> Exec {
        if declare {
            scope.define(name, value);
            Ok(())
        } else {
            scope.assign(name, value, line)
        }
    }

    // -- match patterns ----------------------------------------------------

    /// Test `value` against `pattern`, binding names into `scope`. Returns
    /// `false` rather than erroring when the shape does not fit -- that is the
    /// whole point of matching.
    pub fn match_pattern(
        &mut self,
        pattern: &MatchPattern,
        value: &Value,
        scope: &Env,
    ) -> Result<bool, Signal> {
        match pattern {
            MatchPattern::Literal {
                value: expected, ..
            } => {
                let expected = match expected {
                    LitValue::Null => Value::Null,
                    LitValue::Bool(b) => Value::Bool(*b),
                    LitValue::Number(n) => Value::Number(*n),
                    LitValue::Str(s) => Value::str(s.as_str()),
                };
                Ok(values_equal(value, &expected))
            }
            MatchPattern::Bind { name } => {
                scope.define(&name.text, value.clone());
                Ok(true)
            }
            MatchPattern::Array { elements, rest, .. } => {
                let Value::Array(items) = value else {
                    return Ok(false);
                };
                let items = items.borrow().clone();
                // Unlike destructuring, the length must match exactly unless a
                // rest is given: `case [x]:` swallowing every non-empty array
                // would make matching useless.
                match rest {
                    None if items.len() != elements.len() => return Ok(false),
                    Some(_) if items.len() < elements.len() => return Ok(false),
                    _ => {}
                }
                for (element, item) in elements.iter().zip(items.iter()) {
                    if !self.match_pattern(element, item, scope)? {
                        return Ok(false);
                    }
                }
                if let Some(rest) = rest {
                    let remaining: Vec<Value> =
                        items.iter().skip(elements.len()).cloned().collect();
                    scope.define(&rest.text, Value::array(remaining));
                }
                Ok(true)
            }
            MatchPattern::Object { entries, .. } => {
                let Some(source) = object_fields(value) else {
                    return Ok(false);
                };
                for (key, sub) in entries {
                    let Some((_, found)) = source.iter().find(|(k, _)| k == key) else {
                        return Ok(false);
                    };
                    let found = found.clone();
                    if !self.match_pattern(sub, &found, scope)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            MatchPattern::Struct { name, elements, .. } => {
                let declared = self.env.get(&name.text, Some(name.line))?;
                let Value::Struct(struct_type) = declared else {
                    return Err(type_error(format!(
                        "'{}' is not a struct, so it can't be used as a pattern.",
                        name.text
                    ))
                    .at(Some(name.line)));
                };
                if elements.len() != struct_type.fields.len() {
                    return Err(arity_error(format!(
                        "Pattern for struct {} has {} field(s) but the struct declares {}.",
                        struct_type.name,
                        elements.len(),
                        struct_type.fields.len()
                    ))
                    .at(Some(name.line)));
                }
                let Value::Instance(instance) = value else {
                    return Ok(false);
                };
                if !Rc::ptr_eq(&instance.struct_type, &struct_type) {
                    return Ok(false);
                }
                let fields = instance.fields.borrow().clone();
                for (element, (_, field)) in elements.iter().zip(fields.iter()) {
                    if !self.match_pattern(element, field, scope)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }
}

// -- helpers -----------------------------------------------------------------

fn numbers(left: &Value, right: &Value, op: &str, line: Option<u32>) -> Result<(f64, f64), Signal> {
    match (left.as_number(), right.as_number()) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err(type_error(format!("Operands of '{op}' must be numbers.")).at(line)),
    }
}

fn compare(left: &Value, right: &Value, line: Option<u32>) -> Result<i32, Signal> {
    if let (Value::Str(a), Value::Str(b)) = (left, right) {
        return Ok(match a.cmp(b) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        });
    }
    match (left.as_number(), right.as_number()) {
        (Some(a), Some(b)) => Ok(if a < b {
            -1
        } else if a > b {
            1
        } else {
            0
        }),
        _ => Err(type_error("Comparison operators require two numbers or two strings.").at(line)),
    }
}

fn array_index(index: &Value, len: usize, line: Option<u32>) -> Result<usize, Signal> {
    let Some(n) = index.as_number() else {
        return Err(Signal::error(Kind::IndexError, "Array index must be a number.").at(line));
    };
    if n.fract() != 0.0 {
        return Err(
            Signal::error(Kind::IndexError, "Array index must be a whole number.").at(line),
        );
    }
    let i = n as i64;
    if i < 0 || i as usize >= len {
        return Err(Signal::error(
            Kind::IndexError,
            format!(
                "Array index {} out of bounds for array of length {len}.",
                format_number(n)
            ),
        )
        .at(line));
    }
    Ok(i as usize)
}

/// The field mapping of anything that reads like an object: a plain object, or
/// a struct instance's fields (not its methods).
pub fn object_fields(value: &Value) -> Option<Vec<(String, Value)>> {
    match value {
        Value::Object(map) => Some(
            map.borrow()
                .iter()
                .map(|(k, v)| (stringify(&k.to_value()), v.clone()))
                .collect(),
        ),
        Value::Instance(i) => Some(i.fields.borrow().clone()),
        _ => None,
    }
}

pub fn is_iterable(value: &Value) -> bool {
    matches!(
        value,
        Value::Array(_)
            | Value::Str(_)
            | Value::Object(_)
            | Value::Instance(_)
            | Value::Generator(_)
    )
}

/// JSON-style quoting, matching what both existing implementations use in
/// error messages -- Python's `repr` would quote with apostrophes and a
/// program printing `e.message` would see different text.
pub fn json_quote(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Run a program and return everything it printed, plus its error text if
    /// it stopped early -- the same stream a reader of the terminal sees.
    /// What became of `main`, on both engines -- they must agree, since a
    /// file's entry point cannot depend on which one ran it.
    fn entry_of(source: &str) -> Entry {
        let file = SourceFile::new("test.mrt", source);
        let walked = run(&file).entry;
        let compiled = run_vm(&file).entry;
        assert_eq!(walked, compiled, "engines disagree on the entry point");
        walked
    }

    #[test]
    fn a_declared_main_is_recorded_as_having_run() {
        assert_eq!(entry_of("func main() { print(1); }"), Entry::Ran);
    }

    #[test]
    fn a_file_of_only_declarations_has_no_entry_point() {
        // It runs correctly and prints nothing, which reads as a broken
        // install unless the command says otherwise.
        assert_eq!(entry_of("struct Place { name; }"), Entry::Missing);
        assert_eq!(
            entry_of("func outer() { func main() { print(1); } }"),
            Entry::Missing,
            "a nested main never reaches the globals"
        );
        assert_eq!(
            entry_of("func Main() { print(1); }"),
            Entry::Missing,
            "names are case-sensitive"
        );
    }

    #[test]
    fn an_uncallable_main_is_told_apart_from_a_missing_one() {
        // A different mistake: the name is taken by something that is not a
        // function, and saying "no main" would send the reader looking for
        // something that is right there.
        assert_eq!(
            entry_of("var main = 5;"),
            Entry::NotCallable("number".into())
        );
        assert_eq!(
            entry_of("struct P { x; } var main = P;"),
            Entry::NotCallable("struct".into())
        );
    }

    #[test]
    fn the_entry_point_does_not_reach_the_printed_output() {
        // The note is the CLI's, on stderr. `output` is the stream four
        // implementations are held to match byte for byte, and nothing here
        // may appear in it.
        assert_eq!(go("struct Place { name; }"), "");
        assert_eq!(go("var main = 5;"), "");
    }

    fn go(source: &str) -> String {
        let file = SourceFile::new("test.mrt", source);
        let outcome = run(&file);
        let mut lines = outcome.output;
        if let Some(error) = outcome.error {
            lines.push(error);
        }
        lines.join("\n")
    }

    /// Wrap statements in a `main` so a test reads as a snippet rather than a
    /// program.
    fn main_of(body: &str) -> String {
        go(&format!("func main() {{ {body} }}"))
    }

    // The conformance harness is the real check on behaviour; these cover the
    // decisions this implementation had to make on its own, where agreeing
    // with the other two is not automatic.

    #[test]
    fn aggregates_are_shared_handles_and_scalars_are_not() {
        assert_eq!(
            main_of("var a = [1]; var b = a; push(b, 2); print(a); var x = 1; var y = x; y = 9; print(x);"),
            "[1, 2]\n1"
        );
    }

    #[test]
    fn numbers_print_without_a_trailing_point_zero() {
        assert_eq!(
            main_of("print(1, 1.5, -0.0, 1 / 3);"),
            "1 1.5 0 0.3333333333333333"
        );
    }

    #[test]
    fn very_large_and_very_small_numbers_use_pythons_exponent_form() {
        // Rust would print 1e20 as 100000000000000000000 and 1e-7 as
        // 0.0000001; the reference implementation prints neither.
        assert_eq!(
            main_of("print(pow(10, 20), pow(10, -7), pow(10, 16));"),
            "1e+20 1e-07 1e+16"
        );
    }

    #[test]
    fn only_null_and_false_are_falsy() {
        assert_eq!(
            main_of(
                r#"for (v in [0, "", [], null, false]) { if (v) { print("truthy"); } else { print("falsy"); } }"#
            ),
            "truthy\ntruthy\ntruthy\nfalsy\nfalsy"
        );
    }

    #[test]
    fn true_and_one_are_different_object_keys() {
        // Python's dict conflates them and JavaScript's Map does not; MRT's
        // `==` says they differ, so the key type has to say so too.
        assert_eq!(
            main_of(
                r#"var o = {}; o[true] = "t"; o[1] = "n"; print(len(o), keys(o), o[true], o[1]);"#
            ),
            "2 [true, 1] t n"
        );
    }

    #[test]
    fn negative_zero_is_the_same_object_key_as_zero() {
        assert_eq!(
            main_of(r#"var o = {0: "a"}; o[-0] = "b"; print(len(o), o[0]);"#),
            "1 b"
        );
    }

    #[test]
    fn modulo_takes_the_sign_of_the_dividend() {
        assert_eq!(
            main_of("print(7 % 3, -7 % 3, 7 % -3, -7 % -3);"),
            "1 -1 1 -1"
        );
    }

    #[test]
    fn object_key_order_is_insertion_order_and_survives_reassignment() {
        assert_eq!(
            main_of(r#"var o = {b: 1, a: 2}; o.b = 3; o.c = 4; print(keys(o), values(o));"#),
            "[b, a, c]\n[3, 2, 4]".replace('\n', " ").as_str()
        );
    }

    #[test]
    fn closures_capture_the_variable_not_its_value() {
        assert_eq!(
            main_of("var n = 1; var f = func() { return n; }; n = 2; print(f());"),
            "2"
        );
    }

    #[test]
    fn a_for_in_loop_binds_a_fresh_variable_each_iteration() {
        assert_eq!(
            main_of("var fs = []; for (i in [1, 2, 3]) { push(fs, func() { return i; }); } for (f in fs) { print(f()); }"),
            "1\n2\n3"
        );
    }

    #[test]
    fn a_finally_block_runs_on_every_way_out() {
        assert_eq!(
            go("func f() { try { return \"returned\"; } finally { print(\"finally\"); } }\nfunc main() { print(f()); }"),
            "finally\nreturned"
        );
    }

    #[test]
    fn an_error_raised_in_finally_replaces_the_one_being_carried() {
        assert_eq!(
            main_of(
                r#"try { try { throw "first"; } finally { throw "second"; } } catch (e) { print(e); }"#
            ),
            "second"
        );
    }

    #[test]
    fn indexing_errors_report_the_line_they_were_written_on() {
        assert_eq!(
            go("func main() {\n  var a = [1];\n  print(a[7]);\n}"),
            "Runtime Error: Array index 7 out of bounds for array of length 1. [line 3]"
        );
    }

    #[test]
    fn an_uncaught_throw_reports_the_thrown_value() {
        assert_eq!(
            main_of(r#"print("before"); throw {code: 42}; print("never");"#),
            "before\nRuntime Error: Uncaught {code: 42}"
        );
    }

    #[test]
    fn a_seeded_random_stream_is_deterministic_across_runs() {
        let once = main_of("var r = random(42); print(r(), r(), r());");
        assert_eq!(once, main_of("var r = random(42); print(r(), r(), r());"));
        assert_ne!(once, main_of("var r = random(7); print(r(), r(), r());"));
    }

    #[test]
    fn the_tree_walker_produces_a_generator_by_compiling_it() {
        // This used to assert the opposite -- that generators were refused.
        // The tree-walker still cannot *suspend* one, and never will: it
        // compiles the body and hands back a parked machine frame, so a
        // generator means the same thing whichever engine ran the file.
        assert_eq!(
            go("func g() { yield 1; }\nfunc main() { print(g()); }"),
            "<generator g>"
        );
        assert_eq!(
            go("func g() { yield 1; yield 2; }\nfunc main() { for (x in g()) { print(x); } }"),
            "1\n2"
        );
    }

    // -- modules ---------------------------------------------------------

    /// Write a file tree into a temp directory and run `main.mrt` from it.
    fn go_files(label: &str, files: &[(&str, &str)]) -> String {
        let root = std::env::temp_dir().join(format!("mrt-interp-test-{label}"));
        let _ = std::fs::remove_dir_all(&root);
        for (relative, contents) in files {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
        }
        let entry = root.join("main.mrt");
        let source = std::fs::read_to_string(&entry).unwrap();
        let file = SourceFile::new(entry.to_string_lossy(), &source);
        let outcome = run(&file);
        let mut lines = outcome.output;
        if let Some(error) = outcome.error {
            lines.push(error);
        }
        let _ = std::fs::remove_dir_all(&root);
        lines.join("\n")
    }

    #[test]
    fn importing_without_a_file_to_resolve_against_says_so() {
        // Only reachable through the library API -- the CLI always has a
        // path -- so the conformance harness cannot cover it.
        let file = SourceFile::new("<stdin>", "import { x } from \"./m.mrt\";\nfunc main() { }");
        let parsed = mrt_parser::parse(&file);
        let mut interp = Interpreter::new();
        assert_eq!(
            interp.interpret(&parsed.program),
            Some(
                "Runtime Error: Imports need a file to resolve against; \
                 run this program from a file. [line 1]"
                    .to_string()
            )
        );
    }

    #[test]
    fn a_module_is_evaluated_once_however_many_specifiers_reach_it() {
        assert_eq!(
            go_files(
                "cache",
                &[
                    (
                        "main.mrt",
                        "import { A } from \"./a.mrt\";\n\
                         import { A as B } from \"./lib/../a.mrt\";\n\
                         func main() { print(A, B); }\n"
                    ),
                    ("a.mrt", "print(\"evaluated\");\nexport var A = 1;\n"),
                    ("lib/keep.mrt", "export var unused = 1;\n"),
                ]
            ),
            "evaluated\n1 1"
        );
    }

    #[test]
    fn a_modules_locals_stay_in_the_module_but_globals_reach_in() {
        assert_eq!(
            go_files(
                "scope",
                &[
                    (
                        "main.mrt",
                        "var shared = \"global\";\n\
                         import { peek, look } from \"./m.mrt\";\n\
                         func main() { print(peek(), look()); \
                         try { print(secret); } catch (e) { print(e.kind); } }\n"
                    ),
                    (
                        "m.mrt",
                        "var secret = \"hidden\";\n\
                         export func peek() { return secret; }\n\
                         export func look() { return shared; }\n"
                    ),
                ]
            ),
            "hidden global\nNameError"
        );
    }

    #[test]
    fn a_cycle_is_named_by_basename_innermost_last() {
        assert_eq!(
            go_files(
                "cycle",
                &[
                    (
                        "main.mrt",
                        "import { a } from \"./a.mrt\";\nfunc main() { }\n"
                    ),
                    (
                        "a.mrt",
                        "import { b } from \"./b.mrt\";\nexport var a = 1;\n"
                    ),
                    (
                        "b.mrt",
                        "import { a } from \"./a.mrt\";\nexport var b = 2;\n"
                    ),
                ]
            ),
            "Runtime Error: Circular import: a.mrt -> b.mrt -> a.mrt. [line 1]"
        );
    }

    #[test]
    fn a_namespace_import_is_an_ordinary_object_in_export_order() {
        // Declarations hoist, so `square` is exported before `PI` even though
        // `PI` is written first -- and `keys()` on the namespace shows it.
        assert_eq!(
            go_files(
                "namespace",
                &[
                    (
                        "main.mrt",
                        "import * as m from \"./m.mrt\";\n\
                         func main() { print(type(m), keys(m), m.square(4)); }\n"
                    ),
                    (
                        "m.mrt",
                        "export var PI = 3;\nexport func square(n) { return n * n; }\n"
                    ),
                ]
            ),
            "object [square, PI] 16"
        );
    }

    #[test]
    fn a_syntax_error_is_reported_the_way_the_reference_cli_reports_it() {
        assert_eq!(
            go("func main() { var = 1; }"),
            "Syntax Error: Error at '=': Expect variable name. [line 1]"
        );
    }
}
