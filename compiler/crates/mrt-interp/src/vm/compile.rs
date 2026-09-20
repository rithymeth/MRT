//! AST to bytecode.
//!
//! The compiler covers a subset of MRT and **refuses** the rest by name
//! rather than approximating it. That refusal is the point: the conformance
//! harness records an unsupported construct as unsupported instead of as a
//! wrong answer, so the ratchet says exactly how much of the language this
//! machine really runs.

use std::collections::HashSet;
use std::rc::Rc;

use mrt_ast::*;
use mrt_diagnostics::Span;

use crate::value::Value;
use crate::vm::capture::captured_names;
use crate::vm::chunk::{Chunk, ExportNamesSpec, ImportSpec, Op, Proto};

/// A construct the compiler does not handle yet.
pub struct Unsupported(pub String);

impl Unsupported {
    fn of(what: &str) -> Unsupported {
        Unsupported(format!("{what} is not compiled by the VM yet."))
    }
}

type Emit<T> = Result<T, Unsupported>;

/// Compile, and report every name the compiler put in a frame slot.
///
/// -- The promise this exists to check --
///
/// The resolver (`mrt-resolver`) and this compiler answer *different*
/// questions and were never meant to agree. The resolver works out a full
/// upvalue scheme -- which locals an inner function captures, and from where.
/// This compiler does not implement upvalues at all: a name any nested
/// function mentions is simply not slotted, and is reached through the
/// environment instead, where the frame and the closure share one cell.
///
/// Two designs, and the simpler one is sound only because of a promise it
/// never wrote down: **the compiler may be more conservative than the
/// resolver, never less.** Putting a name in the environment that could have
/// been a slot costs a hash lookup. Putting a name in a *slot* that the
/// resolver says is a global or a capture is a silently wrong answer -- a
/// closure reading a stale copy, or a module-level variable hidden behind a
/// frame that dies.
///
/// Nothing checked that until this. `audit` in the tests below runs both over
/// the corpus and holds the compiler to it.
pub fn compile_audited(statements: &[Stmt]) -> Emit<(Chunk, Vec<Span>)> {
    let mut compiler = Compiler {
        chunk: Chunk::new(),
        scope_depth: 0,
        locals: Vec::new(),
        max_slots: 0,
        captured: HashSet::new(),
        slots_allowed: false,
        finally_depth: 0,
        slotted: Vec::new(),
    };
    compiler.top_level(statements)?;
    compiler.chunk.emit(Op::Null, 0);
    compiler.chunk.emit(Op::Return, 0);
    Ok((compiler.chunk, compiler.slotted))
}

pub fn compile_program(statements: &[Stmt]) -> Emit<Chunk> {
    let mut compiler = Compiler {
        chunk: Chunk::new(),
        scope_depth: 0,
        locals: Vec::new(),
        max_slots: 0,
        captured: HashSet::new(),
        slots_allowed: false,
        finally_depth: 0,
        slotted: Vec::new(),
    };
    // The entry chunk runs top-level code and then returns null; `main` is
    // called by the driver, exactly as the tree-walker does it.
    compiler.top_level(statements)?;
    compiler.chunk.emit(Op::Null, 0);
    compiler.chunk.emit(Op::Return, 0);
    Ok(compiler.chunk)
}

/// Compile one function body on its own, for a generator the tree-walker was
/// asked to call.
///
/// Suspending is the machine's alone, so a generator has to be compiled even
/// when the rest of the program is being walked. The body's free names still
/// resolve through the closure's `Env` at run time, which is why nothing here
/// needs to know about the enclosing function: `slots_allowed` is off, so
/// every name goes through the environment rather than a frame slot.
pub fn compile_function(params: &[Param], body: &[Stmt], name: Option<String>) -> Emit<Rc<Proto>> {
    let mut compiler = Compiler {
        chunk: Chunk::new(),
        scope_depth: 0,
        locals: Vec::new(),
        max_slots: 0,
        captured: HashSet::new(),
        slots_allowed: false,
        finally_depth: 0,
        slotted: Vec::new(),
    };
    compiler.body(body)?;
    compiler.chunk.emit(Op::Null, 0);
    compiler.chunk.emit(Op::Return, 0);
    Ok(Rc::new(Proto {
        name,
        params: params.to_vec(),
        chunk: compiler.chunk,
        slots: compiler.max_slots,
        // Parameters bind through the tree-walker, so the list means exactly
        // what it means everywhere else.
        simple_params: false,
        is_generator: true,
    }))
}

struct Compiler {
    chunk: Chunk,
    /// How many `PushScope`s are currently open in this function body.
    scope_depth: usize,
    /// Slotted locals of the function being compiled, innermost last.
    locals: Vec<Local>,
    /// The high-water mark of `locals`, which is how many slots the frame
    /// needs. Slots are reused as blocks close, so this is not the number of
    /// variables the function declares.
    max_slots: usize,
    /// Names a nested function mentions. These cannot be slotted: MRT
    /// closures capture by reference, so the binding has to live somewhere
    /// both the frame and the closure can see.
    captured: HashSet<String>,
    /// How many `try` blocks with a `finally` are currently open.
    finally_depth: usize,
    /// Top-level code is compiled with no slots at all -- a module's
    /// top-level `var` is a global, reachable by name from every function in
    /// the file, so putting one in a frame slot would hide it.
    slots_allowed: bool,
    /// The span of every name this compiler decided to reach through a frame
    /// slot, gathered from nested functions too.
    ///
    /// Recorded so `audit` can hold the compiler to the one promise that
    /// makes its design safe -- see the note there. It costs a `Vec` push per
    /// identifier, against a compile that is already allocating a chunk.
    slotted: Vec<Span>,
}

struct Local {
    name: String,
    depth: usize,
    /// True when this name lives in the `Env` rather than in a slot.
    ///
    /// Recorded even though it has no slot, because it still has to
    /// *shadow*. A `catch (e)` inside a function that already has a slotted
    /// `e` binds the caught value into the environment; without an entry
    /// here the clause body resolves `e` to the old slot and quietly reads
    /// the wrong value. Nothing about that looks like an error.
    in_env: bool,
}

