"""Tests for examples/lib/json.mrt -- a JSON parser and writer written in
MRT itself.

The library is a real MRT program, so these run the shipped file rather than
a copy: each test writes it next to a small driver and runs the driver, the
way an importer would. Documents under test are handed to MRT as string
literals via json.dumps, whose escaping MRT's lexer shares.
"""

import json
import os

from tests.helpers import run_mrt_files

# One backslash, built rather than written, so that the JSON escape sequences
# in the documents below are never mistaken for Python's own escapes.
BS = chr(92)
U = BS + "u"

LIBRARY = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "examples",
    "lib",
    "json.mrt",
)

with open(LIBRARY) as handle:
    JSON_MRT = handle.read()


def run_with_library(main_source, tmp_path):
    """Run `main_source` with the JSON library importable as ./json.mrt."""
    output, errors = run_mrt_files(
        {
            "main.mrt": main_source,
            "json.mrt": JSON_MRT,
        },
        tmp_path,
    )
    assert errors == []
    return output


def parse_and_print(document, tmp_path):
    """parse() the MRT string literal `document`, then stringify() the result."""
    return run_with_library(
        f"""
        import {{ parse, stringify }} from "./json.mrt";
        func main() {{ print(stringify(parse({document}))); }}
    """,
        tmp_path,
    )


def failure_of(document, tmp_path):
    """The kind, position and message of the JsonError `document` provokes."""
    output = run_with_library(
        f"""
        import {{ parse }} from "./json.mrt";
        func main() {{
            try {{ parse({document}); print("no error"); }}
            catch (e) {{ print("${{e.kind}} ${{e.line}}:${{e.column}} ${{e.message}}"); }}
        }}
    """,
        tmp_path,
    )
    assert len(output) == 1
    return output[0]


def test_round_trips_every_json_type(tmp_path):
    document = '{"s":"text","n":-2.5,"i":12,"t":true,"f":false,"z":null}'
    assert parse_and_print(json.dumps(document), tmp_path) == [document]


def test_nested_structures_keep_their_shape(tmp_path):
    document = '{"a":[1,[2,[3,[]]]],"b":{"c":{"d":{}}}}'
    assert parse_and_print(json.dumps(document), tmp_path) == [document]


def test_whitespace_between_every_token_is_ignored(tmp_path):
    spaced = '  {\n\t"a"  :  [ 1 ,\r\n 2 ]  }  '
    assert parse_and_print(json.dumps(spaced), tmp_path) == ['{"a":[1,2]}']


def test_numbers_cover_the_json_grammar(tmp_path):
    assert parse_and_print(
        json.dumps("[0, -0.5, 1e2, 1E+2, 1e-2, -3.25e1, 10]"), tmp_path
    ) == ["[0,-0.5,100,100,0.01,-32.5,10]"]


def test_escapes_decode_and_encode_again(tmp_path):
    document = (
        '"quote '
        + BS
        + '" solidus '
        + BS
        + "/ backslash "
        + BS
        + BS
        + " tab "
        + BS
        + "t newline "
        + BS
        + 'n"'
    )
    # The solidus is legal escaped, but is written back out bare.
    assert parse_and_print(json.dumps(document), tmp_path) == [
        '"quote '
        + BS
        + '" solidus / backslash '
        + BS
        + BS
        + " tab "
        + BS
        + "t newline "
        + BS
        + 'n"'
    ]


def test_unicode_escapes_decode_through_u007f(tmp_path):
    document = '"' + U + "0041" + U + "0062 " + U + "007e " + U + '0009"'
    assert parse_and_print(json.dumps(document), tmp_path) == ['"Ab ~ ' + BS + 't"']


def test_the_last_of_two_duplicate_keys_wins(tmp_path):
    assert parse_and_print(json.dumps('{"k": 1, "k": 2}'), tmp_path) == ['{"k":2}']


def test_an_indent_pretty_prints_and_still_round_trips(tmp_path):
    output = run_with_library(
        """
        import { parse, stringify } from "./json.mrt";
        func main() {
            var value = parse("{\\"a\\": [1, {\\"b\\": 2}], \\"c\\": [], \\"d\\": {}}");
            print(stringify(value, 2));
            print(stringify(parse(stringify(value, 2))) == stringify(value));
        }
    """,
        tmp_path,
    )
    assert output == [
        '{\n  "a": [\n    1,\n    {\n      "b": 2\n    }\n  ],\n  "c": [],\n  "d": {}\n}',
        "true",
    ]


def test_a_struct_writes_itself_out_as_an_object(tmp_path):
    output = run_with_library(
        """
        import { stringify } from "./json.mrt";
        struct Point { x, y; func magnitude() { return 5; } }
        func main() { print(stringify([Point(3, 4)])); }
    """,
        tmp_path,
    )
    # Fields become members; methods are not data and do not appear.
    assert output == ['[{"x":3,"y":4}]']


def test_a_function_has_no_json_form(tmp_path):
    output = run_with_library(
        """
        import { stringify } from "./json.mrt";
        func main() {
            try { stringify({f: func() { return 1; }}); }
            catch (e) { print(e.kind, "-", e.message); }
        }
    """,
        tmp_path,
    )
    assert output == ["JsonError - a function has no JSON form"]


def test_failures_report_a_line_and_a_column(tmp_path):
    assert failure_of(json.dumps('{"a": 1,}'), tmp_path) == (
        "JsonError 1:9 expected a key, found '}'"
    )
    assert failure_of(json.dumps('{\n  "a" 1\n}'), tmp_path) == (
        "JsonError 2:7 expected ':' after the key \"a\", found '1'"
    )
    assert failure_of(json.dumps("[1, 2"), tmp_path) == (
        "JsonError 1:6 expected ',' or ']' in the array, found end of input"
    )


def test_leading_zeros_and_bare_words_are_rejected(tmp_path):
    assert failure_of(json.dumps("01"), tmp_path) == (
        "JsonError 1:2 a number may not have a leading zero"
    )
    assert failure_of(json.dumps("[nul]"), tmp_path) == (
        "JsonError 1:2 expected true, false or null, found 'n'"
    )
    assert failure_of(json.dumps("1."), tmp_path) == (
        "JsonError 1:3 expected a digit after the decimal point, found end of input"
    )


def test_trailing_content_after_the_document_is_rejected(tmp_path):
    assert failure_of(json.dumps("{} []"), tmp_path) == (
        "JsonError 1:4 unexpected '[' after the document"
    )


def test_an_unterminated_string_and_a_raw_control_character_are_rejected(tmp_path):
    assert failure_of(json.dumps('"open'), tmp_path) == (
        "JsonError 1:6 unterminated string"
    )
    assert failure_of(json.dumps('"a\tb"'), tmp_path) == (
        "JsonError 1:4 a control character must be escaped inside a string"
    )


def test_the_escapes_mrt_strings_cannot_hold_say_so(tmp_path):
    assert failure_of(json.dumps('"' + BS + 'b"'), tmp_path) == (
        "JsonError 1:4 " + BS + "b is valid JSON, but MRT strings cannot hold "
        "that character"
    )
    assert failure_of(json.dumps('"' + U + '00e9"'), tmp_path) == (
        "JsonError 1:8 " + BS + "u escapes are supported through U+007F; "
        "this one is above it"
    )
    assert failure_of(json.dumps('"' + BS + 'q"'), tmp_path) == (
        "JsonError 1:4 unknown escape " + BS + "q"
    )
