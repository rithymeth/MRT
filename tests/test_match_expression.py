"""Tests for `match` used as an expression: `var label = match (v) { ... };`"""

from tests.helpers import run_mrt


def test_a_match_expression_produces_the_matching_arms_value():
    output, errors = run_mrt('''
        func classify(v) {
            return match (v) {
                case 0: "zero",
                case [a, b]: "pair ${a + b}",
                case {kind: "dog", name: n}: "dog ${n}",
                case n if (type(n) == "number" && n < 0): "neg ${n}",
                default: "other"
            };
        }
        func main() {
            print(classify(0), classify(-3), classify(5));
            print(classify([1, 2]));
            print(classify({kind: "dog", name: "Rex"}));
            print(classify("x"));
        }
    ''')
    assert errors == []
    assert output == ["zero neg -3 other", "pair 3", "dog Rex", "other"]


def test_it_composes_wherever_an_expression_goes():
    output, errors = run_mrt('''
        struct Point { x, y }
        func main() {
            var p = Point(1, 2);
            print("at ${ match (p) { case Point(a, b): "${a},${b}", default: "?" } }");
            print(len(match (true) { case true: [1, 2, 3], default: [] }));
            print(map([1, 2], func(n) {
                return match (n) { case 1: "one", default: "many" };
            }));
        }
    ''')
    assert errors == []
    assert output == ["at 1,2", "3", "[one, many]"]


def test_match_expressions_nest():
    output, errors = run_mrt('''
        func main() {
            var v = match (3) {
                case n if (n > 2): match (n) { case 3: "three", default: "big" },
                default: "small"
            };
            print(v);
        }
    ''')
    assert errors == []
    assert output == ["three"]


def test_a_trailing_comma_is_allowed():
    output, errors = run_mrt('func main() { print(match (1) { case 1: "ok", }); }')
    assert errors == []
    assert output == ["ok"]


def test_an_unmatched_match_expression_is_an_error():
    output, errors = run_mrt('''
        func main() {
            try { print(match (99) { case 1: "one" }); }
            catch (e) { print(e.kind, e.message); }
        }
    ''')
    assert errors == []
    assert output == [
        "ValueError No case matched 99 in this match, and there is no 'default'."
    ]


def test_match_in_statement_position_is_still_the_statement_form():
    output, errors = run_mrt('''
        func main() {
            match (2) {
                case 2: print("statement"); print("form");
                default: print("no");
            }
        }
    ''')
    assert errors == []
    assert output == ["statement", "form"]


def test_arms_bind_in_their_own_scope():
    output, errors = run_mrt('''
        func main() {
            var n = "outer";
            print(match (7) { case n: "inner ${n}", default: "?" });
            print(n);
        }
    ''')
    assert errors == []
    assert output == ["inner 7", "outer"]


def test_only_one_default_and_it_must_come_last():
    for source, message in [
        ('func main() { print(match (1) { default: 1, default: 2 }); }',
         "only have one 'default'"),
        ('func main() { print(match (1) { default: 1, case 1: 2 }); }',
         "'default' must be the last clause"),
    ]:
        output, errors = run_mrt(source)
        assert errors != [], source
        assert message in errors[0].message