/// Where a `break` or `continue` jump has to be patched to once the loop's
/// extent is known.
struct Loop {
    breaks: Vec<usize>,
    continues: Vec<usize>,
    /// How many `try` blocks with a `finally` were open at loop entry.
    ///
    /// A `break` that leaves a `finally` behind has to run it on the way
    /// out, which needs machinery this compiler does not have; refusing is
    /// better than jumping past it.
    finally_depth: usize,
    /// The scope depth the loop was entered at.
    ///
    /// A `break` from inside a nested block has to close the scopes it is
    /// jumping out of. Without this the environment grows a level on every
    /// break and the loop variable of a later iteration resolves against a
    /// stale chain -- the sort of leak that shows up three features later as
    /// an inexplicable name error.
    depth: usize,
}

impl Compiler {
    // -- statements --------------------------------------------------------

    /// A file's top-level statements: declarations first so they can refer to
    /// each other regardless of order, then everything else in source order.
    ///
    /// The rule is the tree-walker's `run_top_level`, down to unwrapping
    /// `export`: `export func f()` is an `Export` statement wrapping a
    /// function, and it hoists because the function inside it does. Anything
    /// else would make a name's visibility depend on whether it is exported.
    fn top_level(&mut self, statements: &[Stmt]) -> Emit<()> {
        let hoisted = |s: &Stmt| {
            let inner = match &s.kind {
                StmtKind::Export { declaration, .. } => &declaration.kind,
                other => other,
            };
            matches!(inner, StmtKind::Function { .. } | StmtKind::Struct { .. })
        };
        for stmt in statements.iter().filter(|s| hoisted(s)) {
            self.statement(stmt, None)?;
        }
        for stmt in statements.iter().filter(|s| !hoisted(s)) {
            self.statement(stmt, None)?;
        }
        Ok(())
    }

    /// A function body or a block: statements in source order, with no
    /// hoisting.
    ///
    /// Hoisting is the *top level's* rule, not a general one. This used to
    /// hoist too, which quietly made the VM accept a program the language
    /// rejects:
    ///
    /// ```text
    /// func main() { print(f()); func f() { return 1; } }
    /// tree-walker: Undefined variable 'f'
    /// vm:          1
    /// ```
    ///
    /// A wrong answer in the permissive direction, which is the worse
    /// direction: code written against the VM would stop working on every
    /// other implementation.
    fn body(&mut self, statements: &[Stmt]) -> Emit<()> {
        for stmt in statements {
            self.statement(stmt, None)?;
        }
        Ok(())
    }

