"""Tests for the syntax ergonomics added alongside first-class functions:
the `null` literal, string interpolation, bareword object keys, and
`for (x in ...)` iteration."""

import pytest

from src.errors import MRTSyntaxError
from src.lexer import Lexer
from tests.helpers import run_mrt


# -- null literal ------------------------------------------------------------

def test_null_literal_type_and_equality():
    output, errors = run_mrt('''
        func main() {
            var n = null;
            print(n);
            print(type(n));
            print(n == null, n != null, n == 0, n == false);
        }
    ''')
    assert errors == []
    assert output == ["null", "null", "true false false false"]


def test_null_is_falsy_and_everything_else_is_not():
    output, errors = run_mrt('''
        func main() {
            if (null) { print("t"); } else { print("f"); }
            if (0) { print("t"); } else { print("f"); }
            if ("") { print("t"); } else { print("f"); }
        }
    ''')
    assert errors == []
    assert output == ["f", "t", "t"]


def test_null_round_trips_through_arrays_and_objects():
    output, errors = run_mrt('''
        func main() {
            var d = {a: null};
            print(d, has(d, "a"), d.a == null);
            print([null, 1]);
        }
    ''')
    assert errors == []
    assert output == ["{a: null} true true", "[null, 1]"]


# -- String interpolation ----------------------------------------------------

def test_interpolation_of_variables_and_expressions():
    output, errors = run_mrt('''
        func main() {
            var name = "Ada";
            print("Hi ${name}, ${1 + 2} times!");
            print("${name}");
            print("no interpolation here");
        }
    ''')
    assert errors == []
    assert output == ["Hi Ada, 3 times!", "Ada", "no interpolation here"]


def test_interpolation_renders_values_like_print_does():
    output, errors = run_mrt('''
        func main() {
            print("${[1, 2]} ${ {a: 1} } ${null} ${true} ${1.5}");
        }
    ''')
    assert errors == []
    assert output == ["[1, 2] {a: 1} null true 1.5"]


def test_interpolation_handles_nested_braces_strings_and_calls():
    output, errors = run_mrt('''
        func main() {
            print("${ get({"a": 5}, "a") }");
            print("${ func(x) { return x + 1; }(41) }");
            print("${ "}" }");
        }
    ''')
    assert errors == []
    assert output == ["5", "42", "}"]


def test_escaped_interpolation_is_literal_text():
    output, errors = run_mrt(r'''
        func main() {
            var name = "Ada";
            print("literal \${name}");
        }
    ''')
    assert errors == []
    assert output == ["literal ${name}"]


def test_empty_interpolation_is_a_parse_error():
    output, errors = run_mrt('func main() { print("${}"); }')
    assert errors != []
    assert "Empty interpolation" in errors[0].message


def test_unterminated_interpolation_is_a_syntax_error():
    # Lexer failures raise rather than collecting into parser.errors.
    with pytest.raises(MRTSyntaxError):
        Lexer('func main() { print("${1 + 2"); }').scan_tokens()


# -- Bareword object keys ----------------------------------------------------

def test_bareword_keys_are_string_keys():
    output, errors = run_mrt('''
        func main() {
            var p = {name: "Ada", age: 36};
            print(p);
            print(p.name, p["age"]);
            print(keys(p));
        }
    ''')
    assert errors == []
    assert output == ["{name: Ada, age: 36}", "Ada 36", "[name, age]"]


def test_bareword_key_does_not_read_a_same_named_variable():
    output, errors = run_mrt('''
        func main() {
            var name = "shadow";
            print({name: 1});
            print({(name): 1});
        }
    ''')
    assert errors == []
    # Bareword is the literal key; parenthesise to use the variable's value.
    assert output == ["{name: 1}", "{shadow: 1}"]


def test_quoted_and_computed_keys_still_work():
    output, errors = run_mrt('''
        func main() {
            var k = "dyn";
            print({"quoted": 1, (k): 2, (1 + 1): 3});
        }
    ''')
    assert errors == []
    assert output == ["{quoted: 1, dyn: 2, 2: 3}"]


# -- for-in ------------------------------------------------------------------

def test_for_in_over_array_string_and_object():
    output, errors = run_mrt('''
        func main() {
            for (x in [10, 20]) { print("e", x); }
            for (var c in "hi") { print("c", c); }
            for (k in {a: 1, b: 2}) { print("k", k); }
        }
    ''')
    assert errors == []
    assert output == ["e 10", "e 20", "c h", "c i", "k a", "k b"]


def test_for_in_supports_break_and_continue():
    output, errors = run_mrt('''
        func main() {
            for (x in [1, 2, 3, 4]) {
                if (x == 2) { continue; }
                if (x == 4) { break; }
                print(x);
            }
        }
    ''')
    assert errors == []
    assert output == ["1", "3"]


def test_for_in_binds_a_fresh_variable_per_iteration():
    output, errors = run_mrt('''
        func main() {
            var fns = [];
            for (x in [1, 2, 3]) { push(fns, func() { return x; }); }
            print(map(fns, func(f) { return f(); }));
        }
    ''')
    assert errors == []
    # Each closure captured its own iteration's value, not a shared slot.
    assert output == ["[1, 2, 3]"]


def test_for_in_loop_variable_does_not_leak():
    output, errors = run_mrt('''
        func main() {
            for (x in [1]) { print(x); }
            print(x);
        }
    ''')
    assert errors == []
    assert output[0] == "1"
    assert any("Undefined variable 'x'." in line for line in output)


def test_for_in_rejects_a_non_iterable():
    output, errors = run_mrt('func main() { for (x in 5) { print(x); } }')
    assert errors == []
    assert any("Can only iterate over an array, string, or object." in line for line in output)


def test_c_style_for_loop_still_parses():
    output, errors = run_mrt('''
        func main() {
            for (var i = 0; i < 3; i += 1) { print(i); }
        }
    ''')
    assert errors == []
    assert output == ["0", "1", "2"]
