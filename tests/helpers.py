import os
from typing import Dict, List, Tuple

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


def run_mrt_files(files: Dict[str, str], root, entry: str = "main.mrt"
                  ) -> Tuple[List[str], List[MRTSyntaxError]]:
    """Write a small file tree under `root` and run `entry` from it.

    Used by the module tests, where imports have to resolve against real
    paths relative to the importing file.
    """
    for relative, contents in files.items():
        full = os.path.join(str(root), relative)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w") as handle:
            handle.write(contents)

    entry_path = os.path.join(str(root), entry)
    with open(entry_path) as handle:
        source = handle.read()

    tokens = Lexer(source).scan_tokens()
    parser = Parser(tokens)
    statements = parser.parse()
    if parser.errors:
        return [], parser.errors

    interpreter = Interpreter(module_path=entry_path)
    interpreter.interpret(statements)
    return list(interpreter.output), []
