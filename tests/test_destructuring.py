"""Tests for destructuring patterns in `var`, parameters, `for`-`in` and
`catch`, plus namespace imports and re-exports."""

from tests.helpers import run_mrt, run_mrt_files

# -- Array patterns ----------------------------------------------------------


def test_array_pattern_binds_positionally():
    output, errors = run_mrt("""
        func main() {
            var [x, y] = [1, 2];
            print(x, y);
            var [only] = [7, 8, 9];
            print(only);
        }
    """)
    assert errors == []
    # Extra elements are simply ignored; a pattern names what it wants.
    assert output == ["1 2", "7"]


def test_array_rest_collects_the_remainder():
    output, errors = run_mrt("""
        func main() {
            var [head, ...tail] = [1, 2, 3];
            print(head, tail);
            var [a, ...none] = [1];
            print(a, none, type(none));
        }
    """)
    assert errors == []
    assert output == ["1 [2, 3]", "1 [] array"]


def test_array_pattern_defaults_fill_missing_slots():
    output, errors = run_mrt("""
        func main() {
            var [p, q = 99, r = q + 1] = [5];
            print(p, q, r);
        }
    """)
    assert errors == []
    assert output == ["5 99 100"]


def test_too_few_elements_without_a_default_is_an_error():
    output, errors = run_mrt("func main() { var [a, b] = [1]; }")
    assert errors == []
    assert output == [
        "Runtime Error: Cannot destructure: the array has 1 element(s) "
        "but the pattern needs at least 2. [line 1]"
    ]


def test_destructuring_a_non_array_with_an_array_pattern():
    output, errors = run_mrt("""
        func main() {
            try { var [a] = 5; } catch (e) { print(e.kind, e.message); }
            try { var [b] = {x: 1}; } catch (e) { print(e.message); }
        }
    """)
    assert errors == []
    assert output == [
        "TypeError Cannot destructure number with an array pattern.",
        "Cannot destructure object with an array pattern.",
    ]


# -- Object patterns ---------------------------------------------------------


def test_object_pattern_shorthand_and_renaming():
    output, errors = run_mrt("""
        func main() {
            var {name, age} = {name: "Ada", age: 36};
            print(name, age);
            var {name: who} = {name: "Bob"};
            print(who);
        }
    """)
    assert errors == []
    assert output == ["Ada 36", "Bob"]


def test_object_pattern_defaults_and_rest():
    output, errors = run_mrt("""
        func main() {
            var {a, missing = "n/a"} = {a: 1};
            print(a, missing);
            var {x, ...others} = {x: 1, y: 2, z: 3};
            print(x, others);
            var {q, ...empty} = {q: 1};
            print(empty, type(empty));
        }
    """)
    assert errors == []
    assert output == ["1 n/a", "1 {y: 2, z: 3}", "{} object"]


def test_missing_key_without_a_default_is_an_error():
    output, errors = run_mrt("func main() { var {zz} = {a: 1}; }")
    assert errors == []
    assert output == [
        'Runtime Error: Cannot destructure: no key "zz" in the object. [line 1]'
    ]


def test_destructuring_a_non_object_with_an_object_pattern():
    output, errors = run_mrt(
        "func main() { try { var {a} = [1]; } catch (e) { print(e.kind, e.message); } }"
    )
    assert errors == []
    assert output == ["TypeError Cannot destructure array with an object pattern."]


# -- Nesting -----------------------------------------------------------------


def test_patterns_nest_arbitrarily():
    output, errors = run_mrt("""
        func main() {
            var [{id}, [inner]] = [{id: 7}, [8]];
            print(id, inner);
            var {items: [first, second], meta: {tag}} = {items: [1, 2], meta: {tag: "t"}};
            print(first, second, tag);
        }
    """)
    assert errors == []
    assert output == ["7 8", "1 2 t"]


def test_a_nested_pattern_can_carry_its_own_default():
    output, errors = run_mrt("""
        func main() {
            var {opts: {debug} = {debug: false}} = {};
            print(debug);
        }
    """)
    assert errors == []
    assert output == ["false"]


# -- Parameters --------------------------------------------------------------


def test_destructuring_parameters():
    output, errors = run_mrt("""
        func swap([a, b]) { return [b, a]; }
        func describe({name, age = 0}) { return "${name}/${age}"; }
        func main() {
            print(swap([1, 2]));
            print(describe({name: "Cy"}));
        }
    """)
    assert errors == []
    assert output == ["[2, 1]", "Cy/0"]


def test_a_destructuring_parameter_can_have_a_whole_pattern_default():
    output, errors = run_mrt("""
        func opt({x} = {x: 5}) { return x; }
        func main() { print(opt(), opt({x: 9})); }
    """)
    assert errors == []
    assert output == ["5 9"]


