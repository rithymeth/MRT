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

## Objects

### Object Manipulation
```mrt
func object_demo() {
    var person = {"name": "Ada Lovelace", "age": 36};

    print("Name:", person.name);
    print("Age:", person.age);

    person.age += 1;
    print("After a birthday:", person.age);

    person["job"] = "Mathematician";
    print("Keys:", keys(person));
    print("Has 'job'?", has(person, "job"));
    print("Get 'salary' (default):", get(person, "salary", "unknown"));
}
```

See `examples/objects.mrt` for the full runnable version, which also
covers `type()`, the math built-ins, and compound assignment together.

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

## Operators

### Comparison, Logical and Modulo
```mrt
func classify(n) {
    if (n % 2 == 0) {
        return "even";
    }
    return "odd";
}

func operators_demo() {
    print("3 != 4:", 3 != 4);
    print("2 >= 2:", 2 >= 2);
    print("age >= 18 && hasId:", 25 >= 18 && true);
    print("10 is", classify(10));
}
```

### Break and Continue
```mrt
func sum_even_up_to_ten() {
    var total = 0;
    for (var i = 1; i <= 20; i = i + 1) {
        if (i % 2 != 0) {
            continue;
        }
        if (i > 10) {
            break;
        }
        total = total + i;
    }
    return total;  // 30
}
```

See `examples/operators.mrt` for the full runnable version.

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

## Functions and Closures

### Functions as values
```mrt
func main() {
    var double = func(x) { return x * 2; };
    var apply = func(f, v) { return f(v); };

    print(double(21));            // 42
    print(apply(double, 5));      // 10
    print(type(double));          // function
}
```

### Closures for private state
```mrt
func makeCounter() {
    var count = 0;
    return func() { count += 1; return count; };
}

func main() {
    var a = makeCounter();
    var b = makeCounter();
    a(); a();
    print(a(), b());              // 3 1  -- independent counters
}
```

### A collection pipeline
```mrt
func main() {
    var people = [
        {name: "Ada", age: 36},
        {name: "Bob", age: 17},
        {name: "Cy",  age: 44}
    ];

    var adults = filter(people, func(p) { return p.age >= 18; });
    var names  = sort(map(adults, func(p) { return p.name; }));

    for (n in names) { print("adult: ${n}"); }

    var total = reduce(map(people, func(p) { return p.age; }),
                       func(a, b) { return a + b; });
    print("combined age: ${total}");
}
```

Full program: `examples/functions.mrt`.

## Error Handling

### Structured errors
```mrt
func withdraw(balance, amount) {
    if (amount <= 0) {
        throw {code: "INVALID", reason: "amount must be positive"};
    }
    if (amount > balance) {
        throw {code: "FUNDS", reason: "insufficient balance"};
    }
    return balance - amount;
}

func main() {
    for (amount in [50, -10, 500]) {
        try {
            print("new balance:", withdraw(100, amount));
        } catch (e) {
            print("rejected ${amount}: ${e.code} - ${e.reason}");
        }
    }
}
```

### Recovering from a runtime error
```mrt
func parseOrDefault(text, fallback) {
    try {
        return toNumber(text);
    } catch (e) {
        return fallback;
    }
}

func main() {
    print(parseOrDefault("42", 0));       // 42
    print(parseOrDefault("oops", 0));     // 0
}
```

### Guaranteed cleanup
```mrt
func process(items) {
    try {
        for (item in items) {
            if (item == "bad") { throw "hit a bad item"; }
            print("processed ${item}");
        }
        return "all done";
    } finally {
        print("cleanup runs either way");
    }
}

func main() {
    print(process(["a", "b"]));
    try { process(["a", "bad", "c"]); } catch (e) { print("caught: ${e}"); }
}
```

Full program: `examples/errors.mrt`.

## Modern Syntax

### Interpolation and objects
```mrt
func main() {
    var person = {name: "Ada", age: 36, langs: ["MRT"]};

    print("Hi ${person.name}, you are ${person.age}");
    print("Languages: ${person.langs}");
    print("Next year: ${person.age + 1}");

    for (key in person) {
        print("  ${key} = ${get(person, key)}");
    }
}
```

