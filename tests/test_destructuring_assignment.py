"""Tests for `[a, b] = pair;` and `{x, y} = point;` -- assignment through a
pattern to variables that already exist, and the block/array-literal
ambiguities the parser has to see past."""

from tests.helpers import run_mrt


def test_array_and_object_destructuring_assignment():
    output, errors = run_mrt("""
        func main() {
            var a = 1; var b = 2;
            [a, b] = [b, a];
            print(a, b);
            var x = 0; var y = 0;
            {x, y} = {x: 5, y: 6};
            print(x, y);
        }
    """)
    assert errors == []
    assert output == ["2 1", "5 6"]


def test_it_nests_renames_defaults_and_rests():
    output, errors = run_mrt("""
        func main() {
            var a = 0; var b = 0; var c = 0; var others = {};
            [a, [b, c]] = [1, [2, 3]];
            print(a, b, c);
            {name: a, ...others} = {name: "Ada", x: 1, y: 2};
            print(a, others);
            [b, c = 9] = [7];
            print(b, c);
            var rest = [];
            [a, ...rest] = [1, 2, 3];
            print(a, rest);
        }
    """)
    assert errors == []
    assert output == ["1 2 3", "Ada {x: 1, y: 2}", "7 9", "1 [2, 3]"]


def test_it_assigns_outward_rather_than_declaring():
    output, errors = run_mrt("""
        func main() {
            var a = 1; var b = 2;
            func inner() { [a, b] = [10, 20]; }
            inner();
            print(a, b);
            for (i in [0]) { [a] = [99]; }
            print(a);
        }
    """)
    assert errors == []
    # The pattern assigned through to `main`'s variables; it did not shadow
    # them with new bindings in the inner scopes.
    assert output == ["10 20", "99"]


def test_an_undeclared_name_in_the_pattern_is_an_error():
    output, errors = run_mrt("func main() { [nope] = [1]; }")
    assert errors == []
    assert any("Undefined variable 'nope'." in line for line in output)


def test_shape_mismatches_report_the_same_errors_as_a_declaration():
    output, errors = run_mrt("""
        func main() {
            var a = 0;
            try { [a, a, a] = [1]; } catch (e) { print(e.kind, e.message); }
            try { {k} = {}; } catch (e) { print(e.kind, e.message); }
            try { [a] = 5; } catch (e) { print(e.kind, e.message); }
        }
    """)
    assert errors == []
    assert output == [
        "IndexError Cannot destructure: the array has 1 element(s) "
        "but the pattern needs at least 3.",
        'KeyError Cannot destructure: no key "k" in the object.',
        "TypeError Cannot destructure number with an array pattern.",
    ]


def test_a_leading_brace_is_still_a_block():
    output, errors = run_mrt("""
        func main() {
            var x = 1;
            { x = 2; }
            print(x);
            { print("block"); }
            if (true) { x = 3; }
            print(x);
        }
    """)
    assert errors == []
    assert output == ["2", "block", "3"]


def test_a_leading_bracket_is_still_an_array_expression():
    output, errors = run_mrt("""
        func main() {
            var arr = [1, 2];
            [1, 2][0];
            print(arr[0]);
            print([arr][0][1]);
        }
    """)
    assert errors == []
    assert output == ["1", "2"]


def test_a_destructuring_assignment_can_receive_a_yielded_value():
    output, errors = run_mrt("""
        func pair() {
            var a = 0; var b = 0;
            [a, b] = yield "give me two";
            print("pair", a, b);
        }
        func main() { var g = pair(); next(g); send(g, [7, 8]); }
    """)
    assert errors == []
    assert output == ["pair 7 8"]
