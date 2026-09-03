from src.ast import Binary, Break, Continue, For, Logical, Return
from src.lexer import Lexer
from src.parser import Parser


def parse(source: str):
    tokens = Lexer(source).scan_tokens()
    parser = Parser(tokens)
    statements = parser.parse()
    return statements, parser.errors


def test_semicolons_are_optional():
    statements, errors = parse('func main() {\n    var x = 1\n    print(x)\n}')
    assert errors == []
    assert len(statements) == 1
    body = statements[0].body
    assert len(body) == 2


def test_semicolons_still_allowed():
    statements, errors = parse('func main() {\n    var x = 1;\n    print(x);\n}')
    assert errors == []
    assert len(statements[0].body) == 2


def test_for_loop_is_dedicated_node_not_desugared():
    statements, errors = parse('for (var i = 0; i < 10; i = i + 1) { print(i) }')
    assert errors == []
    assert isinstance(statements[0], For)


def test_break_and_continue_parse():
    statements, errors = parse('while (true) { break }')
    assert errors == []
    body = statements[0].body
    assert isinstance(body.statements[0], Break)


def test_logical_operators_produce_logical_node():
    statements, errors = parse('print(true && false || true);')
    assert errors == []
    expr = statements[0].expressions[0]
    assert isinstance(expr, Logical)


def test_comparison_precedence_over_logical():
    # `a < b && c > d` should parse as (a < b) && (c > d), not mangle the
    # comparisons together.
    statements, errors = parse('print(1 < 2 && 3 > 2);')
    assert errors == []
    expr = statements[0].expressions[0]
    assert isinstance(expr, Logical)
    assert isinstance(expr.left, Binary)
    assert isinstance(expr.right, Binary)


def test_print_accepts_multiple_comma_separated_arguments():
    statements, errors = parse('print("label:", 1, 2 + 3);')
    assert errors == []
    assert len(statements[0].expressions) == 3


def test_return_without_value_does_not_swallow_next_statement():
    statements, errors = parse('func f() {\n    return\n    print("x")\n}')
    assert errors == []
    body = statements[0].body
    assert isinstance(body[0], Return)
    assert body[0].value is None
    assert len(body) == 2


def test_missing_paren_is_a_reported_syntax_error():
    statements, errors = parse('func main( {\n    print("oops")\n}')
    assert len(errors) == 1
    assert errors[0].line == 1


def test_multiple_syntax_errors_are_collected():
    # Two independent malformed declarations; the parser should recover
    # after the first error via synchronize() and keep looking for more
    # rather than stopping at the first one.
    _, errors = parse('var ;\nvar ;\n')
    assert len(errors) == 2
