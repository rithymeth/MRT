"""Tests for first-class functions: anonymous `func(...)` expressions,
closures, and the higher-order built-ins that consume them."""

from tests.helpers import run_mrt


# -- Anonymous function expressions ------------------------------------------

def test_anonymous_function_assigned_and_called():
    output, errors = run_mrt('''
        func main() {
            var double = func(x) { return x * 2; };
            print(double(21));
            print(type(double));
        }
    ''')
    assert errors == []
    assert output == ["42", "function"]


def test_function_passed_as_argument_and_returned():
    output, errors = run_mrt('''
        func apply(f, v) { return f(v); }
        func adder(n) { return func(x) { return x + n; }; }
        func main() {
            print(apply(func(x) { return x + 1; }, 10));
            var add5 = adder(5);
            print(add5(3));
        }
    ''')
    assert errors == []
    assert output == ["11", "8"]


def test_named_declaration_is_also_a_value():
    output, errors = run_mrt('''
        func square(n) { return n * n; }
        func main() {
            var f = square;
            print(f(7));
            print(map([1, 2, 3], square));
        }
    ''')
    assert errors == []
    assert output == ["49", "[1, 4, 9]"]


def test_anonymous_function_arity_is_checked():
    output, errors = run_mrt('''
        func main() {
            var f = func(a, b) { return a + b; };
            print(f(1));
        }
    ''')
    assert errors == []
    assert any("Expected 2 arguments but got 1" in line for line in output)


def test_function_stringifies_with_and_without_a_name():
    output, errors = run_mrt('''
        func named() { return 1; }
        func main() {
            print(toString(named));
            print(toString(func(x) { return x; }));
        }
    ''')
    assert errors == []
    assert output == ["<function named>", "<function>"]


# -- Closures ----------------------------------------------------------------

def test_closure_keeps_private_mutable_state():
    output, errors = run_mrt('''
        func main() {
            var makeCounter = func() {
                var n = 0;
                return func() { n += 1; return n; };
            };
            var a = makeCounter();
            a(); a();
            print(a());
            var b = makeCounter();
            print(b());
        }
    ''')
    assert errors == []
    # Each counter gets its own `n`.
    assert output == ["3", "1"]


def test_closure_captures_defining_scope_not_call_site():
    output, errors = run_mrt('''
        func main() {
            var n = "outer";
            var show = func() { return n; };
            var wrapper = func() {
                var n = "inner";
                return show();
            };
            print(wrapper());
        }
    ''')
    assert errors == []
    assert output == ["outer"]


def test_recursive_anonymous_function_via_its_variable():
    output, errors = run_mrt('''
        func main() {
            var fact = func(n) {
                if (n <= 1) { return 1; }
                return n * fact(n - 1);
            };
            print(fact(5));
        }
    ''')
    assert errors == []
    assert output == ["120"]


# -- Higher-order built-ins --------------------------------------------------

def test_map_filter_reduce():
    output, errors = run_mrt('''
        func main() {
            var xs = [1, 2, 3, 4, 5];
            print(map(xs, func(x) { return x * x; }));
            print(filter(xs, func(x) { return x % 2 == 1; }));
            print(reduce(xs, func(a, b) { return a + b; }));
            print(reduce(xs, func(a, b) { return a + b; }, 100));
        }
    ''')
    assert errors == []
    assert output == ["[1, 4, 9, 16, 25]", "[1, 3, 5]", "15", "115"]


def test_reduce_on_empty_array_needs_an_initial_value():
    output, errors = run_mrt('''
        func main() {
            print(reduce([], func(a, b) { return a + b; }, 0));
            print(reduce([], func(a, b) { return a + b; }));
        }
    ''')
    assert errors == []
    assert output[0] == "0"
    assert any("empty array needs an initial value" in line for line in output)


def test_find_some_every():
    output, errors = run_mrt('''
        func main() {
            var xs = [1, 2, 3];
            print(find(xs, func(x) { return x > 1; }));
            print(find(xs, func(x) { return x > 99; }));
            print(some(xs, func(x) { return x > 2; }));
            print(every(xs, func(x) { return x > 2; }));
        }
    ''')
    assert errors == []
    assert output == ["2", "null", "true", "false"]


def test_sort_default_comparator_and_non_mutation():
    output, errors = run_mrt('''
        func main() {
            var nums = [3, 1, 2];
            print(sort(nums));
            print(nums);
            print(sort(["pear", "apple", "fig"]));
        }
    ''')
    assert errors == []
    assert output == ["[1, 2, 3]", "[3, 1, 2]", "[apple, fig, pear]"]


def test_sort_with_comparator_is_stable():
    output, errors = run_mrt('''
        func main() {
            var recs = [[1, "b"], [0, "a"], [1, "a"]];
            print(sort(recs, func(x, y) { return x[0] - y[0]; }));
            print(sort([1, 2, 3], func(a, b) { return b - a; }));
        }
    ''')
    assert errors == []
    # The two [1, ...] records keep their original relative order.
    assert output == ["[[0, a], [1, b], [1, a]]", "[3, 2, 1]"]


def test_sort_rejects_mixed_types_without_a_comparator():
    output, errors = run_mrt('''
        func main() { print(sort([1, "a"])); }
    ''')
    assert errors == []
    assert any("all numbers or all strings" in line for line in output)


def test_higher_order_builtin_rejects_a_non_function():
    output, errors = run_mrt('''
        func main() { print(map([1, 2], 5)); }
    ''')
    assert errors == []
    assert any("Can only call functions." in line for line in output)


def test_builtins_stringify_without_leaking_the_host_runtime():
    output, errors = run_mrt('''
        func main() {
            print(len);
            print(random(1));
            print([len, map]);
        }
    ''')
    assert errors == []
    # Never a Python repr with a memory address, which would differ between
    # runs and disagree with the Playground interpreter.
    assert output == ["<builtin>", "<builtin>", "[<builtin>, <builtin>]"]
