# The MRT Language Specification

This document is the authoritative description of MRT as implemented by
`src/` (the reference implementation, in Python) and mirrored by
`src/lib/mrtInterpreter.ts` (the browser Playground). Where the two ever
disagree, this spec — and the reference implementation's test suite in
`tests/` — is what's correct; the Playground should be brought in line.

It supersedes informal descriptions in the README and `docs/*.md`, which
are kept as friendlier, example-driven introductions. This document exists
to answer a harder question precisely: *given this exact source text, what
happens?*

## Contents

1. [Design goals and non-goals](#design-goals-and-non-goals)
2. [Lexical grammar](#lexical-grammar)
3. [Types and values](#types-and-values)
4. [Grammar (EBNF)](#grammar-ebnf)
5. [Expressions and operator precedence](#expressions-and-operator-precedence)
6. [Statements and semantics](#statements-and-semantics)
7. [Scoping and closures](#scoping-and-closures)
8. [Error model](#error-model)
9. [Built-in functions](#built-in-functions)
10. [Modules](#modules)
11. [Known ambiguities (by design)](#known-ambiguities-by-design)
12. [Future work / explicitly out of scope](#future-work--explicitly-out-of-scope)

## Design goals and non-goals

MRT is a small, dynamically-typed, tree-walked scripting language meant to
be easy to read and easy to implement in an afternoon of study. It is not
meant to be fast or to interoperate with other languages. Concretely:

- **Goals:** readable C/JS-family syntax; a handful of clean, orthogonal
  data types (number, string, boolean, array, object, null, function);
  first-class functions with lexical closures; predictable, strict runtime
  errors (index out of bounds, division by zero, and similar always raise
  rather than silently producing `null` or `NaN`) that a program may
  nonetheless catch and recover from; a reference implementation short
  enough to read start to finish.
- **Non-goals (for now):** a static type system, integers distinct from
  floats, or user-defined types and classes. See
  [Future work](#future-work--explicitly-out-of-scope).

## Lexical grammar

### Whitespace and comments

Spaces, tabs and carriage returns are insignificant. Newlines are *not*
statement separators (see [Known ambiguities](#known-ambiguities-by-design)).

```
// a line comment, runs to end of line
/* a block comment,
   may span multiple lines */
```

### Literals

| Kind    | Examples                    | Notes |
|---------|------------------------------|-------|
| Number  | `0`, `42`, `3.14159`         | Always stored as a 64-bit float internally, even for `42`. Printed without a trailing `.0` when the value is whole. |
| String  | `"hello"`, `"line1\nline2"`  | Double-quoted only. Supports the escapes `\n \t \r \" \\ \0 \$`. |
| Template | `"Hi ${name}, ${1 + 2}"`    | A string containing one or more `${ expression }` runs. See [String interpolation](#string-interpolation) below. |
| Boolean | `true`, `false`               | |
| Null    | `null`                        | Also the value of a function that falls off its end without a `return`, and of a `var` declared without an initializer. `toString(x)` renders it as `"null"`. |

### String interpolation

Inside a double-quoted string, `${ expression }` is replaced by the
expression's value rendered exactly as `print` and `toString` render it:

```mrt
var name = "Ada";
var scores = [90, 85];
print("Hi ${name}, you have ${len(scores)} scores: ${scores}");
// Hi Ada, you have 2 scores: [90, 85]
```

Details:

- The embedded text is a full expression, including calls, object
  literals, and even an immediately-invoked function — `"${ func(x) { return x + 1; }(41) }"`
  prints `42`. Nested braces and nested string literals are tracked, so
  `"${ get({"a": 5}, "a") }"` works.
- Interpolation is resolved at *parse* time: the lexer captures each
  fragment's source text and the parser re-lexes it into a real AST. There
  is no runtime `eval`, and a malformed fragment is a syntax error
  reported against the line the fragment appears on.
- `\${` produces a literal `${`. An empty `${}` is a syntax error.
- A string containing no `${` is lexed exactly as before, so interpolation
  costs existing programs nothing.

### Identifiers and keywords

Identifiers: `[a-zA-Z_][a-zA-Z0-9_]*`.

Reserved keywords (cannot be used as identifiers):

```
func return if else while for print var
true false break continue
null try catch finally throw in
import export from as
```

The last row is new in the current revision: `import`, `export`, `from` and
`as` are now reserved, so a program that used any of them as a variable or
function name no longer parses. (The row before it added `null`, `try`,
`catch`, `finally`, `throw` and `in` in the previous revision.)

### Operators and punctuation

```
+  -  *  /  %                  arithmetic
+= -= *= /= %=                 compound assignment
=                                assignment
== !=  <  >  <=  >=            comparison
&&  ||  !                      logical AND / OR / NOT
( ) { } [ ]  , ; . :           grouping / delimiters
...                            rest parameter / spread
```

## Types and values

MRT has exactly seven kinds of runtime value:

| Type       | Example                        | Truthy?                                   |
|------------|---------------------------------|--------------------------------------------|
| `number`   | `42`, `-3.5`                    | always truthy (including `0`) |
| `string`   | `"hi"`                          | always truthy (including `""`) |
| `boolean`  | `true`, `false`                 | its own value |
| `array`    | `[1, 2, 3]`                     | always truthy (including `[]`) |
| `object`   | `{"a": 1}`                      | always truthy (including `{}`) |
| `null`     | *(see above)*                   | always falsy |
| `function` | a `func` declaration or a `func(...) { ... }` expression, passed by reference | always truthy |

**Truthiness** (used by `if`, `while`, `for`'s condition, and `&&`/`||`):
only `null` and `false` are falsy. Every other value — including `0`,
`""`, `[]`, and `{}` — is truthy. This is a deliberate simplification over
JavaScript/Python's "many things are falsy" rules: in MRT, *falsy* means
*absent or explicitly false*, nothing else. Use `len(x) == 0` to test for
emptiness.

**Equality** (`==`, `!=`): structural. Two arrays or objects are equal if
their elements/entries are (recursively) equal — there is no reference
identity operator. `null == null` is `true`. A boolean is never equal to a
number (`true != 1`), unlike Python/JS.

**Ordering** (`<`, `>`, `<=`, `>=`): defined for two numbers (numeric
comparison) or two strings (lexicographic, by Unicode code point). Mixing
types, or comparing arrays/objects/booleans/`null`, is a runtime error.

**Arrays** (`array`) are ordered, mutable, growable sequences, indexed
from `0` with `arr[i]`. Reading or writing out of bounds is a runtime
error (arrays never silently extend).

**Objects** (`object`) are unordered-by-contract (the reference
implementation preserves insertion order, but don't depend on it) string/
number/boolean-keyed maps. A bareword key is shorthand for the string of
that name, so `{name: "Ada"}` and `{"name": "Ada"}` mean the same thing;
parenthesise to key by a variable's *value* (`{(k): 1}`). `obj.name` is
sugar for `obj["name"]` and works for both reading and writing.

## Grammar (EBNF)

Terminals are quoted or `ALL_CAPS` (matching the lexer's token names).
`(x)?` is optional, `(x)*` is zero-or-more, `(x)+` is one-or-more.

```ebnf
program        = declaration* EOF ;

declaration    = importDecl
               | exportDecl
               | funcDecl
               | varDecl
               | statement ;

importDecl     = "import" "{" importNames? "}" "from" STRING ";"? ;
importNames    = importName ( "," importName )* ;
importName     = IDENTIFIER ( "as" IDENTIFIER )? ;
               (* only valid at the top level of a file *)

exportDecl     = "export" ( funcDecl | varDecl ) ;
               (* only valid at the top level of a file *)

funcDecl       = "func" IDENTIFIER "(" parameters? ")" block ;
parameters     = parameter ( "," parameter )* ;
parameter      = IDENTIFIER ( "=" expression )?
               | "..." IDENTIFIER ;
               (* a rest parameter must come last; a required parameter may
                  not follow one with a default *)

varDecl        = "var" IDENTIFIER ( "=" expression )? ";"? ;

statement      = exprStmt
               | forStmt
               | forInStmt
               | ifStmt
               | printStmt
               | returnStmt
               | whileStmt
               | breakStmt
               | continueStmt
               | tryStmt
               | throwStmt
               | block ;

exprStmt       = expression ";"? ;

forStmt        = "for" "(" ( varDecl | exprStmt | ";" )
                            expression? ";"
                            expression? ")" statement ;

forInStmt      = "for" "(" "var"? IDENTIFIER "in" expression ")" statement ;

ifStmt         = "if" "(" expression ")" statement ( "else" statement )? ;

printStmt      = "print" "(" ( expression ( "," expression )* )? ")" ";"? ;

returnStmt     = "return" expression? ";"?  ;
               (* the return value, if any, must start on the same
                  source line as `return` -- see Known ambiguities *)

whileStmt      = "while" "(" expression ")" statement ;

breakStmt      = "break" ";"? ;
continueStmt   = "continue" ";"? ;

tryStmt        = "try" block catchClause* ( "finally" block )? ;
catchClause    = "catch" "(" IDENTIFIER ")" ( "if" "(" expression ")" )? block ;
               (* at least one catch clause or a finally must be present;
                  clauses are tried in order, first match wins *)

throwStmt      = "throw" expression ";"? ;

block          = "{" declaration* "}" ;

expression     = assignment ;

assignment     = ( call "." )? IDENTIFIER ( "=" | "+=" | "-=" | "*=" | "/=" | "%=" ) assignment
               | logic_or ;

logic_or       = logic_and ( "||" logic_and )* ;
logic_and      = equality ( "&&" equality )* ;
equality       = comparison ( ( "==" | "!=" ) comparison )* ;
comparison     = term ( ( ">" | ">=" | "<" | "<=" ) term )* ;
term           = factor ( ( "+" | "-" ) factor )* ;
factor         = unary ( ( "*" | "/" | "%" ) unary )* ;
unary          = ( "-" | "!" ) unary | call ;

call           = primary ( "(" arguments? ")" | "[" expression "]" | "." IDENTIFIER )* ;
arguments      = argument ( "," argument )* ;
argument       = "..." expression | expression ;
               (* `...` is also allowed in array literals and print, and
                  nowhere else *)

primary        = NUMBER | STRING | TEMPLATE | "true" | "false" | "null"
               | IDENTIFIER
               | funcExpr
               | "(" expression ")"
               | "[" ( argument ( "," argument )* )? "]"
               | "{" ( objectEntry ( "," objectEntry )* )? "}" ;

funcExpr       = "func" IDENTIFIER? "(" parameters? ")" block ;
               (* takes the same parameter forms as funcDecl *)

objectEntry    = ( IDENTIFIER | expression ) ":" expression ;
               (* a bareword IDENTIFIER key is the *string* of that name;
                  see Known ambiguities *)

TEMPLATE       = (* a string literal containing >= 1 "${" expression "}" *) ;
```

Notes on the grammar as written above (a simplification of the actual
recursive-descent parser in `src/parser.py`):

- The real `assignment` production accepts *any* `logic_or` result as the
  left-hand side and only checks afterwards (once it sees `=` or a
  compound-assignment operator) that it was actually a variable or an
  indexing expression (`x`, `arr[i]`, or `obj.key`) — anything else is a
  parse error ("Invalid assignment target").
- `x += y` desugars to `x = x + y` at parse time (and similarly for
  `-= *= /= %=`); there is no separate compound-assignment AST node.
- `obj.key` desugars to `obj["key"]` at parse time.
- `func` is a *declaration* only when immediately followed by an
  identifier; `func(` in statement position starts an anonymous function
  expression. This is decided with one token of lookahead.
- `for (` likewise uses one token of lookahead to tell `forInStmt` from
  the C-style `forStmt`.
- A TEMPLATE token carries the alternating literal-text and
  expression-source fragments; the parser lexes and parses each fragment
  into a normal expression, so an interpolation is an ordinary AST node
  by the time the interpreter sees it.
- The parser tracks block depth so that `import` and `export` can be
  rejected anywhere but the top level of a file.
- `...` is a single token. `a..b` is therefore still a syntax error, and
  `...` outside an argument list, array literal or `print` is rejected.

## Expressions and operator precedence

From lowest to highest precedence (matching the grammar's nesting above):

| Precedence (low → high) | Operators             | Associativity |
|--------------------------|------------------------|----------------|
| 1 | `=` `+=` `-=` `*=` `/=` `%=` | right |
| 2 | `\|\|`                    | left |
| 3 | `&&`                      | left |
| 4 | `==` `!=`                 | left |
| 5 | `<` `>` `<=` `>=`         | left |
| 6 | `+` `-` (binary)          | left |
| 7 | `*` `/` `%`               | left |
| 8 | unary `-` `!`             | right (prefix) |
| 9 | call `()`, index `[]`, member `.` | left |

`&&` and `||` **short-circuit**: for `a && b`, `b` is only evaluated if
`a` is truthy; for `a || b`, `b` is only evaluated if `a` is falsy. Unlike
`==`, the result is the *last evaluated operand's value*, not necessarily
a boolean (e.g. `0 || "fallback"` — remember `0` is truthy in MRT, so this
is actually `0`, not `"fallback"`; there is no numeric-falsiness the way
JS has).

`+` is overloaded: if either operand is a string, the result is string
concatenation (with the other operand converted via the same rules
`print`/`toString` use — so `"x = " + 5` is `"x = 5"`, not `"x = 5.0"`).
Otherwise both operands must be numbers.

## Statements and semantics

- **`var`**: declares a new binding in the *current* block scope,
  shadowing any outer binding of the same name. `var x;` initializes to
  `null`.
- **`if` / `else`**: standard; `else if` is just `else` followed by
  another `if` statement (no special grammar needed).
- **`while`**: standard.
- **`for (init; cond; incr) body`**: `init` runs once; `cond` is checked
  before each iteration (defaulting to `true` if omitted); `incr` runs
  after each iteration, *including* one that used `continue`. `init`'s
  scope is the loop's own scope (a `var i` in the initializer doesn't leak
  into the surrounding block).
- **`for (x in iterable) body`** / **`for (var x in iterable) body`**:
  iterates an array's *elements*, a string's *characters*, or an object's
  *keys* (in insertion order). Anything else is a runtime error. The loop
  variable is bound afresh in a new scope on each iteration — see
  [Scoping and closures](#scoping-and-closures) for why that matters. The
  optional `var` is accepted but changes nothing.
- **`break`**: exits the nearest enclosing `while`, `for`, or `for`-`in`
  immediately.
- **`continue`**: skips to the next iteration — for `while`, straight to
  the condition check; for `for`, first running the increment, then the
  condition check; for `for`-`in`, straight to the next element.
- **`throw expr`**: raises `expr` (*any* MRT value) as an error, unwinding
  until a `catch` catches it. Uncaught, it halts the program with
  `Runtime Error: Uncaught <value>`.
- **`try` / `catch` / `finally`**: see [Error model](#error-model).
- **`return`**: exits the current function immediately with a value
  (`null` if none given). A bare `return` at the top level, outside any
  function, is a runtime error (there's no implicit outer function).
- **`print(a, b, ...)`**: evaluates each argument left to right, joins
  their `stringify`d forms with a single space, and writes one line.
  `print()` with no arguments prints an empty line.
- **Program entry point**: top-level `func` declarations are registered
  first (so they may refer to each other in any order), then every other
  top-level statement runs in source order, and finally — if a function
  named `main` exists — it is called with no arguments.

  **This changed in the current revision.** Previously, a program that
  defined `main` had its other top-level statements *skipped* entirely; a
  top-level `var` sitting next to a `main` silently never ran. They now
  always run, before `main` is called. Imports made the old behaviour
  untenable (a top-level `import` cannot be skipped), and the old behaviour
  was surprising in its own right.

## Scoping and closures

MRT is lexically (statically) scoped with one environment per block.
Every `{ ... }` — a function body, an `if`/`while`/`for` body, or a bare
block — introduces a new scope whose parent is the enclosing scope.
Variable lookup walks outward through parent scopes; assignment
(`x = ...`, not `var x = ...`) also walks outward and assigns to the
*nearest* existing binding, raising a runtime error if none exists (MRT
has no implicit global creation on assignment).

Functions are values and close over their defining environment, not their
call-site environment — standard lexical closures:

```mrt
func makeCounter() {
    var count = 0;
    func increment() {
        count += 1;
        return count;
    }
    return increment;
}

func main() {
    var counter = makeCounter();
    print(counter());  // 1
    print(counter());  // 2 -- count persisted between calls
}
```

Functions are fully first-class. Besides the `func name(...)` declaration
above, `func(...) { ... }` is an *expression* producing an anonymous
function value:

```mrt
var double = func(x) { return x * 2; };
var adder  = func(n) { return func(x) { return x + n; }; };
print(map([1, 2, 3], double));   // [2, 4, 6]
print(adder(10)(5));             // 15
```

A declaration and an expression produce the same kind of value; they
differ only in that a declaration also binds a name (and that the value
prints as `<function name>` rather than `<function>`). An anonymous
function may optionally carry a name purely for that printed form:
`func fact(n) { ... }` in expression position.

### Parameters: defaults, rest, and spread

```mrt
func greet(name, greeting = "Hello", punct = "!") {
    return "${greeting}, ${name}${punct}";
}
func total(label, ...nums) { return label + ": " + toString(sum(nums)); }

print(greet("Ada"));              // Hello, Ada!
print(total("all", 1, 2, 3));     // all: 6
```

- **Defaults** are evaluated *at call time*, in the callee's own scope, and
  only for arguments that were not supplied. Two consequences worth
  knowing: a default may refer to a parameter to its left
  (`func f(a, b = a * 2)`), and a mutable default such as `items = []`
  produces a **fresh** value on every call rather than one shared object.
  Passing `null` explicitly is a supplied argument, so it does *not*
  trigger the default.
- **A rest parameter** `...name` collects the remaining arguments into an
  array — empty, never `null`, when there are none. It must be the last
  parameter and cannot have a default.
- **A required parameter may not follow a defaulted one**; that is a syntax
  error rather than a runtime surprise, because such a parameter could
  never be filled positionally.
- **Spread** `...expr` expands an array in place. It is allowed in a call's
  arguments, in an array literal, and in `print` — and nowhere else.
  Spreading anything but an array is a runtime error; there is no implicit
  iteration of strings or objects, so `f(...x)` cannot quietly mean two
  different things depending on what `x` holds.

```mrt
var xs = [1, 2];
print(total("spread", ...xs, 3));   // spread: 6
print([0, ...xs, 9]);               // [0, 1, 2, 9]
print(...xs);                       // 1 2
```

Arity errors name the accepted range: `Expected 2 arguments but got 1.`,
`Expected between 1 and 3 arguments but got 4.`, or
`Expected at least 1 arguments but got 0.`

Recursion through a `var` works because the initializer is evaluated
before the name is bound *in the same environment* the closure captured —
by the time the body runs, the name resolves:

```mrt
var fact = func(n) {
    if (n <= 1) { return 1; }
    return n * fact(n - 1);
};
```

### Per-iteration binding in `for`-`in`

The C-style `for` loop has a single loop variable that every iteration
mutates, so closures created in the body all observe its final value.
`for`-`in` deliberately differs: it creates a *new* binding each
iteration, so closures capture that iteration's value.

```mrt
var fns = [];
for (x in [1, 2, 3]) { push(fns, func() { return x; }); }
print(map(fns, func(f) { return f(); }));   // [1, 2, 3], not [3, 3, 3]
```

This is the same choice JavaScript made for `let` in `for...of`, and it is
the main reason to prefer `for`-`in` when the body creates closures.

## Error model

There are two error phases. Syntax errors are always fatal; runtime
errors halt the program *unless* a `try`/`catch` catches them:

1. **Syntax errors** (`MRTSyntaxError`, raised by the lexer or parser).
   The parser collects *all* syntax errors it can find in one pass (via
   panic-mode recovery that resynchronizes at the next `;` or the start
   of the next statement) rather than stopping at the first one, then the
   CLI prints every one of them and exits with status 65. **The program
   is never partially executed when there are syntax errors** — either
   parsing fully succeeds, or nothing runs.
2. **Runtime errors** (`MRTRuntimeError`), such as division/modulo by
   zero, an out-of-bounds array index, an undefined variable, a type
   mismatch (e.g. comparing a number to an array), or calling a
   non-function. Uncaught, these are reported (with a source line number
   when one is available) and the program halts at that point — any output
   already produced via `print` before the error stays in the output.

Error messages include a `[line N]` suffix whenever the offending token's
line is known.

### Catching errors

```mrt
try {
    risky();
} catch (e) {
    print("failed:", e);
} finally {
    print("always runs");
}
```

- **`catch (e)`** binds the error to `e` in a scope private to the catch
  block; an outer variable of the same name is untouched.
- **What `e` holds** depends on where the error came from:
  - `throw expr` delivers `expr` itself, whatever its type — a string, an
    object, `null`, anything. Nothing is added to it.
  - An interpreter-raised runtime error is converted to an *object* with
    exactly four keys, in this order:

    | Key | Type | Meaning |
    |---|---|---|
    | `message` | string | the text, without the `[line N]` suffix |
    | `line` | number \| null | where it happened |
    | `kind` | string | a coarse category — see below |
    | `stack` | array of strings | the MRT call stack, innermost first |

### Error kinds

`e.kind` is one of a small, closed set, so that catching "any bad index"
doesn't mean enumerating every built-in that can produce one:

| Kind | Raised by |
|---|---|
| `TypeError` | a value of the wrong type — indexing a number, `-"x"`, calling a non-function, spreading a non-array |
| `ArityError` | wrong number of arguments, and built-ins whose whole signature is checked at once |
| `IndexError` | an index outside an array or string, or a non-numeric one |
| `KeyError` | an object key that isn't present |
| `NameError` | an undefined variable, or importing a name a module doesn't export |
| `ValueError` | right type, unusable value — `sqrt(-1)`, a zero `range` step, a missing module |
| `ArithmeticError` | division or modulo by zero |
| `RuntimeError` | anything not covered above |

The set is defined once in `src/errors.py` as `ERROR_KINDS` and mirrored by
`ErrorKind` in the Playground interpreter.

### Stack traces

`e.stack` lists the function names the error passed through on its way out,
innermost first, with `<anonymous>` for an unnamed function. It is empty
when the error was raised in the same frame that caught it, and it does not
include the catching frame itself.

```mrt
func c() { var a = [1]; return a[99]; }
func b() { return c(); }
func main() {
    try { b(); } catch (e) { print(e.stack); }   // [c, b]
}
```

### Guarded catch clauses

A `catch` may carry an `if` guard, and several may be chained. They are
tried in order and the first whose guard passes handles the error; a clause
without a guard always matches, so it acts as the final `else`.

```mrt
try {
    risky();
}
catch (e) if (get(e, "kind", "") == "IndexError") { print("bad index"); }
catch (e) if (get(e, "kind", "") == "ArithmeticError") { print("bad maths"); }
catch (e) { print("something else"); }
```

Two things to know about guards:

- If **no** clause matches, the error keeps propagating to the next
  enclosing `try` (and any `finally` here still runs). It is not silently
  swallowed.
- A guard is ordinary code, so **a guard that itself raises replaces the
  original error**. Since a thrown value need not be an object at all,
  prefer `get(e, "kind", "")` over `e.kind` in a guard: `get` never raises,
  whereas `e.kind` on a thrown string or number will.
- **`finally`** runs on every path out of the `try`: normal completion, a
  caught throw, an uncaught throw still unwinding, and a `return`,
  `break` or `continue` passing through it.
- At least one `catch` clause or a `finally` must be present — `try { }`
  alone is a syntax error. A `try`/`finally` with no `catch` runs the
  cleanup and lets the error keep propagating.
- Re-throwing from inside a `catch` block is allowed and propagates to the
  next enclosing `try`.

Because `catch` binds the interpreter's own errors, a program can wrap a
partial operation instead of aborting:

```mrt
func safeDivide(a, b) {
    try { return a / b; } catch (e) { return null; }
}
```

## Built-in functions

All built-ins are ordinary global variables holding function values —
nothing stops a program from shadowing one with `var len = ...;` inside a
narrower scope (not recommended, but not special-cased either), and they
can be passed to higher-order functions like any other value
(`map(xs, toUpper)`).

`toString`/`print` render a function as `<function name>` for a named
declaration, `<function>` for an anonymous one, and `<builtin>` for a
built-in. (Built-ins deliberately do not expose their host-language
identity: rendering them naively would print a Python repr complete with a
memory address on one side and JavaScript source text on the other.)

### Arrays

| Function | Signature | Notes |
|---|---|---|
| `len(x)` | `(array\|object\|string) -> number` | |
| `push(arr, v)` | `(array, any) -> any` | mutates `arr`, returns `v` |
| `pop(arr)` | `(array) -> any` | mutates `arr`; error if empty |
| `slice(arr, start, end?)` | `(array, number, number?) -> array` | negative indices count from the end, like Python |
| `join(arr, sep?)` | `(array, string?) -> string` | `sep` defaults to `""` |
| `indexOf(arr, v)` | `(array, any) -> number` | `-1` if not found |
| `has(arr\|obj, v\|k)` | `(array\|object, any) -> boolean` | value-membership for arrays, key-membership for objects |
| `get(arr\|obj, k, default?)` | `(array\|object, any, any?) -> any` | never raises; returns `default` (or `null`) instead of erroring |
| `reverse(x)` | `(array\|string) -> array\|string` | returns a new value; does not mutate |
| `unique(arr)` | `(array) -> array` | first occurrence wins; uses structural equality |
| `flatten(arr, depth?)` | `(array, number?) -> array` | `depth` defaults to `1` |
| `zip(a, b)` | `(array, array) -> array` | array of `[a[i], b[i]]`, truncated to the shorter |
| `enumerate(arr)` | `(array) -> array` | array of `[index, value]` |
| `count(arr, v)` | `(array, any) -> number` | structural equality |
| `sum(arr)` | `(array) -> number` | error if any element isn't a number; `sum([])` is `0` |
| `range(end)` / `range(start, end)` / `range(start, end, step)` | `(number, number?, number?) -> array` | half-open, like Python; `step` may be negative but not `0` |

### Higher-order

Each takes the function as its *last* argument and never mutates its
input. The callback is called with one element at a time (two for
`reduce` and `sort`).

| Function | Signature | Notes |
|---|---|---|
| `map(arr, f)` | `(array, function) -> array` | |
| `filter(arr, f)` | `(array, function) -> array` | keeps elements where `f(x)` is truthy |
| `reduce(arr, f, init?)` | `(array, function, any?) -> any` | without `init`, starts from `arr[0]`; an empty array without `init` is an error |
| `find(arr, f)` | `(array, function) -> any` | first match, or `null` |
| `some(arr, f)` / `every(arr, f)` | `(array, function) -> boolean` | |
| `sort(arr, compare?)` | `(array, function?) -> array` | returns a **new** array. Without `compare`, the array must be all numbers or all strings. `compare(a, b)` returns negative / zero / positive. Stable in both implementations. |

### Strings

`split`, `substring`, `toUpper`, `toLower`, `trim`, `replace`,
`startsWith`, `endsWith`, `contains` — see `docs/language_guide.md` for
signatures; unchanged in this revision.

| Function | Signature | Notes |
|---|---|---|
| `repeat(s, n)` | `(string, number) -> string` | `n` must not be negative |
| `padStart(s, width, pad?)` | `(string, number, string?) -> string` | `pad` defaults to `" "`; a longer string is returned unchanged; a multi-character pad is truncated to fit |
| `padEnd(s, width, pad?)` | `(string, number, string?) -> string` | as above, on the right |

### Objects

| Function | Signature | Notes |
|---|---|---|
| `keys(obj)` | `(object) -> array` | |
| `values(obj)` | `(object) -> array` | |
| `has(obj, key)` | see above | |
| `get(obj, key, default?)` | see above | |

### Math and conversion

| Function | Signature | Notes |
|---|---|---|
| `abs(n)` | `(number) -> number` | |
| `min(...)` / `max(...)` | `(number...) \| (array) -> number` | either variadic numbers or one array argument |
| `round(n, digits?)` | `(number, number?) -> number` | |
| `floor(n)` / `ceil(n)` | `(number) -> number` | |
| `sqrt(n)` | `(number) -> number` | error if `n < 0` |
| `pow(base, exp)` | `(number, number) -> number` | |
| `type(v)` | `(any) -> string` | one of `"number" "string" "boolean" "array" "object" "null" "function"` |
| `toNumber(v)` | `(string\|number\|boolean) -> number` | error if a string doesn't parse |
| `toString(v)` | `(any) -> string` | same formatting `print` uses |

### Randomness

| Function | Signature | Notes |
|---|---|---|
| `random(seed)` | `(number) -> function` | returns a zero-argument generator producing the next value in `[0, 1)` |

`random` is *seeded and deterministic*: there is no unseeded global
random, and both implementations run the identical xorshift32 over uint32
state, so a seeded program prints the same numbers in the browser
Playground as on the command line (which is also what lets the parity
checker compare them). Each call to `random(seed)` yields an independent
generator.

```mrt
var rng = random(2026);
var roll = floor(rng() * 6) + 1;
```

Raw generator output is a fraction with a 2^32 denominator; prefer
`floor(rng() * n)` when you want whole numbers, both for readability and
because it avoids relying on float-formatting agreement between the two
runtimes.

## Modules

A program may span several files. A file that uses `export` or `import` is
a *module*; every file is loadable as one.

```mrt
// lib/math.mrt
export var PI = 3.14159;
export func square(n) { return n * n; }
func helper() { return "private"; }   // not exported

// main.mrt
import { PI, square as sq } from "./lib/math.mrt";
func main() { print(PI, sq(4)); }
```

### Rules

- **`export` prefixes a `func` or `var` declaration.** The name still binds
  normally inside its own module; `export` additionally records it in the
  module's export table. There is no `export { a, b }` list and no default
  export.
- **`import { a, b as c } from "path";`** binds each named export into the
  importing file's top-level scope, optionally under a new name.
- **Both are only legal at the top level of a file.** Inside any block —
  a function body included — they are a syntax error. Imports are therefore
  always statically visible at the head of a file.
- **Specifiers are explicitly relative and name a file**: they must start
  with `./` or `../`, and the `.mrt` extension is written out. There is no
  search path, no implicit extension, and no package directory, so an
  import names exactly one file and reading it tells you which.
- **A module is evaluated once**, the first time it is imported, and cached
  by resolved path. Its top-level code — including any `print` — runs then,
  not once per importer.
- **Circular imports are an error**, reported with the cycle
  (`Circular import: a.mrt -> b.mrt -> a.mrt.`) rather than deadlocking or
  handing back a half-built module.
- **Importing a name a module doesn't export is a `NameError`**, naming
  both the module and the name.

### Evaluation order

Within any file, function declarations are registered first (so they may
refer to each other in any order), then the remaining top-level statements
run in source order. In the *entry* file, `main()` — if one is defined — is
called after all of that. A module's `main`, if it has one, is never called
by the import.

### What an import binds

An import binds the exported *value* as it stood when the module finished
evaluating. Reassigning an imported name is a local change and is not seen
by the exporting module or by other importers. Shared *state*, though, is
genuinely shared: functions from a module close over that module's
variables, so a counter exported as a pair of functions behaves as one
counter for everybody.

```mrt
// counter.mrt
var n = 0;
export func next() { n += 1; return n; }
export func reset() { n = 0; }
```

### Modules outside a filesystem

Resolution is injected rather than assumed, because the browser Playground
has no filesystem. The reference interpreter resolves against real paths
relative to the importing file; the Playground supplies its own resolver
over a set of built-in virtual modules, and a host that supplies none has
no imports at all (attempting one reports
`Imports need a file to resolve against; run this program from a file.`).
The parity checker exercises multi-file programs through both.

## Known ambiguities (by design)

- **Optional semicolons, no significant newlines.** A statement's `;` is
  never required, but MRT (unlike Python) does not treat a newline as a
  statement separator either. This means a line that would otherwise
  parse as the *continuation* of the previous expression will be — e.g.:

  ```mrt
  var x = foo()
  -bar()
  ```

  parses as the single expression `foo() - bar()`, not two statements.
  This is the same class of hazard as JavaScript's ASI pitfalls. Prefer a
  trailing `;` on any statement immediately followed by a line starting
  with `-`, `(`, or `[`. `return` gets one special-cased exception: a bare
  `return` followed by a value on a *later* line is always treated as
  `return;` (no value), never as swallowing the next line — see the
  grammar note above.
- **`{` at the start of a statement is always a block, never an object
  literal.** `{"a": 1}` used as a standalone statement parses as an
  (invalid) block. This only matters for a dict literal used for its side
  effects and immediately discarded, which isn't meaningful anyway —
  assign it to a variable or pass it to a function instead. Same
  ambiguity as JavaScript.
- **A bareword object key is the string of that name, not a variable
  reference.** `{name: "Ada"}` means `{"name": "Ada"}`, matching
  JavaScript. **This reverses the previous revision's behaviour**, where
  `name` was evaluated as a variable — a program relying on the old
  meaning changes silently rather than erroring, so it is worth grepping
  for unquoted keys when upgrading. To use a variable's *value* as the
  key, parenthesise it:

  ```mrt
  var k = "dyn";
  print({k: 1});     // {k: 1}    -- the literal key "k"
  print({(k): 1});   // {dyn: 1}  -- the variable's value
  ```

  The parser decides with one token of lookahead: an `IDENTIFIER`
  immediately followed by `:` is a bareword key; anything else is parsed
  as an expression.
- **Compound assignment on an indexed target evaluates the target and
  index expressions twice.** `arr[f()] += 1` desugars to
  `arr[f()] = arr[f()] + 1`, calling `f()` twice. Harmless for the
  common case of a simple variable/literal index; avoid a
  side-effecting expression as an index target of `+=` and friends.
- **`true`/`false` and `1`/`0` as *object keys* can collide.** Equality
  (`==`) and array membership (`indexOf`/`has`) correctly treat `true` and
  `1` as unequal everywhere (see Types and values, above). Object keys are
  the one exception: they're stored in the host language's native
  map/dict, and both the Python and JavaScript runtimes hash `true`/`1`
  (and `false`/`0`) identically, so `{1: "a"}[true]` returns `"a"` instead
  of raising a missing-key error. Avoid mixing boolean and numeric keys in
  the same object.
- **`in` is now a reserved word.** `for (x in xs)` needs it, so a program
  that used `in` as a variable or function name no longer parses. The same
  applies to `null`, `try`, `catch`, `finally` and `throw`.
- **`${` inside a string always begins an interpolation.** A string that
  legitimately contains that character pair must escape it as `\${`. This
  is the one way an existing string literal's meaning can change under
  this revision.
- **A `catch` block catches *everything*, including the interpreter's own
  errors.** There is no error-type filter, so a `catch` meant for a
  program's own `throw` will also swallow a typo'd variable name inside
  the `try` block. Keep `try` blocks narrow, and check `type(e)` or
  `has(e, "message")` if the distinction matters.
- **A built-in that validates its whole signature in one check reports
  `ArityError`, even when what was actually wrong was a type.** For
  example `keys(5)` raises `ArityError`, not `TypeError`, because
  `keys() takes exactly one object argument.` covers both mistakes. Where a
  built-in checks count and type separately, the kinds are exact.
- **`e.kind` in a `catch` guard is a trap for thrown values.** A `throw`
  delivers its value untouched, so `e` may be a string or a number with no
  `kind` at all; `e.kind` then raises inside the guard and replaces the
  original error. Use `get(e, "kind", "")`.
- **A rest parameter is always an array, never `null`.** `f()` on
  `func f(...xs)` binds `xs` to `[]`. This differs from a defaulted
  parameter, which can be `null` if that is its default.
- **`sort()` without a comparator refuses mixed types.** JavaScript's
  default of coercing every element to a string and comparing
  lexicographically (so `[10, 9]` sorts to `[10, 9]`) is a well-known
  footgun; MRT raises instead. Pass an explicit comparator for anything
  other than a uniform array of numbers or strings.

## Future work / explicitly out of scope

Deliberately not implemented in this revision (candidates for a future
one, listed so a contributor doesn't have to guess whether an omission
was an oversight):

- User-defined types, classes, or structs.
- Integer vs. float distinction (everything numeric is a 64-bit float).
- Iterator protocol / generators; `for`-`in` works on the three built-in
  container types and nothing else.
- Namespace imports (`import * as m from "..."`), default exports, and
  re-exports. Only named imports of named exports exist.
- Destructuring assignment (`var [a, b] = pair;`).
- File I/O and date/time. `random` is seeded and deterministic by design,
  so there is deliberately no entropy source either.
