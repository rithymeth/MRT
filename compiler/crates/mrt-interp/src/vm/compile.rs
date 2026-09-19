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

use crate::value::Value;
use crate::vm::capture::captured_names;
use crate::vm::chunk::{Chunk, Op, Proto};

/// A construct the compiler does not handle yet.
pub struct Unsupported(pub String);

impl Unsupported {
    fn of(what: &str) -> Unsupported {
        Unsupported(format!("{what} is not compiled by the VM yet."))
    }
}

type Emit<T> = Result<T, Unsupported>;

pub fn compile_program(statements: &[Stmt]) -> Emit<Chunk> {
    let mut compiler = Compiler {
        chunk: Chunk::new(),
        scope_depth: 0,
        locals: Vec::new(),
        max_slots: 0,
        captured: HashSet::new(),
        slots_allowed: false,
    };
    // The entry chunk runs top-level code and then returns null; `main` is
    // called by the driver, exactly as the tree-walker does it.
    compiler.block_body(statements)?;
    compiler.chunk.emit(Op::Null, 0);
    compiler.chunk.emit(Op::Return, 0);
    Ok(compiler.chunk)
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
    /// Top-level code is compiled with no slots at all -- a module's
    /// top-level `var` is a global, reachable by name from every function in
    /// the file, so putting one in a frame slot would hide it.
    slots_allowed: bool,
}

struct Local {
    name: String,
    depth: usize,
}

/// Where a `break` or `continue` jump has to be patched to once the loop's
/// extent is known.
struct Loop {
    breaks: Vec<usize>,
    continues: Vec<usize>,
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