### A formatted table
```mrt
func main() {
    var rows = [
        {label: "apples", n: 7},
        {label: "pears",  n: 128},
        {label: "figs",   n: 44}
    ];

    print(repeat("-", 16));
    for (row in rows) {
        print("${padEnd(row.label, 8)}${padStart(toString(row.n), 5)}");
    }
    print(repeat("-", 16));
    print("${padEnd("total", 8)}${padStart(toString(sum(map(rows, func(r) { return r.n; }))), 5)}");
}
```

### Repeatable randomness
```mrt
func main() {
    // The same seed always produces the same sequence, here and in the
    // browser Playground.
    var rng = random(2026);
    var rolls = [];
    for (i in range(5)) {
        push(rolls, floor(rng() * 6) + 1);
    }
    print("rolls: ${rolls}");
}
```

Full programs: `examples/modern_syntax.mrt`, `examples/stdlib.mrt`.

## Flexible Function Signatures

### Defaults and rest parameters
```mrt
func log(message, level = "info", ...tags) {
    var suffix = "";
    if (len(tags) > 0) { suffix = " [" + join(tags, ",") + "]"; }
    return "${toUpper(level)}: ${message}${suffix}";
}

func main() {
    print(log("started"));
    print(log("disk almost full", "warn"));
    print(log("crashed", "error", "db", "urgent"));
}
```

### Spread
```mrt
func main() {
    var base = ["read", "write"];
    var extra = ["admin"];

    // Build arrays from other arrays.
    var all = [...base, ...extra];
    print(all);

    // Or pass one as arguments.
    print(max(...[3, 9, 4]));
}
```

### A default that depends on an earlier parameter
```mrt
func rect(width, height = width) { return width * height; }

func main() {
    print(rect(3, 4));   // 12
    print(rect(5));      // 25 -- a square
}
```

## Selective Error Handling

### Branching on the kind of failure
```mrt
func classify(action) {
    try {
        action();
        return "ok";
    }
    catch (e) if (get(e, "kind", "") == "IndexError")      { return "out of range"; }
    catch (e) if (get(e, "kind", "") == "ArithmeticError") { return "bad maths"; }
    catch (e) if (get(e, "kind", "") == "NameError")       { return "typo"; }
    catch (e)                                              { return "other"; }
}

func main() {
    print(classify(func() { var a = []; return a[3]; }));
    print(classify(func() { return 1 / 0; }));
    print(classify(func() { return notDefined; }));
    print(classify(func() { return -"x"; }));
    print(classify(func() { return 1 + 1; }));
}
```

### Using the stack trace
```mrt
func parse(text) { return toNumber(text); }
func load(text) { return parse(text); }

func main() {
    try {
        load("not a number");
    } catch (e) {
        print("failed in: ${join(e.stack, " <- ")}");
        print("because: ${e.message}");
    }
}
```

## Multi-File Programs

Given a library file:

```mrt
// lib/text.mrt
export func titleCase(s) {
    var out = [];
    for (word in split(s, " ")) {
        if (len(word) == 0) { continue; }
        push(out, toUpper(substring(word, 0, 1)) + toLower(substring(word, 1, len(word))));
    }
    return join(out, " ");
}

export func initials(name) {
    return join(map(split(name, " "), func(w) { return toUpper(substring(w, 0, 1)); }), ".");
}
```

...a program imports what it needs:

```mrt
// main.mrt
import { titleCase, initials } from "./lib/text.mrt";

func main() {
    print(titleCase("ada lovelace"));   // Ada Lovelace
    print(initials("ada lovelace"));    // A.L
}
```

### Shared state across importers
```mrt
// lib/ids.mrt
var nextId = 0;
export func freshId() { nextId += 1; return nextId; }
```

Every file that imports `freshId` draws from the same counter, because the
function closes over the module's own `nextId`:

```mrt
import { freshId } from "./lib/ids.mrt";

func main() {
    print(freshId(), freshId(), freshId());   // 1 2 3
}
```

Full programs: `examples/modules.mrt` and `examples/lib/`.