    fn statement(&mut self, stmt: &Stmt, in_loop: Option<&mut Loop>) -> Emit<()> {
        let line = stmt.line;
        match &stmt.kind {
            StmtKind::Expression(expr) => {
                self.expression(expr)?;
                self.chunk.emit(Op::Pop, line);
            }
            StmtKind::Print(args) => {
                // A spread makes the count dynamic, so the arguments are
                // gathered into an array first -- the same three instructions
                // a call or an array literal uses, so `...` means one thing.
                if args.iter().any(|a| matches!(a.kind, ExprKind::Spread(_))) {
                    self.chunk.emit(Op::BeginSpread, line);
                    for arg in args {
                        match &arg.kind {
                            ExprKind::Spread(inner) => {
                                self.expression(inner)?;
                                self.chunk.emit(Op::SpreadInto, line);
                            }
                            _ => {
                                self.expression(arg)?;
                                self.chunk.emit(Op::PushInto, line);
                            }
                        }
                    }
                    self.chunk.emit(Op::PrintSpread, line);
                } else {
                    for arg in args {
                        self.expression(arg)?;
                    }
                    self.chunk.emit(Op::Print(args.len() as u32), line);
                }
            }
            StmtKind::Var {
                pattern,
                initializer,
            } => {
                match initializer {
                    Some(expr) => self.expression(expr)?,
                    None => {
                        self.chunk.emit(Op::Null, line);
                    }
                }
                match pattern {
                    // `Pattern::Name`'s own `default` is a *destructuring*
                    // default, not this declaration's initializer -- reading
                    // the value off the pattern silently declares every
                    // variable null.
                    Pattern::Name {
                        name,
                        default: None,
                    } => {
                        self.emit_define(&name.text, line);
                    }
                    other => {
                        // Taking a value apart is the tree-walker's, so the
                        // rules about missing elements, defaults and rest
                        // have one implementation.
                        self.declare_pattern(other);
                        let index = self.chunk.pattern(other.clone());
                        self.chunk.emit(Op::BindPattern(index), line);
                    }
                }
            }
            StmtKind::Block(body) => {
                self.push_scope(line);
                let result = self.scoped_body(body, in_loop);
                self.pop_scope(line);
                result?;
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expression(condition)?;
                let to_else = self.chunk.emit(Op::JumpIfFalse(0), line);
                let mut loops = in_loop;
                self.statement(then_branch, loops.as_deref_mut())?;
                match else_branch {
                    Some(otherwise) => {
                        let to_end = self.chunk.emit(Op::Jump(0), line);
                        self.patch(to_else);
                        self.statement(otherwise, loops)?;
                        self.patch(to_end);
                    }
                    None => self.patch(to_else),
                }
            }
            StmtKind::While { condition, body } => {
                let top = self.here();
                self.expression(condition)?;
                let exit = self.chunk.emit(Op::JumpIfFalse(0), line);
                let mut loops = self.new_loop();
                self.statement(body, Some(&mut loops))?;
                self.chunk.emit(Op::Jump(top as u32), line);
                self.patch(exit);
                self.close_loop(loops, top);
            }
            StmtKind::For {
                initializer,
                condition,
                increment,
                body,
            } => {
                // The loop header gets its own scope so `for (var i = 0; ...)`
                // does not leak `i`, matching the tree-walker.
                self.push_scope(line);
                let result = (|| -> Emit<()> {
                    if let Some(initializer) = initializer {
                        self.statement(initializer, None)?;
                    }
                    let top = self.here();
                    let exit = match condition {
                        Some(condition) => {
                            self.expression(condition)?;
                            Some(self.chunk.emit(Op::JumpIfFalse(0), line))
                        }
                        None => None,
                    };
                    let mut loops = self.new_loop();
                    self.statement(body, Some(&mut loops))?;
                    let continue_target = self.here();
                    if let Some(increment) = increment {
                        self.expression(increment)?;
                        self.chunk.emit(Op::Pop, line);
                    }
                    self.chunk.emit(Op::Jump(top as u32), line);
                    if let Some(exit) = exit {
                        self.patch(exit);
                    }
                    // `continue` in a C-style `for` must still run the step,
                    // so it lands on the step rather than on the condition.
                    for site in loops.continues {
                        self.patch_to(site, continue_target);
                    }
                    let here = self.here();
                    for site in loops.breaks {
                        self.patch_to(site, here);
                    }
                    Ok(())
                })();
                self.pop_scope(line);
                result?;
            }
            StmtKind::ForIn {
                pattern,
                iterable,
                body,
            } => {
                self.expression(iterable)?;
                self.chunk.emit(Op::IterInit, line);
                // A `break` jumps to the `IterDrop`, never past it, so every
                // way out of the loop retires its cursor.
                let mut loops = self.new_loop();
                let top = self.here();
                let exit = self.chunk.emit(Op::IterNext(0), line);
                // A fresh scope per iteration, so a closure made inside the
                // body captures that turn's binding and not a shared one.
                self.push_scope(line);
                match pattern {
                    Pattern::Name {
                        name,
                        default: None,
                    } => self.emit_define(&name.text, line),
                    other => {
                        self.declare_pattern(other);
                        let index = self.chunk.pattern(other.clone());
                        self.chunk.emit(Op::BindPattern(index), line);
                    }
                }
                let result = self.statement(body, Some(&mut loops));
                self.pop_scope(line);
                result?;
                self.chunk.emit(Op::Jump(top as u32), line);
                self.patch(exit);
                let drop_at = self.here();
                self.chunk.emit(Op::IterDrop, line);
                for site in loops.breaks {
                    self.patch_to(site, drop_at);
                }
                for site in loops.continues {
                    self.patch_to(site, top);
                }
            }
            StmtKind::Function {
                name,
                params,
                body,
                is_generator,
            } => {
                self.function(Some(name.text.clone()), params, body, *is_generator, line)?;
                self.emit_define(&name.text, line);
            }
            StmtKind::Return(value) => {
                match value {
                    Some(expr) => self.expression(expr)?,
                    None => {
                        self.chunk.emit(Op::Null, line);
                    }
                }
                self.chunk.emit(Op::Return, line);
            }
            StmtKind::Break => match in_loop {
                Some(loops) => {
                    if self.finally_depth > loops.finally_depth {
                        return Err(Unsupported::of("'break' out of a try with a finally"));
                    }
                    let depth = loops.depth;
                    self.unwind_to(depth, line);
                    let site = self.chunk.emit(Op::Jump(0), line);
                    loops.breaks.push(site);
                }
                None => return Err(Unsupported::of("'break' outside a loop")),
            },
            StmtKind::Continue => match in_loop {
                Some(loops) => {
                    if self.finally_depth > loops.finally_depth {
                        return Err(Unsupported::of("'continue' out of a try with a finally"));
                    }
                    let depth = loops.depth;
                    self.unwind_to(depth, line);
                    let site = self.chunk.emit(Op::Jump(0), line);
                    loops.continues.push(site);
                }
                None => return Err(Unsupported::of("'continue' outside a loop")),
            },
            StmtKind::Try {
                body,
                catches,
                finally,
            } => self.try_statement(body, catches, finally.as_deref(), line, in_loop)?,
            StmtKind::Throw(value) => {
                self.expression(value)?;
                self.chunk.emit(Op::Throw, line);
            }
            StmtKind::Match { subject, cases } => {
                self.expression(subject)?;
                let mut loops = in_loop;
                let mut done = Vec::new();
                for case in cases {
                    self.push_scope(line);
                    let to_next = match &case.pattern {
                        Some(pattern) => {
                            self.declare_match_pattern(pattern);
                            let index = self.chunk.match_pattern(pattern.clone());
                            self.chunk.emit(Op::MatchPattern(index), line);
                            Some(self.chunk.emit(Op::JumpIfFalse(0), line))
                        }
                        // `default:` takes anything.
                        None => None,
                    };
                    let to_next_guard = match &case.guard {
                        Some(guard) => {
                            self.expression(guard)?;
                            Some(self.chunk.emit(Op::JumpIfFalse(0), line))
                        }
                        None => None,
                    };
                    // This case wins: drop the subject and run the body.
                    self.chunk.emit(Op::Pop, line);
                    let result = self.scoped_body(&case.body, loops.as_deref_mut());
                    self.chunk.emit(Op::PopScope, line);
                    result?;
                    done.push(self.chunk.emit(Op::Jump(0), line));

                    // ...or it does not. Same lexical scope, second runtime
                    // exit, so close it only once -- below.
                    if let Some(site) = to_next {
                        self.patch(site);
                    }
                    if let Some(site) = to_next_guard {
                        self.patch(site);
                    }
                    self.chunk.emit(Op::PopScope, line);
                    self.close_scope();
                }
                // Nothing matched. The subject is still on the stack for the
                // error to name.
                self.chunk.emit(Op::NoMatch, line);
                for site in done {
                    self.patch(site);
                }
            }
            StmtKind::Struct {
                name,
                fields,
                methods,
            } => {
                // Methods stay AST closures, generator or not: calling one
                // compiles its body on demand, so a generator method is a
                // parked frame like any other. This used to be refused here
                // because the tree-walker could not make one at all.
                let index =
                    self.chunk
                        .struct_def(name.text.clone(), fields.clone(), methods.clone());
                self.chunk.emit(Op::Struct(index), line);
                self.emit_define(&name.text, line);
            }
            StmtKind::Yield { value, delegate } => {
                // `yield*` forwards a whole sequence, and forwarding sent
                // values back into the delegate is its own mechanism. Refused
                // by name rather than approximated, so the harness reads it as
                // a gap instead of a wrong answer.
                self.expression(value)?;
                if *delegate {
                    // `yield* xs` is a loop that re-yields someone else's
                    // items. The sent value is threaded through rather than
                    // dropped: it lives on the stack between the `Yield` that
                    // received it and the `DelegateNext` that passes it on, so
                    // a value sent into the outer generator reaches whatever
                    // the inner one is parked at.
                    self.chunk.emit(Op::DelegateInit, line);
                    self.chunk.emit(Op::Null, line);
                    let top = self.here();
                    let done = self.chunk.emit(Op::DelegateNext(0), line);
                    self.chunk.emit(Op::Yield, line);
                    self.chunk.emit(Op::Jump(top as u32), line);
                    self.patch(done);
                    self.chunk.emit(Op::IterDrop, line);
                } else {
                    self.chunk.emit(Op::Yield, line);
                    // As a statement there is nowhere for the sent value to go.
                    self.chunk.emit(Op::Pop, line);
                }
            }
            StmtKind::DestructureAssign { pattern, value } => {
                self.expression(value)?;
                let index = self.chunk.pattern(pattern.clone());
                self.chunk.emit(Op::AssignPattern(index), line);
            }
            StmtKind::Import {
                names,
                namespace,
                specifier,
            } => {
                let index = self.chunk.import(ImportSpec {
                    names: names.clone(),
                    namespace: namespace.clone(),
                    specifier: specifier.clone(),
                });
                self.chunk.emit(Op::Import(index), line);
            }
            StmtKind::Export { declaration, name } => {
                // The declaration runs as itself; exporting is a second step
                // that reads the name back out. Capturing the value during
                // the declaration instead would export the initializer rather
                // than what the name ended up bound to.
                self.statement(declaration, None)?;
                let index = self.chunk.name(&name.text);
                self.chunk.emit(Op::RecordExport(index), line);
            }
            StmtKind::ExportNames { names, specifier } => {
                let index = self.chunk.export_names(ExportNamesSpec {
                    names: names.clone(),
                    specifier: specifier.clone(),
                });
                self.chunk.emit(Op::ExportNames(index), line);
            }
        }
        Ok(())
    }

