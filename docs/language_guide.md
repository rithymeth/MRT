# MRT Language Guide

## Table of Contents
1. [Introduction](#introduction)
2. [Basic Syntax](#basic-syntax)
3. [Data Types](#data-types)
4. [Variables](#variables)
5. [Control Flow](#control-flow)
6. [Functions](#functions)
7. [Arrays](#arrays)
8. [String Operations](#string-operations)

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
