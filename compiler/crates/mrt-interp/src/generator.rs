//! Generators: a computation that can be parked mid-body and picked up later.
//!
//! This is the feature the bytecode VM was built for, and the reason it
//! exists as a second engine rather than an optimisation of the first. A
//! tree-walker's state *is* the Rust call stack: to suspend one you would
//! have to park a set of live Rust stack frames in a value and restore them
//! later, which stable Rust gives you no way to do. Python and JavaScript
//! each borrow a coroutine from their host language and are done in a dozen
//! lines; Rust has none to borrow.
//!
//! A machine whose frame is an instruction pointer, an environment and a
//! slice of an operand stack has no such problem, because all of that is
//! plain data. `Suspended` below *is* a VM frame, lifted out of the machine
//! and put in a value. Resuming copies it back.
//!
//! -- Why one frame is enough --
//!
//! `yield` is lexical: it may only appear in the body of the generator
//! function itself, never inside a nested function, because a nested
//! function containing `yield` is its own generator. So a generator can only
//! ever be suspended in its *own* frame, and the machine never has to park a
//! call chain. What it does have to park is everything else that frame owns:
//! open `try` handlers and live `for`-`in` cursors, which is why they are
//! saved here rather than left behind in the machine.

use std::cell::RefCell;
use std::rc::Rc;

use crate::env::Env;
use crate::error::Signal;
use crate::value::{Compiled, Value};
use crate::vm::chunk::Proto;

/// A lazy sequence: the result of calling a generator function, or of a lazy
/// builtin such as `map` over another sequence.
///
/// Nothing runs until something pulls, and only as far as it asks -- which is
/// what makes an endless generator usable. It is *resumable*: a loop that
/// stops early leaves it parked mid-body and the next consumer carries on
/// from there. It is not restartable: once the sequence has run out there is
/// no way back to the start, so `for`-`in` over a finished generator is an
/// error rather than a silently empty loop.
pub struct Generator {
    /// How the generator names itself in errors and when printed:
    /// `<generator counter>`.
    pub label: String,
    pub state: RefCell<GenState>,
}

impl Generator {
    /// A generator function that has been called but not yet stepped.
    pub fn starting(label: String, function: Rc<Compiled>, args: Vec<Value>) -> Generator {
        Generator {
            label,
            state: RefCell::new(GenState::Start { function, args }),
        }
    }

    /// A lazy transform over another sequence -- `map` or `filter`.
    pub fn transform(label: String, kind: Transform, source: Cursor, f: Value) -> Generator {
        Generator {
            label,
            state: RefCell::new(GenState::Transform {
                kind,
                source: Box::new(source),
                f,
            }),
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(&*self.state.borrow(), GenState::Done)
    }
}

pub enum GenState {
    /// Called, but nothing of the body has run. Kept distinct from
    /// `Suspended` because the first step has no `yield` to deliver a sent
    /// value to, so that value is discarded rather than bound.
    Start {
        function: Rc<Compiled>,
        args: Vec<Value>,
    },
    /// Parked at a `yield`, with everything needed to carry on.
    Suspended(Box<Suspended>),
    /// A lazy `map` or `filter` pulling from another sequence.
    Transform {
        kind: Transform,
        source: Box<Cursor>,
        f: Value,
    },
    /// The body is on the stack right now. A program that resumes a generator
    /// from inside itself lands here, and gets a real MRT error rather than
    /// whichever host-language failure would otherwise happen first.
    Running,
    /// Run to completion, returned early, or failed. All of those are final:
    /// a generator is single use.
    Done,
}

#[derive(Clone, Copy)]
pub enum Transform {
    Map,
    Filter,
}

/// A parked VM frame.
///
/// Everything here was read out of the running machine at the `yield` and is
/// written back on resume. The stack region and the handler and cursor stacks
/// travel together because they index into each other: a handler records the
/// operand depth to truncate to, so saving one without the others would
/// restore a frame whose bookkeeping pointed at the wrong places.
pub struct Suspended {
    pub proto: Rc<Proto>,
    pub ip: usize,
    pub env: Env,
    /// The frame's slots and operands: what was `stack[base..]` in the machine.
    pub stack: Vec<Value>,
    pub handlers: Vec<SavedHandler>,
    pub cursors: Vec<Cursor>,
}

/// An open `try` inside a suspended frame, with its depths made relative to
/// the frame's own base so they survive being restored at a different one.
pub struct SavedHandler {
    pub stack: usize,
    pub cursors: usize,
    pub env: Env,
    pub catch_ip: Option<u32>,
    pub finally_ip: Option<u32>,
}

/// A position in a sequence being consumed.
///
/// Iteration is lazy because it has to be: `for (n in naturals())` over an
/// endless generator has to run, and materialising the items first would
/// simply hang. Eager sources keep the shape they already had -- an array's
/// items are a `Vec` and walking it is an index -- so laziness costs nothing
/// where there is nothing to be lazy about.
pub enum Cursor {
    Items { items: Vec<Value>, next: usize },
    Gen(Rc<Generator>),
}

impl Cursor {
    pub fn items(items: Vec<Value>) -> Cursor {
        Cursor::Items { items, next: 0 }
    }
}

/// The result of stepping a generator, as `next()` and `send()` report it:
/// `{done, value}`.
pub fn step_result(done: bool, value: Value) -> Value {
    let mut map = crate::value::ObjMap::new();
    map.insert(
        crate::value::ObjKey::Str(Rc::from("done")),
        Value::Bool(done),
    );
    map.insert(crate::value::ObjKey::Str(Rc::from("value")), value);
    Value::object(map)
}

/// The error a program gets for resuming a generator that is already running.
pub fn already_running(label: &str, line: Option<u32>) -> Signal {
    crate::error::value_error(format!(
        "{label} is already running; a generator can't be resumed from inside itself."
    ))
    .at(line)
}

/// The error a program gets for iterating a generator that has run out.
pub fn already_iterated(label: &str, line: Option<u32>) -> Signal {
    crate::error::value_error(format!(
        "{label} has already been iterated; a generator can only be used once."
    ))
    .at(line)
}
