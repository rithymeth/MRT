# Getting Started with MRT

## Installation

MRT is implemented in Python and requires Python 3.6 or later.

1. Clone the repository:
```bash
git clone <repository-url>
cd mrt
```

2. Install dependencies:
```bash
pip install -e .
```

## Running MRT Programs

To run an MRT program:

```bash
python -m src your_program.mrt
```

## Creating Your First Program

1. Create a new file `hello.mrt`:
```mrt
func main() {
    print("Hello, World!")
}
```

2. Run the program:
```bash
python -m src hello.mrt
```

## Development Environment

MRT files use the `.mrt` extension. Any text editor can be used for MRT development, but we recommend using an editor with syntax highlighting support for better readability.

## Project Structure

- `src/`: Source code for the MRT interpreter
  - `lexer.py`: Tokenizes source code
  - `parser.py`: Parses tokens into AST
  - `interpreter.py`: Executes MRT programs
  - `ast.py`: Abstract Syntax Tree definitions
- `examples/`: Example MRT programs
- `docs/`: Documentation
- `tests/`: Test suite

## Basic Concepts

### Program Structure
Every MRT program consists of functions and statements. The `main()` function is the entry point of the program.

### Variables
Variables are declared using the `var` keyword:
```mrt
var name = "John"
var age = 25
```

### Functions
Functions are declared using the `func` keyword:
```mrt
func greet(name) {
    print("Hello, " + name + "!")
}
```

### Data Types
MRT supports these basic data types:
- Numbers (integers and floats)
- Strings
- Booleans
- Arrays

## Next Steps

1. Review the [Language Guide](language_guide.md) for detailed documentation
2. Check out the [Examples](examples.md) for practical code samples
3. Try writing your own MRT programs
4. Join the community (coming soon)

## Common Issues and Solutions

### Program Not Running
- Ensure Python 3.6+ is installed
- Check that all files are in the correct directory
- Verify syntax in your MRT file

### Syntax Errors
- Check for missing parentheses or braces
- Verify all variables are properly declared
- Ensure strings are properly quoted

## Getting Help

If you encounter any issues:
1. Check the documentation
2. Look at similar examples
3. Report issues on the repository (coming soon)

## Contributing

We welcome contributions! Please see our contributing guidelines (coming soon) for details on how to help improve MRT.
