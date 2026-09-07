"""Tests for default parameter values, rest parameters, and `...` spread."""

from tests.helpers import run_mrt


# -- Defaults ----------------------------------------------------------------

def test_defaults_fill_in_from_the_right():
    output, errors = run_mrt('''
        func greet(name, greeting = "Hello", punct = "!") {
            return "${greeting}, ${name}${punct}";
        }
        func main() {
            print(greet("Ada"));
            print(greet("Ada", "Hi"));
            print(greet("Ada", "Hi", "?"));
        }
    ''')
    assert errors == []
    assert output == ["Hello, Ada!", "Hi, Ada!", "Hi, Ada?"]


def test_a_default_can_refer_to_an_earlier_parameter():
    output, errors = run_mrt('''
        func chained(a, b = a * 2, c = a + b) { return [a, b, c]; }
        func main() {
            print(chained(1));
            print(chained(1, 10));
            print(chained(2, 3, 4));
        }
    ''')
    assert errors == []
    assert output == ["[1, 2, 3]", "[1, 10, 11]", "[2, 3, 4]"]


def test_defaults_are_evaluated_at_call_time_not_declaration_time():
    output, errors = run_mrt('''
        func makeTag(n, tag = "n=${n}") { return tag; }
        func fresh(items = []) { push(items, 1); return len(items); }
        func main() {
            print(makeTag(1), makeTag(2));
            // A fresh array each call -- not one shared default object.
            print(fresh(), fresh(), fresh());
        }
    ''')
    assert errors == []
    assert output == ["n=1 n=2", "1 1 1"]


def test_passing_null_explicitly_does_not_trigger_the_default():
    output, errors = run_mrt('''
        func f(a, b = "default") { return b; }
        func main() {
            print(f(1));
            print(f(1, null));
        }
    ''')
    assert errors == []
    assert output == ["default", "null"]


# -- Rest parameters ---------------------------------------------------------

def test_rest_collects_the_remaining_arguments():
    output, errors = run_mrt('''
        func total(label, ...nums) { return "${label}:${sum(nums)}:${len(nums)}"; }
        func main() {
            print(total("none"));
            print(total("three", 1, 2, 3));
        }
    ''')
    assert errors == []
    assert output == ["none:0:0", "three:6:3"]


def test_rest_only_function_and_empty_rest_is_an_array():
    output, errors = run_mrt('''
        func all(...xs) { return xs; }
        func main() {
            print(all(), type(all()), len(all()));
            print(all(1, "a", [2]));
        }
    ''')
    assert errors == []
    assert output == ["[] array 0", "[1, a, [2]]"]


def test_rest_combines_with_defaults():
    output, errors = run_mrt('''
        func f(a, b = 10, ...rest) { return [a, b, rest]; }
        func main() {
            print(f(1));
            print(f(1, 2));
            print(f(1, 2, 3, 4));
        }
    ''')
    assert errors == []
    assert output == ["[1, 10, []]", "[1, 2, []]", "[1, 2, [3, 4]]"]


# -- Spread ------------------------------------------------------------------

def test_spread_in_calls():
    output, errors = run_mrt('''
        func total(...nums) { return sum(nums); }
        func main() {
            var a = [1, 2, 3];
            print(total(...a));
            print(total(10, ...a, 20));
            print(total(...a, ...a));
            print(total(...[]));
        }
    ''')
    assert errors == []
    assert output == ["6", "36", "12", "0"]


def test_spread_in_array_literals():
    output, errors = run_mrt('''
        func main() {
            var a = [1, 2];
            var b = [3, 4];
            print([...a, ...b]);
            print([0, ...a, 99]);
            print([...[]]);
            print([...a] == a);
        }
    ''')
    assert errors == []
    assert output == ["[1, 2, 3, 4]", "[0, 1, 2, 99]", "[]", "true"]


def test_spread_in_print():
    output, errors = run_mrt('''
        func main() {
            var v = [1, 2, 3];
            print(...v);
            print("x:", ...v, "end");
        }
    ''')
    assert errors == []
    assert output == ["1 2 3", "x: 1 2 3 end"]


def test_spread_copies_rather_than_aliases():
    output, errors = run_mrt('''
        func main() {
            var a = [1, 2];
            var copy = [...a];
            push(copy, 3);
            print(a, copy);
        }
    ''')
    assert errors == []
    assert output == ["[1, 2] [1, 2, 3]"]


def test_spreading_a_non_array_is_a_type_error():
    output, errors = run_mrt('''
        func f(...xs) { return len(xs); }
        func main() {
            for (bad in [1]) { }
            try { f(...5); } catch (e) { print(e.kind, e.message); }
            try { f(..."ab"); } catch (e) { print(e.kind); }
            try { f(...{a: 1}); } catch (e) { print(e.kind); }
        }
    ''')
    assert errors == []
    assert output[0] == "TypeError Can only spread an array with '...'."
    assert output[1:] == ["TypeError", "TypeError"]


# -- Arity -------------------------------------------------------------------

def test_arity_message_describes_the_accepted_range():
    output, errors = run_mrt('''
        func exact(a, b) { return 0; }
        func defaulted(a, b = 1, c = 2) { return 0; }
        func variadic(a, ...r) { return 0; }
        func main() {
            try { exact(1); } catch (e) { print(e.message); }
            try { defaulted(); } catch (e) { print(e.message); }
            try { defaulted(1, 2, 3, 4); } catch (e) { print(e.message); }
            try { variadic(); } catch (e) { print(e.message); }
        }
    ''')
    assert errors == []
    assert output == [
        "Expected 2 arguments but got 1.",
        "Expected between 1 and 3 arguments but got 0.",
        "Expected between 1 and 3 arguments but got 4.",
        "Expected at least 1 arguments but got 0.",
    ]


# -- Shape rules (parse errors) ----------------------------------------------

def test_rest_must_be_last():
    output, errors = run_mrt('func f(...a, b) { } func main() { }')
    assert errors != []
    assert "rest parameter must be the last parameter" in errors[0].message


def test_required_cannot_follow_a_default():
    output, errors = run_mrt('func f(a = 1, b) { } func main() { }')
    assert errors != []
    assert "required parameter can't follow one with a default" in errors[0].message


def test_rest_cannot_have_a_default():
    output, errors = run_mrt('func f(...a = 1) { } func main() { }')
    assert errors != []
    assert "rest parameter can't have a default value" in errors[0].message


def test_spread_is_not_a_general_expression():
    output, errors = run_mrt('func main() { var x = ...[1]; }')
    assert errors != []
