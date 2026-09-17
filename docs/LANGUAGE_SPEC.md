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
10. [Destructuring](#destructuring)
11. [Structs](#structs)
12. [Pattern matching](#pattern-matching)
13. [Generators](#generators)
14. [Modules](#modules)
15. [Known ambiguities (by design)](#known-ambiguities-by-design)
16. [Future work / explicitly out of scope](#future-work--explicitly-out-of-scope)

## Design goals and non-goals

MRT is a small, dynamically-typed, tree-walked scripting language meant to
be easy to read and easy to implement in an afternoon of study. It is not
meant to be fast or to interoperate with other languages. Concretely:

- **Goals:** readable C/JS-family syntax; a handful of clean, orthogonal
  data types (number, string, boolean, array, object, null, function);
  first-class functions with lexical closures; user-defined struct types
  and matching on shape; predictable, strict runtime
  errors (index out of bounds, division by zero, and similar always raise
  rather than silently producing `null` or `NaN`) that a program may
  nonetheless catch and recover from; a reference implementation short
  enough to read start to finish.
- **Non-goals (for now):** a static type system, integers distinct from
  floats, or inheritance between user-defined types. See
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
struct match case default yield
```

The last row is new in the current revision: `struct`, `match`, `case`,
`default` and `yield` are now reserved, so a program that used any of them
as a variable or function name no longer parses. (The row before it added
`import`, `export`, `from` and `as` in the previous revision.)

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

MRT has these kinds of runtime value:

| Type       | Example                        | Truthy?                                   |
|------------|---------------------------------|--------------------------------------------|
| `number`   | `42`, `-3.5`                    | always truthy (including `0`) |
| `string`   | `"hi"`                          | always truthy (including `""`) |
| `boolean`  | `true`, `false`                 | its own value |
| `array`    | `[1, 2, 3]`                     | always truthy (including `[]`) |
| `object`   | `{"a": 1}`                      | always truthy (including `{}`) |
| `null`     | *(see above)*                   | always falsy |
| `function` | a `func` declaration or a `func(...) { ... }` expression, passed by reference | always truthy |
| *a struct name* | an instance of a declared `struct`; `type()` reports the struct's own name, e.g. `"Point"` | always truthy |
| `struct` | the struct type itself, which is also its constructor | always truthy |
| `generator` | the lazy sequence a generator function returns | always truthy |

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
               | structDecl
               | varDecl
               | statement ;

structDecl     = "struct" IDENTIFIER "{" structMember* "}" ;
structMember   = "func" IDENTIFIER "(" parameters? ")" block
               | fieldNames ";"? ;
fieldNames     = field ( "," field )* ;
field          = IDENTIFIER ( "=" expression )? ;

importDecl     = "import" "{" importNames? "}" "from" STRING ";"? ;
importNames    = importName ( "," importName )* ;
importName     = IDENTIFIER ( "as" IDENTIFIER )? ;
               (* only valid at the top level of a file *)

exportDecl     = "export" ( funcDecl | varDecl ) ;
               (* only valid at the top level of a file *)

funcDecl       = "func" IDENTIFIER "(" parameters? ")" block ;
parameters     = parameter ( "," parameter )* ;
parameter      = pattern
               | "..." IDENTIFIER ;
               (* a rest parameter must come last; a required parameter may
                  not follow one with a default *)

varDecl        = "var" pattern ( "=" expression )? ";"? ;
               (* an initializer is required unless the pattern is a
                  plain name *)

pattern        = IDENTIFIER ( "=" expression )?
               | "[" ( pattern ( "," pattern )* ( "," "..." IDENTIFIER )? )? "]" ( "=" expression )?
               | "{" ( objEntry ( "," objEntry )* ( "," "..." IDENTIFIER )? )? "}" ( "=" expression )? ;
objEntry       = IDENTIFIER ( ":" pattern | ( "=" expression )? ) ;

statement      = exprStmt
               | matchStmt
               | yieldStmt
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

forInStmt      = "for" "(" "var"? pattern "in" expression ")" statement ;

ifStmt         = "if" "(" expression ")" statement ( "else" statement )? ;

printStmt      = "print" "(" ( expression ( "," expression )* )? ")" ";"? ;

returnStmt     = "return" expression? ";"?  ;
               (* the return value, if any, must start on the same
                  source line as `return` -- see Known ambiguities *)

whileStmt      = "while" "(" expression ")" statement ;

breakStmt      = "break" ";"? ;
continueStmt   = "continue" ";"? ;

tryStmt        = "try" block catchClause* ( "finally" block )? ;
catchClause    = "catch" "(" pattern ")" ( "if" "(" expression ")" )? block ;
               (* at least one catch clause or a finally must be present;
                  clauses are tried in order, first match wins *)

throwStmt      = "throw" expression ";"? ;

yieldStmt      = "yield" expression ";"? ;
               (* only inside a function; its presence makes that function
                  a generator *)

matchStmt      = "match" "(" expression ")" "{" caseClause* defaultClause? "}" ;
caseClause     = "case" matchPattern ( "if" "(" expression ")" )? ":" statement* ;
defaultClause  = "default" ":" statement* ;

matchPattern   = NUMBER | STRING | "true" | "false" | "null" | "-" NUMBER
               | IDENTIFIER
               | IDENTIFIER "(" ( matchPattern ( "," matchPattern )* )? ")"
               | "[" ( matchPattern ( "," matchPattern )* ( "," "..." IDENTIFIER )? )? "]"
               | "{" matchEntry ( "," matchEntry )* "}" ;
matchEntry     = IDENTIFIER ( ":" matchPattern )? ;

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
- A leading `[` or `{` only starts a *pattern* when what follows could
  plausibly be one (a name, a closing bracket, or `...`). Otherwise it is
  parsed as whatever it would have been before, which keeps a missing
  paren — `func main( {` — reported on its own line instead of wherever
  the brace's contents happen to start.
- `for (` uses a speculative parse rather than lookahead to tell the
  for-in form from the C-style one, since a pattern can be arbitrarily
  long; it rewinds if the `in` never arrives.
- Whether a function is a generator is settled at parse time by whether a
  `yield` appeared directly in its body (not inside a nested function).

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
declaration, `<function>` for an anonymous one, `<builtin>` for a built-in,
`<struct Name>` for a struct type, `<generator name>` for a generator, and
`Name(field: value, ...)` for a struct instance. (Built-ins deliberately do not expose their host-language
identity: rendering them naively would print a Python repr complete with a
memory address on one side and JavaScript source text on the other.)

### Arrays

| Function | Signature | Notes |
|---|---|---|
| `len(x)` | `(array\|object\|string\|struct) -> number` | a struct instance counts its fields |
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
| `toArray(x)` | `(iterable) -> array` | materialises an array, string, object's keys, or a generator |
| `take(x, n)` | `(iterable, number) -> array` | the first `n` items; safe on an endless generator |

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

## Destructuring

Anywhere a name is bound — `var`, a function parameter, a `for`-`in` loop
variable, a `catch` clause — a *pattern* may take the value apart instead.

```mrt
var [first, ...rest] = [1, 2, 3];
var {name, role = "unknown"} = person;

func distance([x1, y1], [x2, y2]) { ... }
for ([key, value] in pairs) { ... }
try { ... } catch ({kind, message}) { ... }
```

### Array patterns

- Bind positionally. **Extra elements are ignored**: `[a]` happily matches a
  three-element array, because a pattern names what it wants.
- `...rest` collects the remainder into an array, empty rather than `null`
  when there is nothing left. It must be last.
- A slot with no corresponding element is an error unless it has a default
  (`[a, b = 0]`).
- Destructuring a non-array with an array pattern is a `TypeError`.

### Object patterns

- `{name}` is shorthand for `{name: name}`; `{name: local}` binds under a
  different name; `{name = "anon"}` supplies a default.
- `...rest` collects the *unlisted* keys into a new object.
- A missing key without a default is a `KeyError`.
- The value may be a plain object **or a struct instance**, whose fields
  (not its methods) are what gets read.

### Everywhere else

- Patterns nest arbitrarily: `var [{id}, [inner]] = ...`.
- A whole pattern may carry a default, which matters for parameters:
  `func f({x} = {x: 5})`.
- A `for`-`in` pattern still binds afresh each iteration, so closures made
  in the body capture that iteration's values.
- `var` with a non-name pattern **requires** an initializer; `var [a, b];`
  is a syntax error.
- `export var` requires a plain name, since a destructuring declaration
  binds several at once.

Destructuring is strict on purpose. The alternative — quietly binding
`null` for anything absent — turns a typo'd key into a value that fails
somewhere else entirely, which is exactly what the language's
raise-early rules exist to avoid.

## Structs

A `struct` declares a named type with a fixed set of fields and the methods
that operate on them.

```mrt
struct Point {
    x, y;

    func magnitude() { return sqrt(this.x * this.x + this.y * this.y); }
    func scaled(k) { return Point(this.x * k, this.y * k); }
}

var p = Point(3, 4);
print(p.magnitude());        // 5
print(p);                    // Point(x: 3, y: 4)
print(type(p));              // Point
```

- **Fields** are declared as a comma-separated run, each optionally with a
  default. A default is evaluated at construction time in a scope where the
  fields to its left are bound, so `struct C { host, url = "http://${host}"; }`
  works. A field without a default may not follow one with a default.
- **Construction** is the struct name applied to positional field values:
  `Point(3, 4)`. Arity is checked exactly as for a function call.
- **Methods** are ordinary functions — defaults, rest parameters and
  generators all work — that additionally see `this` bound to the receiver.
  A method read off an instance (`var m = p.magnitude;`) stays bound to it.
- **Fields are mutable but fixed**: `p.x = 6` is fine, `p.z = 1` is a
  `KeyError`. A struct is a shape, not a bag.
- **Equality** is structural *and* per-struct: two instances are equal when
  they share a struct and every field matches. An instance never equals a
  plain object, even one with the same keys.
- **`type()`** returns the struct's own name for an instance and `"struct"`
  for the type itself.
- Struct declarations are **hoisted** alongside functions, so they may refer
  to one another in any order, and they can be `export`ed.
- `keys`, `values`, `has`, `get`, `len` and object destructuring all treat
  an instance as its fields — methods are deliberately not included, so
  those built-ins describe the data.

There is no inheritance, no visibility modifiers and no user-defined
operators. A struct is the smallest thing that gives a value a name and a
guaranteed shape.

## Pattern matching

`match` dispatches on a value's *shape*, binding as it goes.

```mrt
match (shape) {
    case Circle(r):                    return 3.14159 * r * r;
    case Rect(w, h):                   return w * h;
    case [x, y]:                       return x * y;
    case {kind: "error", message: m}:  return m;
    case n if (n > 100):               return "big";
    case n:                            return "other";
    default:                           return "nothing matched";
}
```

Cases are tried in order and the **first** whose pattern fits, and whose
guard passes, runs. There is **no fall-through**, so no `break` is needed.

### Pattern kinds

| Pattern | Matches |
|---|---|
| `0`, `-1`, `"s"`, `true`, `null` | that value, by the same structural equality `==` uses |
| `name` | anything, binding it to `name` |
| `[a, b]` | an array of **exactly** that length, element-wise |
| `[a, ...rest]` | an array of at least that length |
| `{k: p, ...}` | an object **or struct instance** having at least those keys |
| `Point(a, b)` | an instance of exactly that struct, fields in declaration order |

Note the deliberate difference from destructuring: a *match* array pattern
requires an exact length (unless it has a rest), because `case [x]:`
silently swallowing every non-empty array would make matching useless. An
*object* pattern stays partial, because listing every key of a large object
to match on one of them would be worse.

### Other rules

- A **guard** is `case <pattern> if (expr):`, evaluated with the pattern's
  bindings already in scope.
- Bindings are **scoped to their case** and do not leak.
- `default:` must be last, and there may be at most one.
- **If nothing matches and there is no `default`, that is a runtime
  error**, not a silent no-op. MRT cannot check exhaustiveness statically,
  so it checks it at the moment it matters.
- A struct pattern whose field count disagrees with the declaration is an
  `ArityError`, and a name that isn't a struct is a `TypeError` — both are
  program bugs rather than failed matches.

## Generators

A function whose body contains `yield` is a **generator function**. Calling
it runs nothing; it returns a lazy sequence that computes each value only
when something asks for it.

```mrt
func naturals() { var n = 0; while (true) { yield n; n += 1; } }
func squares(source) { for (x in source) { yield x * x; } }

print(take(naturals(), 5));            // [0, 1, 2, 3, 4]
print(take(squares(naturals()), 4));   // [0, 1, 4, 9]
```

- **`yield` is a statement**, not an expression, and nothing is sent back
  in. That restriction is what makes generators implementable in a
  tree-walking interpreter without rewriting every expression path: only
  statement execution has to be suspendable.
- `yield` is legal inside `if`, `while`, `for`, `for`-`in`, `try`/`catch`/
  `finally`, `match` and nested blocks. It is a syntax error outside a
  function.
- Whether a function is a generator is decided **at parse time**, and a
  `yield` inside a *nested* function belongs to that inner function.
- **`return` ends the sequence** (its value is discarded); falling off the
  end does the same.
- A generator is **single use**. Iterating one a second time is an error
  rather than an empty loop, matching what the host languages do and
  turning a silent bug into a loud one.
- `for`-`in` pulls lazily, so `break` simply stops asking. Errors and
  `throw`s propagate out to whatever is iterating, with the generator's
  name added to `e.stack`.
- A generator keeps its own scope across suspensions, so the consumer
  running arbitrary code in between cannot disturb it.
- Struct methods can be generators too.

`toArray(x)` materialises any iterable into an array, and `take(x, n)`
takes the first `n` — the latter being what makes an endless generator
usable. Both also accept arrays, strings and objects.

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

- **`export` prefixes a `func`, `var` or `struct` declaration.** The name
  still binds normally inside its own module; `export` additionally records
  it in the module's export table. `export var` requires a plain name, not
  a destructuring pattern.
- **`export { a, b as c };`** re-exports names already declared in this
  module, and **`export { a } from "./m.mrt";`** forwards another module's
  export without binding it locally. There is still no default export.
- **`import * as m from "./m.mrt";`** binds one ordinary MRT object holding
  every export, so `m.thing`, `keys(m)` and destructuring all work on it
  with no special rules. Private names are simply absent from it.
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
- **A match array pattern is exact, a destructuring array pattern is
  not.** `var [a] = [1, 2, 3]` binds `a` and ignores the rest, but
  `case [a]:` does *not* match `[1, 2, 3]`. The two do different jobs:
  destructuring says "give me this piece", matching asks "is it this
  shape?".
- **An object pattern matches a struct instance.** That is usually what you
  want (`case {x, y}:` catches any point-like value), but it means a struct
  pattern is the only way to insist on a *particular* struct.
- **A struct's methods are invisible to `keys`/`values`/`has`/`get` and to
  object destructuring.** Those describe a value's data. Use `p.method` to
  reach a method, and note `has(p, "method")` is `false`.
- **Field and parameter defaults are evaluated left to right at call
  time**, so `struct C { a, b = a; }` works but `struct C { a = b, b; }`
  does not — and the latter is already rejected, since a field without a
  default cannot follow one that has one.
- **A generator is single use, and `for`-`in` consumes it.** Iterating the
  same generator value twice is an error; call the generator function again
  to get a fresh sequence.
- **`yield` cannot appear in an expression.** `var x = yield 1;` is a
  syntax error. Generators produce values; they do not receive them.
- **`sort()` without a comparator refuses mixed types.** JavaScript's
  default of coercing every element to a string and comparing
  lexicographically (so `[10, 9]` sorts to `[10, 9]`) is a well-known
  footgun; MRT raises instead. Pass an explicit comparator for anything
  other than a uniform array of numbers or strings.

## Future work / explicitly out of scope

Deliberately not implemented in this revision (candidates for a future
one, listed so a contributor doesn't have to guess whether an omission
was an oversight):

- Inheritance, interfaces or traits between structs; a struct is a flat
  shape with methods and nothing more.
- Integer vs. float distinction (everything numeric is a 64-bit float).
- A user-implementable iterator protocol: `for`-`in` drives the built-in
  containers and generators, and a struct cannot yet make itself iterable
  except by exposing a generator method.
- Two-way generators (`var x = yield v;`), generator delegation
  (`yield*`), and lazy `map`/`filter` built-ins that return generators
  rather than arrays.
- Destructuring *assignment* to existing variables (`[a, b] = pair;`);
  patterns only appear in declarations and bindings.
- `match` as an expression rather than a statement, and exhaustiveness
  checking.
- Default exports and `export * from "..."`.
- File I/O and date/time. `random` is seeded and deterministic by design,
  so there is deliberately no entropy source either.
