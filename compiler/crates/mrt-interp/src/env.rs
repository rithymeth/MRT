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

    pub fn get(&self, name: &str, line: Option<u32>) -> Result<Value, Signal> {
        let mut scope = Some(self.clone());
        while let Some(env) = scope {
            if let Some(value) = env.0.values.borrow().get(name) {
                return Ok(value.clone());
            }
            scope = env.0.enclosing.clone();
        }
        Err(Signal::error(Kind::NameError, format!("Undefined variable '{name}'.")).at(line))
    }

    pub fn assign(&self, name: &str, value: Value, line: Option<u32>) -> Result<(), Signal> {
        let mut scope = Some(self.clone());
        while let Some(env) = scope {
            if env.0.values.borrow().contains_key(name) {
                env.0.values.borrow_mut().insert(name.to_string(), value);
                return Ok(());
            }
            scope = env.0.enclosing.clone();
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
