"""A file that runs correctly and does nothing should say so.

MRT calls a top-level `main` if the file defines one. A file of only
declarations therefore runs to completion, prints nothing and exits 0 --
which reads, to anyone who has just installed the thing, as a broken
installation rather than a missing entry point.

The note goes to stderr and leaves the exit code alone: the program was
never wrong. Keeping it off stdout is what lets the conformance corpus go on
comparing printed output across four implementations byte for byte.
"""

import subprocess
import sys

from mrt.interpreter import ENTRY_MISSING, ENTRY_RAN, Interpreter
from mrt.lexer import Lexer
from mrt.parser import Parser


def entry_of(source: str) -> str:
    """What became of `main` in this program."""
    interpreter = Interpreter()
    interpreter.interpret(Parser(Lexer(source).scan_tokens()).parse())
    return interpreter.entry


def run_cli(tmp_path, source: str):
    program = tmp_path / "program.mrt"
    program.write_text(source)
    return subprocess.run(
        [sys.executable, "-m", "mrt", str(program)],
        capture_output=True,
        text=True,
    )


def test_a_declared_main_is_recorded_as_having_run():
    assert entry_of('func main() { print("hi"); }') == ENTRY_RAN


def test_no_main_at_all():
    assert entry_of("struct Place { name; }") == ENTRY_MISSING


def test_a_main_that_is_not_top_level_does_not_count():
    # Declared inside another function, so it never reaches the globals --
    # the mistake is invisible without the note.
    assert entry_of("func outer() { func main() { print(1); } }") == ENTRY_MISSING


def test_main_is_case_sensitive():
    assert entry_of('func Main() { print("hi"); }') == ENTRY_MISSING


def test_an_uncallable_main_is_told_apart_from_a_missing_one():
    # A different mistake: the name is taken.
    assert entry_of("var main = 5;") == "number"
    assert entry_of("struct P { x; } var main = P;") == "struct"


def test_the_cli_explains_a_file_with_no_main(tmp_path):
    result = run_cli(tmp_path, "struct Place { name; }\n")
    assert result.stdout == ""
    assert "no top-level 'main' function" in result.stderr
    assert "func main()" in result.stderr, "the note should say what to add"
    # Not an error: the program was correct, it simply had no entry point.
    assert result.returncode == 0


def test_the_cli_names_an_uncallable_main(tmp_path):
    result = run_cli(tmp_path, "var main = 5;\n")
    assert "the top-level 'main' is a number, not a function" in result.stderr
    assert result.returncode == 0


def test_no_note_when_the_program_printed_something(tmp_path):
    # Top-level statements run whether or not there is a `main`, so a file
    # that printed has done something and needs no explaining.
    result = run_cli(tmp_path, 'print("top level");\n')
    assert result.stdout == "top level\n"
    assert result.stderr == ""


def test_no_note_when_main_ran(tmp_path):
    result = run_cli(tmp_path, 'func main() { print("ran"); }\n')
    assert result.stdout == "ran\n"
    assert result.stderr == ""


def test_a_silent_main_is_not_explained(tmp_path):
    # It ran. That it chose to print nothing is the program's business, and
    # second-guessing it would make the note noise.
    result = run_cli(tmp_path, "func main() { var x = 1; }\n")
    assert result.stdout == ""
    assert result.stderr == ""
