import sys
from .lexer import Lexer
from .parser import Parser
from .interpreter import Interpreter, ENTRY_RAN, ENTRY_MISSING
from .errors import MRTError

def run_file(path: str) -> int:
    try:
        with open(path, 'r') as file:
            source = file.read()
    except OSError as e:
        print(f"Could not read file '{path}': {e.strerror}", file=sys.stderr)
        return 1

    # The path is passed through so that `import "./x.mrt"` resolves
    # relative to this file rather than the working directory.
    return run(source, path)

def run(source: str, path: str = None) -> int:
    # Create lexer and generate tokens
    try:
        lexer = Lexer(source)
        tokens = lexer.scan_tokens()
    except MRTError as e:
        print(f"Syntax Error: {e}", file=sys.stderr)
        return 65

    # Parse tokens into AST
    parser = Parser(tokens)
    statements = parser.parse()

    if parser.errors:
        for error in parser.errors:
            print(f"Syntax Error: {error}", file=sys.stderr)
        return 65

    # Interpret the AST
    interpreter = Interpreter(module_path=path)
    interpreter.interpret(statements)
    explain_if_nothing_ran(interpreter, path)
    return 0


def explain_if_nothing_ran(interpreter: Interpreter, path: str = None) -> None:
    """Say so when a file ran correctly and did nothing.

    MRT calls a top-level `main` if the file defines one, and a file of only
    declarations therefore runs to completion, prints nothing and exits 0 --
    which is indistinguishable from a broken installation. The program was
    never wrong, so this is not an error and does not change the exit code;
    it goes to stderr, which also keeps it out of the printed output that
    four implementations are held to match byte for byte.
    """
    if interpreter.entry == ENTRY_RAN or interpreter.output:
        return

    where = f" in '{path}'" if path else ""
    if interpreter.entry == ENTRY_MISSING:
        print(
            f"mrt: nothing ran{where} -- no top-level 'main' function.",
            file=sys.stderr,
        )
        print(
            "hint: add  func main() { ... }  at the top level (not indented "
            "inside another function), or write statements at file scope.",
            file=sys.stderr,
        )
    else:
        # The name is taken by something uncallable, which is a different
        # mistake from not having one and deserves to be named as such.
        print(
            f"mrt: nothing ran{where} -- the top-level 'main' is a "
            f"{interpreter.entry}, not a function.",
            file=sys.stderr,
        )

def main():
    if len(sys.argv) != 2:
        print("Usage: mrt <script>")
        sys.exit(1)

    sys.exit(run_file(sys.argv[1]))

if __name__ == "__main__":
    main()
