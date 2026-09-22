"""Tests for `struct` declarations: construction, fields, methods and `this`,
equality, and how structs interact with the object built-ins."""

from tests.helpers import run_mrt, run_mrt_files

# -- Construction and fields -------------------------------------------------


def test_construction_and_field_access():
    output, errors = run_mrt("""
        struct Point { x, y; }
        func main() {
            var p = Point(3, 4);
            print(p);
            print(p.x, p.y, p["x"]);
        }
    """)
    assert errors == []
    assert output == ["Point(x: 3, y: 4)", "3 4 3"]


def test_type_reports_the_struct_name():
    output, errors = run_mrt("""
        struct Point { x; }
        func main() {
            print(type(Point(1)));
            print(type(Point));
        }
    """)
    assert errors == []
    assert output == ["Point", "struct"]


def test_field_defaults_may_refer_to_earlier_fields():
    output, errors = run_mrt("""
        struct Config { host, port = 8080, url = "http://${host}:${port}"; }
        func main() {
            print(Config("a"));
            print(Config("a", 99));
        }
    """)
    assert errors == []
    assert output == [
        "Config(host: a, port: 8080, url: http://a:8080)",
        "Config(host: a, port: 99, url: http://a:99)",
    ]


def test_fields_are_assignable_but_only_declared_ones():
    output, errors = run_mrt("""
        struct P { x; }
        func main() {
            var p = P(1);
            p.x = 5;
            print(p.x);
            try { p.y = 1; } catch (e) { print(e.kind, e.message); }
            try { print(p.z); } catch (e) { print(e.kind, e.message); }
        }
    """)
    assert errors == []
    assert output == [
        "5",
        'KeyError Struct P has no field "y".',
        'KeyError Struct P has no field or method "z".',
    ]


def test_constructor_arity_is_checked():
    output, errors = run_mrt("""
        struct P { x, y; }
        struct Q { a, b = 2; }
        func main() {
            try { P(1); } catch (e) { print(e.kind, e.message); }
            try { P(1, 2, 3); } catch (e) { print(e.message); }
            try { Q(); } catch (e) { print(e.message); }
        }
    """)
    assert errors == []
    assert output == [
        "ArityError Struct P takes 2 field values but got 1.",
        "Struct P takes 2 field values but got 3.",
        "Struct Q takes between 1 and 2 field values but got 0.",
    ]


# -- Methods -----------------------------------------------------------------


def test_methods_see_this():
    output, errors = run_mrt("""
        struct Point {
            x, y;
            func mag() { return sqrt(this.x * this.x + this.y * this.y); }
            func scaled(k) { return Point(this.x * k, this.y * k); }
        }
        func main() {
            var p = Point(3, 4);
            print(p.mag());
            print(p.scaled(2));
        }
    """)
    assert errors == []
    assert output == ["5", "Point(x: 6, y: 8)"]


def test_a_method_taken_as_a_value_keeps_its_receiver():
    output, errors = run_mrt("""
        struct Counter { n; func bump() { this.n += 1; return this.n; } }
        func main() {
            var c = Counter(0);
            var bump = c.bump;
            bump(); bump();
            print(c.n, bump());
        }
    """)
    assert errors == []
    assert output == ["2 3"]


def test_methods_take_defaults_and_rest_like_any_function():
    output, errors = run_mrt("""
        struct Tag {
            name;
            func render(prefix = "#", ...extra) { return "${prefix}${this.name}${len(extra)}"; }
        }
        func main() {
            var t = Tag("x");
            print(t.render());
            print(t.render("@", 1, 2));
        }
    """)
    assert errors == []
    assert output == ["#x0", "@x2"]


def test_methods_are_usable_with_higher_order_builtins():
    output, errors = run_mrt("""
        struct Person { name, age; func isAdult() { return this.age >= 18; } }
        func main() {
            var people = [Person("Ada", 36), Person("Bob", 17)];
            print(map(filter(people, func(p) { return p.isAdult(); }),
                      func(p) { return p.name; }));
        }
    """)
    assert errors == []
    assert output == ["[Ada]"]


