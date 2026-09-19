//! The instruction set, and the compiled form of one function body.

use std::rc::Rc;

use mrt_ast::{BinOp, MatchPattern, Param, Pattern, Stmt};

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

    /// Read, assign to, and declare a variable by name, through the `Env`
    /// chain the tree-walker uses.
    ///
    /// Still the path for anything a nested closure captures, for globals,
    /// and for top-level code -- all the places where a binding has to
    /// outlive the frame or be reachable from another one.
    GetVar(u32),
    SetVar(u32),
    DefineVar(u32),

    /// Read, assign to, and initialise a frame slot.
    ///
    /// The fast path: a function-local nothing captures lives at a fixed
    /// offset from the frame's base, so reading it is an index into the
    /// operand stack instead of a hash lookup per enclosing scope.
    GetLocal(u32),
    SetLocal(u32),
    DefineLocal(u32),

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

    /// Raise the value on top of the stack as a thrown signal.
    Throw,

    /// Enter a `try`. Records where to resume if a signal unwinds through
    /// here: `catch` for the clause chain, `finally` for the copy of the
    /// finally block that re-raises afterwards. `u32::MAX` means absent.
    PushHandler {
        catch: u32,
        finally: u32,
    },
    /// Leave a `try` normally.
    PopHandler,
    /// Declare a struct type from its definition, closing over the current
    /// environment for its methods.
    Struct(u32),
    /// Test the value on top of the stack against a match pattern, binding
    /// what it captures. Pushes the verdict and leaves the value in place
    /// for the next case.
    MatchPattern(u32),
    /// Fail a `match` that ran out of cases, naming the value that matched
    /// nothing.
    NoMatch,

    /// Bind the value on top of the stack through a pattern, declaring the
    /// names it introduces. Pops the value; a pattern that cannot take it
    /// apart is an error, unlike `BindCatch`.
    BindPattern(u32),
    /// Assign through a pattern to variables that already exist.
    AssignPattern(u32),
    /// Append the iterable on top of the stack to the array below it -- how
    /// `...xs` is spread into a call's arguments or an array literal.
    SpreadInto,
    /// Start collecting a spread argument list: push an empty array.
    BeginSpread,
    /// Append the value on top of the stack to the array below it.
    PushInto,
    /// Call with the argument array on top of the stack.
    CallSpread,

    /// Bind the value on top of the stack through a catch clause's pattern,
    /// pushing `true` if it bound and `false` if the clause does not apply.
    /// The value itself stays on the stack for the next clause to try.
    BindCatch(u32),
    /// Discard the signal parked by the unwinder: a clause handled it.
    DropPending,
    /// End a finally block reached by unwinding: resume whatever was parked
    /// -- re-raise the signal, or complete the return it interrupted.
    EndFinally,

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
    /// Catch-clause and destructuring patterns, indexed by the `u32` in
    /// `BindCatch`, `BindPattern` and `AssignPattern`. Kept as AST and bound
    /// by the tree-walker, so a pattern means one thing in both engines --
    /// the same reason parameter lists are kept whole.
    pub patterns: Vec<Pattern>,
    /// Match patterns, indexed by the `u32` in `MatchPattern`. A separate
    /// list because they are a separate language: a binding pattern takes a
    /// value apart and fails loudly, a match pattern *tests* one.
    pub match_patterns: Vec<MatchPattern>,
    /// Struct definitions, indexed by the `u32` in `Struct`: the name, its
    /// fields, and its methods as AST.
    pub structs: Vec<(String, Vec<Param>, Vec<Stmt>)>,
}

impl Chunk {
    pub fn new() -> Chunk {
        Chunk {
            code: Vec::new(),
            lines: Vec::new(),
            constants: Vec::new(),
            names: Vec::new(),
            protos: Vec::new(),
            patterns: Vec::new(),
            match_patterns: Vec::new(),
            structs: Vec::new(),
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

    pub fn pattern(&mut self, pattern: Pattern) -> u32 {
        self.patterns.push(pattern);
        (self.patterns.len() - 1) as u32
    }

    pub fn match_pattern(&mut self, pattern: MatchPattern) -> u32 {
        self.match_patterns.push(pattern);
        (self.match_patterns.len() - 1) as u32
    }

    pub fn struct_def(&mut self, name: String, fields: Vec<Param>, methods: Vec<Stmt>) -> u32 {
        self.structs.push((name, fields, methods));
        (self.structs.len() - 1) as u32
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
    /// How many frame slots this function needs.
    pub slots: usize,
    /// True when every parameter is a plain name with no default, no rest
    /// and no destructuring, *and* none of them is captured.
    ///
    /// Then the arguments already sitting on the operand stack at call time
    /// *are* slots 0..n, and the call costs no binding work at all: no child
    /// `Env`, no hash insert per parameter. Anything more interesting falls
    /// back to the tree-walker's `bind_params`, which is the only place that
    /// knows what a default or a rest parameter means.
    pub simple_params: bool,
}
