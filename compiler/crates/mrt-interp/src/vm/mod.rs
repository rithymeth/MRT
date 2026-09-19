//! A bytecode virtual machine for MRT.
//!
//! # Why this exists
//!
//! Not, in the first instance, for speed. It exists because a *suspended*
//! computation needs an explicit instruction pointer, and a tree-walker has
//! none: its state is the Rust call stack, which cannot be parked in a value
//! and resumed later. That is the whole reason the Rust interpreter still
//! refuses generators while both older implementations have them -- Python
//! and JavaScript each borrow a coroutine from their host, and stable Rust
//! has none to borrow.
//!
//! So the feature that is hardest to port is the one that asks for the new
//! execution model, and this is that model: a frame is an `ip`, an
//! environment and a slice of the operand stack. All data. Parking one is
//! copying a struct.
//!
//! # What it deliberately does not own
//!
//! The language. Arithmetic, indexing, iteration, parameter binding,
//! printing and every builtin are delegated to the tree-walking
//! `Interpreter`, so `+` has exactly one meaning and the two engines cannot
//! drift. The machine owns dispatch and control flow; that is all.
//!
//! # What it does not compile yet
//!
//! `try`/`catch`, `throw`, `match`, structs, modules, destructuring, spread
//! -- and generators, which are the point of the exercise and come once the
//! ground under them is complete. The compiler refuses each by name rather
//! than approximating it, so the conformance harness records an unsupported
//! program as unsupported instead of as a wrong answer.

pub mod capture;
pub mod chunk;
pub mod compile;
pub mod machine;

pub use chunk::{Chunk, Op, Proto};
pub use compile::{compile_program, Unsupported};
pub use machine::Vm;

#[cfg(test)]
mod tests {
    use mrt_diagnostics::SourceFile;

    /// Run a program on the VM and return everything it printed, plus its
    /// error text if it stopped early.
    fn vm(source: &str) -> String {
        let file = SourceFile::new("test.mrt", source);
        let outcome = crate::run_vm(&file);
        let mut lines = outcome.output;
        if let Some(error) = outcome.error {
            lines.push(error);
        }
        lines.join("\n")
    }

    /// The same program on the tree-walker. Most VM tests assert against this
    /// rather than against a literal, because the requirement is not "the VM
    /// prints X" -- it is "the VM prints what the other engine prints".
    fn tree(source: &str) -> String {
        let file = SourceFile::new("test.mrt", source);
        let outcome = crate::run(&file);
        let mut lines = outcome.output;
        if let Some(error) = outcome.error {
            lines.push(error);
        }
        lines.join("\n")
    }

    fn agree(source: &str) -> String {
        let from_vm = vm(source);
        assert_eq!(from_vm, tree(source), "engines disagree on: {source}");
        from_vm
    }

    #[test]
    fn arithmetic_variables_and_calls() {
        assert_eq!(
            agree("func add(a, b) { return a + b; } func main() { print(add(2, 3) * 4); }"),
            "20"
        );
    }

    #[test]
    fn a_declaration_binds_its_initializer() {
        // The first thing this compiler got wrong: it read the value off the
        // pattern's destructuring default, so every variable was declared
        // null and every later comparison failed on a type error.
        assert_eq!(agree("func main() { var i = 7; print(i); }"), "7");
    }