# -- Equality ----------------------------------------------------------------


def test_equality_is_structural_and_per_struct():
    output, errors = run_mrt("""
        struct A { v; }
        struct B { v; }
        func main() {
            print(A(1) == A(1), A(1) == A(2), A(1) == B(1));
            print(A([1, 2]) == A([1, 2]));
            print(A(1) == {v: 1});
        }
    """)
    assert errors == []
    assert output == ["true false false", "true", "false"]


def test_instances_work_with_structural_membership():
    output, errors = run_mrt("""
        struct A { v; }
        func main() {
            print(indexOf([A(1), A(2)], A(2)));
            print(has([A(1)], A(1)));
            print(unique([A(1), A(1), A(2)]));
        }
    """)
    assert errors == []
    assert output == ["1", "true", "[A(v: 1), A(v: 2)]"]


# -- Interaction with object built-ins ---------------------------------------


def test_object_builtins_see_fields_not_methods():
    output, errors = run_mrt("""
        struct Point { x, y; func mag() { return 0; } }
        func main() {
            var p = Point(3, 4);
            print(keys(p), values(p), len(p));
            print(has(p, "x"), has(p, "mag"));
            print(get(p, "x", 0), get(p, "zz", "dflt"));
        }
    """)
    assert errors == []
    # Methods are deliberately not fields: keys/has/get describe the data.
    assert output == ["[x, y] [3, 4] 2", "true false", "3 dflt"]


def test_instances_can_be_object_destructured():
    output, errors = run_mrt("""
        struct Point { x, y; }
        func main() {
            var {x, y} = Point(3, 4);
            print(x, y);
            var {x: a, ...rest} = Point(1, 2);
            print(a, rest);
        }
    """)
    assert errors == []
    assert output == ["3 4", "1 {y: 2}"]


# -- Declaration rules -------------------------------------------------------


def test_structs_are_hoisted_like_functions():
    output, errors = run_mrt("""
        func main() { print(Later(1)); }
        struct Later { v; }
    """)
    assert errors == []
    assert output == ["Later(v: 1)"]


def test_duplicate_field_names_are_rejected():
    output, errors = run_mrt("struct P { x, x; } func main() { }")
    assert errors != []
    assert "duplicate field name" in errors[0].message


def test_a_field_and_method_cannot_share_a_name():
    output, errors = run_mrt("struct P { x; func x() { return 1; } } func main() { }")
    assert errors != []
    assert "field and a method both named 'x'" in errors[0].message


def test_a_field_without_a_default_cannot_follow_one_with_a_default():
    output, errors = run_mrt("struct P { a = 1, b; } func main() { }")
    assert errors != []
    assert "can't follow one with a default value" in errors[0].message


def test_a_struct_can_be_exported_and_imported(tmp_path):
    output, errors = run_mrt_files(
        {
            "main.mrt": """
            import { Point, origin } from "./geo.mrt";
            func main() {
                var p = Point(1, 2);
                print(p, type(p));
                print(origin);
            }
        """,
            "geo.mrt": """
            export struct Point { x, y; func mag() { return this.x + this.y; } }
            export var origin = Point(0, 0);
        """,
        },
        tmp_path,
    )
    assert errors == []
    assert output == ["Point(x: 1, y: 2) Point", "Point(x: 0, y: 0)"]


def test_instances_of_an_imported_struct_share_its_identity(tmp_path):
    output, errors = run_mrt_files(
        {
            "main.mrt": """
            import { Point } from "./geo.mrt";
            import { make } from "./other.mrt";
            func main() { print(Point(1, 2) == make()); }
        """,
            "geo.mrt": "export struct Point { x, y; }",
            "other.mrt": """
            import { Point } from "./geo.mrt";
            export func make() { return Point(1, 2); }
        """,
        },
        tmp_path,
    )
    assert errors == []
    # One module instance means one struct identity, so the two are equal.
    assert output == ["true"]
