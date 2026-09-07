from typing import List, Tuple

from src.errors import MRTSyntaxError
from src.interpreter import Interpreter
from src.lexer import Lexer
from src.parser import Parser


def run_mrt(source: str) -> Tuple[List[str], List[MRTSyntaxError]]:
    """Lex, parse and interpret MRT source.

    Returns (output_lines, parse_errors). Each entry in output_lines is the
    text produced by one `print(...)` call. If there are parse errors the
    program is never executed and output_lines is empty.
    """
    tokens = Lexer(source).scan_tokens()
    parser = Parser(tokens)
    statements = parser.parse()

    if parser.errors:
        return [], parser.errors

    interpreter = Interpreter()
    interpreter.interpret(statements)
    return list(interpreter.output), []