    /// Declarations first, then everything else -- the same hoisting rule the
    /// tree-walker applies, so a `main` can call what is written below it.
    fn block_body(&mut self, statements: &[Stmt]) -> Emit<()> {
        let hoisted = |s: &Stmt| matches!(s.kind, StmtKind::Function { .. });
        for stmt in statements.iter().filter(|s| hoisted(s)) {
            self.statement(stmt, None)?;
        }
        for stmt in statements.iter().filter(|s| !hoisted(s)) {
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
                for arg in args {
                    self.no_spread(arg)?;
                    self.expression(arg)?;
                }
                self.chunk.emit(Op::Print(args.len() as u32), line);
            }
            StmtKind::Var {
                pattern,
                initializer,
            } => {
                // `Pattern::Name`'s own `default` is a *destructuring*
                // default, not this declaration's initializer -- reading the
                // value off the pattern silently declares every variable null.
                let Pattern::Name { name, .. } = pattern else {
                    return Err(Unsupported::of("destructuring in a declaration"));
                };
                match initializer {
                    Some(expr) => self.expression(expr)?,
                    None => {
                        self.chunk.emit(Op::Null, line);
                    }
                }
                self.emit_define(&name.text, line);
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
                let Pattern::Name { name, .. } = pattern else {
                    return Err(Unsupported::of("destructuring in a for-in loop"));
                };
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
                self.emit_define(&name.text, line);
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
                if *is_generator {
                    return Err(Unsupported::of("generators"));
                }
                self.function(Some(name.text.clone()), params, body, line)?;
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
                    let depth = loops.depth;
                    self.unwind_to(depth, line);
                    let site = self.chunk.emit(Op::Jump(0), line);
                    loops.breaks.push(site);
                }
                None => return Err(Unsupported::of("'break' outside a loop")),
            },
            StmtKind::Continue => match in_loop {
                Some(loops) => {
                    let depth = loops.depth;
                    self.unwind_to(depth, line);
                    let site = self.chunk.emit(Op::Jump(0), line);
                    loops.continues.push(site);
                }
                None => return Err(Unsupported::of("'continue' outside a loop")),
            },
            StmtKind::Try { .. } => return Err(Unsupported::of("try/catch")),
            StmtKind::Throw(_) => return Err(Unsupported::of("throw")),
            StmtKind::Match { .. } => return Err(Unsupported::of("match")),
            StmtKind::Struct { .. } => return Err(Unsupported::of("structs")),
            StmtKind::Yield { .. } => return Err(Unsupported::of("generators")),
            StmtKind::DestructureAssign { .. } => {
                return Err(Unsupported::of("destructuring assignment"))
            }
            StmtKind::Import { .. } | StmtKind::Export { .. } | StmtKind::ExportNames { .. } => {
                return Err(Unsupported::of("modules"))
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
        });
        self.max_slots = self.max_slots.max(self.locals.len());
        slot
    }

    /// The slot `name` resolves to, innermost first so an inner declaration
    /// shadows an outer one.
    fn resolve_local(&self, name: &str) -> Option<u32> {
        self.locals
            .iter()
            .rposition(|local| local.name == name)
            .map(|i| i as u32)
    }

    /// Emit a read of `name`, through a slot where possible.
    fn emit_get(&mut self, name: &str, line: u32) {
        match self.resolve_local(name) {
            Some(slot) => self.chunk.emit(Op::GetLocal(slot), line),
            None => {
                let index = self.chunk.name(name);
                self.chunk.emit(Op::GetVar(index), line)
            }
        };
    }

    fn emit_set(&mut self, name: &str, line: u32) {
        match self.resolve_local(name) {
            Some(slot) => self.chunk.emit(Op::SetLocal(slot), line),
            None => {
                let index = self.chunk.name(name);
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
            ExprKind::Variable(name) => self.emit_get(&name.text, line),
            ExprKind::Assign { name, value } => {
                self.expression(value)?;
                self.emit_set(&name.text, line);
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
                for arg in args {
                    self.no_spread(arg)?;
                }
                self.expression(callee)?;
                for arg in args {
                    self.expression(arg)?;
                }
                self.chunk.emit(Op::Call(args.len() as u32), line);
            }
            ExprKind::Array(items) => {
                for item in items {
                    self.no_spread(item)?;
                    self.expression(item)?;
                }
                self.chunk.emit(Op::Array(items.len() as u32), line);
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
                if *is_generator {
                    return Err(Unsupported::of("generators"));
                }
                self.function(name.as_ref().map(|n| n.text.clone()), params, body, line)?;
            }
            ExprKind::Spread(_) => return Err(Unsupported::of("spread")),
            ExprKind::Match { .. } => return Err(Unsupported::of("match")),
            ExprKind::Yield(_) => return Err(Unsupported::of("generators")),
        }
        Ok(())
    }

    fn function(
        &mut self,
        name: Option<String>,
        params: &[Param],
        body: &[Stmt],
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
        };
        if simple_params {
            for param in params {
                if let Pattern::Name { name, .. } = &param.pattern {
                    inner.declare_local(&name.text);
                }
            }
        }
        inner.block_body(body)?;
        // Falling off the end of a body returns null.
        inner.chunk.emit(Op::Null, line);
        inner.chunk.emit(Op::Return, line);
        let proto = Rc::new(Proto {
            name,
            params: params.to_vec(),
            chunk: inner.chunk,
            slots: inner.max_slots,
            simple_params,
        });
        let index = self.chunk.proto(proto);
        self.chunk.emit(Op::Closure(index), line);
        Ok(())
    }

    /// Spread arguments are evaluated by a path this compiler does not have
    /// yet; refusing one is better than silently passing an array.
    fn no_spread(&self, expr: &Expr) -> Emit<()> {
        match expr.kind {
            ExprKind::Spread(_) => Err(Unsupported::of("spread")),
            _ => Ok(()),
        }
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
            | Op::IterNext(slot) => *slot = target,
            other => unreachable!("tried to patch {other:?}"),
        }
    }

    fn new_loop(&self) -> Loop {
        Loop {
            breaks: Vec::new(),
            continues: Vec::new(),
            depth: self.scope_depth,
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
