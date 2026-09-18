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
use crate::error::{type_error, Kind, Signal};
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
    /// Where this frame's operands begin in the shared stack.
    base: usize,
}

pub struct Vm<'a> {
    interp: &'a mut Interpreter,
    stack: Vec<Value>,
    frames: Vec<Frame>,
    /// Live `for`-`in` cursors, innermost last.
    ///
    /// Kept in their own typed stack rather than as a `Value` on the operand
    /// stack: a cursor is machine state, not an MRT value, and putting one
    /// where a program could observe it would be inventing a type the
    /// language does not have.
    cursors: Vec<Cursor>,
}

/// A `for`-`in` cursor: the materialised items and how far through them the
/// loop has got.
struct Cursor {
    items: Vec<Value>,
    next: usize,
}

impl<'a> Vm<'a> {
    pub fn new(interp: &'a mut Interpreter) -> Vm<'a> {
        Vm {
            interp,
            stack: Vec::new(),
            frames: Vec::new(),
            cursors: Vec::new(),
        }
    }

    /// Run a compiled top-level chunk in the interpreter's current scope.
    /// Wrapped in a parameterless prototype so that top-level code and a
    /// function body are the same kind of thing to the machine -- there is
    /// one frame representation, not two.
    pub fn run(&mut self, chunk: Chunk) -> Result<Value, Signal> {
        let env = self.interp.env.clone();
        self.frames.push(Frame {
            proto: Rc::new(Proto {
                name: None,
                params: Vec::new(),
                chunk,
            }),
            ip: 0,
            env,
            base: 0,
        });
        self.execute()
    }

    /// Call an already-evaluated callable and run it to completion.
    ///
    /// The driver needs this to invoke `main` after top-level code, which is
    /// a runtime lookup rather than something the compiler can emit a call to.
    pub fn call_and_run(&mut self, callee: Value, args: Vec<Value>) -> Result<Value, Signal> {
        match callee {
            Value::Compiled(_) => {
                self.call(callee, args, 0)?;
                self.execute()
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
                    let frame = self.frames.last_mut().expect("a frame");
                    frame.env = frame.env.child();
                }
                Op::PopScope => {
                    let frame = self.frames.last_mut().expect("a frame");
                    frame.env = frame.env.parent().expect("a scope to leave");
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

                Op::Closure(i) => {
                    let proto = self.frame().proto.chunk.protos[i as usize].clone();
                    let env = self.frame().env.clone();
                    self.push(Value::Compiled(Rc::new(Compiled {
                        proto,
                        closure: env,
                    })));
                }

                Op::Call(argc) => {
                    let args = self.pop_n(argc as usize);
                    let callee = self.pop();
                    self.call(callee, args, line)?;
                }

                Op::Return => {
                    let value = self.pop();
                    let frame = self.frames.pop().expect("a frame to return from");
                    // Discard anything the frame left behind, then hand the
                    // result to the caller.
                    self.stack.truncate(frame.base);
                    if self.frames.is_empty() {
                        return Ok(value);
                    }
                    self.push(value);
                }

                Op::IterInit => {
                    let iterable = self.pop();
                    let items = self.interp.iterate(&iterable, Some(line))?;
                    self.cursors.push(Cursor { items, next: 0 });
                }
                Op::IterNext(target) => {
                    let cursor = self.cursors.last_mut().expect("a cursor");
                    match cursor.items.get(cursor.next).cloned() {
                        Some(item) => {
                            cursor.next += 1;
                            self.push(item);
                        }
                        None => self.jump(target),
                    }
                }
                Op::IterDrop => {
                    self.cursors.pop();
                }
            }
        }
    }

    fn jump(&mut self, target: u32) {
        self.frames.last_mut().expect("a frame").ip = target as usize;
    }

    fn binary(&mut self, left: Value, op: BinOp, right: Value, line: u32) -> Result<Value, Signal> {
        self.interp.binary(left, op, right, line)
    }

    /// Call a value. A compiled function pushes a frame; anything else the
    /// language can call is handed to the interpreter, which already knows
    /// how.
    fn call(&mut self, callee: Value, args: Vec<Value>, line: u32) -> Result<(), Signal> {
        match callee {
            Value::Compiled(function) => {
                if !crate::value::accepts(&function.proto.params, args.len()) {
                    return Err(Signal::error(
                        Kind::ArityError,
                        format!(
                            "Expected {} arguments but got {}.",
                            crate::value::arity_description(&function.proto.params),
                            args.len()
                        ),
                    )
                    .at(Some(line)));
                }
                let scope = function.closure.child();
                // Parameter binding -- defaults, rest, destructuring -- runs
                // through the tree-walker, so a parameter list means one
                // thing in both engines.
                self.interp
                    .bind_params(&function.proto.params, args, &scope, Some(line))?;
                let base = self.stack.len();
                self.frames.push(Frame {
                    proto: function.proto.clone(),
                    ip: 0,
                    env: scope,
                    base,
                });
                Ok(())
            }
            other => {
                let value = self.interp.call_value(other, args, Some(line))?;
                self.push(value);
                Ok(())
            }
        }
    }
}
