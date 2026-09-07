"""Tests for `throw`, `try`/`catch`/`finally`, and the shape of the error
value a `catch` block receives."""

from tests.helpers import run_mrt


def test_throw_and_catch_a_plain_value():
    output, errors = run_mrt('''
        func main() {
            try {
                throw "boom";
                print("unreachable");
            } catch (e) {
                print("caught", e);
            }
            print("after");
        }
    ''')
    assert errors == []
    assert output == ["caught boom", "after"]


def test_any_value_can_be_thrown():
    output, errors = run_mrt('''
        func main() {
            try { throw {code: 7, msg: "nope"}; } catch (e) { print(e.code, e.msg); }
            try { throw [1, 2]; } catch (e) { print(e); }
            try { throw null; } catch (e) { print(type(e)); }
        }
    ''')
    assert errors == []
    assert output == ["7 nope", "[1, 2]", "null"]


def test_finally_runs_on_normal_completion_and_after_catch():
    output, errors = run_mrt('''
        func main() {
            try { print("body"); } finally { print("f1"); }
            try { throw 1; } catch (e) { print("caught"); } finally { print("f2"); }
        }
    ''')
    assert errors == []
    assert output == ["body", "f1", "caught", "f2"]


def test_finally_runs_when_returning_through_it():
    output, errors = run_mrt('''
        func helper() {
            try { return "value"; } finally { print("cleanup"); }
        }
        func main() { print(helper()); }
    ''')
    assert errors == []
    # The finally block runs before the function actually returns.
    assert output == ["cleanup", "value"]


def test_finally_runs_while_an_uncaught_throw_unwinds():
    output, errors = run_mrt('''
        func main() {
            try {
                try { throw "x"; } finally { print("cleanup"); }
            } catch (e) {
                print("got", e);
            }
        }
    ''')
    assert errors == []
    assert output == ["cleanup", "got x"]


def test_builtin_runtime_errors_are_catchable():
    output, errors = run_mrt('''
        func main() {
            try {
                var a = [1];
                print(a[99]);
            } catch (e) {
                print(type(e));
                print(keys(e));
                print(e.message);
            }
        }
    ''')
    assert errors == []
    assert output[0] == "object"
    assert output[1] == "[message, line]"
    assert "out of bounds" in output[2]


def test_division_by_zero_is_catchable_and_recoverable():
    output, errors = run_mrt('''
        func safeDiv(a, b) {
            try { return a / b; } catch (e) { return null; }
        }
        func main() {
            print(safeDiv(10, 2));
            print(safeDiv(10, 0));
            print("still running");
        }
    ''')
    assert errors == []
    assert output == ["5", "null", "still running"]


def test_undefined_variable_is_catchable():
    output, errors = run_mrt('''
        func main() {
            try { print(nope); } catch (e) { print(e.message); }
        }
    ''')
    assert errors == []
    assert output == ["Undefined variable 'nope'."]


def test_nested_try_and_rethrow():
    output, errors = run_mrt('''
        func main() {
            try {
                try { throw "inner"; } catch (e) { print("first", e); throw "second"; }
            } catch (e) {
                print("outer", e);
            }
        }
    ''')
    assert errors == []
    assert output == ["first inner", "outer second"]


def test_uncaught_throw_halts_the_program():
    output, errors = run_mrt('''
        func main() {
            print("before");
            throw {code: 42};
            print("never");
        }
    ''')
    assert errors == []
    assert output == ["before", "Runtime Error: Uncaught {code: 42}"]


def test_throw_propagates_out_of_a_function_to_an_outer_catch():
    output, errors = run_mrt('''
        func deep() { throw "from deep"; }
        func middle() { deep(); print("never"); }
        func main() {
            try { middle(); } catch (e) { print("caught", e); }
        }
    ''')
    assert errors == []
    assert output == ["caught from deep"]


def test_catch_variable_is_scoped_to_the_catch_block():
    output, errors = run_mrt('''
        func main() {
            var e = "outer";
            try { throw "thrown"; } catch (e) { print(e); }
            print(e);
        }
    ''')
    assert errors == []
    assert output == ["thrown", "outer"]


def test_try_without_catch_or_finally_is_a_parse_error():
    output, errors = run_mrt('func main() { try { print(1); } }')
    assert errors != []
    assert "Expect 'catch' or 'finally'" in errors[0].message


def test_break_still_escapes_a_loop_from_inside_try():
    output, errors = run_mrt('''
        func main() {
            for (i in range(5)) {
                try {
                    if (i == 2) { break; }
                    print(i);
                } finally {
                    print("f", i);
                }
            }
            print("done");
        }
    ''')
    assert errors == []
    assert output == ["0", "f 0", "1", "f 1", "f 2", "done"]
