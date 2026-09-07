"""Tests for the module system: `export`, `import`, path resolution,
caching, and cycle detection."""

from tests.helpers import run_mrt, run_mrt_files


def test_named_imports_and_aliasing(tmp_path):
    output, errors = run_mrt_files({
        "main.mrt": '''
            import { PI, square, cube as cubed } from "./lib/math.mrt";
            func main() { print(PI, square(4), cubed(3)); }
        ''',
        "lib/math.mrt": '''
            export var PI = 3.14159;
            export func square(n) { return n * n; }
            export func cube(n) { return n * square(n); }
        ''',
    }, tmp_path)
    assert errors == []
    assert output == ["3.14159 16 27"]


def test_a_module_is_evaluated_once_however_many_importers(tmp_path):
    output, errors = run_mrt_files({
        "main.mrt": '''
            import { a } from "./a.mrt";
            import { b } from "./b.mrt";
            func main() { print(a(), b()); }
        ''',
        "a.mrt": 'import { tick } from "./shared.mrt";\nexport func a() { return tick(); }',
        "b.mrt": 'import { tick } from "./shared.mrt";\nexport func b() { return tick(); }',
        "shared.mrt": 'print("evaluated");\nvar n = 0;\nexport func tick() { n += 1; return n; }',
    }, tmp_path)
    assert errors == []
    # One "evaluated", and a shared counter proving one instance.
    assert output == ["evaluated", "1 2"]


def test_imported_functions_share_their_module_state(tmp_path):
    output, errors = run_mrt_files({
        "main.mrt": '''
            import { next, reset } from "./counter.mrt";
            func main() { print(next(), next()); reset(); print(next()); }
        ''',
        "counter.mrt": '''
            var n = 0;
            export func next() { n += 1; return n; }
            export func reset() { n = 0; }
        ''',
    }, tmp_path)
    assert errors == []
    assert output == ["1 2", "1"]


def test_private_names_are_not_importable(tmp_path):
    output, errors = run_mrt_files({
        "main.mrt": '''
            import { shown } from "./m.mrt";
            func main() {
                print(shown());
                try { print(hidden()); } catch (e) { print(e.kind); }
            }
        ''',
        "m.mrt": 'func hidden() { return "no"; }\nexport func shown() { return "yes"; }',
    }, tmp_path)
    assert errors == []
    assert output == ["yes", "NameError"]


def test_importing_an_undeclared_export_is_an_error(tmp_path):
    output, errors = run_mrt_files({
        "main.mrt": 'import { nothere } from "./m.mrt";\nfunc main() { }',
        "m.mrt": "export var here = 1;",
    }, tmp_path)
    assert errors == []
    assert output == [
        'Runtime Error: Module "./m.mrt" has no export named \'nothere\'. [line 1]'
    ]


def test_a_missing_module_names_the_specifier(tmp_path):
    output, errors = run_mrt_files({
        "main.mrt": 'import { x } from "./nope.mrt";\nfunc main() { }',
    }, tmp_path)
    assert errors == []
    assert output == ['Runtime Error: Cannot find module "./nope.mrt". [line 1]']


def test_a_bare_specifier_is_rejected(tmp_path):
    output, errors = run_mrt_files({
        "main.mrt": 'import { x } from "math";\nfunc main() { }',
    }, tmp_path)
    assert errors == []
    assert "must start with './' or '../'" in output[0]


def test_parent_relative_paths_resolve(tmp_path):
    output, errors = run_mrt_files({
        "main.mrt": 'import { v } from "./nested/deep.mrt";\nfunc main() { print(v); }',
        "nested/deep.mrt": 'import { base } from "../base.mrt";\nexport var v = base + 1;',
        "base.mrt": "export var base = 41;",
    }, tmp_path)
    assert errors == []
    assert output == ["42"]


def test_circular_imports_are_detected(tmp_path):
    output, errors = run_mrt_files({
        "main.mrt": 'import { a } from "./a.mrt";\nfunc main() { print(a()); }',
        "a.mrt": 'import { b } from "./b.mrt";\nexport func a() { return "a"; }',
        "b.mrt": 'import { a } from "./a.mrt";\nexport func b() { return "b"; }',
    }, tmp_path)
    assert errors == []
    assert output[0].startswith("Runtime Error: Circular import: a.mrt -> b.mrt -> a.mrt.")


def test_module_top_level_code_runs_before_the_entry_main(tmp_path):
    output, errors = run_mrt_files({
        "main.mrt": '''
            import { VALUE } from "./side.mrt";
            print("entry top level");
            func main() { print("main", VALUE); }
        ''',
        "side.mrt": 'print("side effect");\nexport var VALUE = 42;',
    }, tmp_path)
    assert errors == []
    assert output == ["side effect", "entry top level", "main 42"]


def test_exports_combine_with_defaults_and_rest(tmp_path):
    output, errors = run_mrt_files({
        "main.mrt": '''
            import { describe, tally } from "./util.mrt";
            func main() {
                print(describe("x"));
                print(describe("x", "!"));
                print(tally(1, 2, 3));
            }
        ''',
        "util.mrt": '''
            export func describe(name, suffix = "?") { return "${name}${suffix}"; }
            export func tally(first, ...rest) { return first + sum(rest); }
        ''',
    }, tmp_path)
    assert errors == []
    assert output == ["x?", "x!", "6"]


def test_a_module_syntax_error_is_reported_against_the_import(tmp_path):
    output, errors = run_mrt_files({
        "main.mrt": 'import { x } from "./bad.mrt";\nfunc main() { }',
        "bad.mrt": "export var x = ;",
    }, tmp_path)
    assert errors == []
    assert "has syntax errors" in output[0]


# -- Placement rules ---------------------------------------------------------

def test_import_is_only_allowed_at_the_top_level():
    output, errors = run_mrt('func main() { import { x } from "./a.mrt"; }')
    assert errors != []
    assert "only allowed at the top level" in errors[0].message


def test_export_is_only_allowed_at_the_top_level():
    output, errors = run_mrt('func main() { export var x = 1; }')
    assert errors != []
    assert "only allowed at the top level" in errors[0].message


def test_export_requires_a_declaration():
    output, errors = run_mrt('export 5;\nfunc main() { }')
    assert errors != []
    assert "Expect a 'func' or 'var' declaration after 'export'" in errors[0].message


def test_importing_without_a_file_path_is_a_clear_error():
    # run_mrt() has no file to resolve against, which is the same situation
    # as a Playground page with no module resolver configured.
    output, errors = run_mrt('import { x } from "./a.mrt";\nfunc main() { }')
    assert errors == []
    assert "Imports need a file to resolve against" in output[0]


def test_top_level_statements_run_even_when_main_exists():
    # Changed in this revision: they used to be skipped entirely whenever a
    # main() was defined, which silently dropped top-level `var`s -- and
    # would have dropped top-level imports too.
    output, errors = run_mrt('''
        print("top A");
        var x = later();
        print("top B", x);
        func later() { return "from func"; }
        func main() { print("main last", x); }
    ''')
    assert errors == []
    assert output == ["top A", "top B from func", "main last from func"]


def test_a_program_without_main_still_runs_top_level_statements():
    output, errors = run_mrt('print("just this");')
    assert errors == []
    assert output == ["just this"]
