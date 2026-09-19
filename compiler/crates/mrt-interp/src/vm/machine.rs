//! The stack machine.
//!
//! What this file owns is the *execution model*: an instruction pointer, an
//! operand stack, and a frame stack. What it deliberately does not own is the
//! language. Arithmetic, indexing, iteration, printing and the whole builtin
//! library are delegated to the `Interpreter` it borrows, so there is exactly
//! one implementation of what `+` means and two engines cannot drift apart on
//! it. A second set of semantics would be the expensive kind of bug -- the
//! kind the conformance corpus exists to catch, found late.

use std::rc::Rc;

use mrt_ast::BinOp;

use crate::env::Env;
use crate::error::{type_error, Kind, Signal, Thrown};
use crate::generator::{Cursor, GenState, Generator, SavedHandler, Suspended};
use crate::value::{stringify, Compiled, ObjKey, ObjMap, Value};
use crate::vm::chunk::{Chunk, Op, Proto};
use crate::Interpreter;

/// One suspended or running function activation.
///
/// This is the shape that makes a resumable computation possible: a frame is
/// its instruction pointer, its environment and its slice of the operand
/// stack, all of them plain data. Nothing about it lives on the Rust call
/// stack, which is exactly what a tree-walker cannot say.
struct Frame {
    proto: Rc<Proto>,
    ip: usize,
    env: Env,
    /// Where this frame's slots begin in the shared stack.
    ///
    /// `stack[base .. base + proto.slots]` are the frame's locals; operands
    /// push above them. Laying slots out on the shared stack rather than in a
    /// per-frame `Vec` is what makes a call free of allocation, and makes the
    /// arguments a caller already pushed *be* the callee's first slots.
    base: usize,
}

pub struct Vm<'a> {
    interp: &'a mut Interpreter,
    stack: Vec<Value>,
    frames: Vec<Frame>,
    /// Open `try` blocks, innermost last.
    handlers: Vec<Handler>,
    /// What an unwinding `finally` has to resume when it finishes.
    ///
    /// A stack because `finally` blocks nest: an inner one running while an
    /// outer signal is in flight must not lose the outer one.
    pending: Vec<Pending>,
    /// Live `for`-`in` cursors, innermost last.
    ///
    /// A cursor is lazy, so `for (n in naturals())` pulls one value per turn
    /// of the loop rather than trying to collect an endless sequence first.
    ///
    /// Kept in their own typed stack rather than as a `Value` on the operand
    /// stack: a cursor is machine state, not an MRT value, and putting one
    /// where a program could observe it would be inventing a type the
    /// language does not have.
    cursors: Vec<Cursor>,
    /// True when frame 0 is a generator's body rather than an ordinary call.
    ///
    /// Such a frame names itself on a trace as the *generator* -- `<generator
    /// boom>` -- which `resume_generator` adds as the signal leaves. Without
    /// this the frame would also contribute its function name and appear
    /// twice under two spellings.
    generator_body: bool,
}

/// Whether the machine should keep stepping.
enum Step {
    Running,
    Done(Value),
    /// The running generator parked itself at a `yield`.
    Yielded(Value),
}

/// One open `try`.
///
/// The recorded depths are what makes unwinding sound: a signal raised
/// twelve frames and thirty operands deep has to leave the machine exactly
/// as the `try` found it, and the only way to know "exactly" is to have
/// written it down on the way in.
struct Handler {
    frame: usize,
    stack: usize,
    cursors: usize,
    env: Env,
    catch_ip: Option<u32>,
    finally_ip: Option<u32>,
}

/// What a `finally` reached by unwinding must do when it ends.
enum Pending {
    /// Re-raise the signal the `finally` interrupted.
    Signal(Signal),
    /// Complete the `return` the `finally` interrupted.
    Return(Value),
}

