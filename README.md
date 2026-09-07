# MRT Programming Language

MRT is a modern, expressive programming language designed for simplicity and readability. It combines intuitive syntax with powerful features for both beginners and experienced programmers.

## Features

- Simple and expressive syntax
- Dynamic typing with type inference
- First-class functions
- Rich built-in functions
- Comprehensive array operations
- Powerful string manipulation
- Modern module system

## Installation

You can install MRT using pip:

```bash
pip install mrt-lang
```

After installation, you can run MRT programs using the `mrt` command:
```bash
mrt your_program.mrt
```

## Quick Start

1. Create a file `hello.mrt`:
```mrt
func main() {
    print("Hello, World!")
}
```

2. Run the program:
```bash
mrt hello.mrt
```

## Language Examples

### Variables and Basic Types
```mrt
func variables_demo() {
    var name = "Alice"
    var age = 25
    var height = 1.75
    var isStudent = true
    
    print("Name: " + name)
}
```

### Arrays
```mrt
func array_demo() {
    var numbers = [1, 2, 3, 4, 5]
    push(numbers, 6)
    print("Length:", len(numbers))
    print("First element:", numbers[0])
}
```

### String Operations
```mrt
func string_demo() {
    var text = "  Hello, World!  "
    print("Trimmed:", trim(text))
    print("Uppercase:", toUpper(text))
    print("Contains 'World':", contains(text, "World"))
}
```

### Objects
```mrt
func object_demo() {
    var person = {"name": "Ada", "age": 36}
    print(person.name)
    person.age += 1
    print("Keys:", keys(person))
}
```

## Operators

```mrt
+  -  *  /  %              // arithmetic (% is modulo)
== !=  <  >  <=  >=        // comparison
&&  ||  !                   // logical AND / OR / NOT
=  += -= *= /= %=           // assignment and compound assignment
```

Loops also support `break` and `continue`.

## Built-in Functions

### Array Operations
- `len(array)`: Returns array length
- `push(array, element)`: Adds element to end
- `pop(array)`: Removes and returns last element
- `slice(array, start, end)`: Returns array subset
- `join(array, separator)`: Joins elements into string
- `indexOf(array, element)`: Finds element index
- `has(array, element)` / `get(array, index, default)`: Membership check / safe read

### Object Operations
- `keys(obj)` / `values(obj)`: Arrays of an object's keys/values
- `has(obj, key)` / `get(obj, key, default)`: Membership check / safe read
- `len(obj)`: Number of keys

### Type and Math
- `type(value)`: `"number" | "string" | "boolean" | "array" | "object" | "null" | "function"`
- `toNumber(value)` / `toString(value)`: Convert to/from a string
- `abs`, `min`, `max`, `round`, `floor`, `ceil`, `sqrt`, `pow`

### String Operations
- `split(str, separator)`: Splits string into array
- `substring(str, start, end)`: Extracts string portion
- `toUpper(str)`: Converts to uppercase
- `toLower(str)`: Converts to lowercase
- `trim(str)`: Removes whitespace
- `replace(str, old, new)`: Replaces text
- `startsWith(str, prefix)`: Checks string start
- `endsWith(str, suffix)`: Checks string end
- `contains(str, substr)`: Checks for substring

## Project Structure

- `src/`: Source code for the MRT interpreter
  - `lexer.py`: Tokenizes source code
  - `parser.py`: Parses tokens into AST
  - `interpreter.py`: Executes MRT programs
  - `ast.py`: Abstract Syntax Tree definitions
- `docs/`: Comprehensive documentation
  - `language_guide.md`: Complete language reference
  - `examples.md`: Example programs and tutorials
  - `getting_started.md`: Installation and quick start
- `examples/`: Example MRT programs
- `tests/`: Test suite

## Documentation

For more detailed information, check out:
- [Language Specification](docs/LANGUAGE_SPEC.md): Formal grammar, types, and semantics -- the precise reference
- [Language Guide](docs/language_guide.md): Friendlier, example-driven reference
- [Examples](docs/examples.md): Example programs and tutorials
- [Getting Started](docs/getting_started.md): Installation and quick start

## Development

To set up the development environment:

1. Clone the repository:
```bash
git clone https://github.com/rithymeth/MRT.git
cd MRT
```

2. Install in development mode:
```bash
pip install -e .
```

3. Install development dependencies:
```bash
pip install -r requirements-dev.txt
```

4. Run the test suite:
```bash
pytest tests/
```

## Contributing

We welcome contributions! Whether it's:
- Bug reports
- Feature requests
- Documentation improvements
- Code contributions

Please feel free to open issues and pull requests.

## License

MIT License - see [LICENSE.txt](LICENSE.txt) for details.