def test_arity_still_counts_destructuring_parameters():
    output, errors = run_mrt("""
        func f([a], {b}) { return 0; }
        func main() { try { f([1]); } catch (e) { print(e.message); } }
    """)
    assert errors == []
    assert output == ["Expected 2 arguments but got 1."]


# -- for-in and catch --------------------------------------------------------


def test_destructuring_in_for_in():
    output, errors = run_mrt("""
        func main() {
            for ([k, v] in [["a", 1], ["b", 2]]) { print(k, "=", v); }
            for ({name} in [{name: "x"}, {name: "y"}]) { print(name); }
        }
    """)
    assert errors == []
    assert output == ["a = 1", "b = 2", "x", "y"]


def test_destructured_for_in_binds_fresh_per_iteration():
    output, errors = run_mrt("""
        func main() {
            var fns = [];
            for ([a, b] in [[1, 2], [3, 4]]) { push(fns, func() { return a + b; }); }
            print(map(fns, func(f) { return f(); }));
        }
    """)
    assert errors == []
    assert output == ["[3, 7]"]


def test_c_style_for_loop_is_unaffected():
    output, errors = run_mrt("""
        func main() { for (var i = 0; i < 3; i += 1) { print(i); } }
    """)
    assert errors == []
    assert output == ["0", "1", "2"]


def test_destructuring_in_catch():
    output, errors = run_mrt("""
        func main() {
            try { var a = []; print(a[3]); }
            catch ({kind, message}) { print(kind, "|", message); }
        }
    """)
    assert errors == []
    assert output[0].startswith("IndexError | Array index 3 out of bounds")


# -- Parse-level rules -------------------------------------------------------


def test_a_destructuring_declaration_needs_an_initializer():
    output, errors = run_mrt("func main() { var [a, b]; }")
    assert errors != []
    assert "needs an initializer" in errors[0].message


def test_a_stray_brace_still_errors_where_the_mistake_is():
    # `func main( {` is a missing paren, not an object pattern -- the error
    # should stay on line 1 rather than following the brace's contents.
    output, errors = run_mrt('func main( {\n    print("oops")\n}')
    assert errors != []
    assert errors[0].line == 1


def test_export_rejects_a_destructuring_declaration():
    output, errors = run_mrt("export var [a, b] = [1, 2];")
    assert errors != []
    assert "Only a plain `var name` can be exported" in errors[0].message


# -- Namespace imports and re-exports ----------------------------------------


def test_namespace_import_binds_one_object(tmp_path):
    output, errors = run_mrt_files(
        {
            "main.mrt": """
            import * as math from "./math.mrt";
            func main() {
                print(math.PI, math.square(4));
                print(type(math), keys(math));
            }
        """,
            "math.mrt": "export var PI = 3.14;\nexport func square(n) { return n * n; }",
        },
        tmp_path,
    )
    assert errors == []
    # Functions are hoisted, so they land in the export table first.
    assert output == ["3.14 16", "object [square, PI]"]


def test_namespace_import_excludes_private_names(tmp_path):
    output, errors = run_mrt_files(
        {
            "main.mrt": """
            import * as m from "./m.mrt";
            func main() {
                try { print(m.hidden); } catch (e) { print(e.kind); }
            }
        """,
            "m.mrt": "func hidden() { return 1; }\nexport var shown = 2;",
        },
        tmp_path,
    )
    assert errors == []
    assert output == ["KeyError"]


def test_re_export_forwards_without_binding_locally(tmp_path):
    output, errors = run_mrt_files(
        {
            "main.mrt": """
            import { PI, sq } from "./index.mrt";
            func main() { print(PI, sq(3)); }
        """,
            "index.mrt": 'export { PI, square as sq } from "./math.mrt";',
            "math.mrt": "export var PI = 3.14;\nexport func square(n) { return n * n; }",
        },
        tmp_path,
    )
    assert errors == []
    assert output == ["3.14 9"]


def test_export_names_re_exports_a_local_declaration(tmp_path):
    output, errors = run_mrt_files(
        {
            "main.mrt": 'import { a, renamed } from "./m.mrt";\nfunc main() { print(a, renamed); }',
            "m.mrt": "var a = 1;\nvar b = 2;\nexport { a, b as renamed };",
        },
        tmp_path,
    )
    assert errors == []
    assert output == ["1 2"]


def test_re_exporting_a_name_the_source_lacks_is_an_error(tmp_path):
    output, errors = run_mrt_files(
        {
            "main.mrt": 'import { a } from "./mid.mrt";\nfunc main() { }',
            "mid.mrt": 'export { nope } from "./base.mrt";',
            "base.mrt": "export var yes = 1;",
        },
        tmp_path,
    )
    assert errors == []
    assert "has no export named 'nope'" in output[0]