impl<'a> Vm<'a> {
    /// Point the interpreter at the running frame's scope.
    ///
    /// The two engines share more than values: the tree-walker's helpers are
    /// the VM's semantics, and some of them read `Interpreter::env` rather
    /// than taking a scope -- resolving a struct name in a match pattern,
    /// for one. So while the VM runs, the interpreter's notion of the
    /// current scope has to *be* the running frame's, or a delegated helper
    /// quietly looks names up in the wrong place. Keeping the invariant
    /// wherever the frame's scope changes is cheaper to get right than
    /// remembering it at every call site.
    fn set_env(&mut self, env: Env) {
        self.interp.env = env.clone();
        if let Some(frame) = self.frames.last_mut() {
            frame.env = env;
        }
    }

    /// Restore the interpreter's scope from the frame that is now on top.
    fn resync_env(&mut self) {
        if let Some(frame) = self.frames.last() {
            self.interp.env = frame.env.clone();
        }
    }

    pub fn new(interp: &'a mut Interpreter) -> Vm<'a> {
        Vm {
            interp,
            stack: Vec::new(),
            frames: Vec::new(),
            handlers: Vec::new(),
            pending: Vec::new(),
            cursors: Vec::new(),
            generator_body: false,
        }
    }

    /// Run a compiled top-level chunk in the interpreter's current scope.
    /// Wrapped in a parameterless prototype so that top-level code and a
    /// function body are the same kind of thing to the machine -- there is
    /// one frame representation, not two.
    pub fn run(&mut self, chunk: Chunk) -> Result<Value, Signal> {
        let entry = self.interp.env.clone();
        let env = entry.clone();
        self.frames.push(Frame {
            proto: Rc::new(Proto {
                name: None,
                params: Vec::new(),
                chunk,
                slots: 0,
                simple_params: true,
                is_generator: false,
            }),
            ip: 0,
            env,
            base: 0,
        });
        let result = self.execute();
        // The machine borrowed the interpreter's scope; hand it back.
        self.interp.env = entry;
        result
    }

    /// Call an already-evaluated callable and run it to completion.
    ///
    /// The driver needs this to invoke `main` after top-level code, which is
    /// a runtime lookup rather than something the compiler can emit a call to.
    pub fn call_and_run(&mut self, callee: Value, args: Vec<Value>) -> Result<Value, Signal> {
        match callee {
            Value::Compiled(_) => {
                let entry = self.interp.env.clone();
                let argc = args.len();
                for arg in args {
                    self.push(arg);
                }
                let result = self
                    .call_with_args_on_stack(callee, argc, 0)
                    .and_then(|()| self.execute());
                self.interp.env = entry;
                result
            }
            // Anything else is the tree-walker's to run anyway.
            other => self.interp.call_value(other, args, None),
        }
    }

    fn frame(&self) -> &Frame {
        self.frames.last().expect("a frame to be running")
    }

    fn push(&mut self, value: Value) {
        self.stack.push(value);
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("a value on the operand stack")
    }

    fn pop_n(&mut self, n: usize) -> Vec<Value> {
        let at = self.stack.len() - n;
        self.stack.split_off(at)
    }

    fn execute(&mut self) -> Result<Value, Signal> {
        loop {
            match self.step() {
                Ok(Step::Running) => {}
                Ok(Step::Done(value)) => return Ok(value),
                // Reached only if a yield were compiled outside a generator,
                // which the compiler refuses; running it would be a bug here
                // rather than a program error.
                Ok(Step::Yielded(_)) => unreachable!("yield outside a generator"),
                Err(signal) => self.unwind(signal)?,
            }
        }
    }

    /// Discard frames down to `depth`, naming each one on the signal as it
    /// goes.
    ///
    /// The trace is built on the way out, exactly as the tree-walker builds
    /// it: a raise site knows nothing about who called it, but every frame
    /// the signal passes through knows its own name. Popping from the top
    /// gives innermost first, which is the order the language specifies.
    fn unwind_frames(&mut self, mut signal: Signal, depth: usize) -> Signal {
        while self.frames.len() > depth {
            let frame = self.frames.pop().expect("a frame to unwind");
            // The entry chunk is not a function, so it contributes no name --
            // the same reason the tree-walker's top level does not.
            // The bottom frame names itself only if it is an ordinary call.
            // A generator body is named by its label as the signal leaves the
            // machine, and the entry chunk is not a function at all -- the
            // same reason the tree-walker's top level contributes nothing.
            let bottom = self.frames.is_empty();
            if bottom && self.generator_body {
                continue;
            }
            match &frame.proto.name {
                Some(name) => signal = signal.with_frame(name),
                None if bottom => {}
                None => signal = signal.with_frame("<anonymous>"),
            }
        }
        signal
    }

    /// Unwind to the innermost handler that wants `signal`, or give up and
    /// let it out of the machine.
    fn unwind(&mut self, mut signal: Signal) -> Result<(), Signal> {
        // `break`, `continue` and `return` are compiled into jumps rather
        // than raised, so anything arriving here is a genuine failure or a
        // `throw` -- except a `return` parked by a `finally`, which is
        // resumed rather than unwound.
        if !matches!(signal, Signal::Throw(_) | Signal::Error(_)) {
            return Err(signal);
        }

        while let Some(handler) = self.handlers.pop() {
            signal = self.unwind_frames(signal, handler.frame);
            self.stack.truncate(handler.stack);
            self.cursors.truncate(handler.cursors);
            let Some(frame) = self.frames.last_mut() else {
                break;
            };
            frame.env = handler.env.clone();
            self.interp.env = handler.env.clone();

            if let Some(catch_ip) = handler.catch_ip {
                // The value a clause sees: a thrown one as thrown, and an
                // interpreter failure as the standard error object, with its
                // `kind` and the trace collected so far for guards to read.
                let value = match &signal {
                    Signal::Throw(t) => t.value.clone(),
                    Signal::Error(e) => crate::error_value(e),
                    _ => unreachable!("checked above"),
                };
                frame.ip = catch_ip as usize;
                let resume = handler.finally_ip;
                self.pending.push(Pending::Signal(signal));
                self.stack.push(value);
                // The `try` is done but the `finally` is not. Leave a
                // finally-only handler behind, so that a `return` or a fresh
                // `throw` from inside the clause -- or no clause matching at
                // all -- still runs it. Without this the most natural thing
                // to write in a catch block, `return`, is exactly what skips
                // the block that promised to always run.
                if let Some(finally_ip) = resume {
                    let frame_depth = self.frames.len();
                    self.handlers.push(Handler {
                        frame: frame_depth,
                        stack: handler.stack,
                        cursors: handler.cursors,
                        env: handler.env,
                        catch_ip: None,
                        finally_ip: Some(finally_ip),
                    });
                }
                return Ok(());
            }
            if let Some(finally_ip) = handler.finally_ip {
                frame.ip = finally_ip as usize;
                self.pending.push(Pending::Signal(signal));
                return Ok(());
            }
        }
        // Nothing wanted it: name the frames it is still inside on the way
        // out of the machine.
        Err(self.unwind_frames(signal, 0))
    }

    fn step(&mut self) -> Result<Step, Signal> {
        {
            let (op, line) = {
                let frame = self.frames.last_mut().expect("a frame to be running");
                let op = frame.proto.chunk.code[frame.ip];
                let line = frame.proto.chunk.lines[frame.ip];
                frame.ip += 1;
                (op, line)
            };

            match op {
                Op::Const(i) => {
                    let value = self.frame().proto.chunk.constants[i as usize].clone();
                    self.push(value);
                }
                Op::Null => self.push(Value::Null),
                Op::True => self.push(Value::Bool(true)),
                Op::False => self.push(Value::Bool(false)),

                Op::GetVar(i) => {
                    let name = Rc::clone(&self.frame().proto.chunk.names[i as usize]);
                    let value = self.frame().env.get(&name, Some(line))?;
                    self.push(value);
                }
                Op::SetVar(i) => {
                    let name = Rc::clone(&self.frame().proto.chunk.names[i as usize]);
                    // Assignment is an expression, so the value stays on the
                    // stack as its result.
                    let value = self.stack.last().expect("a value to assign").clone();
                    self.frame().env.assign(&name, value, Some(line))?;
                }
                Op::DefineVar(i) => {
                    let name = Rc::clone(&self.frame().proto.chunk.names[i as usize]);
                    let value = self.pop();
                    self.frame().env.define(&name, value);
                }

                Op::GetLocal(slot) => {
                    let at = self.frame().base + slot as usize;
                    let value = self.stack[at].clone();
                    self.push(value);
                }
                Op::SetLocal(slot) => {
                    // Assignment is an expression, so the value stays on the
                    // stack as its result.
                    let at = self.frame().base + slot as usize;
                    self.stack[at] = self.stack.last().expect("a value to assign").clone();
                }
                Op::DefineLocal(slot) => {
                    let at = self.frame().base + slot as usize;
                    self.stack[at] = self.pop();
                }

                Op::Pop => {
                    self.pop();
                }

                Op::Neg => {
                    let value = self.pop();
                    match value {
                        Value::Number(n) => self.push(Value::Number(-n)),
                        other => {
                            return Err(type_error(format!(
                                "Operand of '-' must be a number, got {}.",
                                crate::value::type_name(&other)
                            ))
                            .at(Some(line)))
                        }
                    }
                }
                Op::Not => {
                    let value = self.pop();
                    self.push(Value::Bool(!value.is_truthy()));
                }
                Op::Binary(op) => {
                    let right = self.pop();
                    let left = self.pop();
                    let value = self.binary(left, op, right, line)?;
                    self.push(value);
                }

                Op::Jump(target) => self.jump(target),
                Op::JumpIfFalse(target) => {
                    let value = self.pop();
                    if !value.is_truthy() {
                        self.jump(target);
                    }
                }
                Op::JumpIfFalsyKeep(target) => {
                    if !self.stack.last().expect("a condition").is_truthy() {
                        self.jump(target);
                    }
                }
                Op::JumpIfTruthyKeep(target) => {
                    if self.stack.last().expect("a condition").is_truthy() {
                        self.jump(target);
                    }
                }

                Op::PushScope => {
                    let child = self.frame().env.child();
                    self.set_env(child);
                }
                Op::PopScope => {
                    let parent = self.frame().env.parent().expect("a scope to leave");
                    self.set_env(parent);
                }

                Op::Array(n) => {
                    let items = self.pop_n(n as usize);
                    self.push(Value::array(items));
                }
                Op::Dict(n) => {
                    let flat = self.pop_n(n as usize * 2);
                    let mut map = ObjMap::new();
                    for pair in flat.chunks(2) {
                        let Some(key) = ObjKey::from_value(&pair[0]) else {
                            return Err(type_error(
                                "Object keys must be numbers, strings, or booleans (not arrays or objects).",
                            )
                            .at(Some(line)));
                        };
                        map.insert(key, pair[1].clone());
                    }
                    self.push(Value::object(map));
                }
                Op::Interp(n) => {
                    let parts = self.pop_n(n as usize);
                    let text: String = parts.iter().map(stringify).collect();
                    self.push(Value::str(text));
                }

                Op::Index => {
                    let index = self.pop();
                    let target = self.pop();
                    let value = self.interp.index_get(&target, &index, Some(line))?;
                    self.push(value);
                }
                Op::IndexSet => {
                    let value = self.pop();
                    let index = self.pop();
                    let target = self.pop();
                    self.interp
                        .index_set(&target, &index, value.clone(), Some(line))?;
                    self.push(value);
                }

                Op::Print(n) => {
                    let args = self.pop_n(n as usize);
                    self.interp.print_values(&args);
                }
                Op::PrintSpread => {
                    let gathered = self.pop();
                    let Value::Array(args) = gathered else {
                        unreachable!("BeginSpread pushes an array");
                    };
                    let args = args.borrow().clone();
                    self.interp.print_values(&args);
                }

                Op::Closure(i) => {
                    let proto = self.frame().proto.chunk.protos[i as usize].clone();
                    let env = self.frame().env.clone();
                    self.push(Value::Compiled(Rc::new(Compiled {
                        proto,
                        closure: env,
                    })));
                }

                Op::Call(argc) => {
                    // The callee sits just below its arguments. Lifting it
                    // out leaves the arguments where a simple-parameter
                    // callee's slots want them: no copying, no binding.
                    let at = self.stack.len() - argc as usize - 1;
                    let callee = self.stack.remove(at);
                    self.call_with_args_on_stack(callee, argc as usize, line)?;
                }

                Op::Return => {
                    let value = self.pop();
                    // A `return` from inside a `try` still owes its
                    // `finally`. Park the value, run the finally, and let
                    // `EndFinally` complete the return -- otherwise the one
                    // construct whose whole promise is "this always runs"
                    // would be skipped by the most ordinary exit there is.
                    if let Some(finally_ip) = self.take_frame_finally() {
                        self.pending.push(Pending::Return(value));
                        self.jump(finally_ip);
                        return Ok(Step::Running);
                    }
                    return Ok(self.finish_return(value));
                }

                Op::IterInit => {
                    let iterable = self.pop();
                    let cursor = self.interp.cursor(&iterable, Some(line))?;
                    self.cursors.push(cursor);
                }
                Op::IterNext(target) => {
                    // Lifted out for the pull: advancing a generator runs MRT
                    // code, which needs the interpreter, and leaving the
                    // cursor in place would hold a borrow across that.
                    let mut cursor = self.cursors.pop().expect("a cursor");
                    let item = self.interp.advance(&mut cursor, Some(line));
                    self.cursors.push(cursor);
                    match item? {
                        Some(item) => self.push(item),
                        None => self.jump(target),
                    }
                }
                Op::IterDrop => {
                    self.cursors.pop();
                }

                Op::Struct(index) => {
                    let (name, fields, methods) =
                        self.frame().proto.chunk.structs[index as usize].clone();
                    let env = self.frame().env.clone();
                    // Methods stay AST closures, so they run on the
                    // tree-walker. Nothing observable turns on which engine
                    // runs a method body, and keeping one representation of
                    // `StructType` means `this`-binding, field access and
                    // instance printing have one implementation.
                    let methods = methods
                        .iter()
                        .filter_map(|m| match &m.kind {
                            mrt_ast::StmtKind::Function {
                                name,
                                params,
                                body,
                                is_generator,
                            } => Some((
                                name.text.clone(),
                                Rc::new(crate::value::Function {
                                    name: Some(name.text.clone()),
                                    params: params.clone(),
                                    body: body.clone(),
                                    closure: env.clone(),
                                    is_generator: *is_generator,
                                }),
                            )),
                            _ => None,
                        })
                        .collect();
                    self.push(Value::Struct(Rc::new(crate::value::StructType {
                        name,
                        fields,
                        methods,
                    })));
                }

                Op::MatchPattern(index) => {
                    let pattern = self.frame().proto.chunk.match_patterns[index as usize].clone();
                    // The subject stays on the stack: a case that does not
                    // match leaves the next one something to test.
                    let subject = self.stack.last().expect("a match subject").clone();
                    let scope = self.frame().env.clone();
                    let matched = self.interp.match_pattern(&pattern, &subject, &scope)?;
                    self.push(Value::Bool(matched));
                }

                Op::NoMatch => {
                    let subject = self.pop();
                    return Err(crate::error::value_error(format!(
                        "No case matched {} in this match, and there is no 'default'.",
                        stringify(&subject)
                    ))
                    .at(Some(line)));
                }

                Op::BindPattern(index) | Op::AssignPattern(index) => {
                    // `declare` distinguishes the two: a declaration
                    // introduces names, an assignment reaches existing ones.
                    let declare = matches!(op, Op::BindPattern(_));
                    let pattern = self.frame().proto.chunk.patterns[index as usize].clone();
                    let value = self.pop();
                    let scope = self.frame().env.clone();
                    self.interp
                        .bind_pattern(&pattern, Some(value), &scope, Some(line), declare)?;
                }

                Op::BeginSpread => self.push(Value::array(Vec::new())),
                Op::PushInto => {
                    let value = self.pop();
                    let Value::Array(items) = self.stack.last().expect("a gather array") else {
                        unreachable!("BeginSpread pushes an array");
                    };
                    items.borrow_mut().push(value);
                }
                Op::SpreadInto => {
                    let spread = self.pop();
                    // Arrays only, and not merely "anything iterable": `...`
                    // over a string or an object is a type error in MRT, with
                    // its own message. Reaching for `iterate` here would make
                    // the VM accept programs the language rejects.
                    let Value::Array(items) = spread else {
                        return Err(
                            type_error("Can only spread an array with '...'.").at(Some(line))
                        );
                    };
                    let taken = items.borrow().clone();
                    let Value::Array(gathered) = self.stack.last().expect("a gather array") else {
                        unreachable!("BeginSpread pushes an array");
                    };
                    gathered.borrow_mut().extend(taken);
                }
                Op::CallSpread => {
                    let gathered = self.pop();
                    let Value::Array(items) = gathered else {
                        unreachable!("BeginSpread pushes an array");
                    };
                    let args = items.borrow().clone();
                    let argc = args.len();
                    for arg in args {
                        self.push(arg);
                    }
                    let at = self.stack.len() - argc - 1;
                    let callee = self.stack.remove(at);
                    self.call_with_args_on_stack(callee, argc, line)?;
                }

                Op::DelegateInit => {
                    let delegate = self.pop();
                    let cursor = self.interp.cursor(&delegate, Some(line))?;
                    self.cursors.push(cursor);
                }
                Op::DelegateNext(target) => {
                    // The sent value is on the stack because the `Yield` that
                    // received it left it there; it goes into the delegate
                    // rather than to this frame.
                    let sent = self.pop();
                    let mut cursor = self.cursors.pop().expect("a delegate cursor");
                    let item = self.interp.advance_with(&mut cursor, sent, Some(line));
                    self.cursors.push(cursor);
                    match item? {
                        Some(item) => self.push(item),
                        None => self.jump(target),
                    }
                }

                Op::Yield => {
                    // Only the generator's own frame can yield: `yield` is
                    // lexical, so a nested function containing one is its own
                    // generator and never this one's frame.
                    debug_assert_eq!(self.frames.len(), 1, "yield outside a generator frame");
                    let value = self.pop();
                    return Ok(Step::Yielded(value));
                }

                Op::Throw => {
                    let value = self.pop();
                    return Err(Signal::Throw(Thrown {
                        value,
                        stack: Vec::new(),
                    }));
                }

                Op::PushHandler { catch, finally } => {
                    const NONE: u32 = u32::MAX;
                    self.handlers.push(Handler {
                        frame: self.frames.len(),
                        stack: self.stack.len(),
                        cursors: self.cursors.len(),
                        env: self.frame().env.clone(),
                        catch_ip: (catch != NONE).then_some(catch),
                        finally_ip: (finally != NONE).then_some(finally),
                    });
                }
                Op::PopHandler => {
                    self.handlers.pop();
                }
                Op::BindCatch(index) => {
                    let pattern = self.frame().proto.chunk.patterns[index as usize].clone();
                    let value = self.stack.last().expect("a thrown value").clone();
                    let scope = self.frame().env.clone();
                    // A clause that cannot take the value apart does not
                    // match; that is a choice about which clause runs, not an
                    // error the program should see.
                    let bound = self
                        .interp
                        .bind_pattern(&pattern, Some(value), &scope, Some(line), true)
                        .is_ok();
                    self.push(Value::Bool(bound));
                }
                Op::DropPending => {
                    self.pending.pop();
                }
                Op::EndFinally => match self.pending.pop() {
                    Some(Pending::Signal(signal)) => return Err(signal),
                    Some(Pending::Return(value)) => {
                        if let Some(finally_ip) = self.take_frame_finally() {
                            self.pending.push(Pending::Return(value));
                            self.jump(finally_ip);
                            return Ok(Step::Running);
                        }
                        return Ok(self.finish_return(value));
                    }
                    None => {}
                },
            }
        }
        Ok(Step::Running)
    }

    /// Pop this frame's innermost handler if it still owes a `finally`.
    fn take_frame_finally(&mut self) -> Option<u32> {
        let depth = self.frames.len();
        let handler = self.handlers.last()?;
        if handler.frame != depth {
            return None;
        }
        let finally_ip = handler.finally_ip?;
        self.handlers.pop();
        Some(finally_ip)
    }

    fn finish_return(&mut self, value: Value) -> Step {
        let frame = self.frames.pop().expect("a frame to return from");
        // Discard anything the frame left behind, then hand the result to
        // the caller.
        self.stack.truncate(frame.base);
        // Handlers opened by the frame that is going away go with it.
        let depth = self.frames.len();
        self.handlers.retain(|h| h.frame <= depth);
        if self.frames.is_empty() {
            return Step::Done(value);
        }
        self.resync_env();
        self.push(value);
        Step::Running
    }

    fn jump(&mut self, target: u32) {
        self.frames.last_mut().expect("a frame").ip = target as usize;
    }

    fn binary(&mut self, left: Value, op: BinOp, right: Value, line: u32) -> Result<Value, Signal> {
        self.interp.binary(left, op, right, line)
    }

    /// Call a value whose `argc` arguments are already on top of the stack.
    ///
    /// A compiled function pushes a frame; anything else the language can
    /// call is handed to the interpreter, which already knows how.
    fn call_with_args_on_stack(
        &mut self,
        callee: Value,
        argc: usize,
        line: u32,
    ) -> Result<(), Signal> {
        let Value::Compiled(function) = callee else {
            let args = self.pop_n(argc);
            let value = self.interp.call_value(callee, args, Some(line))?;
            self.push(value);
            return Ok(());
        };

        // Calling a generator function runs none of its body: it hands back
        // a sequence parked before the first instruction. The arity check
        // still happens now, at the call, because that is where the reference
        // implementation reports it and where the program wrote the call.
        if function.proto.is_generator {
            if !crate::value::accepts(&function.proto.params, argc) {
                return Err(arity_error(&function.proto.params, argc, Some(line)));
            }
            let args = self.pop_n(argc);
            self.push(Value::Generator(Rc::new(Generator::starting(
                generator_label(&function.proto.name),
                function,
                args,
            ))));
            return Ok(());
        }

        self.push_call_frame(function, argc, line)
    }

    /// Push a frame for an ordinary call: check arity, bind parameters, lay
    /// out slots. Split out from `call_with_args_on_stack` so that resuming a
    /// generator can reach it without going back through the check that would
    /// hand it another generator.
    fn push_call_frame(
        &mut self,
        function: Rc<Compiled>,
        argc: usize,
        line: u32,
    ) -> Result<(), Signal> {
        if !crate::value::accepts(&function.proto.params, argc) {
            return Err(arity_error(&function.proto.params, argc, Some(line)));
        }

        let base = self.stack.len() - argc;
        let scope = if function.proto.simple_params {
            // The arguments are already slots 0..argc. Nothing to bind.
            function.closure.child()
        } else {
            // Defaults, rest and destructuring run through the tree-walker,
            // so a parameter list means one thing in both engines.
            let args = self.stack.split_off(base);
            let scope = function.closure.child();
            self.interp
                .bind_params(&function.proto.params, args, &scope, Some(line))?;
            scope
        };
        // Slots the body declares start as null; the body's `DefineLocal`s
        // fill them before any read the compiler will emit.
        self.stack.resize(base + function.proto.slots, Value::Null);
        self.frames.push(Frame {
            proto: function.proto.clone(),
            ip: 0,
            env: scope.clone(),
            base,
        });
        self.interp.env = scope;
        Ok(())
    }

    // -- generators --------------------------------------------------------

    /// Put a parked frame back into an empty machine, ready to carry on.
    ///
    /// The saved depths are absolute within the frame because the frame's
    /// base is always 0 here: a resumed generator is the only thing on this
    /// machine, which is what makes restoring it a copy rather than a
    /// relocation.
    fn install(&mut self, state: GenState, sent: Value, line: Option<u32>) -> Result<(), Signal> {
        self.generator_body = true;
        match state {
            GenState::Start { function, args } => {
                let argc = args.len();
                for arg in args {
                    self.push(arg);
                }
                // The call path does the arity check, parameter binding and
                // slot layout, so a generator's parameter list means exactly
                // what any other function's does.
                self.push_call_frame(function, argc, line.unwrap_or(0))?;
                // The value sent on the first step is discarded: the body has
                // not reached a `yield` yet, so there is nothing to receive it.
                Ok(())
            }
            GenState::Suspended(parked) => {
                let Suspended {
                    proto,
                    ip,
                    env,
                    stack,
                    handlers,
                    cursors,
                } = *parked;
                self.stack = stack;
                self.cursors = cursors;
                self.handlers = handlers
                    .into_iter()
                    .map(|h| Handler {
                        frame: 1,
                        stack: h.stack,
                        cursors: h.cursors,
                        env: h.env,
                        catch_ip: h.catch_ip,
                        finally_ip: h.finally_ip,
                    })
                    .collect();
                self.frames.push(Frame {
                    proto,
                    ip,
                    env: env.clone(),
                    base: 0,
                });
                self.interp.env = env;
                // The sent value becomes the result of the `yield` the body is
                // parked at, which is what makes `var got = yield x;` work.
                self.push(sent);
                Ok(())
            }
            GenState::Running | GenState::Done | GenState::Transform { .. } => {
                unreachable!("handled before reaching the machine")
            }
        }
    }

    /// Lift the running frame out of the machine and park it in a value.
    fn park(&mut self) -> Suspended {
        let frame = self.frames.pop().expect("the generator's frame");
        Suspended {
            proto: frame.proto,
            ip: frame.ip,
            env: frame.env,
            stack: std::mem::take(&mut self.stack),
            handlers: std::mem::take(&mut self.handlers)
                .into_iter()
                .map(|h| SavedHandler {
                    stack: h.stack,
                    cursors: h.cursors,
                    env: h.env,
                    catch_ip: h.catch_ip,
                    finally_ip: h.finally_ip,
                })
                .collect(),
            cursors: std::mem::take(&mut self.cursors),
        }
    }

    /// Step the installed generator body until it yields or finishes.
    fn drive(&mut self) -> Result<Option<Value>, Signal> {
        loop {
            match self.step() {
                Ok(Step::Running) => {}
                // `return` inside a generator simply ends the sequence: the
                // returned value is not an item, because a generator's items
                // are the ones it yielded.
                Ok(Step::Done(_)) => return Ok(None),
                Ok(Step::Yielded(value)) => return Ok(Some(value)),
                Err(signal) => self.unwind(signal)?,
            }
        }
    }
}

/// Run a generator's body until its next `yield`.
///
/// A fresh machine per step, holding just this generator's frame. That is
/// affordable because a frame is data: installing one is a move, not a
/// replay, and it keeps a resumed generator from inheriting the operand
/// stack of whatever happened to pull from it.
pub fn resume_generator(
    interp: &mut Interpreter,
    generator: &Rc<Generator>,
    state: GenState,
    sent: Value,
    line: Option<u32>,
) -> Result<Option<Value>, Signal> {
    // The body and whoever consumes it take turns using the interpreter's
    // current scope, so the hand-off restores the consumer's.
    let saved = interp.env.clone();
    let outcome = {
        let mut vm = Vm::new(interp);
        match vm.install(state, sent, line) {
            Ok(()) => match vm.drive() {
                Ok(Some(value)) => Ok(Parked::Yielded(value, vm.park())),
                Ok(None) => Ok(Parked::Finished),
                Err(signal) => Err(signal),
            },
            Err(signal) => Err(signal),
        }
    };
    interp.env = saved;

    match outcome {
        Ok(Parked::Yielded(value, parked)) => {
            generator
                .state
                .replace(GenState::Suspended(Box::new(parked)));
            Ok(Some(value))
        }
        Ok(Parked::Finished) => {
            generator.state.replace(GenState::Done);
            Ok(None)
        }
        Err(signal) => {
            // A generator that failed is finished: there is no resuming a
            // body that did not reach a `yield`.
            generator.state.replace(GenState::Done);
            Err(signal.with_frame(&generator.label))
        }
    }
}

fn arity_error(params: &[mrt_ast::Param], argc: usize, line: Option<u32>) -> Signal {
    Signal::error(
        Kind::ArityError,
        format!(
            "Expected {} arguments but got {}.",
            crate::value::arity_description(params),
            argc
        ),
    )
    .at(line)
}

/// How a generator names itself when printed or named in a trace.
pub fn generator_label(name: &Option<String>) -> String {
    match name {
        Some(name) => format!("<generator {name}>"),
        None => "<generator>".into(),
    }
}

enum Parked {
    Yielded(Value, Suspended),
    Finished,
}
