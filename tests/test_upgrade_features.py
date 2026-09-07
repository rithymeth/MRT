"""Tests for the language upgrade: compound assignment, objects (dicts),
dot-access sugar, and the new built-ins (type/math/object helpers)."""

from tests.helpers import run_mrt


# -- Compound assignment -----------------------------------------------------

def test_compound_assignment_on_variable():
    output, errors = run_mrt('''
        func main() {
            var x = 10;
            x += 5;  print(x);
            x -= 3;  print(x);
            x *= 2;  print(x);
            x /= 4;  print(x);
            x %= 4;  print(x);
        }
    ''')
    assert errors == []
    assert output == ["15", "12", "24", "6", "2"]


def test_compound_assignment_on_array_element():
    output, errors = run_mrt('''
        func main() {
            var arr = [1, 2, 3];
            arr[0] += 100;
            arr[1] *= 10;
            print(arr);
        }
    ''')
    assert errors == []
    assert output == ["[101, 20, 3]"]


def test_compound_assignment_string_concatenation():
    output, errors = run_mrt('''
        func main() {
            var s = "a";
            s += "b";
            print(s);
        }
    ''')
    assert errors == []
    assert output == ["ab"]


def test_invalid_assignment_target_is_a_syntax_error():
    output, errors = run_mrt('func main() { 5 = 3; }')
    assert len(errors) == 1


# -- Objects (dicts) ----------------------------------------------------------

def test_dict_literal_and_bracket_access():
    output, errors = run_mrt('''
        func main() {
            var person = {"name": "Ada", "age": 36};
            print(person["name"]);
            print(person["age"]);
        }
    ''')
    assert errors == []
    assert output == ["Ada", "36"]


def test_dict_dot_access_sugar():
    output, errors = run_mrt('''
        func main() {
            var person = {"name": "Ada"};
            print(person.name);
            person.name = "Lovelace";
            print(person.name);
        }
    ''')
    assert errors == []
    assert output == ["Ada", "Lovelace"]


def test_dict_compound_assignment_via_dot():
    output, errors = run_mrt('''
        func main() {
            var person = {"age": 36};
            person.age += 1;
            print(person.age);
        }
    ''')
    assert errors == []
    assert output == ["37"]


def test_dict_stringifies_like_json_ish():
    output, errors = run_mrt('func main() { print({"a": 1, "b": 2}); }')
    assert errors == []
    assert output == ["{a: 1, b: 2}"]


def test_dict_missing_key_is_a_runtime_error():
    output, errors = run_mrt('func main() { var d = {"a": 1}; print(d["missing"]); }')
    assert errors == []
    assert len(output) == 1
    assert "Runtime Error" in output[0]


def test_dict_new_key_via_assignment():
    output, errors = run_mrt('''
        func main() {
            var d = {"a": 1};
            d["b"] = 2;
            print(len(d));
            print(d["b"]);
        }
    ''')
    assert errors == []
    assert output == ["2", "2"]


def test_dict_equality_is_structural():
    output, errors = run_mrt('func main() { print({"a": 1} == {"a": 1}); }')
    assert errors == []
    assert output == ["true"]


def test_array_equality_is_structural():
    output, errors = run_mrt('func main() { print([1, [2, 3]] == [1, [2, 3]]); }')
    assert errors == []
    assert output == ["true"]


def test_boolean_never_equals_number_even_nested():
    output, errors = run_mrt('''
        func main() {
            print(true == 1);
            print([true] == [1]);
        }
    ''')
    assert errors == []
    assert output == ["false", "false"]


def test_string_indexing_read_only():
    output, errors = run_mrt('func main() { var s = "hello"; print(s[0]); print(s[4]); }')
    assert errors == []
    assert output == ["h", "o"]


def test_string_index_assignment_is_a_runtime_error():
    output, errors = run_mrt('func main() { var s = "hi"; s[0] = "x"; }')
    assert errors == []
    assert len(output) == 1
    assert "Runtime Error" in output[0]


# -- New built-ins ------------------------------------------------------------

def test_type_builtin():
    output, errors = run_mrt('''
        func main() {
            print(type(5));
            print(type("hi"));
            print(type(true));
            print(type([1]));
            print(type({"a": 1}));
        }
    ''')
    assert errors == []
    assert output == ["number", "string", "boolean", "array", "object"]


def test_type_of_null():
    output, errors = run_mrt('''
        func noop() {}
        func main() {
            print(type(noop()));
        }
    ''')
    assert errors == []
    assert output == ["null"]


def test_math_builtins():
    output, errors = run_mrt('''
        func main() {
            print(abs(-5));
            print(min(3, 1, 2));
            print(max([3, 1, 9, 2]));
            print(round(3.14159, 2));
            print(floor(3.9));
            print(ceil(3.1));
            print(sqrt(16));
            print(pow(2, 10));
        }
    ''')
    assert errors == []
    assert output == ["5", "1", "9", "3.14", "3", "4", "4", "1024"]


def test_to_number_and_to_string():
    output, errors = run_mrt('''
        func main() {
            print(toNumber("42") + 1);
            print(toString(42) + "!");
        }
    ''')
    assert errors == []
    assert output == ["43", "42!"]


def test_to_number_invalid_string_is_runtime_error():
    output, errors = run_mrt('func main() { print(toNumber("not a number")); }')
    assert errors == []
    assert len(output) == 1
    assert "Runtime Error" in output[0]


def test_keys_values_has_get():
    output, errors = run_mrt('''
        func main() {
            var d = {"a": 1, "b": 2};
            print(keys(d));
            print(values(d));
            print(has(d, "a"));
            print(has(d, "z"));
            print(get(d, "z", "fallback"));
            print(has([1, 2, 3], 2));
            print(get([1, 2, 3], 10, "oob"));
        }
    ''')
    assert errors == []
    assert output == [
        "[a, b]",
        "[1, 2]",
        "true",
        "false",
        "fallback",
        "true",
        "oob",
    ]


def test_indexof_and_has_use_structural_equality():
    output, errors = run_mrt('''
        func main() {
            var arr = [[1, 2], [3, 4]];
            print(indexOf(arr, [3, 4]));
            print(has(arr, [1, 2]));
        }
    ''')
    assert errors == []
    assert output == ["1", "true"]