    #[test]
    fn logical_operators_yield_the_operand_not_a_boolean() {
        assert_eq!(
            agree(r#"func main() { print(true && "yes", false || "fallback", null && 1); }"#),
            "yes fallback null"
        );
    }

    #[test]
    fn a_break_from_a_nested_block_closes_the_scopes_it_leaves() {
        // A break that skipped its PopScope left the frame one scope deeper
        // every time round, so a later iteration resolved names against a
        // stale chain. Nothing about the printed output says "scope leak",
        // which is exactly why it is pinned here.
        assert_eq!(
            agree(
                "func main() { var hits = 0; \
                 for (var i = 0; i < 5; i = i + 1) { { var inner = i; if (inner > 2) { break; } hits = hits + 1; } } \
                 print(hits); }"
            ),
            "3"
        );
    }

    #[test]
    fn a_for_in_loop_binds_a_fresh_variable_each_iteration() {
        assert_eq!(
            agree(
                "func main() { var fs = []; for (i in [1, 2, 3]) { push(fs, func() { return i; }); } \
                 for (f in fs) { print(f()); } }"
            ),
            "1\n2\n3"
        );
    }

    #[test]
    fn breaking_out_of_a_for_in_retires_its_cursor() {
        assert_eq!(
            agree(
                "func main() { for (a in [1, 2, 3]) { for (b in [10, 20]) { if (b == 20) { break; } \
                 print(a, b); } } }"
            ),
            "1 10\n2 10\n3 10"
        );
    }

    #[test]
    fn a_compiled_closure_is_callable_from_a_builtin() {
        // `map` lives in the shared builtin library and calls back through
        // the tree-walker, so a closure the VM made has to be runnable from
        // there -- otherwise a value is uncallable purely because of which
        // engine built it.
        assert_eq!(
            agree("func main() { print(map([1, 2, 3], func(n) { return n * n; })); }"),
            "[1, 4, 9]"
        );
    }

    #[test]
    fn closures_capture_the_variable_not_its_value() {
        assert_eq!(
            agree("func main() { var n = 1; var f = func() { return n; }; n = 2; print(f()); }"),
            "2"
        );
    }

    #[test]
    fn interpolation_objects_and_indexing() {
        assert_eq!(
            agree(
                r#"func main() { var o = {a: 1}; var xs = [10, 20]; xs[0] = 99; print("${o.a} ${xs[0]} ${len(xs)}"); }"#
            ),
            "1 99 2"
        );
    }

    #[test]
    fn a_runtime_error_reports_the_line_the_other_engine_reports() {
        assert_eq!(
            agree("func main() {\n  var a = [1];\n  print(a[7]);\n}"),
            "Runtime Error: Array index 7 out of bounds for array of length 1. [line 3]"
        );
    }

    // -- frame slots -------------------------------------------------------
    //
    // Slots are where a compiler silently gets scoping wrong, so each of
    // these asserts the two engines agree rather than asserting a literal.

    #[test]
    fn an_inner_declaration_shadows_an_outer_one_and_restores_it() {
        assert_eq!(
            agree(
                r#"func f() { var x = "outer"; { var x = "inner"; print(x); } print(x); return x; }
                   func main() { print(f()); }"#
            ),
            "inner\nouter\nouter"
        );
    }

    #[test]
    fn sibling_blocks_reuse_slots_without_leaking_values() {
        assert_eq!(
            agree(
                "func f() { { var a = 1; print(a); } { var b = 2; print(b); } \
                 { var c = 3; var d = 4; print(c, d); } } func main() { f(); }"
            ),
            "1\n2\n3 4"
        );
    }

    #[test]
    fn a_captured_variable_stays_shared_while_its_neighbour_is_slotted() {
        // `kept` is captured so it must live in the Env where both the frame
        // and the closure see one cell; `plain` next to it is still a slot.
        // Getting this wrong gives the closure a stale copy -- a silent wrong
        // answer rather than a crash.
        assert_eq!(
            agree(
                "func f() { var kept = 0; var g = func() { kept = kept + 1; return kept; }; \
                 var plain = 100; print(g(), g(), plain); return kept; } \
                 func main() { print(f()); }"
            ),
            "1 2 100\n2"
        );
    }

    #[test]
    fn a_captured_parameter_falls_back_to_full_binding() {
        // Capturing a parameter disqualifies the whole fast path: the
        // arguments can no longer just *be* the slots.
        assert_eq!(
            agree(
                "func f(a, b) { var g = func() { return a; }; return g() + b; } \
                 func main() { print(f(5, 6)); }"
            ),
            "11"
        );
    }

    #[test]
    fn defaults_and_rest_still_bind_through_the_tree_walker() {
        assert_eq!(
            agree(
                "func f(a, b = 10, ...rest) { return a + b + len(rest); } \
                 func main() { print(f(1), f(1, 2), f(1, 2, 3, 4)); }"
            ),
            "11 3 5"
        );
    }

    #[test]
    fn a_parameter_can_be_shadowed_by_a_local_of_the_same_name() {
        assert_eq!(
            agree("func f(x) { { var x = x + 1; return x; } } func main() { print(f(41)); }"),
            "42"
        );
    }

    #[test]
    fn break_and_continue_still_work_when_locals_are_slotted() {
        assert_eq!(
            agree(
                "func f() { var hits = 0; for (var i = 0; i < 6; i = i + 1) { \
                 { var t = i * 2; if (t > 6) { break; } if (t == 2) { continue; } hits = hits + t; } } \
                 return hits; } func main() { print(f()); }"
            ),
            "10"
        );
    }

    #[test]
    fn recursion_goes_far_deeper_than_the_tree_walker_can() {
        // Not a conformance claim -- a consequence of the design. MRT frames
        // are heap data here, so depth is bounded by memory rather than by
        // the Rust call stack, which the tree-walker overflows well before
        // this. It is the same property that makes suspension possible.
        assert_eq!(
            vm(
                "func deep(n) { if (n <= 0) { return 0; } return 1 + deep(n - 1); } \
                func main() { print(deep(20000)); }"
            ),
            "20000"
        );
    }

    #[test]
    fn an_uncompilable_construct_is_refused_by_name() {
        // Not a wrong answer and not a panic: the conformance harness tells
        // "cannot compile this yet" apart from "compiles it wrongly" purely
        // by this text.
        assert_eq!(
            vm("func main() { try { print(1); } catch (e) { } }"),
            "Runtime Error: try/catch is not compiled by the VM yet."
        );
        assert_eq!(
            vm("func g() { yield 1; } func main() { }"),
            "Runtime Error: generators is not compiled by the VM yet."
        );
    }
}