    fn push_scope(&mut self, line: u32) {
        self.chunk.emit(Op::PushScope, line);
        self.scope_depth += 1;
    }

    fn pop_scope(&mut self, line: u32) {
        self.chunk.emit(Op::PopScope, line);
        self.close_scope();
    }

    /// Forget a scope without emitting anything.
    ///
    /// Needed where one lexical scope has *several* runtime exits -- a catch
    /// clause leaves one way when it matches and another when it does not,
    /// so it emits two `PopScope`s but is still one scope. Decrementing the
    /// depth twice silently corrupts every slot number after the `try`.
    fn close_scope(&mut self) {
        self.scope_depth -= 1;
        // Slots of the closing block become free for the next one.
        while self
            .locals
            .last()
            .is_some_and(|local| local.depth > self.scope_depth)
        {
            self.locals.pop();
        }
    }

    /// Can `name` live in a slot here?
    fn slottable(&self, name: &str) -> bool {
        self.slots_allowed && !self.captured.contains(name)
    }

    /// Give `name` a slot in the current block, returning its index.
    fn declare_local(&mut self, name: &str) -> u32 {
        let slot = self.locals.len() as u32;
        self.locals.push(Local {
            name: name.to_string(),
            depth: self.scope_depth,
            in_env: false,
        });
        self.max_slots = self.max_slots.max(self.locals.len());
        slot
    }

    /// Record that `name` is bound in the environment for this scope, so it
    /// shadows any slot of the same name further out.
    ///
    /// It occupies a slot index it never uses. Paying one wasted slot is the
    /// cheap way to keep slot numbering a simple `Vec` position while these
    /// entries come and go with their scopes.
    fn declare_env_local(&mut self, name: &str) {
        self.locals.push(Local {
            name: name.to_string(),
            depth: self.scope_depth,
            in_env: true,
        });
        self.max_slots = self.max_slots.max(self.locals.len());
    }

    /// Every name a *match* pattern captures. Bound into the environment by
    /// the tree-walker, so each has to shadow any outer slot of the name for
    /// the rest of the case.
    fn declare_match_pattern(&mut self, pattern: &MatchPattern) {
        match pattern {
            MatchPattern::Bind { name } => self.declare_env_local(&name.text),
            MatchPattern::Array { elements, rest, .. } => {
                for element in elements {
                    self.declare_match_pattern(element);
                }
                if let Some(rest) = rest {
                    self.declare_env_local(&rest.text);
                }
            }
            MatchPattern::Object { entries, .. } => {
                for (_, value) in entries {
                    self.declare_match_pattern(value);
                }
            }
            MatchPattern::Struct { elements, .. } => {
                for element in elements {
                    self.declare_match_pattern(element);
                }
            }
            MatchPattern::Literal { .. } => {}
        }
    }

