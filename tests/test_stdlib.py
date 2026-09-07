"""Tests for the expanded standard library: sequence helpers, string
padding/repetition, and the seeded random generator."""

from tests.helpers import run_mrt


# -- Sequences ---------------------------------------------------------------

def test_reverse_on_arrays_and_strings():
    output, errors = run_mrt('''
        func main() {
            var a = [1, 2, 3];
            print(reverse(a));
            print(a);
            print(reverse("abc"));
        }
    ''')
    assert errors == []
    # reverse() returns a new array; the original is untouched.
    assert output == ["[3, 2, 1]", "[1, 2, 3]", "cba"]


def test_unique_uses_structural_equality_and_keeps_first_order():
    output, errors = run_mrt('''
        func main() {
            print(unique([3, 1, 3, 2, 1]));
            print(unique([[1], [1], [2]]));
            print(unique([]));
        }
    ''')
    assert errors == []
    assert output == ["[3, 1, 2]", "[[1], [2]]", "[]"]


def test_flatten_defaults_to_one_level():
    output, errors = run_mrt('''
        func main() {
            print(flatten([[1], [2, [3]]]));
            print(flatten([[1], [2, [3]]], 2));
            print(flatten([[1], [2]], 0));
        }
    ''')
    assert errors == []
    assert output == ["[1, 2, [3]]", "[1, 2, 3]", "[[1], [2]]"]


def test_zip_truncates_to_the_shorter_input():
    output, errors = run_mrt('''
        func main() {
            print(zip([1, 2, 3], ["a", "b"]));
            print(zip([], [1]));
        }
    ''')
    assert errors == []
    assert output == ["[[1, a], [2, b]]", "[]"]


def test_enumerate_pairs_index_with_value():
    output, errors = run_mrt('func main() { print(enumerate(["x", "y"])); }')
    assert errors == []
    assert output == ["[[0, x], [1, y]]"]


def test_count_and_sum():
    output, errors = run_mrt('''
        func main() {
            print(count([1, 1, 2], 1));
            print(count([[1], [1]], [1]));
            print(sum([1, 2, 3.5]));
            print(sum([]));
        }
    ''')
    assert errors == []
    assert output == ["2", "2", "6.5", "0"]


def test_range_variants():
    output, errors = run_mrt('''
        func main() {
            print(range(3));
            print(range(2, 5));
            print(range(0, 10, 3));
            print(range(3, 0, -1));
            print(range(0));
        }
    ''')
    assert errors == []
    assert output == ["[0, 1, 2]", "[2, 3, 4]", "[0, 3, 6, 9]", "[3, 2, 1]", "[]"]


def test_range_rejects_a_zero_step():
    output, errors = run_mrt('func main() { print(range(0, 5, 0)); }')
    assert errors == []
    assert any("step must not be zero" in line for line in output)


# -- Strings -----------------------------------------------------------------

def test_repeat():
    output, errors = run_mrt('''
        func main() {
            print(repeat("ab", 3));
            print(repeat("x", 0));
        }
    ''')
    assert errors == []
    assert output == ["ababab", ""]


def test_pad_start_and_end():
    output, errors = run_mrt('''
        func main() {
            print(padStart("7", 3, "0"));
            print(padEnd("7", 3, "."));
            print(padStart("x", 5, "ab"));
            print(padStart("toolong", 2));
            print(padStart("5", 3));
        }
    ''')
    assert errors == []
    # A multi-character pad is truncated to exactly fill the width.
    assert output == ["007", "7..", "ababx", "toolong", "  5"]


def test_pad_rejects_an_empty_pad_string():
    output, errors = run_mrt('func main() { print(padStart("x", 5, "")); }')
    assert errors == []
    assert any("pad string must not be empty" in line for line in output)


# -- Seeded randomness -------------------------------------------------------

def test_random_is_deterministic_for_a_given_seed():
    output, errors = run_mrt('''
        func main() {
            var a = random(42);
            var b = random(42);
            print(a() == b(), a() == b());
            var c = random(7);
            var d = random(42);
            print(c() == d());
        }
    ''')
    assert errors == []
    assert output == ["true true", "false"]


def test_random_generators_are_independent_and_advance():
    output, errors = run_mrt('''
        func main() {
            var r = random(1);
            var first = r();
            var second = r();
            print(first == second);
            print(first >= 0 && first < 1);
        }
    ''')
    assert errors == []
    assert output == ["false", "true"]


def test_random_generator_takes_no_arguments():
    output, errors = run_mrt('func main() { var r = random(1); print(r(5)); }')
    assert errors == []
    assert any("takes no arguments" in line for line in output)


def test_random_seed_must_be_a_number():
    output, errors = run_mrt('func main() { print(random("x")); }')
    assert errors == []
    assert any("must be a number" in line for line in output)
