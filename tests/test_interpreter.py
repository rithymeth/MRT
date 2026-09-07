from tests.helpers import run_mrt


def test_return_actually_returns_a_value():
    # Regression test: `return` used to be swallowed because the runtime's
    # internal control-flow signal class shared a name with the `Return`
    # AST node, shadowing it so the statement never matched in evaluate().
    output, errors = run_mrt('''
        func add(a, b) {
            return a + b;
        }
        func main() {
            print(add(2, 3));
        }
    ''')
    assert errors == []
    assert output == ["5"]


def test_recursive_function_calls():
    output, errors = run_mrt('''
        func fib(n) {
            if (n <= 1) { return n; }
            return fib(n - 1) + fib(n - 2);
        }
        func main() {
            print(fib(10));
        }
    ''')
    assert errors == []
    assert output == ["55"]


def test_whole_numbers_print_without_trailing_zero():
    output, _ = run_mrt('func main() { print(5); print(2.5); }')
    assert output == ["5", "2.5"]


def test_booleans_print_lowercase():
    output, _ = run_mrt('func main() { print(true); print(false); }')
    assert output == ["true", "false"]


def test_arrays_print_with_clean_numbers():
    output, _ = run_mrt('func main() { print([1, 2, 3]); }')
    assert output == ["[1, 2, 3]"]


def test_not_equal_operator():
    output, _ = run_mrt('func main() { print(3 != 4); print(3 != 3); }')
    assert output == ["true", "false"]


def test_greater_and_less_equal_operators():
    output, _ = run_mrt('''
        func main() {
            print(2 >= 2);
            print(3 >= 2);
            print(1 >= 2);
            print(2 <= 2);
            print(1 <= 2);
            print(3 <= 2);
        }
    ''')
    assert output == ["true", "true", "false", "true", "true", "false"]


def test_while_loop_with_less_equal_condition():
    # This is the exact pattern used in the docs' "count to ten" example;
    # it silently ran zero times before `<=` was implemented.
    output, _ = run_mrt('''
        func main() {
            var i = 1;
            while (i <= 3) {
                print(i);
                i = i + 1;
            }
        }
    ''')
    assert output == ["1", "2", "3"]


def test_string_comparison():
    output, _ = run_mrt('func main() { print("apple" < "banana"); }')
    assert output == ["true"]


def test_modulo_operator():
    output, _ = run_mrt('func main() { print(10 % 3); }')
    assert output == ["1"]


def test_modulo_by_zero_is_a_runtime_error():
    output, _ = run_mrt('func main() { print(10 % 0); }')
    assert len(output) == 1
    assert "Runtime Error" in output[0]


def test_logical_and_or_short_circuit():
    output, _ = run_mrt('''
        func main() {
            print(true && false);
            print(true || false);
            print(false || false);
        }
    ''')
    assert output == ["false", "true", "false"]


def test_logical_not():
    output, _ = run_mrt('func main() { print(!true); print(!false); }')
    assert output == ["false", "true"]


def test_break_exits_loop_early():
    output, _ = run_mrt('''
        func main() {
            for (var i = 0; i < 10; i = i + 1) {
                if (i == 3) { break; }
                print(i);
            }
        }
    ''')
    assert output == ["0", "1", "2"]


def test_continue_still_runs_the_increment():
    output, _ = run_mrt('''
        func main() {
            for (var i = 0; i < 5; i = i + 1) {
                if (i == 2) { continue; }
                print(i);
            }
        }
    ''')
    assert output == ["0", "1", "3", "4"]


def test_continue_in_while_loop():
    output, _ = run_mrt('''
        func main() {
            var i = 0;
            while (i < 5) {
                i = i + 1;
                if (i == 3) { continue; }
                print(i);
            }
        }
    ''')
    assert output == ["1", "2", "4", "5"]


def test_string_escape_sequences_in_output():
    output, _ = run_mrt('func main() { print("a\\nb"); }')
    assert output == ["a\nb"]


def test_string_concatenation_formats_numbers_cleanly():
    output, _ = run_mrt('func main() { print("F(" + 3 + ")"); }')
    assert output == ["F(3)"]


def test_division_by_zero_is_a_runtime_error():
    output, _ = run_mrt('func main() { print(1 / 0); }')
    assert len(output) == 1
    assert "Runtime Error" in output[0]


def test_array_out_of_bounds_is_a_runtime_error():
    output, _ = run_mrt('func main() { var a = [1, 2]; print(a[5]); }')
    assert len(output) == 1
    assert "Runtime Error" in output[0]


def test_undefined_variable_is_a_runtime_error():
    output, _ = run_mrt('func main() { print(doesNotExist); }')
    assert len(output) == 1
    assert "Runtime Error" in output[0]


def test_array_builtins():
    output, _ = run_mrt('''
        func main() {
            var nums = [1, 2, 3];
            push(nums, 4);
            print(nums);
            print(len(nums));
            print(pop(nums));
            print(join(nums, "-"));
            print(indexOf(nums, 2));
        }
    ''')
    assert output == ["[1, 2, 3, 4]", "4", "4", "1-2-3", "1"]


def test_string_builtins():
    output, _ = run_mrt('''
        func main() {
            var text = "  Hello, World!  ";
            print(trim(text));
            print(toUpper(trim(text)));
            print(toLower(trim(text)));
            print(startsWith(trim(text), "Hello"));
            print(contains(text, "World"));
        }
    ''')
    assert output == [
        "Hello, World!",
        "HELLO, WORLD!",
        "hello, world!",
        "true",
        "true",
    ]


def test_print_accepts_multiple_arguments():
    # Every doc example and several bundled .mrt programs call print with
    # multiple comma-separated arguments (e.g. print("Label:", value)).
    output, errors = run_mrt('func main() { print("Sum:", 2 + 3, "!"); }')
    assert errors == []
    assert output == ["Sum: 5 !"]


def test_no_main_function_runs_top_level_statements():
    output, _ = run_mrt('print("no main needed");')
    assert output == ["no main needed"]


def test_closures_capture_outer_variables():
    output, _ = run_mrt('''
        func makeAdder(x) {
            func adder(y) {
                return x + y;
            }
            return adder;
        }
        func main() {
            var addFive = makeAdder(5);
            print(addFive(10));
        }
    ''')
    assert output == ["15"]