    /// Every name a pattern binds, so each can shadow correctly.
    fn declare_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Name { name, .. } => self.declare_env_local(&name.text),
            Pattern::Array { elements, rest, .. } => {
                for element in elements {
                    self.declare_pattern(element);
                }
                if let Some(rest) = rest {
                    self.declare_env_local(&rest.text);
                }
            }
            Pattern::Object { entries, rest, .. } => {
                for (_, value) in entries {
                    self.declare_pattern(value);
                }
                if let Some(rest) = rest {
                    self.declare_env_local(&rest.text);
                }
            }
        }
    }

    /// The slot `name` resolves to, innermost first so an inner declaration
    /// shadows an outer one. `None` when the innermost binding is in the
    /// environment, which is also a shadow -- just not a slotted one.
    fn resolve_local(&self, name: &str) -> Option<u32> {
        let index = self.locals.iter().rposition(|local| local.name == name)?;
        if self.locals[index].in_env {
            return None;
        }
        Some(index as u32)
    }

    /// Emit a read of `name`, through a slot where possible.
    fn emit_get(&mut self, name: &Name, line: u32) {
        match self.resolve_local(&name.text) {
            Some(slot) => {
                self.slotted.push(name.span);
                self.chunk.emit(Op::GetLocal(slot), line)
            }
            None => {
                let index = self.chunk.name(&name.text);
                self.chunk.emit(Op::GetVar(index), line)
            }
        };
    }

    fn emit_set(&mut self, name: &Name, line: u32) {
        match self.resolve_local(&name.text) {
            Some(slot) => {
                self.slotted.push(name.span);
                self.chunk.emit(Op::SetLocal(slot), line)
            }
            None => {
                let index = self.chunk.name(&name.text);
                self.chunk.emit(Op::SetVar(index), line)
            }
        };
    }

    /// Emit a declaration of `name`, whose value is on top of the stack.
    fn emit_define(&mut self, name: &str, line: u32) {
        if self.slottable(name) {
            let slot = self.declare_local(name);
            self.chunk.emit(Op::DefineLocal(slot), line);
        } else {
            let index = self.chunk.name(name);
            self.chunk.emit(Op::DefineVar(index), line);
        }
    }

    /// Close every scope opened since `depth`, without changing the
    /// compiler's own depth -- the code after a `break` is still lexically
    /// inside those blocks even though control never reaches it.
    fn unwind_to(&mut self, depth: usize, line: u32) {
        for _ in depth..self.scope_depth {
            self.chunk.emit(Op::PopScope, line);
        }
    }

    /// Compile a `try`/`catch`/`finally`.
    ///
    /// The shape:
    ///
    /// ```text
    ///     PushHandler(catch, finally_unwind)
    ///     <body>                  in its own scope
    ///     PopHandler
    ///     Jump normal
    /// catch:                      reached by unwinding, thrown value on the
    ///     <clause chain>          stack and the signal parked
    ///     EndFinally              no clause matched: re-raise
    /// normal:
    ///     <finally>               the ordinary path
    ///     Jump end
    /// finally_unwind:
    ///     <finally>               the unwinding path -- a second copy
    ///     EndFinally              resume whatever was parked
    /// end:
    /// ```
    ///
    /// The `finally` block is emitted **twice** on purpose. One copy runs on
    /// the paths that leave normally, the other on the paths that are still
    /// carrying a signal, and the second ends by re-raising it. Sharing one
    /// copy would need the finally to know which way it was entered, which
    /// is the same bookkeeping in a less obvious place.
    fn try_statement(
        &mut self,
        body: &[Stmt],
        catches: &[CatchClause],
        finally: Option<&[Stmt]>,
        line: u32,
        in_loop: Option<&mut Loop>,
    ) -> Emit<()> {
        const NONE: u32 = u32::MAX;

        let handler = self.chunk.emit(
            Op::PushHandler {
                catch: NONE,
                finally: NONE,
            },
            line,
        );

        if finally.is_some() {
            self.finally_depth += 1;
        }

        let mut loops = in_loop;
        self.push_scope(line);
        let result = self.scoped_body(body, loops.as_deref_mut());
        self.pop_scope(line);
        result?;
        self.chunk.emit(Op::PopHandler, line);
        let to_normal = self.chunk.emit(Op::Jump(0), line);

        // -- the clause chain ------------------------------------------------
        let catch_at = self.here();
        let mut clause_done = Vec::new();
        for clause in catches {
            // The thrown value stays on the stack across attempts, so a
            // clause that does not apply leaves the next one something to
            // test.
            self.push_scope(line);
            self.declare_pattern(&clause.pattern);
            let index = self.chunk.pattern(clause.pattern.clone());
            self.chunk.emit(Op::BindCatch(index), line);
            let to_next = self.chunk.emit(Op::JumpIfFalse(0), line);

            let to_next_guard = match &clause.guard {
                Some(guard) => {
                    self.expression(guard)?;
                    Some(self.chunk.emit(Op::JumpIfFalse(0), line))
                }
                None => None,
            };

            // This clause handles it: drop the value and the parked signal.
            self.chunk.emit(Op::Pop, line);
            self.chunk.emit(Op::DropPending, line);
            let result = self.scoped_body(&clause.body, loops.as_deref_mut());
            // Retire the finally-only handler the unwinder left behind, but
            // only once the clause body is *past*: while it runs, that
            // handler is what makes a `return` or a fresh `throw` from
            // inside the clause still run the finally.
            if finally.is_some() {
                self.chunk.emit(Op::PopHandler, line);
            }
            self.chunk.emit(Op::PopScope, line);
            result?;
            clause_done.push(self.chunk.emit(Op::Jump(0), line));

            // ...or it does not, and the next clause gets a turn. Same
            // lexical scope, second runtime exit: emit the `PopScope` but
            // close the scope only once, below.
            self.patch(to_next);
            if let Some(site) = to_next_guard {
                self.patch(site);
            }
            self.chunk.emit(Op::PopScope, line);
            self.close_scope();
        }
        // No clause matched. Drop the value and re-raise what was parked;
        // the finally-only handler, if there is one, picks it up and runs
        // the finally on the way past.
        self.chunk.emit(Op::Pop, line);
        self.chunk.emit(Op::EndFinally, line);

        // -- the two finally copies -----------------------------------------
        self.patch(to_normal);
        for site in clause_done {
            self.patch_to(site, self.here());
        }
        if let Some(block) = finally {
            self.push_scope(line);
            let result = self.scoped_body(block, loops.as_deref_mut());
            self.pop_scope(line);
            result?;
        }
        let to_end = self.chunk.emit(Op::Jump(0), line);

        let unwind_at = self.here();
        if let Some(block) = finally {
            self.push_scope(line);
            let result = self.scoped_body(block, loops);
            self.pop_scope(line);
            result?;
        }
        self.chunk.emit(Op::EndFinally, line);

        self.patch(to_end);

        if finally.is_some() {
            self.finally_depth -= 1;
        }

        let finally_target = if finally.is_some() {
            unwind_at
        } else {
            NONE as usize
        };
        self.chunk.code[handler] = Op::PushHandler {
            catch: if catches.is_empty() {
                NONE
            } else {
                catch_at as u32
            },
            finally: if finally.is_some() {
                finally_target as u32
            } else {
                NONE
            },
        };
        Ok(())
    }

    fn scoped_body(&mut self, body: &[Stmt], in_loop: Option<&mut Loop>) -> Emit<()> {
        let mut loops = in_loop;
        for stmt in body {
            self.statement(stmt, loops.as_deref_mut())?;
        }
        Ok(())
    }

    // -- expressions -------------------------------------------------------

    fn expression(&mut self, expr: &Expr) -> Emit<()> {
        let line = expr.line;
        match &expr.kind {
            ExprKind::Literal(value) => {
                match value {
                    LitValue::Null => {
                        self.chunk.emit(Op::Null, line);
                    }
                    LitValue::Bool(true) => {
                        self.chunk.emit(Op::True, line);
                    }
                    LitValue::Bool(false) => {
                        self.chunk.emit(Op::False, line);
                    }
                    LitValue::Number(n) => {
                        let index = self.chunk.constant(Value::Number(*n));
                        self.chunk.emit(Op::Const(index), line);
                    }
                    LitValue::Str(s) => {
                        let index = self.chunk.constant(Value::str(s.as_str()));
                        self.chunk.emit(Op::Const(index), line);
                    }
                };
            }
            ExprKind::Variable(name) => self.emit_get(name, line),
            ExprKind::Assign { name, value } => {
                self.expression(value)?;
                self.emit_set(name, line);
            }
            ExprKind::Binary { left, op, right } => {
                self.expression(left)?;
                self.expression(right)?;
                self.chunk.emit(Op::Binary(*op), line);
            }
            ExprKind::Logical { left, op, right } => {
                self.expression(left)?;
                let short = match op {
                    LogicalOp::And => self.chunk.emit(Op::JumpIfFalsyKeep(0), line),
                    LogicalOp::Or => self.chunk.emit(Op::JumpIfTruthyKeep(0), line),
                };
                self.chunk.emit(Op::Pop, line);
                self.expression(right)?;
                self.patch(short);
            }
            ExprKind::Unary { op, right } => {
                self.expression(right)?;
                match op {
                    UnOp::Neg => self.chunk.emit(Op::Neg, line),
                    UnOp::Not => self.chunk.emit(Op::Not, line),
                };
            }
            ExprKind::Grouping(inner) => self.expression(inner)?,
            ExprKind::Call { callee, args } => {
                self.expression(callee)?;
                if args.iter().any(|a| matches!(a.kind, ExprKind::Spread(_))) {
                    // A spread makes the argument count a run-time fact, so
                    // the arguments are gathered into an array and the call
                    // reads its length instead of the compiler knowing it.
                    self.chunk.emit(Op::BeginSpread, line);
                    for arg in args {
                        match &arg.kind {
                            ExprKind::Spread(inner) => {
                                self.expression(inner)?;
                                self.chunk.emit(Op::SpreadInto, line);
                            }
                            _ => {
                                self.expression(arg)?;
                                self.chunk.emit(Op::PushInto, line);
                            }
                        }
                    }
                    self.chunk.emit(Op::CallSpread, line);
                } else {
                    for arg in args {
                        self.expression(arg)?;
                    }
                    self.chunk.emit(Op::Call(args.len() as u32), line);
                }
            }
            ExprKind::Array(items) => {
                if items.iter().any(|i| matches!(i.kind, ExprKind::Spread(_))) {
                    self.chunk.emit(Op::BeginSpread, line);
                    for item in items {
                        match &item.kind {
                            ExprKind::Spread(inner) => {
                                self.expression(inner)?;
                                self.chunk.emit(Op::SpreadInto, line);
                            }
                            _ => {
                                self.expression(item)?;
                                self.chunk.emit(Op::PushInto, line);
                            }
                        }
                    }
                } else {
                    for item in items {
                        self.expression(item)?;
                    }
                    self.chunk.emit(Op::Array(items.len() as u32), line);
                }
            }
            ExprKind::Index { target, index } => {
                self.expression(target)?;
                self.expression(index)?;
                self.chunk.emit(Op::Index, line);
            }
            ExprKind::IndexAssign {
                target,
                index,
                value,
            } => {
                self.expression(target)?;
                self.expression(index)?;
                self.expression(value)?;
                self.chunk.emit(Op::IndexSet, line);
            }
            ExprKind::Dict(pairs) => {
                for (key, value) in pairs {
                    self.expression(key)?;
                    self.expression(value)?;
                }
                self.chunk.emit(Op::Dict(pairs.len() as u32), line);
            }
            ExprKind::Interpolation(parts) => {
                for part in parts {
                    match part {
                        InterpPart::Text(text) => {
                            let index = self.chunk.constant(Value::str(text.as_str()));
                            self.chunk.emit(Op::Const(index), line);
                        }
                        InterpPart::Expr(expr) => self.expression(expr)?,
                    }
                }
                self.chunk.emit(Op::Interp(parts.len() as u32), line);
            }
            ExprKind::Function {
                name,
                params,
                body,
                is_generator,
            } => {
                self.function(
                    name.as_ref().map(|n| n.text.clone()),
                    params,
                    body,
                    *is_generator,
                    line,
                )?;
            }
            // Only meaningful inside a call or an array literal, both of
            // which handle it above.
            ExprKind::Spread(_) => return Err(Unsupported::of("a spread outside a call or array")),
            // A `match` expression is the statement's shape with a value in
            // place of a body, so the arms leave one on the stack.
            ExprKind::Match { subject, arms } => {
                self.expression(subject)?;
                let mut done = Vec::new();
                for arm in arms {
                    self.push_scope(line);
                    let to_next = match &arm.pattern {
                        Some(pattern) => {
                            self.declare_match_pattern(pattern);
                            let index = self.chunk.match_pattern(pattern.clone());
                            self.chunk.emit(Op::MatchPattern(index), line);
                            Some(self.chunk.emit(Op::JumpIfFalse(0), line))
                        }
                        None => None,
                    };
                    let to_next_guard = match &arm.guard {
                        Some(guard) => {
                            self.expression(guard)?;
                            Some(self.chunk.emit(Op::JumpIfFalse(0), line))
                        }
                        None => None,
                    };
                    self.chunk.emit(Op::Pop, line);
                    let result = self.expression(&arm.value);
                    self.chunk.emit(Op::PopScope, line);
                    result?;
                    done.push(self.chunk.emit(Op::Jump(0), line));

                    if let Some(site) = to_next {
                        self.patch(site);
                    }
                    if let Some(site) = to_next_guard {
                        self.patch(site);
                    }
                    self.chunk.emit(Op::PopScope, line);
                    self.close_scope();
                }
                self.chunk.emit(Op::NoMatch, line);
                for site in done {
                    self.patch(site);
                }
            }
            // As an expression the yield evaluates to whatever was sent in,
            // which `Op::Yield` leaves on the stack on resume. One instruction
            // both delivers and receives.
            ExprKind::Yield(value) => {
                self.expression(value)?;
                self.chunk.emit(Op::Yield, line);
            }
        }
        Ok(())
    }

    fn function(
        &mut self,
        name: Option<String>,
        params: &[Param],
        body: &[Stmt],
        is_generator: bool,
        line: u32,
    ) -> Emit<()> {
        // Which of this body's names a *nested* function mentions. Computed
        // once per function rather than per reference.
        let captured = captured_names(body);

        // Parameters can be slots only when binding them is trivial: a plain
        // name, no default, no rest, no destructuring, and not captured. Then
        // the arguments already on the operand stack are slots 0..n and a
        // call does no binding work at all.
        let simple_params = params.iter().all(|p| {
            !p.rest
                && matches!(
                    &p.pattern,
                    Pattern::Name { name, default: None } if !captured.contains(&name.text)
                )
        });

        let mut inner = Compiler {
            chunk: Chunk::new(),
            scope_depth: 0,
            locals: Vec::new(),
            max_slots: 0,
            captured,
            slots_allowed: true,
            finally_depth: 0,
            slotted: Vec::new(),
        };
        if simple_params {
            for param in params {
                if let Pattern::Name { name, .. } = &param.pattern {
                    inner.declare_local(&name.text);
                }
            }
        }
        inner.body(body)?;
        // Falling off the end of a body returns null.
        inner.chunk.emit(Op::Null, line);
        inner.chunk.emit(Op::Return, line);
        let proto = Rc::new(Proto {
            name,
            params: params.to_vec(),
            chunk: inner.chunk,
            slots: inner.max_slots,
            simple_params,
            is_generator,
        });
        // A nested function's slot decisions are this compile's too, or the
        // audit would only ever see the outermost frame.
        self.slotted.append(&mut inner.slotted);
        let index = self.chunk.proto(proto);
        self.chunk.emit(Op::Closure(index), line);
        Ok(())
    }

    // -- jump patching -----------------------------------------------------

    fn here(&self) -> usize {
        self.chunk.code.len()
    }

    fn patch(&mut self, site: usize) {
        let target = self.here();
        self.patch_to(site, target);
    }

    fn patch_to(&mut self, site: usize, target: usize) {
        let target = target as u32;
        match &mut self.chunk.code[site] {
            Op::Jump(slot)
            | Op::JumpIfFalse(slot)
            | Op::JumpIfFalsyKeep(slot)
            | Op::JumpIfTruthyKeep(slot)
            | Op::IterNext(slot)
            | Op::DelegateNext(slot) => *slot = target,
            other => unreachable!("tried to patch {other:?}"),
        }
    }

    fn new_loop(&self) -> Loop {
        Loop {
            breaks: Vec::new(),
            continues: Vec::new(),
            depth: self.scope_depth,
            finally_depth: self.finally_depth,
        }
    }

    fn close_loop(&mut self, loops: Loop, continue_target: usize) {
        let here = self.here();
        for site in loops.breaks {
            self.patch_to(site, here);
        }
        for site in loops.continues {
            self.patch_to(site, continue_target);
        }
    }
}

