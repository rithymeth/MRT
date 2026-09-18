//! Runtime failures, and the non-local control flow the interpreter uses.
//!
//! Python and TypeScript both express `break`, `continue`, `return` and
//! `throw` as exceptions. Rust has no exceptions, so they travel in the error
//! channel of a `Result` instead -- which is not a workaround but an
//! improvement: the type of every `execute` call now says that it might not
//! fall through, and the compiler will not let a new statement kind forget to
//! propagate it.

use crate::value::Value;

/// The closed set of error kinds a program can branch on from a `catch`
/// guard. Deliberately small: catching "any bad index" should not mean
/// enumerating every built-in that can produce one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    TypeError,
    ArityError,
    IndexError,
    KeyError,
    NameError,
    ValueError,
    ArithmeticError,
    RuntimeError,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::TypeError => "TypeError",
            Kind::ArityError => "ArityError",
            Kind::IndexError => "IndexError",
            Kind::KeyError => "KeyError",
            Kind::NameError => "NameError",
            Kind::ValueError => "ValueError",
            Kind::ArithmeticError => "ArithmeticError",
            Kind::RuntimeError => "RuntimeError",
        }
    }
}

/// An interpreter-raised failure, as distinct from a value the program threw.
#[derive(Clone, Debug)]
pub struct RuntimeError {
    pub message: String,
    pub line: Option<u32>,
    pub kind: Kind,
    /// The MRT-level call stack, innermost first. Filled in as the error
    /// propagates outward rather than captured at the raise site, because
    /// only the callers know their own names.
    pub stack: Vec<String>,
}

impl RuntimeError {
    pub fn new(kind: Kind, message: impl Into<String>) -> Self {
        RuntimeError {
            message: message.into(),
            line: None,
            kind,
            stack: Vec::new(),
        }
    }

    pub fn at(mut self, line: Option<u32>) -> Self {
        if self.line.is_none() {
            self.line = line;
        }
        self
    }

    /// The text `python -m src` prints for this error.
    pub fn render(&self) -> String {
        match self.line {
            Some(line) => format!("Runtime Error: {} [line {line}]", self.message),
            None => format!("Runtime Error: {}", self.message),
        }
    }
}

/// A value thrown by `throw`, unwinding until a `catch` takes it.
#[derive(Clone, Debug)]
pub struct Thrown {
    pub value: Value,
    pub stack: Vec<String>,
}

/// Everything that can interrupt ordinary execution.
#[derive(Clone, Debug)]
pub enum Signal {
    Error(RuntimeError),
    Throw(Thrown),
    Break,
    Continue,
    Return(Value),
}

impl Signal {
    pub fn error(kind: Kind, message: impl Into<String>) -> Signal {
        Signal::Error(RuntimeError::new(kind, message))
    }

    /// Attach a call frame as the signal passes outward through a function.
    pub fn with_frame(mut self, frame: &str) -> Signal {
        match &mut self {
            Signal::Error(e) => e.stack.push(frame.to_string()),
            Signal::Throw(t) => t.stack.push(frame.to_string()),
            _ => {}
        }
        self
    }

    /// Attach a line to an error that does not have one yet.
    pub fn at(mut self, line: Option<u32>) -> Signal {
        if let Signal::Error(e) = &mut self {
            if e.line.is_none() {
                e.line = line;
            }
        }
        self
    }
}

impl From<RuntimeError> for Signal {
    fn from(e: RuntimeError) -> Signal {
        Signal::Error(e)
    }
}

pub type Exec = Result<(), Signal>;
pub type Eval = Result<Value, Signal>;

pub fn type_error(message: impl Into<String>) -> Signal {
    Signal::error(Kind::TypeError, message)
}

pub fn arity_error(message: impl Into<String>) -> Signal {
    Signal::error(Kind::ArityError, message)
}

pub fn value_error(message: impl Into<String>) -> Signal {
    Signal::error(Kind::ValueError, message)
}
