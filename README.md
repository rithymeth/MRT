# MRT Programming Language

MRT is a modern, expressive programming language designed for simplicity and readability. It combines intuitive syntax with powerful features for both beginners and experienced programmers.

## Features

- Simple and expressive syntax
- Dynamic typing
- First-class functions: anonymous `func(...)` values, lexical closures, and
  a `map`/`filter`/`reduce`/`sort` pipeline
- Default and rest parameters, plus `...` spread, and destructuring patterns
  in declarations, bindings and assignments (`[a, b] = [b, a];`)
- `struct` types with fields, defaults and methods, and an `iter()` method
  that makes one iterable everywhere an array is
- `match` / `case` pattern matching on shape, as a statement or an expression
- Generators: `yield` makes a function a lazy, composable sequence, with
  `send`/`next` to drive one by hand, `yield*` delegation, and lazy
  `map`/`filter`
- A module system: `export` / `import` across files, namespace imports and
  re-exports
- Recoverable errors with `throw` and `try`/`catch`/`finally`, classified by
  kind and carrying a stack trace
- String interpolation, `null`, bareword object keys, and `for`-`in` loops
- Rich built-in functions for arrays, objects, strings and math
- Deterministic seeded randomness
- A browser Playground running a second interpreter kept byte-identical to
  the reference one by an automated parity check

Everything above is enough to write real libraries in the language itself:
`examples/lib/json.mrt` is a complete JSON parser and writer, structs and
all, with no help from the host.

## Installation

You can install MRT using pip:

```bash
pip install mrt-lang
```

After installation, you can run MRT programs using the `mrt` command:
```bash
mrt your_program.mrt
```

### On Windows, use `mrt-lang`

```powershell
mrt-lang your_program.mrt
```

`mrt` is not usable as a command on Windows, through no fault of this
package: `C:\Windows\System32\MRT.exe` is Microsoft's Malicious Software
Removal Tool, filenames on Windows are case-insensitive, and `System32`
comes before Python's `Scripts` directory on `PATH`. Typing `mrt` therefore
starts the malware scanner, which ignores the file you gave it and exits
without printing anything -- so the first run of this package looks like a
broken install rather than a name collision.

`mrt-lang` is the same program under a name nothing else claims, and works
everywhere. `python -m mrt your_program.mrt` also works and cannot be
shadowed at all. If you would rather keep typing `mrt`, a function in your
PowerShell profile (`notepad $PROFILE`) takes precedence over any
executable:

```powershell
function mrt { python -m mrt @args }
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
mrt hello.mrt        # or: mrt-lang hello.mrt   (required on Windows)
```

A file with no top-level `main` runs its top-level statements and nothing
else, so a file of only declarations prints nothing. That is not an error --
the command says so on stderr rather than leaving you to guess.

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

### Destructuring

```mrt
var [first, ...rest] = [1, 2, 3];                       // 1, [2, 3]
var {name, role = "unknown"} = person;

func distance([x1, y1], [x2, y2]) { ... }
for ([key, value] in pairs) { ... }
try { risky(); } catch ({kind, message}) { ... }
```

### Structs

```mrt
struct Point {
    x, y;
    func magnitude() { return sqrt(this.x * this.x + this.y * this.y); }
}

var p = Point(3, 4);
print(p);                                               // Point(x: 3, y: 4)
print(p.magnitude(), type(p));                          // 5 Point
```

### Pattern matching

```mrt
match (shape) {
    case Circle(r):                    print("circle", r);
    case [x, y]:                       print("pair", x, y);
    case {kind: "error", message: m}:  print("failed:", m);
    case n if (n > 100):               print("big");
    default:                           print("something else");
}
```

### Generators

```mrt
func naturals() { var n = 0; while (true) { yield n; n += 1; } }

print(take(naturals(), 5));                                   // [0, 1, 2, 3, 4]
print(take(map(naturals(), func(n) { return n * n; }), 4));   // [0, 1, 4, 9]

// Two-way: `yield` produces whatever send() hands back.
func echo() { var got = yield "ready"; yield "saw ${got}"; }
var g = echo();
print(next(g).value);                                         // ready
print(send(g, "hi").value);                                   // saw hi
```

### Iterable structs