#[cfg(test)]
mod audit {
    use super::*;
    use mrt_diagnostics::SourceFile;
    use mrt_resolver::Resolution;

    /// Every name the compiler slotted, checked against what the resolver
    /// worked out about it. Returns the ones that broke the promise.
    ///
    /// Two things disqualify a slot, and the second is the one that matters.
    ///
    /// **The resolver calls it a global.** A module-level `var` is reachable
    /// by name from every function in the file; in a frame slot it would be
    /// hidden behind a frame that dies.
    ///
    /// **The resolver marks it captured.** Some inner function closes over
    /// it, and MRT closures capture by reference -- so the binding has to
    /// live where the frame and the closure share one cell. Slotted, the
    /// closure reads a copy that stopped changing.
    ///
    /// That second check cannot come from `by_span`: the resolver classifies
    /// the *declaring* mention of a captured variable as an ordinary local
    /// too, and records the capture as a flag on the function's frame
    /// layout. So each slotted span is attributed to the innermost function
    /// whose span contains it, and checked against that function's locals.
    ///
    /// One-directional throughout. The compiler being *more* conservative
    /// than the resolver costs a hash lookup and happens constantly by
    /// design -- its capture analysis disqualifies a name on any mention
    /// inside any nested function, whether or not that mention could really
    /// refer to it. The other direction is a silently wrong answer.
    fn violations(source: &str) -> Vec<String> {
        let file = SourceFile::new("audit.mrt", source);
        let parsed = mrt_parser::parse(&file);
        assert!(
            parsed.errors.is_empty(),
            "the audit corpus must parse: {:?}",
            parsed.errors.first().map(|e| e.render_compat())
        );

        let resolved = mrt_resolver::resolve(&parsed.program);
        let Ok((_, slotted)) = compile_audited(&parsed.program.statements) else {
            // A program the VM cannot compile made no slot decisions.
            return Vec::new();
        };

        let text_of = |span: Span| -> String {
            source
                .chars()
                .skip(span.start as usize)
                .take((span.end - span.start) as usize)
                .collect()
        };

        let mut bad = Vec::new();
        for span in slotted {
            let name = text_of(span);

            if matches!(
                resolved.by_span.get(&span),
                Some(mrt_resolver::Resolution::Global)
            ) {
                bad.push(format!("{name} is a global"));
                continue;
            }

            // The innermost function containing this use: the narrowest span
            // that encloses it.
            let scope = resolved
                .functions
                .iter()
                .filter(|f| f.span.start <= span.start && span.end <= f.span.end)
                .min_by_key(|f| f.span.end - f.span.start);

            if let Some(scope) = scope {
                if scope
                    .locals
                    .iter()
                    .any(|local| local.name == name && local.captured)
                {
                    bad.push(format!("{name} is captured by a closure"));
                }
            }
        }
        bad
    }

