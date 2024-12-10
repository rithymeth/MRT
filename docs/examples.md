# MRT Examples

This document provides practical examples of MRT programming language features.

## Basic Examples

### Hello World
```mrt
func main() {
    print("Hello, World!")
}
```

### Variable Usage
```mrt
func variables_demo() {
    var name = "Alice"
    var age = 25
    var height = 1.75
    var isStudent = true
    
    print("Name: " + name)
    print("Age: " + age)
    print("Height: " + height)
    print("Is Student: " + isStudent)
}
```

### Control Flow
```mrt
func check_number(n) {
    if (n > 0) {
        print("Positive")
    } else if (n < 0) {
        print("Negative")
    } else {
        print("Zero")
    }
}

func count_to_ten() {
    for (var i = 1; i <= 10; i = i + 1) {
        print(i)
    }
}
```

## Array Operations

### Array Manipulation
```mrt
func array_demo() {
    var numbers = [1, 2, 3, 4, 5]
    
    // Add elements
    push(numbers, 6)
    print("After push:", numbers)
    
    // Remove last element
    var last = pop(numbers)
    print("Popped value:", last)
    print("After pop:", numbers)
    
    // Get array slice
    var subset = slice(numbers, 1, 4)
    print("Slice [1:4]:", subset)
    
    // Join array elements
    var joined = join(numbers, ", ")
    print("Joined string:", joined)
    
    // Find element index
    var index = indexOf(numbers, 3)
    print("Index of 3:", index)
}
```

## String Operations

### String Manipulation
```mrt
func string_demo() {
    var text = "  Hello, World!  "
    
    // Basic operations
    print("Original:", text)
    print("Trimmed:", trim(text))
    print("Uppercase:", toUpper(text))
    print("Lowercase:", toLower(text))
    
    // Substring
    var hello = substring(trim(text), 0, 5)
    print("First word:", hello)
    
    // Split and join
    var words = split(trim(text), ", ")
    print("Split words:", words)
    print("Rejoined:", join(words, "-"))
    
    // Search operations
    print("Starts with 'Hello':", startsWith(trim(text), "Hello"))
    print("Contains 'World':", contains(text, "World"))
    print("Ends with '!':", endsWith(trim(text), "!"))
    
    // Replace
    var new_text = replace(text, "World", "MRT")
    print("After replace:", new_text)
}
```

## Advanced Examples

### Calculator
```mrt
func calculator(a, b, operation) {
    if (operation == "+") {
        return a + b
    } else if (operation == "-") {
        return a - b
    } else if (operation == "*") {
        return a * b
    } else if (operation == "/") {
        if (b == 0) {
            print("Error: Division by zero")
            return 0
        }
        return a / b
    } else {
        print("Error: Invalid operation")
        return 0
    }
}

func calc_demo() {
    print("5 + 3 =", calculator(5, 3, "+"))
    print("10 - 4 =", calculator(10, 4, "-"))
    print("6 * 2 =", calculator(6, 2, "*"))
    print("15 / 3 =", calculator(15, 3, "/"))
}
```

### Word Counter
```mrt
func count_words(text) {
    var words = split(trim(text), " ")
    return len(words)
}

func word_counter_demo() {
    var text = "The quick brown fox jumps over the lazy dog"
    var count = count_words(text)
    print("Word count:", count)
    
    var words = split(text, " ")
    print("Words:", words)
    print("First word:", words[0])
    print("Last word:", words[len(words) - 1])
}
```
