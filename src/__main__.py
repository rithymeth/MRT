import sys
from .lexer import Lexer
from .parser import Parser
from .interpreter import Interpreter
from .errors import MRTError

def run_file(path: str) -> int:
    try:
        with open(path, 'r') as file:
            source = file.read()
    except OSError as e:
        print(f"Could not read file '{path}': {e.strerror}", file=sys.stderr)
        return 1

    return run(source)

def run(source: str) -> int:
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
    interpreter = Interpreter()
    interpreter.interpret(statements)
    return 0

def main():
    if len(sys.argv) != 2:
        print("Usage: mrt <script>")
        sys.exit(1)

    sys.exit(run_file(sys.argv[1]))

if __name__ == "__main__":
    main()
