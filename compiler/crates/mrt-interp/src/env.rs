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

    /// `get`, but starting `hint` scopes up when a previous lookup at the
    /// same bytecode position already found the answer there.
    ///
    /// Sound because MRT's scoping is static: which names a given block can
    /// bind is fixed by its source text, never by which branch ran to reach
    /// it, so the hop count for one fixed *bytecode position* to reach a
    /// non-slotted name is the same on every execution of that instruction.
    /// This is what lets the VM skip the run of failed hash lookups through
    /// intermediate scopes that a global or an outer closure variable
    /// otherwise costs on every read.
    ///
    /// Still just a fast path, not a proof: a hint that misses falls back to
    /// the ordinary walk from the top rather than reporting a wrong answer,
    /// so a bug in that reasoning costs speed, never correctness. Returns how
    /// many hops it actually took, so the caller can update its cache.
    /// **Correctness here relies on a contract `Env` cannot itself check**:
    /// a `hint` must only ever be the value this call site's own *previous*
    /// call to `get_hinted` returned, never a hint learned at some other
    /// call site or for some other name. `Env` trusts a hit at the named
    /// scope immediately, and if that scope happens to bind the same name
    /// for an unrelated reason, this returns that binding -- correctly by
    /// its own rules, wrongly if the contract was broken to get there. The
    /// VM upholds it by keeping one cache slot per bytecode instruction (see
    /// `Chunk::var_hints`), so a hint is never read back anywhere but the
    /// exact site that wrote it.
    pub fn get_hinted(
        &self,
        name: &str,
        hint: i32,
        line: Option<u32>,
    ) -> Result<(Value, i32), Signal> {
        if hint >= 0 {
            if let Some(scope) = self.up(hint as u32) {
                if let Some(value) = scope.values.borrow().get(name) {
                    return Ok((value.clone(), hint));
                }
            }
        }
        let mut scope = &self.0;
        let mut hops = 0i32;
        loop {
            if let Some(value) = scope.values.borrow().get(name) {
                return Ok((value.clone(), hops));
            }
            match &scope.enclosing {
                Some(enclosing) => {
                    scope = &enclosing.0;
                    hops += 1;
                }
                None => break,
            }
        }
        Err(Signal::error(Kind::NameError, format!("Undefined variable '{name}'.")).at(line))
    }

    /// `assign`, with the same hinting as `get_hinted` and for the same
    /// reason.
    pub fn assign_hinted(
        &self,
        name: &str,
        value: Value,
        hint: i32,
        line: Option<u32>,
    ) -> Result<i32, Signal> {
        if hint >= 0 {
            if let Some(scope) = self.up(hint as u32) {
                if let Some(slot) = scope.values.borrow_mut().get_mut(name) {
                    *slot = value;
                    return Ok(hint);
                }
            }
        }
        let mut scope = &self.0;
        let mut hops = 0i32;
        loop {
            if let Some(slot) = scope.values.borrow_mut().get_mut(name) {
                *slot = value;
                return Ok(hops);
            }
            match &scope.enclosing {
                Some(enclosing) => {
                    scope = &enclosing.0;
                    hops += 1;
                }
                None => break,
            }
        }
        Err(Signal::error(Kind::NameError, format!("Undefined variable '{name}'.")).at(line))
    }

    /// The scope `hops` levels out, or `None` if the chain is shorter than
    /// that -- which only a stale hint should ever produce.
    fn up(&self, hops: u32) -> Option<&Scope> {
        let mut scope = &self.0;
        for _ in 0..hops {
            scope = &scope.enclosing.as_ref()?.0;
        }
        Some(scope)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::values_equal;

    fn chain(depth: usize) -> Env {
        let mut env = Env::new();
        for _ in 0..depth {
            env = env.child();
        }
        env
    }

    #[test]
    fn a_hint_of_minus_one_does_the_ordinary_walk() {
        let outer = Env::new();
        outer.define("x", Value::Number(1.0));
        let inner = outer.child();
        let (value, hops) = inner.get_hinted("x", -1, None).unwrap();
        assert!(values_equal(&value, &Value::Number(1.0)));
        assert_eq!(hops, 1, "one hop from inner to outer");
    }

    #[test]
    fn a_correct_hint_is_used_as_is() {
        let outer = Env::new();
        outer.define("x", Value::Number(7.0));
        let inner = outer.child().child().child();
        // Found the slow way first, to learn the real hop count.
        let (_, hops) = inner.get_hinted("x", -1, None).unwrap();
        assert_eq!(hops, 3);
        // Then used directly: passing it back must not need to walk past it.
        let (value, hops_again) = inner.get_hinted("x", hops, None).unwrap();
        assert!(values_equal(&value, &Value::Number(7.0)));
        assert_eq!(
            hops_again, hops,
            "a correct hint is reported back unchanged"
        );
    }

    #[test]
    fn a_hint_naming_a_scope_with_no_such_binding_falls_back() {
        // The defensive case `get_hinted` actually protects against: a hint
        // that lands on a real scope in the chain, just not one that binds
        // this name (the scope in between has nothing called "x" at all).
        // Correctness cannot depend on every hint always being exactly
        // right -- only on a wrong one never being *silently* accepted.
        let outer = Env::new();
        outer.define("x", Value::Number(9.0));
        let middle = outer.child(); // binds nothing
        let inner = middle.child();

        // The true hop count to reach x is 2. Lie and say 1 (middle, which
        // has no x at all).
        let (value, hops) = inner.get_hinted("x", 1, None).unwrap();
        assert!(
            values_equal(&value, &Value::Number(9.0)),
            "still found outer's x"
        );
        assert_eq!(hops, 2, "and the corrected hop count was reported");
    }

    #[test]
    fn a_hint_longer_than_the_chain_does_not_panic() {
        let env = chain(2);
        env.define("x", Value::Number(3.0));
        // The chain is only 2 deep; a hint of 50 has nowhere to land.
        let (value, hops) = env.get_hinted("x", 50, None).unwrap();
        assert!(values_equal(&value, &Value::Number(3.0)));
        assert_eq!(hops, 0);
    }

    #[test]
    fn assign_hinted_updates_through_a_correct_hint_with_no_allocation_path() {
        let outer = Env::new();
        outer.define("x", Value::Number(1.0));
        let inner = outer.child().child();
        let (_, hops) = inner.get_hinted("x", -1, None).unwrap();

        let reported = inner
            .assign_hinted("x", Value::Number(2.0), hops, None)
            .unwrap();
        assert_eq!(reported, hops);
        assert!(values_equal(
            &outer.get("x", None).unwrap(),
            &Value::Number(2.0)
        ));
    }

    #[test]
    fn assign_hinted_self_heals_from_a_stale_hint() {
        let outer = Env::new();
        outer.define("x", Value::Number(1.0));
        let inner = outer.child();

        // A hint of 5 does not exist in a 1-deep chain: assign must still
        // land on outer's x rather than erroring or silently doing nothing.
        let hops = inner
            .assign_hinted("x", Value::Number(2.0), 5, None)
            .unwrap();
        assert_eq!(hops, 1);
        assert!(values_equal(
            &outer.get("x", None).unwrap(),
            &Value::Number(2.0)
        ));
    }

    #[test]
    fn an_undefined_name_is_still_an_error_with_or_without_a_hint() {
        let env = Env::new();
        assert!(env.get_hinted("nope", -1, None).is_err());
        assert!(env.get_hinted("nope", 3, None).is_err());
        assert!(env.assign_hinted("nope", Value::Null, -1, None).is_err());
    }

    #[test]
    fn get_hinted_only_ever_looks_at_the_scope_the_hint_names() {
        // Not "self-healing" this time -- the opposite case, spelled out so
        // the limit is on the record. A hint that happens to land on a real
        // binding of the right *name* is trusted immediately, even if it is
        // not the binding a fresh walk from here would have found first.
        // `get_hinted` has no way to tell a right hint from a coincidentally
        // matching wrong one; that guarantee belongs one level up, in the
        // VM, which never hands a hint learned at one bytecode position to
        // a lookup at another.
        let outer = Env::new();
        outer.define("x", Value::Number(1.0));
        let inner = outer.child();
        inner.define("x", Value::Number(2.0));

        let (value, hops) = inner.get_hinted("x", 1, None).unwrap();
        assert!(
            values_equal(&value, &Value::Number(1.0)),
            "the hint said one hop out, and outer's x is exactly one hop out"
        );
        assert_eq!(hops, 1);
    }
}
