//! The instruction set, and the compiled form of one function body.

use std::rc::Rc;

use mrt_ast::{BinOp, Param};

use crate::value::Value;

/// One instruction.
///
/// `Copy`, and every operand a plain integer, so stepping the machine reads
/// one out of the code vector without touching the allocator. It was not,
/// briefly, and the cost showed up immediately as the VM running *slower*
/// than the tree-walker it was meant to beat.
///
/// Stack-based rather than register-based, which is not the end state the
/// MRT 2.0 proposal describes. It is the right *first* state: the reason
/// this machine exists is that a suspended computation needs an explicit
/// instruction pointer, and on a stack machine a suspended frame is its `ip`
/// plus a slice of the operand stack -- nothing else. Register allocation is
/// an optimisation to make once there is something to measure.
#[derive(Clone, Copy, Debug)]
pub enum Op {
    /// Push a constant from the pool.
    Const(u32),
    Null,
    True,
    False,

    /// Read, assign to, and declare a variable by name.
    ///
    /// By *name*, not by frame slot, and that is deliberate. The resolver can
    /// already produce slots, but the benchmarks put name lookup at ~10% of
    /// run time against ~66% for dispatch -- so slots are an optimisation,
    /// while sharing `Env` with the tree-walker means closures, per-iteration
    /// loop bindings and module scopes are the semantics that are already
    /// proven rather than a second set to get right.
    GetVar(u32),
    SetVar(u32),
    DefineVar(u32),

    Pop,

    Neg,
    Not,
    Binary(BinOp),

    /// Absolute jump targets: a patched-in index into `code`.
    Jump(u32),
    /// Pops the condition.
    JumpIfFalse(u32),
    /// Leaves the operand in place -- `&&` and `||` yield the operand itself,
    /// not a boolean.
    JumpIfFalsyKeep(u32),
    JumpIfTruthyKeep(u32),

    /// Enter and leave a block scope.
    PushScope,
    PopScope,

    /// Build an array or object from the top `n` (or `2n`) stack slots.
    Array(u32),
    Dict(u32),
    /// Join the top `n` slots with their `stringify`d forms.
    Interp(u32),

    Index,
    IndexSet,

    Call(u32),
    /// Build a closure over the current environment from a prototype.
    Closure(u32),
    Return,

    Print(u32),

    /// Materialise the iterable on top of the stack into a cursor, held in
    /// the machine's own cursor stack rather than as an operand.
    IterInit,
    /// Push the next item, or jump to the argument when the cursor is spent.
    IterNext(u32),
    /// Discard the innermost cursor. Every exit from a `for`-`in` -- falling
    /// out of it or breaking from it -- passes through one of these.
    IterDrop,
}

/// A function body, compiled.
pub struct Chunk {
    pub code: Vec<Op>,
    /// Line per instruction, for errors. Parallel to `code`.
    pub lines: Vec<u32>,
    pub constants: Vec<Value>,
    /// Variable names, indexed by the `u32` in `GetVar`/`SetVar`/`DefineVar`.
    ///
    /// `Rc<str>` rather than `String`: the machine takes one on every
    /// variable access, and a refcount bump is not a heap allocation.
    pub names: Vec<Rc<str>>,
    /// Nested function prototypes, indexed by the `u32` in `Closure`.
    pub protos: Vec<Rc<Proto>>,
}

impl Chunk {
    pub fn new() -> Chunk {
        Chunk {
            code: Vec::new(),
            lines: Vec::new(),
            constants: Vec::new(),
            names: Vec::new(),
            protos: Vec::new(),
        }
    }

    pub fn emit(&mut self, op: Op, line: u32) -> usize {
        self.code.push(op);
        self.lines.push(line);
        self.code.len() - 1
    }

    /// Record a constant, reusing an identical earlier one.
    ///
    /// Only scalars are deduplicated: two array literals that look alike are
    /// still two arrays, because aggregates have reference semantics and
    /// sharing one would make `push` on one visible through the other.
    pub fn constant(&mut self, value: Value) -> u32 {
        if matches!(
            value,
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Str(_)
        ) {
            if let Some(i) = self.constants.iter().position(|c| {
                crate::value::values_equal(c, &value)
                    && crate::value::type_name(c) == crate::value::type_name(&value)
            }) {
                return i as u32;
            }
        }
        self.constants.push(value);
        (self.constants.len() - 1) as u32
    }

    pub fn name(&mut self, name: &str) -> u32 {
        if let Some(i) = self.names.iter().position(|n| &**n == name) {
            return i as u32;
        }
        self.names.push(Rc::from(name));
        (self.names.len() - 1) as u32
    }

    pub fn proto(&mut self, proto: Rc<Proto>) -> u32 {
        self.protos.push(proto);
        (self.protos.len() - 1) as u32
    }
}

impl Default for Chunk {
    fn default() -> Self {
        Chunk::new()
    }
}

/// Everything about a function except the environment it closes over.
pub struct Proto {
    pub name: Option<String>,
    /// Kept as AST: parameter binding -- defaults, rest, destructuring -- is
    /// evaluated by the tree-walker, so there is exactly one implementation
    /// of what a parameter list means.
    pub params: Vec<Param>,
    pub chunk: Chunk,
}
