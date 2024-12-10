import sys
from .lexer import Lexer
from .parser import Parser
from .interpreter import Interpreter

def run_file(path: str):
    with open(path, 'r') as file:
        source = file.read()
        run(source)

def run(source: str):
    # Create lexer and generate tokens
    lexer = Lexer(source)
    tokens = lexer.scan_tokens()

    # Parse tokens into AST
    parser = Parser(tokens)
    statements = parser.parse()

    # Interpret the AST
    interpreter = Interpreter()
    interpreter.interpret(statements)

def main():
    if len(sys.argv) != 2:
        print("Usage: python -m mrt <script>")
        sys.exit(1)

    run_file(sys.argv[1])

if __name__ == "__main__":
    main()
