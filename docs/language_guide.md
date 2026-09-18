# MRT Language Guide

## Table of Contents
1. [Introduction](#introduction)
2. [Basic Syntax](#basic-syntax)
3. [Data Types](#data-types)
4. [Variables](#variables)
5. [Control Flow](#control-flow)
6. [Functions](#functions)
7. [Arrays](#arrays)
8. [Objects](#objects)
9. [String Operations](#string-operations)
10. [Error Handling](#error-handling)
11. [Sequence and Utility Functions](#sequence-and-utility-functions)
12. [Destructuring](#destructuring)
13. [Structs](#structs)
14. [Pattern Matching](#pattern-matching)
15. [Generators](#generators)
16. [Making your own types iterable](#making-your-own-types-iterable)
17. [Destructuring assignment](#destructuring-assignment)
18. [Match as an expression](#match-as-an-expression)
19. [`from` and `as` are not reserved](#from-and-as-are-not-reserved)
20. [Modules](#modules)

See also the [Operators](#operators) reference, [Break and Continue](#break-and-continue),
and the [full language specification](LANGUAGE_SPEC.md) for precise grammar and semantics.

## Introduction

MRT is a modern, expressive programming language designed for simplicity and readability. It combines intuitive syntax with powerful features, making it suitable for both beginners and experienced programmers.

## Basic Syntax

### Comments
```mrt
// Single line comment

/* Multi-line
   comment */
```

### Statements
- Each statement can end with an optional semicolon
- Blocks are defined using curly braces `{}`
- Because semicolons are optional, avoid starting a line with `-`, `(` or
  `[` right after a statement with no semicolon -- MRT doesn't treat
  newlines as separators, so `foo()\n-bar()` parses as the single
  expression `foo() - bar()`. Adding a `;` after `foo()` removes the
  ambiguity (this is the same caveat JavaScript's ASI has).

### Operators
```mrt
+  -  *  /  %              // arithmetic (% takes the sign of the dividend)
== !=  <  >  <=  >=        // comparison (numbers or strings)
&&  ||  !                   // logical AND / OR (short-circuiting) / NOT
=  += -= *= /= %=           // assignment and compound assignment
```

Compound assignment works on plain variables and on indexed targets:
```mrt
var total = 10
total += 5        // 15
total *= 2         // 30

var scores = [1, 2, 3]
scores[0] += 100   // [101, 2, 3]
```

## Data Types

MRT supports the following basic data types:

1. **Numbers**: Integer and floating-point numbers
   ```mrt
   var x = 42        // integer
   var pi = 3.14159  // float
   ```

2. **Strings**: Text enclosed in double quotes
   ```mrt
   var message = "Hello, World!"
   ```

   Strings support interpolation with `${ ... }`, which renders any value
   the way `print` does:
   ```mrt
   var name = "Ada"
   print("Hi ${name}, ${1 + 2} times!")   // Hi Ada, 3 times!
   print("Escape it with a backslash: \${name}")
   ```

3. **Booleans**: `true` or `false`
   ```mrt
   var isValid = true
   var isDone = false
   ```

4. **Arrays**: Ordered collections of values
   ```mrt
   var numbers = [1, 2, 3, 4, 5]
   var names = ["Alice", "Bob", "Charlie"]
   ```

5. **Objects**: Key/value maps, written like JSON. A bareword key is
   shorthand for the string of that name; quote it or parenthesise an
   expression when you need something else
   ```mrt
   var person = {name: "Ada", age: 36}      // same as {"name": ..., "age": ...}
   var key = "dyn"
   var computed = {(key): 1}                // {dyn: 1}
   ```

6. **Null**: the absence of a value
   ```mrt
   var nothing = null
   var alsoNothing;                         // no initializer -> null
   ```
   `null` and `false` are the only falsy values: `0`, `""`, `[]` and `{}`
   are all truthy.

7. **Functions**: values like any other -- see [Functions](#functions)
   ```mrt
   var double = func(x) { return x * 2; }
   ```

## Variables

Variables are declared using the `var` keyword:

```mrt
var name = "John"
var age = 25
var scores = [95, 87, 92]
```

## Control Flow

### If Statements
```mrt
if (condition) {
    // code
} else if (another_condition) {
    // code
} else {
    // code
}
```

### While Loops
```mrt
while (condition) {
    // code
}
```

### For Loops
```mrt
for (var i = 0; i < 10; i = i + 1) {
    // code
}
```

### For-In Loops

`for (x in ...)` walks an array's elements, a string's characters, or an
object's keys:

```mrt
for (n in [10, 20]) { print(n) }
for (ch in "hi") { print(ch) }
for (key in {a: 1, b: 2}) { print(key) }
```

Unlike the C-style loop, the loop variable is a **fresh binding each
iteration**, so functions created in the body capture that iteration's
value:

```mrt
var fns = []
for (x in [1, 2, 3]) { push(fns, func() { return x; }) }
print(map(fns, func(f) { return f(); }))   // [1, 2, 3]
```

### Break and Continue
```mrt
for (var i = 0; i < 10; i = i + 1) {
    if (i == 3) { continue }  // skip this iteration, still runs i = i + 1
    if (i == 6) { break }     // exit the loop entirely
    print(i)
}
```

Both work in `for`-`in` loops too.

## Functions

Functions are declared using the `func` keyword:

```mrt
func add(a, b) {
    return a + b
}

func greet(name) {
    print("Hello, " + name + "!")
}
```

### Functions as values

`func(...) { ... }` without a name is an *expression*, so a function can be
stored in a variable, passed as an argument, or returned:

```mrt
var double = func(x) { return x * 2; };
print(double(21));            // 42

var apply = func(f, v) { return f(v); };
print(apply(double, 5));      // 10
```

A declared function's name is an ordinary variable holding the same kind of
value, so `map([1, 2, 3], add)` works just as well as an inline `func`.

### Default and rest parameters

```mrt
func greet(name, greeting = "Hello", punct = "!") {
    return "${greeting}, ${name}${punct}";
}
print(greet("Ada"));            // Hello, Ada!
print(greet("Ada", "Hi"));      // Hi, Ada!

func total(label, ...nums) { return "${label}: ${sum(nums)}"; }
print(total("all", 1, 2, 3));   // all: 6
print(total("none"));           // none: 0   -- rest is [], never null
```

Defaults are worked out on each call, so they can refer to an earlier
parameter, and a `[]` default is a fresh array every time rather than one
shared between calls:

```mrt
func box(width, height = width) { return width * height; }
print(box(4));                  // 16

func collect(item, into = []) { push(into, item); return into; }
print(collect(1), collect(2));  // [1] [2]
```

Spread `...` expands an array into arguments, into an array literal, or
into `print`:

```mrt
var xs = [1, 2];
print(total("spread", ...xs, 3));   // spread: 6
print([0, ...xs, 9]);               // [0, 1, 2, 9]
print(...xs);                       // 1 2
```

### Closures

A function remembers the scope it was *defined* in, which makes private
state easy:

```mrt
func makeCounter() {
    var count = 0;
    return func() { count += 1; return count; };
}

var next = makeCounter();
next(); next();
print(next());                // 3
```

Each call to `makeCounter()` produces an independent counter.

### Higher-order built-ins

```mrt
var nums = [5, 3, 8, 1];

print(map(nums, func(x) { return x * x; }));           // [25, 9, 64, 1]
print(filter(nums, func(x) { return x > 3; }));        // [5, 8]
print(reduce(nums, func(a, b) { return a + b; }));     // 17
print(find(nums, func(x) { return x > 4; }));          // 5
print(some(nums, func(x) { return x > 7; }));          // true
print(every(nums, func(x) { return x > 0; }));         // true
```

All of these except `sort` take **any iterable** — an array, a string, an
object's keys, a generator, or a struct with an `iter()` method:

```mrt
print(map("abc", toUpper));                            // [A, B, C]
print(filter({a: 1, bb: 2}, func(k) { return len(k) > 1; }));   // [bb]
```

`map` and `filter` hand back a **generator** when given one, so a pipeline
over an endless sequence stays lazy — see Generators, below.

`sort(arr)` returns a **new** sorted array and leaves the original alone.
Without a comparator the array must be all numbers or all strings; with one,
return a negative number, zero, or a positive number:

```mrt
print(sort([3, 1, 2]));                                    // [1, 2, 3]
print(sort([3, 1, 2], func(a, b) { return b - a; }));      // [3, 2, 1]

var people = [{name: "Cy", age: 44}, {name: "Ada", age: 36}];
var byAge = sort(people, func(a, b) { return a.age - b.age; });
print(map(byAge, func(p) { return p.name; }));             // [Ada, Cy]
```

See `examples/functions.mrt` for a longer tour.

## Arrays

MRT provides comprehensive array operations:

### Array Creation
```mrt
var numbers = [1, 2, 3, 4, 5]
```

### Built-in Array Functions

1. **len(array)**: Returns array length
   ```mrt
   var length = len(numbers)  // returns 5
   ```

2. **push(array, element)**: Adds element to end
   ```mrt
   push(numbers, 6)  // numbers is now [1, 2, 3, 4, 5, 6]
   ```

3. **pop(array)**: Removes and returns last element
   ```mrt
   var last = pop(numbers)  // removes and returns 6
   ```

4. **slice(array, start, end)**: Returns array subset
   ```mrt
   var subset = slice(numbers, 1, 4)  // returns [2, 3, 4]
   ```

5. **join(array, separator)**: Joins elements into string
   ```mrt
   var str = join(numbers, ", ")  // "1, 2, 3, 4, 5"
   ```

6. **indexOf(array, element)**: Finds element index
   ```mrt
   var index = indexOf(numbers, 3)  // returns 2
   ```

7. **has(array, element)**: Checks whether a value is present
   ```mrt
   print(has(numbers, 3))  // true
   ```

8. **get(array, index, default)**: Reads an index without raising if it's
   out of bounds
   ```mrt
   print(get(numbers, 99, "n/a"))  // "n/a"
   ```

## Objects

Objects are key/value maps. Keys are expressions -- write them quoted,
JSON-style; `{name: "Ada"}` (unquoted `name`) is *not* the same thing (see
the [language spec](LANGUAGE_SPEC.md#known-ambiguities-by-design) for why).

### Object Creation and Access
```mrt
var person = {"name": "Ada", "age": 36}

print(person["name"])   // bracket access
print(person.name)      // dot access -- sugar for person["name"]

person.age = 37         // dot assignment
person["job"] = "Mathematician"  // bracket assignment (adds a new key)
```

### Built-in Object Functions

1. **keys(obj)**: Returns an array of an object's keys
   ```mrt
   print(keys(person))  // [name, age, job]
   ```

2. **values(obj)**: Returns an array of an object's values
   ```mrt
   print(values(person))
   ```

3. **has(obj, key)**: Checks whether a key exists
   ```mrt
   print(has(person, "name"))  // true
   ```

4. **get(obj, key, default)**: Reads a key without raising if it's missing
   ```mrt
   print(get(person, "email", "unknown"))  // "unknown"
   ```

5. **len(obj)**: Returns the number of keys
   ```mrt
   print(len(person))
   ```

## Type and Math Functions

- **type(value)**: Returns `"number"`, `"string"`, `"boolean"`, `"array"`,
  `"object"`, `"null"`, or `"function"`
- **toNumber(value)** / **toString(value)**: Convert to/from a string
- **abs(n)**, **min(...)**, **max(...)**, **round(n, digits?)**,
  **floor(n)**, **ceil(n)**, **sqrt(n)**, **pow(base, exp)**

```mrt
print(type([1, 2]))          // "array"
print(toNumber("42") + 1)    // 43
print(round(3.14159, 2))     // 3.14
print(max([4, 9, 2]))        // 9
```

## String Operations

MRT offers powerful string manipulation functions:

### String Functions

1. **split(str, separator)**: Splits string into array
   ```mrt
   var words = split("hello world", " ")  // ["hello", "world"]
   ```

2. **substring(str, start, end)**: Extracts string portion
   ```mrt
   var part = substring("Hello", 1, 4)  // "ell"
   ```

3. **toUpper(str)**: Converts to uppercase
   ```mrt
   var upper = toUpper("hello")  // "HELLO"
   ```

4. **toLower(str)**: Converts to lowercase
   ```mrt
   var lower = toLower("HELLO")  // "hello"
   ```

5. **trim(str)**: Removes whitespace
   ```mrt
   var trimmed = trim("  hello  ")  // "hello"
   ```

6. **replace(str, old, new)**: Replaces text
   ```mrt
   var new_str = replace("hello world", "world", "MRT")  // "hello MRT"
   ```

7. **startsWith(str, prefix)**: Checks string start
   ```mrt
   var starts = startsWith("hello", "he")  // true
   ```

8. **endsWith(str, suffix)**: Checks string end
   ```mrt
   var ends = endsWith("hello", "lo")  // true
   ```

9. **contains(str, substr)**: Checks for substring
   ```mrt
   var has = contains("hello world", "world")  // true
   ```


## Error Handling

Use `throw` to raise an error and `try`/`catch`/`finally` to handle it.

```mrt
try {
    throw "something went wrong";
} catch (e) {
    print("caught:", e);
} finally {
    print("always runs");
}
```

Any value can be thrown, so an object makes a useful structured error:

```mrt
func validateAge(age) {
    if (age < 0) {
        throw {field: "age", reason: "must not be negative"};
    }
    return age;
}

try {
    validateAge(-5);
} catch (e) {
    print(e.field, "-", e.reason);      // age - must not be negative
}
```

The interpreter's own runtime errors are catchable too. They arrive as an
object with `message` and `line` keys:

```mrt
try {
    var arr = [1, 2, 3];
    print(arr[99]);
} catch (e) {
    print(e.message);   // Array index 99 out of bounds for array of length 3.
}
```

That makes it possible to recover from a failure instead of halting:

```mrt
func safeDivide(a, b) {
    try { return a / b; } catch (e) { return null; }
}

print(safeDivide(10, 2));   // 5
print(safeDivide(10, 0));   // null
```

### Sorting errors by kind

Every interpreter-raised error carries a `kind` you can branch on, and a
`catch` clause can take an `if` guard. Clauses are tried in order, and one
without a guard is the fallback:

```mrt
try {
    risky();
}
catch (e) if (get(e, "kind", "") == "IndexError") { print("bad index"); }
catch (e) if (get(e, "kind", "") == "ArithmeticError") { print("bad maths"); }
catch (e) { print("something else:", e); }
```

The kinds are `TypeError`, `ArityError`, `IndexError`, `KeyError`,
`NameError`, `ValueError`, `ArithmeticError` and `RuntimeError`.

Use `get(e, "kind", "")` rather than `e.kind` in a guard: a `throw` can
deliver any value, including a string with no `kind` at all, and `e.kind`
would then fail *inside* the guard.

### Stack traces

`e.stack` names the functions the error came through, innermost first:

```mrt
func c() { var a = [1]; return a[99]; }
func b() { return c(); }
func main() {
    try { b(); } catch (e) { print(e.stack); }   // [c, b]
}
```

Notes:

- `finally` runs on every exit path — normal completion, a caught error, an
  error still propagating, and a `return` passing through it.
- You need at least one `catch` clause or a `finally`.
- An unguarded `catch` catches *everything*, including typos inside the
  `try`, so keep `try` blocks narrow.
- If no guard matches, the error keeps propagating rather than vanishing.

See `examples/errors.mrt`.

## Sequence and Utility Functions

```mrt
print(reverse([1, 2, 3]));            // [3, 2, 1]   (also works on strings)
print(unique([3, 1, 3, 2]));          // [3, 1, 2]
print(flatten([[1], [2, [3]]]));      // [1, 2, [3]]
print(flatten([[1], [2, [3]]], 2));   // [1, 2, 3]
print(zip([1, 2, 3], ["a", "b"]));    // [[1, a], [2, b]]
print(enumerate(["x", "y"]));         // [[0, x], [1, y]]
print(count([1, 1, 2], 1));           // 2
print(sum([1, 2, 3]));                // 6
print(range(4));                      // [0, 1, 2, 3]
print(range(2, 6));                   // [2, 3, 4, 5]
print(range(0, 10, 3));               // [0, 3, 6, 9]
print(range(3, 0, -1));               // [3, 2, 1]
```

String helpers:

```mrt
print(repeat("-=", 3));               // -=-=-=
print(padStart("7", 3, "0"));         // 007
print(padEnd("7", 3, "."));           // 7..
```

### Seeded randomness

`random(seed)` returns a generator function. The same seed always produces
the same sequence — in this interpreter and in the browser Playground alike.
There is no unseeded global random.

```mrt
var rng = random(2026);
for (i in range(3)) {
    print(floor(rng() * 6) + 1);      // a repeatable dice roll
}
```

See `examples/stdlib.mrt`.


## Modules

A program can span several files. Mark what a file offers with `export`,
and pull it in elsewhere with `import`.

```mrt
// lib/math.mrt
export var PI = 3.14159;
export func square(n) { return n * n; }
func helper() { return "private"; }     // not exported

// main.mrt
import { PI, square } from "./lib/math.mrt";
import { square as sq } from "./lib/math.mrt";   // or rename it

func main() { print(PI, square(4), sq(3)); }
```

Run it the usual way — the entry file's directory is what relative paths
resolve against:

```bash
python -m src main.mrt
```

Things worth knowing:

- **Paths are explicitly relative and name a file.** They start with `./`
  or `../` and include the `.mrt` extension. There is no search path, so an
  import always tells you exactly which file it loads.
- **`import` and `export` only work at the top level of a file** — not
  inside a function or any other block.
- **A module runs once**, however many files import it. Its top-level code
  (including `print`) runs at that first import.
- **Anything not exported stays private** to its file.
- **Circular imports are reported**, with the cycle named, rather than
  hanging or handing back a half-built module.

Functions from a module close over that module's variables, so shared state
really is shared:

```mrt
// counter.mrt
var n = 0;
export func next() { n += 1; return n; }
export func reset() { n = 0; }
```

Every importer of `counter.mrt` sees the same `n`.

### In the browser

The Playground edits one buffer, so it ships a couple of built-in modules
(`./stats.mrt` and `./text.mrt`) you can import to try the feature out. The
same program run from the command line resolves those imports against real
files instead — the semantics are identical.

See `examples/modules.mrt` and `examples/lib/`.


## Destructuring

Wherever you name something, you can take it apart instead.

```mrt
var [first, second] = [1, 2];
var [head, ...tail] = [1, 2, 3, 4];
var {name, role} = person;
var {name: who, missing = "n/a"} = person;
```

It works in function parameters, `for`-`in` loops and `catch` clauses too:

```mrt
func distance([x1, y1], [x2, y2]) {
    return sqrt(pow(x2 - x1, 2) + pow(y2 - y1, 2));
}

for ([key, value] in [["a", 1], ["b", 2]]) { print(key, value); }

try { risky(); } catch ({kind, message}) { print(kind, message); }
```

Patterns nest, and `...rest` collects what's left:

```mrt
var {items: [firstItem, ...others], meta: {tag}} = payload;
var {name, ...everythingElse} = person;
```

Destructuring is **strict**: a missing element or key is an error unless
that slot has a default. It never quietly hands you `null`.

```mrt
var [a, b] = [1];        // error: the array has 1 element(s)...
var [a, b = 0] = [1];    // fine: b is 0
```

See `examples/destructuring.mrt`.

## Structs

A `struct` gives a value a name and a guaranteed shape.

```mrt
struct Point {
    x, y;

    func magnitude() { return sqrt(this.x * this.x + this.y * this.y); }
    func scaled(k) { return Point(this.x * k, this.y * k); }
}

var p = Point(3, 4);
print(p);                // Point(x: 3, y: 4)
print(p.magnitude());    // 5
print(type(p));          // Point
```

- Construct with the struct's name and positional field values.
- Methods see `this`, and a method pulled off an instance stays bound to it.
- Fields have defaults, and a default can use the fields before it:

```mrt
struct Config { host, port = 8080, url = "http://${host}:${port}"; }
print(Config("example"));     // Config(host: example, port: 8080, url: http://example:8080)
```

- Fields are mutable but **fixed** — assigning an undeclared one is an error:

```mrt
p.x = 6;      // fine
p.z = 1;      // KeyError: Struct Point has no field "z".
```

- Equality is structural and per-struct: `Point(1,2) == Point(1,2)` is
  `true`, but a `Point` never equals a plain object with the same keys.
- `keys`, `values`, `has`, `get` and object destructuring see the **fields**,
  not the methods.

See `examples/structs.mrt`.

## Pattern Matching

`match` branches on a value's shape and binds as it goes.

```mrt
match (value) {
    case 0:                           print("zero");
    case [x, y]:                      print("a pair", x, y);
    case {kind: "error", message: m}: print("failed:", m);
    case Point(x, y):                 print("point", x, y);
    case n if (n > 100):              print("big");
    case n:                           print("something else", n);
    default:                          print("nothing matched");
}
```

The first case that fits wins, and there's no fall-through — no `break`
needed.

Patterns can be a literal, a name (which matches anything and binds it), an
array, an object, or a struct:

| Pattern | Matches |
|---|---|
| `0`, `"s"`, `true`, `null`, `-1` | that exact value |
| `n` | anything, bound to `n` |
| `[a, b]` | an array of exactly two elements |
| `[a, ...rest]` | an array of at least one |
| `{kind: k}` | an object (or struct) that has a `kind` key |
| `Circle(r)` | an instance of the `Circle` struct |

Two things to remember:

- A **match** array pattern needs an exact length (unless it has a rest),
  while destructuring ignores extras. `case [x]:` will not match `[1, 2]`.
- If nothing matches and there's no `default`, that's an **error** — MRT
  would rather tell you than quietly do nothing.

See `examples/matching.mrt`.

## Generators

A function containing `yield` is lazy. Calling it runs nothing; it hands
back a sequence that produces values on demand — so an endless one is
perfectly usable.

```mrt
func naturals() {
    var n = 0;
    while (true) { yield n; n += 1; }
}

print(take(naturals(), 5));    // [0, 1, 2, 3, 4]
```

Generators can consume other generators, which is how you build a pipeline
that never materialises anything in the middle:

```mrt
func mapped(source, f) { for (x in source) { yield f(x); } }
func until(source, pred) { for (x in source) { if (!pred(x)) { return; } yield x; } }

var squares = mapped(naturals(), func(n) { return n * n; });
print(toArray(until(squares, func(n) { return n < 100; })));
```

`for`-`in` pulls lazily, so `break` just stops asking:

```mrt
for (n in naturals()) {
    if (n > 3) { break; }
    print(n);
}
```

`map` and `filter` do the same thing with less typing: given a generator
they return a generator, so the pipeline above is just

```mrt
print(take(map(naturals(), func(n) { return n * n; }), 5));   // [0, 1, 4, 9, 16]
print(take(filter(naturals(), func(n) { return n % 2 == 0; }), 4));  // [0, 2, 4, 6]
```

On an array — or a string, or an object — they stay eager and give you an
array back.

### Resuming a generator

A generator is a *position* in a sequence. A consumer that stops early
leaves it suspended, and the next one picks up from there:

```mrt
var g = naturals();
print(take(g, 3));      // [0, 1, 2]
print(take(g, 2));      // [3, 4]
```

Once it has run out, iterating it again is an error rather than an empty
loop — call the generator function for a fresh sequence.

### Sending values back in

`yield` is a statement, with one exception: as the *whole* right-hand side
of a declaration or an assignment, it produces whatever the consumer sends
in. `next(g)` advances one step and returns `{done, value}`; `send(g, v)`
does the same and hands `v` to the waiting `yield`.

```mrt
func echo() {
    var got = yield "ready";
    while (got != "stop") { got = yield "saw ${got}"; }
}

var g = echo();
print(next(g).value);          // ready
print(send(g, "a").value);     // saw a
print(send(g, "stop").done);   // true
```

The value sent on the first step is dropped — the body hasn't reached a
`yield` yet to catch it. When nothing sends anything (a `for`-`in` loop,
`toArray`), a `yield` expression is `null`.

### `yield*`

`yield* other;` re-yields everything in another iterable as if it were
yours, and forwards sent values into it:

```mrt
func inner() { yield 1; yield 2; }
func outer() { yield 0; yield* inner(); yield* [8, 9]; }
print(toArray(outer()));       // [0, 1, 2, 8, 9]
```

Notes:

- `return` ends the sequence early.
- `toArray(x)` materialises any iterable; `take(x, n)` takes the first `n`,
  pulling exactly `n`.
- Struct methods can be generators too.
- `yield` outside the four assignment shapes (`f(yield 1)`, `1 + yield 2`)
  is a syntax error, and `yield*` is a statement only.

See `examples/generators.mrt` and `examples/coroutines.mrt`.

## Making your own types iterable

A struct with a method named `iter()` is iterable everywhere an array is —
`for`-`in`, `toArray`, `take`, `map`, `filter`, `reduce`, `find`, `some`,
`every`:

```mrt
struct Span {
    lo, hi;
    func iter() { var n = this.lo; while (n < this.hi) { yield n; n += 1; } }
}

for (n in Span(1, 4)) { print(n); }                          // 1 2 3
print(reduce(Span(1, 101), func(a, b) { return a + b; }));   // 5050
print(take(Span(0, 1000000), 3));                            // [0, 1, 2]
```

`iter()` may return anything iterable, not only a generator:

```mrt
struct Deck { cards; func iter() { return this.cards; } }
print(toArray(Deck(["A", "K"])));    // [A, K]
```

It is called afresh each time, so unlike a generator a struct isn't used up
by being iterated. A struct *without* `iter()` iterates its field names,
just as a plain object does.

See `examples/lazy.mrt`.

## Destructuring assignment

The patterns from the Destructuring section also assign to variables that
already exist, which makes a swap one line and needs no temporary:

```mrt
var a = 1;
var b = 2;
[a, b] = [b, a];
print(a, b);                    // 2 1

var x = 0; var y = 0;
{x, y} = {x: 10, y: 20};
print(x, y);                    // 10 20

var head = 0; var tail = [];
[head, ...tail] = [1, 2, 3];
print(head, tail);              // 1 [2, 3]
```

Every name in the pattern must already be declared — nothing new is
created, so the assignment reaches outward through enclosing scopes exactly
as `x = 1` does. The leaves are plain names: `[obj.field] = pair;` is not a
thing.

A leading `{` is still a block and a leading `[` still an array literal
whenever no `=` follows, so nothing that used to parse changed meaning.

## Match as an expression

In expression position a `match` *is* its value. Arms are separated by
commas and each is a single expression:

```mrt
var label = match (code) {
    case 200: "OK",
    case 404: "Not Found",
    case n if (n >= 500): "Server Error (${n})",
    default: "Unknown (${code})"
};

print("area: ${ match (shape) { case Circle(r): 3.14159 * r * r, default: 0 } }");
```

Same patterns, same guards, same "no match and no `default` is an error"
rule as the statement form. Which one you get depends only on position: a
`match` that starts a statement is the statement form.

See `examples/expressions.mrt`.

## `from` and `as` are not reserved

They mean something only inside an `import` or `export` clause, so you can
use them freely as names:

```mrt
struct Trip { from, to }
var route = {from: "LHR", to: "JFK"};
func fare(from, as) { return "${from} -> ${as}"; }
```
