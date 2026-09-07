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
10. [Known ambiguities (by design)](#known-ambiguities-by-design)
11. [Future work / explicitly out of scope](#future-work--explicitly-out-of-scope)

## Design goals and non-goals

MRT is a small, dynamically-typed, tree-walked scripting language meant to
be easy to read and easy to implement in an afternoon of study. It is not
meant to be fast, to have a module system, or to interoperate with other
languages. Concretely:

- **Goals:** readable C/JS-family syntax; a handful of clean, orthogonal
  data types (number, string, boolean, array, object, null, function);
  predictable, strict runtime errors (index out of bounds, division by
  zero, and similar always raise rather than silently producing `null` or
  `NaN`); a reference implementation short enough to read start to finish.
- **Non-goals (for now):** a module/import system, exceptions/`try`-`catch`,
  a static type system, integers distinct from floats, user-defined types
  or classes, string interpolation, or multi-file programs. See
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
| String  | `"hello"`, `"line1\nline2"`  | Double-quoted only. Supports the escapes `\n \t \r \" \\ \0`. No string interpolation. |
| Boolean | `true`, `false`               | |
| Null    | *(no literal — see below)*    | There is no `null` keyword. A value is `null` when a function falls off its end without a `return`, or a `var` is declared without an initializer. `toString(x)` renders it as `"null"`. |

### Identifiers and keywords

Identifiers: `[a-zA-Z_][a-zA-Z0-9_]*`.

Reserved keywords (cannot be used as identifiers):

```
func return if else while for print var
true false break continue
```

### Operators and punctuation

```
+  -  *  /  %                  arithmetic
+= -= *= /= %=                 compound assignment
=                                assignment
== !=  <  >  <=  >=            comparison
&&  ||  !                      logical AND / OR / NOT
( ) { } [ ]  , ; . :           grouping / delimiters
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
| `function` | a `func` declaration, passed by reference | always truthy |

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
number/boolean-keyed maps. Keys are values, not identifiers: object
literals require keys to be expressions, typically quoted strings —
`{"name": "Ada"}`, not `{name: "Ada"}`. `obj.name` is sugar for
`obj["name"]` and works for both reading and writing.

## Grammar (EBNF)

Terminals are quoted or `ALL_CAPS` (matching the lexer's token names).
`(x)?` is optional, `(x)*` is zero-or-more, `(x)+` is one-or-more.

```ebnf
program        = declaration* EOF ;

declaration    = funcDecl
               | varDecl
               | statement ;

funcDecl       = "func" IDENTIFIER "(" parameters? ")" block ;
parameters     = IDENTIFIER ( "," IDENTIFIER )* ;

varDecl        = "var" IDENTIFIER ( "=" expression )? ";"? ;

statement      = exprStmt
               | forStmt
               | ifStmt
               | printStmt
               | returnStmt
               | whileStmt
               | breakStmt
               | continueStmt
               | block ;

exprStmt       = expression ";"? ;

forStmt        = "for" "(" ( varDecl | exprStmt | ";" )
                            expression? ";"
                            expression? ")" statement ;

ifStmt         = "if" "(" expression ")" statement ( "else" statement )? ;

printStmt      = "print" "(" ( expression ( "," expression )* )? ")" ";"? ;

returnStmt     = "return" expression? ";"?  ;
               (* the return value, if any, must start on the same
                  source line as `return` -- see Known ambiguities *)

whileStmt      = "while" "(" expression ")" statement ;

breakStmt      = "break" ";"? ;
continueStmt   = "continue" ";"? ;

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
arguments      = expression ( "," expression )* ;

primary        = NUMBER | STRING | "true" | "false"
               | IDENTIFIER
               | "(" expression ")"
               | "[" ( expression ( "," expression )* )? "]"
               | "{" ( objectEntry ( "," objectEntry )* )? "}" ;
objectEntry    = expression ":" expression ;
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
- **`break`**: exits the nearest enclosing `while` or `for` immediately.
- **`continue`**: skips to the next iteration — for `while`, straight to
  the condition check; for `for`, first running the increment, then the
  condition check.
- **`return`**: exits the current function immediately with a value
  (`null` if none given). A bare `return` at the top level, outside any
  function, is a runtime error (there's no implicit outer function).
- **`print(a, b, ...)`**: evaluates each argument left to right, joins
  their `stringify`d forms with a single space, and writes one line.
  `print()` with no arguments prints an empty line.
- **Program entry point**: after all top-level `func` declarations are
  registered, the interpreter looks for a function named `main` and calls
  it with no arguments. If there is no `main`, every top-level
  *non-function* statement runs instead, in source order.

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

Functions are not first-class in the sense of anonymous function
expressions (`func` is always a *declaration* with a name) — but a
declared function's name is just a regular variable holding a function
value, so it can be passed around, stored in arrays/objects, and returned,
as above.

## Error model

There are exactly two error phases, both fatal to the whole program (MRT
has no exception handling — a runtime error always stops execution):

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
   non-function. These are reported (with a source line number when one
   is available) and the program halts at that point — any output already
   produced via `print` before the error stays in the output.

Error messages include a `[line N]` suffix whenever the offending token's
line is known.

## Built-in functions

All built-ins are ordinary global variables holding function values —
nothing stops a program from shadowing one with `var len = ...;` inside a
narrower scope (not recommended, but not special-cased either).

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

### Strings

`split`, `substring`, `toUpper`, `toLower`, `trim`, `replace`,
`startsWith`, `endsWith`, `contains` — see `docs/language_guide.md` for
signatures; unchanged in this revision.

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
- **Object keys are expressions, not bareword identifiers.**
  `{"name": "Ada"}` is a string-keyed object. `{name: "Ada"}` is *not*
  shorthand for the same thing — `name` there is parsed as a variable
  reference and evaluated, raising "Undefined variable 'name'" unless a
  variable named `name` happens to be in scope (in which case its
  *value* becomes the key, probably not what was intended). This is a
  deliberate departure from JavaScript's object-literal shorthand, chosen
  to keep the grammar unambiguous without lookahead: always quote your
  keys.
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

## Future work / explicitly out of scope

Deliberately not implemented in this revision (candidates for a future
one, listed so a contributor doesn't have to guess whether an omission
was an oversight):

- `try` / `catch` / user-recoverable exceptions (today, any runtime error
  halts the whole program).
- Anonymous function expressions / lambdas (`func` is always a named
  declaration).
- A module or `import` system (every program is a single file).
- User-defined types, classes, or structs.
- Integer vs. float distinction (everything numeric is a 64-bit float).
- String interpolation (`` `Hello, ${name}` ``-style).
- A standard library beyond the built-ins listed above (no file I/O,
  no randomness, no date/time).