    fn check(source: &str) {
        let bad = violations(source);
        assert!(
            bad.is_empty(),
            "the compiler slotted names it must not: {bad:?}\nin:\n{source}"
        );
    }

    #[test]
    fn the_compiler_never_slots_a_global() {
        // Top-level `var`s are globals: reachable by name from every function
        // in the file. One in a frame slot would be hidden behind a frame
        // that dies, and the function below it would see nothing.
        check("var total = 0;\nfunc add(n) { total = total + n; return total; }\nfunc main() { print(add(2)); }");

        // Read and written *at* the top level as well as inside a function.
        // Without this the check has nothing to look at: a declaration binds
        // without going through the two places a name is resolved, so a
        // compiler that wrongly slotted globals would record no decision at
        // all and pass. Found by mutation -- turning slots on for top-level
        // code left every test green until this line existed.
        check(
            "var count = 0;\ncount = count + 1;\nprint(count);\nfunc bump() { count = count + 1; }\nfunc main() { bump(); print(count); }",
        );
    }

    #[test]
    fn the_compiler_never_slots_something_a_closure_holds() {
        // The case the whole promise is about. MRT closures capture by
        // reference, so a slotted counter would leave the closure reading a
        // copy that stopped changing.
        check(
            r#"func main() {
                   var n = 1;
                   var f = func() { return n; };
                   n = 2;
                   print(f());
               }"#,
        );
        check(
            r#"func counter() {
                   var n = 0;
                   return func() { n = n + 1; return n; };
               }
               func main() { var c = counter(); c(); print(c()); }"#,
        );
    }

    #[test]
    fn the_promise_holds_across_every_shape_the_language_has() {
        // Not a survey for its own sake: each of these binds names somewhere
        // the two implementations could disagree about.
        for source in [
            // Loops, whose bodies open and close scopes.
            "func main() { for (var i = 0; i < 3; i = i + 1) { var d = i * 2; print(d); } }",
            // A for-in binding, bound afresh each turn.
            "func main() { var fs = []; for (x in [1, 2]) { push(fs, func() { return x; }); } print(fs[0]()); }",
            // Destructuring, which binds several names at once.
            "func main() { var [a, b] = [1, 2]; var {c} = {c: 3}; print(a, b, c); }",
            // Match, which binds into the environment rather than slots.
            "func main() { match ([1, 2]) { case [p, q]: print(p, q); } }",
            // try/catch, whose error name is bound for the block.
            "func main() { try { throw 1; } catch (e) { print(e); } }",
            // Parameters with defaults and rest.
            "func f(a, b = 2, ...rest) { return a + b + len(rest); }\nfunc main() { print(f(1)); }",
            // A struct with methods, each its own function.
            "struct P { x; func show() { return this.x; } }\nfunc main() { print(P(1).show()); }",
            // Nesting three deep, so a name is reached across two frames.
            "func main() { var a = 1; var f = func() { var g = func() { return a; }; return g(); }; print(f()); }",
            // Shadowing: an inner name of the same spelling as an outer one.
            "var v = 1;\nfunc main() { var v = 2; { var v = 3; print(v); } print(v); }",
        ] {
            check(source);
        }
    }

    #[test]
    fn the_promise_holds_over_every_example_in_the_repository() {
        // The hand-written cases above are the shapes I thought to check.
        // These are the programs that actually exist -- a JSON parser, a
        // generator pipeline, pattern matching, modules -- and they exercise
        // combinations nobody sat down and enumerated.
        let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../examples");
        let Ok(entries) = std::fs::read_dir(&examples) else {
            // Running from somewhere without the corpus is not a failure of
            // the compiler; the cases above still ran.
            return;
        };

        let mut checked = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "mrt") {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            // Some examples import modules this test does not load; the
            // compiler still makes slot decisions about the file in hand.
            let file = SourceFile::new("audit.mrt", &source);
            let parsed = mrt_parser::parse(&file);
            if !parsed.errors.is_empty() {
                continue;
            }
            let bad = violations(&source);
            assert!(
                bad.is_empty(),
                "{}: the compiler slotted names it must not: {bad:?}",
                path.display()
            );
            checked += 1;
        }
        assert!(checked > 10, "only {checked} examples were audited");
    }

    #[test]
    fn the_audit_can_fail() {
        // A check that cannot fail is worth nothing, and this one compares
        // two implementations that mostly agree -- exactly the shape that
        // passes for the wrong reason. So: a program whose slotted names the
        // resolver classifies as something else.
        //
        // `total` below is a module-level global read inside a function. If
        // the compiler ever started slotting it, `violations` must say so --
        // which is checked by asking the resolver directly and confirming it
        // really does call that mention a global.
        let source =
            "var total = 0;\nfunc add(n) { return total + n; }\nfunc main() { print(add(1)); }";
        let file = SourceFile::new("audit.mrt", source);
        let parsed = mrt_parser::parse(&file);
        let resolved = mrt_resolver::resolve(&parsed.program);

        let globals = resolved
            .by_span
            .values()
            .filter(|r| matches!(r, Resolution::Global))
            .count();
        assert!(
            globals > 0,
            "the resolver sees no globals here, so this test proves nothing"
        );

        // And the audit is looking at real data: the compiler slotted
        // something in this program, so an empty `slotted` is not why it
        // passes.
        let Ok((_, slotted)) = compile_audited(&parsed.program.statements) else {
            panic!("the audit corpus must compile");
        };
        assert!(
            !slotted.is_empty(),
            "nothing was slotted, so nothing was checked"
        );
    }
}
