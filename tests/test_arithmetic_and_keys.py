"""Two behaviours where the reference interpreter and the Playground's used to
disagree, found while probing semantics before writing a third implementation.

Neither was caught by the parity suite, because nothing in it exercised a
negative modulo or mixed boolean and numeric object keys."""

from tests.helpers import run_mrt

# -- `%` takes the sign of the dividend --------------------------------------


def test_modulo_takes_the_sign_of_the_dividend():
    output, errors = run_mrt("""
        func main() {
            print(7 % 3, -7 % 3, 7 % -3, -7 % -3);
            print(7.5 % 2, -7.5 % 2);
            print(0 % 5, -0 % 5);
        }
    """)
    assert errors == []
    # As in C, Java, JavaScript, Rust and Go. Python's own `%` takes the sign
    # of the divisor, which is why the reference implementation used to answer
    # `2` for `-7 % 3` while the Playground answered `-1`.
    assert output == ["1 -1 1 -1", "1.5 -1.5", "0 0"]


def test_modulo_by_zero_is_still_an_error():
    output, errors = run_mrt("""
        func main() {
            try { print(1 % 0); } catch (e) { print(e.kind, e.message); }
            try { print(-1 % 0); } catch (e) { print(e.kind, e.message); }
        }
    """)
    assert errors == []
    assert output == [
        "ArithmeticError Modulo by zero.",
        "ArithmeticError Modulo by zero.",
    ]


def test_modulo_still_works_for_the_common_wrapping_idiom():
    output, errors = run_mrt("""
        func main() {
            var out = [];
            for (i in range(0, 7)) { push(out, i % 3); }
            print(out);
        }
    """)
    assert errors == []
    assert output == ["[0, 1, 2, 0, 1, 2, 0]"]


# -- Object keys follow the language's own equality --------------------------


def test_boolean_and_numeric_keys_are_distinct():
    output, errors = run_mrt("""
        func main() {
            var byNumber = {1: "one", 0: "zero"};
            print(get(byNumber, true, "<missing>"), get(byNumber, false, "<missing>"));
            var byBool = {true: "yes", false: "no"};
            print(get(byBool, 1, "<missing>"), get(byBool, 0, "<missing>"));
            print(has(byNumber, true), has(byBool, 1));
        }
    """)
    assert errors == []
    # `==` says `true != 1` at every nesting level, so keys must agree.
    assert output == ["<missing> <missing>", "<missing> <missing>", "false false"]


def test_a_boolean_and_a_number_key_coexist_in_one_object():
    output, errors = run_mrt("""
        func main() {
            var mixed = {1: "number", true: "boolean"};
            print(len(mixed), keys(mixed), values(mixed));
            print(mixed[1], mixed[true]);
            print(mixed);
        }
    """)
    assert errors == []
    # Previously the boolean overwrote the number and this object had one key.
    assert output == [
        "2 [1, true] [number, boolean]",
        "number boolean",
        "{1: number, true: boolean}",
    ]


def test_indexing_a_missing_boolean_key_reports_it():
    output, errors = run_mrt("""
        func main() {
            var byNumber = {1: "one"};
            try { print(byNumber[true]); } catch (e) { print(e.kind, e.message); }
        }
    """)
    assert errors == []
    assert output == ['KeyError Key "true" not found in object.']


def test_boolean_keys_round_trip_through_assignment_and_iteration():
    output, errors = run_mrt("""
        func main() {
            var o = {};
            o[true] = "t";
            o[1] = "n";
            o[false] = "f";
            o[0] = "z";
            print(len(o), keys(o));
            for (k in o) { print(k, type(k), o[k]); }
        }
    """)
    assert errors == []
    assert output == [
        "4 [true, 1, false, 0]",
        "true boolean t",
        "1 number n",
        "false boolean f",
        "0 number z",
    ]
