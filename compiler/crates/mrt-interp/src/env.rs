//! Lexical environments.
//!
//! A chain of scopes, resolved by name at run time, exactly as the reference
//! implementation does it. That is deliberate for now: this interpreter's
//! first job is to be *the same language*, and the benchmarks say the chain
//! walk is worth 17% at eight levels of nesting while dispatch is worth far
//! more. Swapping this for the resolver's frame slots is a later, measurable
//! step, not a prerequisite.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::error::{Kind, Signal};
use crate::value::Value;

#[derive(Clone)]
pub struct Env(Rc<Scope>);

struct Scope {
    values: RefCell<HashMap<String, Value>>,
    enclosing: Option<Env>,
}

impl Env {
    pub fn new() -> Env {
        Env(Rc::new(Scope {
            values: RefCell::new(HashMap::new()),
            enclosing: None,
        }))
    }

    pub fn child(&self) -> Env {
        Env(Rc::new(Scope {
            values: RefCell::new(HashMap::new()),
            enclosing: Some(self.clone()),
        }))
    }

    pub fn define(&self, name: &str, value: Value) {
        self.0.values.borrow_mut().insert(name.to_string(), value);
    }

    /// Look a name up, innermost scope first.
    ///
    /// The walk borrows rather than cloning. An `Rc::clone` per level is a
    /// refcount increment and a matching decrement for every scope between
    /// the use and the binding -- paid on every read that is not a frame
    /// slot, which measurement put at a quarter to a third of all variable
    /// operations in recursive and string-heavy code. References cost
    /// nothing and the lifetimes work out, because nothing here outlives the
    /// borrow.
    pub fn get(&self, name: &str, line: Option<u32>) -> Result<Value, Signal> {
        let mut scope = &self.0;
        loop {
            if let Some(value) = scope.values.borrow().get(name) {
                return Ok(value.clone());
            }
            match &scope.enclosing {
                Some(enclosing) => scope = &enclosing.0,
                None => break,
            }
        }
        Err(Signal::error(Kind::NameError, format!("Undefined variable '{name}'.")).at(line))
    }

    /// Assign to an existing binding, innermost scope first.
    ///
    /// One hash lookup and no allocation. This used to ask `contains_key` and
    /// then `insert(name.to_string(), ..)`, which hashed the name twice and
    /// **allocated a fresh String for a key that was already in the map** --
    /// on every assignment to anything not in a frame slot. `get_mut` does
    /// the whole job.
    pub fn assign(&self, name: &str, value: Value, line: Option<u32>) -> Result<(), Signal> {
        let mut scope = &self.0;
        loop {
            if let Some(slot) = scope.values.borrow_mut().get_mut(name) {
                *slot = value;
                return Ok(());
            }
            match &scope.enclosing {
                Some(enclosing) => scope = &enclosing.0,
                None => break,
            }
        }
        Err(Signal::error(Kind::NameError, format!("Undefined variable '{name}'.")).at(line))
    }

    /// The enclosing scope, if any. The VM leaves a block by rebinding its
    /// frame's environment to this rather than by unwinding a Rust frame.
    pub fn parent(&self) -> Option<Env> {
        self.0.enclosing.clone()
    }

    pub fn has_here(&self, name: &str) -> bool {
        self.0.values.borrow().contains_key(name)
    }
}

impl Default for Env {
    fn default() -> Self {
        Env::new()
    }
}