```mrt
struct Span {
    lo, hi;
    func iter() { var n = this.lo; while (n < this.hi) { yield n; n += 1; } }
}

for (n in Span(1, 4)) { print(n); }                           // 1 2 3
print(reduce(Span(1, 101), func(a, b) { return a + b; }));    // 5050
```

### Match as an expression, destructuring assignment

```mrt
var label = match (code) {
    case 200: "OK",
    case n if (n >= 500): "Server Error (${n})",
    default: "Unknown"
};

var a = 1;
var b = 2;
[a, b] = [b, a];                                              // 2 1
```

### Modules

```mrt
// lib/math.mrt
export var PI = 3.14159;
export func square(n) { return n * n; }
export struct Point { x, y; }

// main.mrt
import { PI, square as sq } from "./lib/math.mrt";
import * as math from "./lib/math.mrt";
func main() { print(PI, sq(4), math.PI); }
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
+  -  *  /  %              // arithmetic (% takes the sign of the dividend)
== !=  <  >  <=  >=        // comparison
&&  ||  !                   // logical AND / OR / NOT
=  += -= *= /= %=           // assignment and compound assignment
```

Loops also support `break` and `continue`.

Reserved words: `func return if else while for print var true false break
continue null try catch finally throw in import export from as struct match
case default yield`.

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

### Generators and Iteration
- `toArray(x)`: Materialises any iterable into an array
- `take(x, n)`: The first `n` items; safe on an endless generator
- `next(g)`: One step of a generator, as `{done, value}`
- `send(g, v)`: One step, with `v` as the value of the waiting `yield`

### Higher-Order Functions
All but `sort` take any iterable: an array, string, object, generator, or a
struct with `iter()`.
- `map(seq, f)` / `filter(seq, f)`: lazy (a generator) when `seq` is a
  generator, an array otherwise
- `reduce(seq, f, init)` / `find(seq, f)` / `some(seq, f)` / `every(seq, f)`
- `sort(arr, compare)`: array only; returns a new sorted array (stable; never mutates)

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

MRT has three independent implementations of one language, held to a single
conformance corpus and required to produce byte-identical output.

- `mrt/`: the **reference interpreter** in Python -- what `pip install
  mrt-lang` ships, and the answer every other implementation is checked against
  - `lexer.py`: tokenizes source code
  - `parser.py`: parses tokens into an AST
  - `interpreter.py`: executes MRT programs
  - `ast.py`: abstract syntax tree definitions
- `compiler/`: the **MRT 2.0 compiler** in Rust -- lexer, parser, resolver with
  real source spans, and a tree-walking interpreter (`mrt-run`). See
  [`compiler/README.md`](https://github.com/rithymeth/MRT/blob/main/compiler/README.md)
- `src/`: the **Playground** -- a React app and the TypeScript interpreter
  (`src/lib/mrtInterpreter.ts`) that runs MRT in a browser
- `scripts/`: the conformance harnesses that hold the three together, plus the
  benchmark runner
- `docs/`: language specification, guide, examples and getting started
- `examples/`: example MRT programs
  - `examples/lib/`: library modules the programs above import, including
    `json.mrt` -- a JSON parser and writer written in MRT itself
  - `examples/ai_*.mrt`: programs using MRT-AI, an *engine extension* the Rust
    engines have and the other two deliberately do not. See "Engine extensions"
    in the specification, and
    [`compiler/crates/mrt-ai/README.md`](https://github.com/rithymeth/MRT/blob/main/compiler/crates/mrt-ai/README.md)
- `benchmarks/`: ten programs and a three-way timing comparison
- `tests/`: test suite

## Documentation

For more detailed information, check out:
- [Language Specification](https://github.com/rithymeth/MRT/blob/main/docs/LANGUAGE_SPEC.md): Formal grammar, types, and semantics -- the precise reference
- [Language Guide](https://github.com/rithymeth/MRT/blob/main/docs/language_guide.md): Friendlier, example-driven reference
- [Examples](https://github.com/rithymeth/MRT/blob/main/docs/examples.md): Example programs and tutorials
- [Getting Started](https://github.com/rithymeth/MRT/blob/main/docs/getting_started.md): Installation and quick start

Links are absolute so they resolve on the PyPI project page as well as on
GitHub.

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
