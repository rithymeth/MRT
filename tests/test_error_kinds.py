"""Tests for error classification (`e.kind`), call-stack traces (`e.stack`),
and guarded `catch (e) if (...)` clauses."""

from src.errors import ERROR_KINDS
from tests.helpers import run_mrt


# -- kind --------------------------------------------------------------------

def test_each_error_kind_is_reported_for_its_own_failure():
    output, errors = run_mrt('''
        func main() {
            var probes = [
                func() { var a = []; return a[5]; },
                func() { return 1 / 0; },
                func() { return undefinedThing; },
                func() { return -"x"; },
                func() { return len(); },
                func() { return sqrt(-1); },
                func() { var o = {a: 1}; return o.missing; }
            ];
            for (p in probes) {
                try { p(); } catch (e) { print(e.kind); }
            }
        }
    ''')
    assert errors == []
    assert output == [
        "IndexError", "ArithmeticError", "NameError", "TypeError",
        "ArityError", "ValueError", "KeyError",
    ]


def test_every_reported_kind_is_in_the_documented_set():
    output, errors = run_mrt('''
        func main() {
            var probes = [
                func() { var a = []; return a[5]; },
                func() { return 1 % 0; },
                func() { return nope; },
                func() { return [1] + 1; },
                func() { return push([]); },
                func() { return range(0, 5, 0); },
                func() { var o = {}; return o.x; },
                func() { return "a" < 1; }
            ];
            for (p in probes) {
                try { p(); } catch (e) { print(e.kind); }
            }
        }
    ''')
    assert errors == []
    assert output, "expected at least one classified error"
    for kind in output:
        assert kind in ERROR_KINDS, f"{kind!r} is not a documented error kind"


def test_a_thrown_value_has_no_kind_of_its_own():
    output, errors = run_mrt('''
        func main() {
            try { throw "plain"; } catch (e) { print(type(e), get(e, "kind", "<none>")); }
            try { throw {kind: "Mine"}; } catch (e) { print(e.kind); }
        }
    ''')
    assert errors == []
    # `kind` is only added to interpreter-raised errors; a thrown value is
    # delivered exactly as written, including one that happens to have a
    # `kind` key of its own.
    assert output == ["string <none>", "Mine"]


# -- stack -------------------------------------------------------------------

def test_stack_is_innermost_first():
    output, errors = run_mrt('''
        func c() { var a = [1]; return a[99]; }
        func b() { return c(); }
        func a() { return b(); }
        func main() {
            try { a(); } catch (e) { print(e.stack); }
        }
    ''')
    assert errors == []
    assert output == ["[c, b, a]"]


def test_stack_is_empty_for_an_error_raised_in_the_catching_frame():
    output, errors = run_mrt('''
        func main() {
            try { var a = []; print(a[0]); } catch (e) { print(e.stack, len(e.stack)); }
        }
    ''')
    assert errors == []
    # The error never crossed a call boundary before being caught.
    assert output == ["[] 0"]


def test_anonymous_frames_are_named_in_the_stack():
    output, errors = run_mrt('''
        func main() {
            var boom = func() { var a = []; return a[1]; };
            try { boom(); } catch (e) { print(e.stack); }
        }
    ''')
    assert errors == []
    assert output == ["[<anonymous>]"]


# -- guards ------------------------------------------------------------------

def test_guards_select_the_first_matching_clause():
    output, errors = run_mrt('''
        func attempt(n) {
            try {
                if (n == 0) { var a = []; print(a[9]); }
                if (n == 1) { print(1 / 0); }
                if (n == 2) { throw {code: "USER"}; }
            }
            catch (e) if (get(e, "kind", "") == "IndexError") { print("index"); }
            catch (e) if (get(e, "kind", "") == "ArithmeticError") { print("math"); }
            catch (e) { print("fallback", get(e, "code", "?")); }
        }
        func main() { for (n in range(3)) { attempt(n); } }
    ''')
    assert errors == []
    assert output == ["index", "math", "fallback USER"]


def test_an_unguarded_clause_acts_as_the_final_else():
    output, errors = run_mrt('''
        func main() {
            try { throw "anything"; }
            catch (e) if (false) { print("skipped"); }
            catch (e) { print("caught", e); }
        }
    ''')
    assert errors == []
    assert output == ["caught anything"]


def test_no_matching_guard_lets_the_error_propagate_and_still_runs_finally():
    output, errors = run_mrt('''
        func main() {
            try {
                try { throw {code: "X"}; }
                catch (e) if (get(e, "code", "") == "Y") { print("never"); }
                finally { print("inner finally"); }
            } catch (outer) {
                print("outer got", outer.code);
            }
        }
    ''')
    assert errors == []
    assert output == ["inner finally", "outer got X"]


def test_no_matching_guard_at_top_level_halts_the_program():
    output, errors = run_mrt('''
        func main() {
            print("before");
            try { throw "boom"; } catch (e) if (false) { print("never"); }
            print("unreachable");
        }
    ''')
    assert errors == []
    assert output == ["before", "Runtime Error: Uncaught boom"]


def test_a_guard_that_raises_propagates_rather_than_being_swallowed():
    output, errors = run_mrt('''
        func main() {
            try {
                try { throw "a plain string"; }
                catch (e) if (e.kind == "IndexError") { print("never"); }
            } catch (outer) {
                print(outer.kind);
            }
        }
    ''')
    assert errors == []
    # `e.kind` on a thrown string indexes a string with a non-number, so the
    # guard itself fails. That error replaces the original -- which is why
    # `get(e, "kind", "")` is the idiom for guards.
    assert output == ["IndexError"]


def test_guard_sees_the_error_binding_but_it_does_not_leak():
    output, errors = run_mrt('''
        func main() {
            var e = "outer";
            try { throw 5; } catch (e) if (e > 3) { print("big", e); }
            print(e);
        }
    ''')
    assert errors == []
    assert output == ["big 5", "outer"]


def test_multiple_catch_clauses_without_guards_is_a_first_wins():
    output, errors = run_mrt('''
        func main() {
            try { throw "x"; }
            catch (e) { print("first"); }
            catch (e) { print("second"); }
        }
    ''')
    assert errors == []
    assert output == ["first"]


def test_missing_key_message_matches_the_playground_quoting():
    output, errors = run_mrt('''
        func main() {
            var o = {a: 1};
            try { print(o.missing); } catch (e) { print(e.message); }
        }
    ''')
    assert errors == []
    # Double quotes, as JSON.stringify produces in the Playground -- Python's
    # repr would use apostrophes and the two interpreters would disagree.
    assert output == ['Key "missing" not found in object.']
