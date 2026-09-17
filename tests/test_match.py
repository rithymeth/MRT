"""Tests for `match` / `case`: literal, array, object and struct patterns,
guards, binding scope, and the no-match rule."""

from tests.helpers import run_mrt


# -- Literal patterns --------------------------------------------------------

def test_literal_patterns():
    output, errors = run_mrt('''
        func classify(v) {
            match (v) {
                case 0: return "zero";
                case -1: return "minus one";
                case 1.5: return "one and a half";
                case "hi": return "greeting";
                case true: return "yes";
                case false: return "no";
                case null: return "nothing";
                case n: return "other";
            }
        }
        func main() {
            for (v in [0, -1, 1.5, "hi", true, false, null, 42]) { print(classify(v)); }
        }
    ''')
    assert errors == []
    assert output == ["zero", "minus one", "one and a half", "greeting",
                      "yes", "no", "nothing", "other"]


def test_literal_patterns_use_structural_equality():
    output, errors = run_mrt('''
        func main() {
            match ([1, [2]]) {
                case n if (n == [1, [2]]): print("equal by value");
                default: print("no");
            }
            // true and 1 are never equal, here as anywhere else.
            match (1) { case true: print("bad"); default: print("1 is not true"); }
        }
    ''')
    assert errors == []
    assert output == ["equal by value", "1 is not true"]


# -- Array patterns ----------------------------------------------------------

def test_array_patterns_require_an_exact_length_without_a_rest():
    output, errors = run_mrt('''
        func f(v) {
            match (v) {
                case []: return "empty";
                case [a]: return "one";
                case [a, b]: return "two";
                case [h, ...t]: return "many+${len(t)}";
                case n: return "not an array";
            }
        }
        func main() {
            for (v in [[], [1], [1, 2], [1, 2, 3], "s"]) { print(f(v)); }
        }
    ''')
    assert errors == []
    assert output == ["empty", "one", "two", "many+2", "not an array"]


def test_array_patterns_bind_and_nest():
    output, errors = run_mrt('''
        func main() {
            match ([1, [2, 3]]) {
                case [a, [b, c]]: print(a, b, c);
                default: print("no");
            }
        }
    ''')
    assert errors == []
    assert output == ["1 2 3"]


# -- Object patterns ---------------------------------------------------------

def test_object_patterns_are_partial():
    output, errors = run_mrt('''
        func f(v) {
            match (v) {
                case {kind: "circle", radius: r}: return "circle ${r}";
                case {kind: k}: return "tagged ${k}";
                case other: return "none";
            }
        }
        func main() {
            print(f({kind: "circle", radius: 5, extra: 1}));
            print(f({kind: "square"}));
            print(f({nope: 1}));
        }
    ''')
    assert errors == []
    # Extra keys are fine; a missing listed key fails the case.
    assert output == ["circle 5", "tagged square", "none"]


def test_object_pattern_shorthand_binds_the_key():
    output, errors = run_mrt('''
        func main() {
            match ({name: "Ada", age: 36}) {
                case {name, age}: print(name, age);
                default: print("no");
            }
        }
    ''')
    assert errors == []
    assert output == ["Ada 36"]


# -- Struct patterns ---------------------------------------------------------

def test_struct_patterns_match_the_exact_struct():
    output, errors = run_mrt('''
        struct Circle { radius; }
        struct Rect { w, h; }
        func area(s) {
            match (s) {
                case Circle(r): return round(3.14159 * r * r, 2);
                case Rect(w, h): return w * h;
                default: return -1;
            }
        }
        func main() { print(area(Circle(2)), area(Rect(3, 4)), area("x")); }
    ''')
    assert errors == []
    assert output == ["12.57 12 -1"]


def test_struct_patterns_can_carry_literals_and_nest():
    output, errors = run_mrt('''
        struct P { x, y; }
        func f(v) {
            match (v) {
                case P(0, 0): return "origin";
                case P(0, y): return "on y at ${y}";
                case P(x, y): return "point ${x},${y}";
                default: return "not a point";
            }
        }
        func main() { print(f(P(0, 0)), "|", f(P(0, 5)), "|", f(P(1, 2)), "|", f(5)); }
    ''')
    assert errors == []
    assert output == ["origin | on y at 5 | point 1,2 | not a point"]


