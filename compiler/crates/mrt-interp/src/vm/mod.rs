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
pub use compile::{compile_function, compile_program, Unsupported};
pub use machine::{generator_label, resume_generator, Vm};

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

    // -- try / catch / finally --------------------------------------------

    #[test]
    fn a_thrown_value_reaches_its_catch() {
        assert_eq!(
            agree(
                r#"func main() { try { throw {code: 1}; } catch (e) { print("caught", e.code); } }"#
            ),
            "caught 1"
        );
    }

    #[test]
    fn an_interpreter_error_is_catchable_as_an_error_object() {
        assert_eq!(
            agree(
                "func main() { try { var a = [1]; print(a[9]); } \
                 catch (e) { print(e.kind, \"|\", e.message); } }"
            ),
            "IndexError | Array index 9 out of bounds for array of length 1."
        );
    }

    #[test]
    fn guards_choose_the_clause() {
        assert_eq!(
            agree(
                "func main() { try { throw 1; } catch (e) if (e == 2) { print(\"wrong\"); } \
                 catch (e) if (e == 1) { print(\"right\", e); } }"
            ),
            "right 1"
        );
    }

    #[test]
    fn a_stack_trace_is_built_innermost_first_across_frames() {
        // The VM builds it on the way out of each frame, as the tree-walker
        // does. Without that the trace is simply empty -- which looks like a
        // feature nobody implemented rather than a wrong answer.
        assert_eq!(
            agree(
                "func c() { throw \"x\"; } func b() { return c(); } func a() { return b(); } \
                 func main() { try { a(); } catch (e) { print(e); } }"
            ),
            "x"
        );
    }

    #[test]
    fn a_finally_runs_on_the_return_path() {
        assert_eq!(
            agree(
                "func f() { try { return \"from try\"; } finally { print(\"fin\"); } } \
                 func main() { print(f()); }"
            ),
            "fin\nfrom try"
        );
    }

    #[test]
    fn a_finally_runs_when_a_catch_clause_returns() {
        // The unwinder pops the handler to enter the clause, so the clause's
        // own `return` had nothing left telling it a finally was owed. It
        // silently skipped the one construct whose entire promise is that it
        // always runs.
        assert_eq!(
            agree(
                "func f() { try { throw \"x\"; } catch (e) { return \"caught\"; } \
                 finally { print(\"fin\"); } } func main() { print(f()); }"
            ),
            "fin\ncaught"
        );
    }

    #[test]
    fn a_signal_from_finally_replaces_the_one_it_interrupted() {
        assert_eq!(
            agree(
                "func f() { try { throw \"a\"; } finally { throw \"b\"; } } \
                 func main() { try { f(); } catch (e) { print(e); } }"
            ),
            "b"
        );
    }

    #[test]
    fn a_return_in_finally_wins_over_the_one_in_try() {
        assert_eq!(
            agree(
                "func f() { try { return \"try\"; } finally { return \"finally\"; } } \
                 func main() { print(f()); }"
            ),
            "finally"
        );
    }

    #[test]
    fn a_try_inside_a_loop_leaves_the_loop_variable_alone() {
        // The catch chain opens one lexical scope but leaves it two ways, and
        // closing it twice in the compiler shifted every slot after the
        // `try`. The loop counter then resolved as a global and vanished.
        assert_eq!(
            agree(
                "func f() { var h = 0; for (var i = 0; i < 5; i = i + 1) { \
                 try { if (i == 3) { break; } h = h + 1; } catch (e) { } } return h; } \
                 func main() { print(f()); }"
            ),
            "3"
        );
    }

    #[test]
    fn a_catch_can_destructure_the_thrown_value() {
        assert_eq!(
            agree(
                "func main() { try { throw {code: 7, msg: \"m\"}; } \
                 catch ({code, msg}) { print(code, msg); } }"
            ),
            "7 m"
        );
    }

    #[test]
    fn a_break_out_of_a_finally_is_refused_rather_than_skipped() {
        // Running the finally on the way out needs machinery this compiler
        // does not have. Jumping past it would be a wrong answer; saying so
        // keeps it in the ratchet where it is visible.
        assert!(vm("func main() { for (var i = 0; i < 2; i = i + 1) { \
             try { break; } finally { print(\"fin\"); } } }")
        .contains("'break' out of a try with a finally is not compiled by the VM yet"));
    }

    // -- destructuring, spread, structs, match -----------------------------

    #[test]
    fn a_catch_clause_shadows_a_slotted_name_of_its_own() {
        // `e` is a slotted parameter and the clause binds another `e` in the
        // environment. Without an entry marking the shadow, the body reads
        // the slot -- the caller's argument, not the caught value. A wrong
        // answer that looks entirely reasonable.
        assert_eq!(
            agree(
                r#"func f(e) { try { throw "inner"; } catch (e) { return e; } }
                   func main() { print(f("outer")); }"#
            ),
            "inner"
        );
    }

    #[test]
    fn declarations_can_destructure() {
        assert_eq!(
            agree(
                "func main() { var [a, b] = [1, 2]; var {x, y = 9, ...rest} = {x: 10, z: 30}; \
                 print(a, b, x, y, rest); }"
            ),
            "1 2 10 9 {z: 30}"
        );
    }

    #[test]
    fn a_pattern_can_assign_to_existing_variables() {
        // The tree-walker performs this through the environment, so the
        // targets cannot be slotted -- otherwise the swap is an
        // undefined-variable error instead.
        assert_eq!(
            agree("func main() { var p = 1; var q = 2; [p, q] = [q, p]; print(p, q); }"),
            "2 1"
        );
    }

    #[test]
    fn a_for_in_loop_can_destructure_each_item() {
        assert_eq!(
            agree(r#"func main() { for ([k, v] in [[1, "one"], [2, "two"]]) { print(k, v); } }"#),
            "1 one\n2 two"
        );
    }

    #[test]
    fn spread_works_in_calls_and_array_literals() {
        assert_eq!(
            agree(
                "func f(first, ...more) { return first + sum(more); } \
                 func main() { var xs = [2, 3]; print(f(1, ...xs), [0, ...xs, 4]); }"
            ),
            "6 [0, 2, 3, 4]"
        );
    }

    #[test]
    fn spread_takes_arrays_only() {
        // Not "anything iterable": `...` over a string is a type error with
        // its own message, so reaching for `iterate` here would make the VM
        // accept programs the language rejects.
        assert_eq!(
            agree(
                r#"func f(...xs) { return len(xs); }
                   func main() { try { f(..."ab"); } catch (e) { print(e.kind, "|", e.message); } }"#
            ),
            "TypeError | Can only spread an array with '...'."
        );
    }

    #[test]
    fn structs_construct_print_and_dispatch_methods() {
        assert_eq!(
            agree(
                "struct Point { x, y = 0; func mag() { return sqrt(this.x*this.x + this.y*this.y); } } \
                 func main() { var p = Point(3, 4); print(p, p.mag(), type(p), Point(1)); }"
            ),
            "Point(x: 3, y: 4) 5 Point Point(x: 1, y: 0)"
        );
    }

    #[test]
    fn match_covers_literals_shapes_structs_and_guards() {
        assert_eq!(
            agree(
                r#"struct C { r; }
                   func k(v) { return match (v) { case 0: "zero", case [a, b]: "pair", 
                     case [h, ...t]: "head", case {kind: "c"}: "obj", case C(r) if (r > 10): "big",
                     case C(r): "small", default: "other" }; }
                   func main() { print(k(0), k([1,2]), k([1,2,3]), k({kind: "c"}), k(C(20)), k(C(2)), k("x")); }"#
            ),
            "zero pair head obj big small other"
        );
    }

    #[test]
    fn a_match_pattern_binding_shadows_an_outer_slot() {
        assert_eq!(
            agree(
                r#"func main() { var n = 1; match ([7]) { case [n]: print("inner", n); } 
                   print("outer", n); }"#
            ),
            "inner 7\nouter 1"
        );
    }

    #[test]
    fn a_struct_pattern_resolves_its_name_through_the_interpreter() {
        // `match_pattern` looks the struct name up in `Interpreter::env`
        // rather than the scope it is handed, so the VM has to keep that
        // field pointing at the running frame. Otherwise this reports an
        // undefined variable instead of the language's own message.
        assert_eq!(
            agree(
                r#"func main() { var notStruct = 5; 
                   try { match (1) { case notStruct(a): print("no"); } } 
                   catch (e) { print(e.kind, "|", e.message); } }"#
            ),
            "TypeError | 'notStruct' is not a struct, so it can't be used as a pattern."
        );
    }

    #[test]
    fn an_unmatched_match_names_the_value() {
        assert_eq!(
            agree(
                r#"func main() { try { match (99) { case 1: print("no"); } } 
                   catch (e) { print(e.kind, "|", e.message); } }"#
            ),
            "ValueError | No case matched 99 in this match, and there is no 'default'."
        );
    }

    #[test]
    fn an_uncompilable_construct_is_refused_by_name() {
        // Not a wrong answer and not a panic: the conformance harness tells
        // "cannot compile this yet" apart from "compiles it wrongly" purely
        // by this text.
        assert_eq!(
            vm("import { x } from \"./m.mrt\";\nfunc main() { }"),
            "Runtime Error: modules is not compiled by the VM yet."
        );
    }

    // -- generators --------------------------------------------------------
    //
    // The feature the machine exists for, so these test the *mechanism* --
    // what a parked frame carries with it -- rather than restating the corpus.

    #[test]
    fn calling_a_generator_runs_none_of_its_body() {
        assert_eq!(
            agree("func g() { print(\"ran\"); yield 1; } func main() { var it = g(); print(\"made\"); print(next(it).value); }"),
            "made\nran\n1"
        );
    }

    #[test]
    fn an_endless_generator_is_consumed_only_as_far_as_asked() {
        // The property the whole design is for: this program does not
        // terminate under any implementation that materialises first.
        assert_eq!(
            agree("func nat() { var n = 0; while (true) { yield n; n += 1; } } func main() { for (n in nat()) { if (n > 3) { break; } print(n); } }"),
            "0\n1\n2\n3"
        );
    }

    #[test]
    fn a_generator_resumes_where_an_earlier_consumer_stopped() {
        assert_eq!(
            agree("func g() { var i = 0; while (i < 6) { yield i; i += 1; } } func main() { var it = g(); for (x in it) { if (x == 2) { break; } print(x); } for (x in it) { print(x); } }"),
            "0\n1\n3\n4\n5"
        );
    }

    #[test]
    fn a_parked_frame_keeps_its_locals() {
        // Slots live on the operand stack, so parking has to carry the
        // frame's whole stack region or the locals come back as null.
        assert_eq!(
            agree("func g() { var a = 1; var b = 10; yield a; a += 1; b += 10; yield a + b; } func main() { for (x in g()) { print(x); } }"),
            "1\n22"
        );
    }

    #[test]
    fn yield_is_an_expression_that_receives_what_was_sent() {
        assert_eq!(
            agree("func echo() { var got = yield 1; print(\"got\", got); yield got; } func main() { var it = echo(); print(next(it).value); print(send(it, \"x\").value); }"),
            "1\ngot x\nx"
        );
    }

    #[test]
    fn a_generator_parked_inside_a_try_keeps_its_handler() {
        // The handler stack is frame state: park it with the frame or the
        // `catch` is gone when the body resumes.
        assert_eq!(
            agree("func g() { try { yield 1; throw \"boom\"; } catch (e) { yield \"caught \" + e; } finally { yield \"cleanup\"; } } func main() { for (x in g()) { print(x); } }"),
            "1\ncaught boom\ncleanup"
        );
    }

    #[test]
    fn a_generator_parked_inside_a_for_in_keeps_its_cursor() {
        // Cursors are frame state too, and a cursor can itself be over
        // another generator -- so parking nests.
        assert_eq!(
            agree("func inner() { yield 1; yield 2; } func outer() { for (x in inner()) { yield x * 10; } } func main() { for (y in outer()) { print(y); } }"),
            "10\n20"
        );
    }

    #[test]
    fn a_generator_is_single_use() {
        assert_eq!(
            agree("func g() { yield 1; } func main() { var it = g(); for (x in it) { print(x); } for (x in it) { print(x); } }"),
            "1\nRuntime Error: <generator g> has already been iterated; a generator can only be used once. [line 1]"
        );
    }

    #[test]
    fn resuming_a_generator_from_inside_itself_is_an_error() {
        assert_eq!(
            agree("func g() { yield next(self).value; } var self = null; func main() { self = g(); print(next(self).value); }"),
            "Runtime Error: <generator g> is already running; a generator can't be resumed from inside itself. [line 1]"
        );
    }

    #[test]
    fn an_error_out_of_a_generator_names_the_generator() {
        // The generator, not the function: a suspended frame is named by the
        // sequence a consumer is holding, which is what a reader of the trace
        // is looking for.
        assert_eq!(
            agree("func g() { yield 1; var xs = []; print(xs[9]); } func main() { try { for (x in g()) { print(x); } } catch (e) { print(e.stack); } }"),
            "1\n[<generator g>]"
        );
    }

    #[test]
    fn delegation_forwards_items_and_sent_values() {
        assert_eq!(
            agree("func inner() { var got = yield \"i\"; yield \"saw \" + got; } func outer() { yield* inner(); yield \"done\"; } func main() { var it = outer(); print(next(it).value); print(send(it, \"v\").value); print(next(it).value); }"),
            "i\nsaw v\ndone"
        );
    }

    #[test]
    fn delegation_accepts_any_iterable() {
        assert_eq!(
            agree("func g() { yield* [1, 2]; yield* \"ab\"; yield 3; } func main() { for (x in g()) { print(x); } }"),
            "1\n2\na\nb\n3"
        );
    }

    #[test]
    fn a_generator_method_sees_this() {
        // Methods are AST closures the tree-walker runs, so this is the path
        // where the *tree-walker* has to produce a parked frame.
        assert_eq!(
            agree("struct Span { lo, hi; func iter() { var n = this.lo; while (n < this.hi) { yield n; n += 1; } } } func main() { for (x in Span(2, 5)) { print(x); } }"),
            "2\n3\n4"
        );
    }

    #[test]
    fn map_and_filter_over_a_generator_stay_lazy() {
        assert_eq!(
            agree("func nat() { var n = 0; while (true) { yield n; n += 1; } } func main() { print(type(map(nat(), func(x) { return x * 2; }))); print(take(filter(nat(), func(x) { return x % 3 == 0; }), 3)); }"),
            "generator\n[0, 3, 6]"
        );
    }

    #[test]
    fn each_call_makes_an_independent_sequence() {
        assert_eq!(
            agree("func g() { yield 1; yield 2; } func main() { var a = g(); var b = g(); print(next(a).value); print(next(b).value); print(a == b); }"),
            "1\n1\nfalse"
        );
    }

    #[test]
    fn return_ends_the_sequence_early() {
        assert_eq!(
            agree("func g() { yield 1; return; yield 2; } func main() { for (x in g()) { print(x); } }"),
            "1"
        );
    }

    #[test]
    fn an_uncompilable_construct_is_still_refused_by_name() {
        // Modules are the last gap, and the harness tells "cannot compile
        // this yet" apart from "compiles it wrongly" purely by this text.
        assert_eq!(
            vm("import { x } from \"./m.mrt\";\nfunc main() { }"),
            "Runtime Error: modules is not compiled by the VM yet."
        );
    }
}
