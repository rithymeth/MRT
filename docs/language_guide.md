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
+  -  *  /  %              // arithmetic (% is modulo/remainder)
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

5. **Objects**: Key/value maps, written like JSON (keys must be
   expressions -- typically quoted strings, not bare identifiers)
   ```mrt
   var person = {"name": "Ada", "age": 36}
   ```

6. **Null**: the absence of a value (there's no `null` literal -- a `var`
   with no initializer, or a function that falls off its end without
   `return`, produces it)
   ```mrt
   var nothing;  // nothing == null
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

### Break and Continue
```mrt
for (var i = 0; i < 10; i = i + 1) {
    if (i == 3) { continue }  // skip this iteration, still runs i = i + 1
    if (i == 6) { break }     // exit the loop entirely
    print(i)
}
```

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