def test_a_struct_pattern_with_the_wrong_field_count_is_an_error():
    output, errors = run_mrt('''
        struct P { x, y; }
        func main() {
            try { match (P(1, 2)) { case P(a): print("no"); default: print("d"); } }
            catch (e) { print(e.kind, e.message); }
        }
    ''')
    assert errors == []
    assert output == [
        "ArityError Pattern for struct P has 1 field(s) but the struct declares 2."
    ]


def test_a_non_struct_name_in_a_struct_pattern_is_an_error():
    output, errors = run_mrt('''
        func main() {
            var notAStruct = 5;
            try { match (1) { case notAStruct(a): print("no"); } }
            catch (e) { print(e.kind, e.message); }
        }
    ''')
    assert errors == []
    assert output == [
        "TypeError 'notAStruct' is not a struct, so it can't be used as a pattern."
    ]


def test_an_object_pattern_also_matches_a_struct_instance():
    output, errors = run_mrt('''
        struct P { x, y; }
        func main() {
            match (P(1, 2)) {
                case {x, y}: print("by field", x, y);
                default: print("no");
            }
        }
    ''')
    assert errors == []
    assert output == ["by field 1 2"]


# -- Guards, ordering and scope ----------------------------------------------

def test_guards_and_first_match_wins():
    output, errors = run_mrt('''
        func size(n) {
            match (n) {
                case x if (x < 0): return "negative";
                case 0: return "zero";
                case x if (x > 100): return "big";
                case x: return "small";
            }
        }
        func main() { for (n in [-5, 0, 500, 7]) { print(size(n)); } }
    ''')
    assert errors == []
    assert output == ["negative", "zero", "big", "small"]


def test_a_failing_guard_falls_through_to_the_next_case():
    output, errors = run_mrt('''
        func main() {
            match (5) {
                case n if (n > 100): print("big");
                case n if (n > 3): print("medium");
                default: print("small");
            }
        }
    ''')
    assert errors == []
    assert output == ["medium"]


def test_bindings_are_scoped_to_their_case():
    output, errors = run_mrt('''
        func main() {
            var n = "outer";
            match ([1, 2]) {
                case [n, m]: print("inside", n, m);
                default: print("no");
            }
            print("after", n);
        }
    ''')
    assert errors == []
    assert output == ["inside 1 2", "after outer"]


def test_there_is_no_fall_through():
    output, errors = run_mrt('''
        func main() {
            match (1) {
                case 1: print("one");
                case 1: print("also one");
                default: print("default");
            }
        }
    ''')
    assert errors == []
    assert output == ["one"]


def test_default_runs_when_nothing_matches():
    output, errors = run_mrt('''
        func main() {
            match ("zzz") { case 1: print("one"); default: print("fallback"); }
        }
    ''')
    assert errors == []
    assert output == ["fallback"]


def test_no_match_without_a_default_is_an_error():
    output, errors = run_mrt('''
        func main() {
            print("before");
            match (99) { case 1: print("one"); }
            print("never");
        }
    ''')
    assert errors == []
    assert output == [
        "before",
        "Runtime Error: No case matched 99 in this match, and there is no 'default'. [line 4]",
    ]


def test_match_works_as_a_statement_inside_loops_and_functions():
    output, errors = run_mrt('''
        func main() {
            var seen = [];
            for (v in [1, "a", [2]]) {
                match (v) {
                    case n if (type(n) == "number"): push(seen, "num");
                    case s if (type(s) == "string"): push(seen, "str");
                    default: push(seen, "other");
                }
            }
            print(seen);
        }
    ''')
    assert errors == []
    assert output == ["[num, str, other]"]


# -- Parse rules -------------------------------------------------------------

def test_default_must_be_last():
    output, errors = run_mrt('func main() { match (1) { default: print("d"); case 1: print("1"); } }')
    assert errors != []
    assert "'default' must be the last clause" in errors[0].message


def test_only_one_default_is_allowed():
    output, errors = run_mrt('func main() { match (1) { default: print("a"); default: print("b"); } }')
    assert errors != []
    assert "only have one 'default'" in errors[0].message


def test_a_match_needs_at_least_one_case():
    output, errors = run_mrt('func main() { match (1) { } }')
    assert errors != []
    assert "needs at least one case" in errors[0].message
