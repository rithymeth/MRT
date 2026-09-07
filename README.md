# MRT Programming Language

MRT is a modern, expressive programming language designed for simplicity and readability. It combines intuitive syntax with powerful features for both beginners and experienced programmers.

## Features

- Simple and expressive syntax
- Dynamic typing
- First-class functions: anonymous `func(...)` values, lexical closures, and
  a `map`/`filter`/`reduce`/`sort` pipeline
- Default and rest parameters, plus `...` spread
- A module system: `export` / `import` across files
- Recoverable errors with `throw` and `try`/`catch`/`finally`, classified by
  kind and carrying a stack trace
- String interpolation, `null`, bareword object keys, and `for`-`in` loops
- Rich built-in functions for arrays, objects, strings and math
- Deterministic seeded randomness
- A browser Playground running a second interpreter kept byte-identical to
  the reference one by an automated parity check

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

### Functions, closures and pipelines

```mrt
var double = func(x) { return x * 2; };
print(map([1, 2, 3], double));                          // [2, 4, 6]

func makeCounter() {
    var count = 0;
    return func() { count += 1; return count; };
}
var next = makeCounter();
next();
print(next());                                          // 2

var nums = [5, 3, 8, 1];
print(filter(nums, func(x) { return x > 3; }));         // [5, 8]
print(reduce(nums, func(a, b) { return a + b; }));      // 17
print(sort(nums, func(a, b) { return b - a; }));        // [8, 5, 3, 1]
```

### Parameters: defaults, rest and spread

```mrt
func greet(name, greeting = "Hello") { return "${greeting}, ${name}!"; }
func total(label, ...nums) { return "${label}: ${sum(nums)}"; }

print(greet("Ada"));                                    // Hello, Ada!
print(total("all", 1, 2, 3));                           // all: 6

var xs = [1, 2];
print(total("spread", ...xs, 3));                       // spread: 6
print([0, ...xs, 9]);                                   // [0, 1, 2, 9]
```

### Error handling

```mrt
func safeDivide(a, b) {
    try { return a / b; } catch (e) { return null; }
}

print(safeDivide(10, 0));                               // null

// Errors carry a kind, and catch clauses can be guarded.
try {
    risky();
}
catch (e) if (get(e, "kind", "") == "IndexError") { print("bad index"); }
catch (e) { print("something else:", e); }

try {
    throw {field: "age", reason: "must not be negative"};
} catch (e) {
    print(e.field, "-", e.reason);
} finally {
    print("always runs");
}
```

### Modules

```mrt
// lib/math.mrt
export var PI = 3.14159;
export func square(n) { return n * n; }

// main.mrt
import { PI, square as sq } from "./lib/math.mrt";
func main() { print(PI, sq(4)); }
```

### Interpolation, null and for-in

```mrt
var name = "Ada";
var person = {name: "Ada", age: 36};                    // bareword keys

print("Hi ${name}, you are ${person.age}!");
print(type(null));                                      // null

for (key in person) { print("${key} = ${get(person, key)}"); }
for (ch in "hi") { print(ch); }
```

## Operators

```mrt
+  -  *  /  %              // arithmetic (% is modulo)
== !=  <  >  <=  >=        // comparison
&&  ||  !                   // logical AND / OR / NOT
=  += -= *= /= %=           // assignment and compound assignment
```

Loops also support `break` and `continue`.

Reserved words: `func return if else while for print var true false break
continue null try catch finally throw in import export from as`.

`...` marks a rest parameter or spreads an array.

## Built-in Functions

### Array Operations
- `len(array)`: Returns array length
- `push(array, element)`: Adds element to end
- `pop(array)`: Removes and returns last element
- `slice(array, start, end)`: Returns array subset
- `join(array, separator)`: Joins elements into string
- `indexOf(array, element)`: Finds element index
- `has(array, element)` / `get(array, index, default)`: Membership check / safe read
- `reverse(x)`, `unique(arr)`, `flatten(arr, depth)`, `zip(a, b)`, `enumerate(arr)`
- `count(arr, value)`, `sum(arr)`, `range(start, end, step)`

### Higher-Order Functions
- `map(arr, f)` / `filter(arr, f)` / `reduce(arr, f, init)`
- `find(arr, f)` / `some(arr, f)` / `every(arr, f)`
- `sort(arr, compare)`: Returns a new sorted array (stable; never mutates)

### Object Operations
- `keys(obj)` / `values(obj)`: Arrays of an object's keys/values
- `has(obj, key)` / `get(obj, key, default)`: Membership check / safe read
- `len(obj)`: Number of keys

### Type and Math
- `type(value)`: `"number" | "string" | "boolean" | "array" | "object" | "null" | "function"`
- `toNumber(value)` / `toString(value)`: Convert to/from a string
- `abs`, `min`, `max`, `round`, `floor`, `ceil`, `sqrt`, `pow`
- `random(seed)`: Returns a deterministic generator function

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
- `repeat(str, n)`: Repeats a string
- `padStart(str, width, pad)` / `padEnd(str, width, pad)`: Pads to a width

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
- `examples/`: Example MRT programs (`functions.mrt`, `errors.mrt`,
  `modern_syntax.mrt`, `stdlib.mrt`, `modules.mrt`, and more)
  - `examples/lib/`: library modules imported by `modules.mrt`
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
