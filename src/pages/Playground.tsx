import React from 'react'
import Editor from '@monaco-editor/react'
import { Play, Download, Share, RotateCcw, Settings } from 'lucide-react'
import { runMRT } from '../lib/mrtInterpreter'

// The Playground edits a single buffer, so there is no file tree for
// `import` to resolve against. Instead it ships a few built-in modules,
// keyed by the exact specifier a program writes. They are ordinary MRT
// source, run by the same interpreter as everything else -- this is the
// seam the interpreter's `resolveModule` option exists for, and on the
// command line the identical programs resolve against real files.
const PLAYGROUND_MODULES: Record<string, string> = {
  './stats.mrt': "// A small statistics module, importable from the Playground.\n\nexport func mean(nums) {\n    if (len(nums) == 0) { throw \"mean() needs at least one number\"; }\n    return sum(nums) / len(nums);\n}\n\nexport func median(nums) {\n    if (len(nums) == 0) { throw \"median() needs at least one number\"; }\n    var ordered = sort(nums);\n    var mid = floor(len(ordered) / 2);\n    if (len(ordered) % 2 == 1) { return ordered[mid]; }\n    return (ordered[mid - 1] + ordered[mid]) / 2;\n}\n\nexport func spread(nums) {\n    var ordered = sort(nums);\n    return [ordered[0], ordered[len(ordered) - 1]];\n}\n\nexport func describe(nums, label = \"data\") {\n    var bounds = spread(nums);\n    return \"${label}: n=${len(nums)} mean=${round(mean(nums), 2)} median=${median(nums)} range=${bounds[0]}..${bounds[1]}\";\n}\n",
  './text.mrt': "// Small text helpers, importable from the Playground.\n\nexport func titleCase(s) {\n    var out = [];\n    for (word in split(s, \" \")) {\n        if (len(word) == 0) { continue; }\n        push(out, toUpper(substring(word, 0, 1)) + toLower(substring(word, 1, len(word))));\n    }\n    return join(out, \" \");\n}\n\nexport func wordCount(s) {\n    return len(filter(split(trim(s), \" \"), func(w) { return len(w) > 0; }));\n}\n\nexport func truncate(s, width = 20, ellipsis = \"...\") {\n    if (len(s) <= width) { return s; }\n    return substring(s, 0, width - len(ellipsis)) + ellipsis;\n}\n",
}

const resolvePlaygroundModule = (specifier: string) => {
  const source = PLAYGROUND_MODULES[specifier]
  return source === undefined ? null : { path: specifier, source }
}

const Playground: React.FC = () => {
  const [code, setCode] = React.useState(`// Welcome to the MRT Playground!
// Try editing this code and click "Run" to see the output

func main() {
    print("Hello from MRT Playground!")
    
    // Array example
    var numbers = [1, 2, 3, 4, 5]
    push(numbers, 6)
    print("Numbers:", join(numbers, ", "))
    
    // String example
    var message = "  MRT is awesome!  "
    print("Trimmed:", trim(message))
    print("Uppercase:", toUpper(message))
}`)

  const [output, setOutput] = React.useState('')
  const [isRunning, setIsRunning] = React.useState(false)
  const [theme, setTheme] = React.useState<'light' | 'dark'>('dark')

  const examples = [
    {
      name: 'Hello World',
      code: `func main() {
    print("Hello, World from MRT!")
}`
    },
    {
      name: 'Array Operations',
      code: `func main() {
    var numbers = [1, 2, 3, 4, 5]
    
    push(numbers, 6)
    print("After push:", numbers)
    
    var last = pop(numbers)
    print("Popped:", last)
    
    var subset = slice(numbers, 1, 4)
    print("Slice [1:4]:", subset)
    
    print("Joined:", join(numbers, " -> "))
}`
    },
    {
      name: 'String Processing',
      code: `func main() {
    var text = "  Hello, MRT World!  "
    
    print("Original:", text)
    print("Trimmed:", trim(text))
    print("Uppercase:", toUpper(text))
    print("Lowercase:", toLower(text))
    
    var words = split(trim(text), " ")
    print("Words:", words)
    print("First word:", words[0])
    
    print("Contains 'MRT':", contains(text, "MRT"))
    print("Starts with 'Hello':", startsWith(trim(text), "Hello"))
}`
    },
    {
      name: 'Calculator',
      code: `func calculator(a, b, operation) {
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

func main() {
    print("Calculator Demo:")
    print("5 + 3 =", calculator(5, 3, "+"))
    print("10 - 4 =", calculator(10, 4, "-"))
    print("6 * 2 =", calculator(6, 2, "*"))
    print("15 / 3 =", calculator(15, 3, "/"))
    print("10 / 0 =", calculator(10, 0, "/"))
}`
    },
    {
      name: 'Fibonacci',
      code: `func fibonacci(n) {
    if (n <= 1) {
        return n
    }
    return fibonacci(n - 1) + fibonacci(n - 2)
}

func main() {
    print("First 8 Fibonacci numbers:")
    for (var i = 0; i < 8; i = i + 1) {
        print("F(" + i + ") =", fibonacci(i))
    }
}`
    },
    {
      name: 'Variables Demo',
      code: `func main() {
    var name = "Alice"
    var age = 25
    var height = 1.75
    var isStudent = true
    
    print("Name:", name)
    print("Age:", age)
    print("Height:", height)
    print("Is Student:", isStudent)
    
    // Array operations
    var scores = [95, 87, 92, 88]
    print("Scores:", scores)
    print("Average:", (scores[0] + scores[1] + scores[2] + scores[3]) / 4)
}`
    },
    {
      name: 'Functions & Closures',
      code: `func main() {
    // Functions are values: store them, pass them, return them.
    var double = func(x) { return x * 2; };
    print("double(21) =", double(21));

    // A closure keeps its own private state.
    var makeCounter = func() {
        var count = 0;
        return func() { count += 1; return count; };
    };
    var next = makeCounter();
    next(); next();
    print("counter:", next());

    // The collection pipeline.
    var nums = [5, 3, 8, 1, 9];
    print("squares:", map(nums, func(x) { return x * x; }));
    print("big ones:", filter(nums, func(x) { return x > 4; }));
    print("total:", reduce(nums, func(a, b) { return a + b; }));
    print("sorted:", sort(nums));
    print("descending:", sort(nums, func(a, b) { return b - a; }));
}`
    },
    {
      name: 'Error Handling',
      code: `func main() {
    // Throw any value you like.
    try {
        throw {field: "age", reason: "must not be negative"};
    } catch (e) {
        print("validation failed:", e.field, "-", e.reason);
    }

    // The interpreter's own errors are catchable too.
    try {
        var arr = [1, 2, 3];
        print(arr[99]);
    } catch (e) {
        print("runtime error:", e.message);
    }

    // So you can recover instead of halting.
    print("10 / 0 =", safeDivide(10, 0));

    try {
        print("working");
    } finally {
        print("finally always runs");
    }
}

func safeDivide(a, b) {
    try { return a / b; } catch (e) { return null; }
}`
    },
    {
      name: 'Structs & Matching',
      code: `// A struct gives a value a name and a fixed shape.
struct Circle { radius; func area() { return 3.14159 * this.radius * this.radius; } }
struct Rect   { w, h;   func area() { return this.w * this.h; } }

func describe(value) {
    // match branches on a value's *shape*, not just equality.
    match (value) {
        case Circle(r) if (r > 10):  return "a huge circle";
        case Circle(r):              return "a circle of radius \${r}";
        case Rect(w, h) if (w == h): return "a \${w}x\${h} square";
        case Rect(w, h):             return "a \${w}x\${h} rectangle";
        case [a, b]:                 return "a pair: \${a}, \${b}";
        case {kind: k}:              return "something tagged \${k}";
        case n if (type(n) == "number"): return "the number \${n}";
        default:                     return "something else";
    }
}

func main() {
    var shapes = [Circle(2), Circle(50), Rect(3, 3), Rect(3, 4)];
    for (s in shapes) {
        print(describe(s), "-> area", round(s.area(), 2));
    }

    for (v in [[1, 2], {kind: "note"}, 42, "text"]) {
        print(describe(v));
    }

    // Structs are values: equal by fields, usable in the pipeline.
    print(Circle(2) == Circle(2), Circle(2) == Rect(1, 1));
    print(map(shapes, func(s) { return round(s.area(), 1); }));
}`
    },
    {
      name: 'Generators',
      code: `// A function containing \`yield\` is lazy: calling it runs nothing and
// hands back a sequence that computes values only when asked.

func naturals() {
    var n = 0;
    while (true) { yield n; n += 1; }
}

func fibonacci() {
    var a = 0; var b = 1;
    while (true) { yield a; var following = a + b; a = b; b = following; }
}

// Generators consuming generators: nothing in between is materialised.
func mapped(source, f)   { for (x in source) { yield f(x); } }
func kept(source, pred)  { for (x in source) { if (pred(x)) { yield x; } } }
func until(source, pred) { for (x in source) { if (!pred(x)) { return; } yield x; } }

func main() {
    print("naturals:", take(naturals(), 8));
    print("fibonacci:", take(fibonacci(), 10));

    var squares = mapped(naturals(), func(n) { return n * n; });
    print("squares under 200:", toArray(until(squares, func(n) { return n < 200; })));

    var evenFibs = kept(fibonacci(), func(n) { return n % 2 == 0; });
    print("even fibonacci:", take(evenFibs, 6));

    // break just stops asking the source for more.
    var seen = [];
    for (n in naturals()) {
        if (len(seen) == 4) { break; }
        push(seen, n * 10);
    }
    print("stopped early:", seen);
}`
    },
    {
      name: 'Coroutines',
      code: `// \`yield\` is normally a statement, but as the whole right-hand side of a
// declaration or an assignment it produces whatever \`send()\` hands back --
// which turns a generator into a small coroutine.

func running() {
    var total = 0;
    var count = 0;
    while (true) {
        var n = yield {total: total, count: count};
        if (n == null) { return; }
        total += n;
        count += 1;
    }
}

func door() {
    var state = "closed";
    while (true) {
        var command = yield state;
        state = match (command) {
            case "open": "open",
            case "close": "closed",
            case "lock": match (state) { case "open": "open", default: "locked" },
            default: state
        };
    }
}

// \`yield*\` re-yields another sequence as if it were ours, and forwards
// anything sent in straight through to it.
func prelude() { yield "ready"; }
func greeter() {
    yield* prelude();
    var name = yield "name?";
    yield "hello \${name}";
}

func main() {
    print("-- a running total --");
    var totals = running();
    next(totals);                     // start it; the first sent value is dropped
    for (n in [5, 3, 12]) {
        var step = send(totals, n);
        print("after \${n}: \${step.value.total} over \${step.value.count}");
    }
    print("finished:", send(totals, null).done);

    print("-- a state machine --");
    var d = door();
    print(next(d).value);
    for (command in ["open", "lock", "close", "lock", "wiggle"]) {
        print("\${command} -> \${send(d, command).value}");
    }

    print("-- delegation --");
    var g = greeter();
    print(next(g).value);
    print(send(g, null).value);
    print(send(g, "Ada").value);
}`
    },
    {
      name: 'Lazy Pipelines',
      code: `// \`map\` and \`filter\` hand back a generator when given one, so a pipeline
// over an endless sequence computes only what is pulled out of the end.

func naturals() {
    var n = 0;
    while (true) { yield n; n += 1; }
}

func isPrime(n) {
    if (n < 2) { return false; }
    var d = 2;
    while (d * d <= n) {
        if (n % d == 0) { return false; }
        d += 1;
    }
    return true;
}

// A struct with an \`iter()\` method is iterable everywhere an array is.
struct Span {
    lo, hi;
    func iter() {
        var n = this.lo;
        while (n < this.hi) { yield n; n += 1; }
    }
}

struct Deck {
    cards;
    func iter() { return this.cards; }   // any iterable will do
}

func main() {
    print("-- lazy --");
    var squares = map(naturals(), func(n) { return n * n; });
    print(take(squares, 6));
    print(take(squares, 3));             // resumes where the last take stopped
    print(take(filter(naturals(), isPrime), 8));

    var calls = 0;
    var watched = map(naturals(), func(n) { calls += 1; return n; });
    take(watched, 4);
    print("pulled 4, called \${calls} times");

    print("-- eager on everything else --");
    print(map([1, 2, 3], func(n) { return n * 10; }));
    print(map("abc", toUpper));
    print(filter({a: 1, bb: 2, ccc: 3}, func(k) { return len(k) > 1; }));

    print("-- structs that iterate themselves --");
    for (n in Span(1, 5)) { print(n); }
    print(reduce(Span(1, 101), func(a, b) { return a + b; }));
    print(toArray(Deck(["A", "K", "Q"])), take(Span(0, 1000000), 3));
}`
    },
    {
      name: 'Expressions',
      code: `// \`match\` in expression position *is* its value, and a pattern can assign
// to variables that already exist.

struct Circle { radius; }
struct Rect { w, h; }

func area(shape) {
    return match (shape) {
        case Circle(r): round(3.14159 * r * r, 2),
        case Rect(w, h): w * h,
        default: 0
    };
}

func httpMessage(code) {
    return match (code) {
        case 200: "OK",
        case 404: "Not Found",
        case n if (n >= 500): "Server Error (\${n})",
        case n if (n >= 400): "Client Error (\${n})",
        default: "Unknown (\${code})"
    };
}

func main() {
    print("-- match as an expression --");
    print(area(Circle(2)), area(Rect(3, 4)), area("nope"));
    for (code in [200, 404, 503, 418]) { print("\${code}: \${httpMessage(code)}"); }
    print(map([1, 2, 3], func(n) {
        return match (n % 2) { case 0: "even", default: "odd" };
    }));

    print("-- destructuring assignment --");
    var a = 1;
    var b = 2;
    [a, b] = [b, a];
    print(a, b);

    var x = 0;
    var y = 0;
    var rest = {};
    {x, y, ...rest} = {x: 10, y: 20, label: "origin-ish"};
    print(x, y, rest);

    var head = 0;
    var tail = [];
    [head, ...tail] = [1, 2, 3, 4];
    print(head, tail);

    // Fibonacci with no temporary.
    var p = 0;
    var q = 1;
    var seq = [];
    for (var i = 0; i < 10; i += 1) {
        push(seq, p);
        [p, q] = [q, p + q];
    }
    print(seq);

    // \`from\` and \`as\` are contextual keywords, so they are usable as names.
    var trip = {from: "LHR", as: "economy", to: "JFK"};
    print(trip.from, trip.as, trip.to);
}`
    },
    {
      name: 'Destructuring',
      code: `// Wherever you name something, you can take it apart instead.

func midpoint([x1, y1], [x2, y2]) {
    return [(x1 + x2) / 2, (y1 + y2) / 2];
}

func greet({name, greeting = "Hello"}) {
    return "\${greeting}, \${name}!";
}

func main() {
    var [first, ...rest] = [1, 2, 3, 4];
    print(first, rest);

    var {name, role = "unknown"} = {name: "Ada", city: "London"};
    print(name, role);

    var {name: who, ...others} = {name: "Bob", team: "ops", level: 3};
    print(who, others);

    print(midpoint([0, 0], [4, 6]));
    print(greet({name: "Ada"}), greet({name: "Bob", greeting: "Hi"}));

    for ([label, count] in [["apples", 3], ["pears", 7]]) {
        print("\${label}: \${count}");
    }

    // Destructuring is strict: a missing piece is an error, not null.
    try { var [a, b] = [1]; } catch ({kind, message}) { print(kind, "-", message); }
}`
    },
    {
      name: 'Modules',
      code: `// The Playground ships two built-in modules you can import.
// Run the same program from the command line and the imports
// resolve against real files instead -- the semantics are identical.
import { mean, median, describe } from "./stats.mrt";
import { titleCase, wordCount, truncate } from "./text.mrt";

func main() {
    var scores = [88, 92, 79, 95, 84, 92];
    print("mean:  ", round(mean(scores), 2));
    print("median:", median(scores));
    print(describe(scores, "scores"));

    var phrase = "the quick brown fox jumps";
    print(titleCase(phrase));
    print("words:", wordCount(phrase));
    print(truncate(phrase, 15));

    // Modules throw like any other code, and you can catch it.
    try { mean([]); } catch (e) { print("caught:", e); }
}`
    },
    {
      name: 'Modern Syntax',
      code: `func main() {
    // String interpolation and bareword object keys.
    var person = {name: "Ada", age: 36};
    print("Hi \${person.name}, you are \${person.age}!");

    // null is a real literal now.
    var missing = null;
    print("missing:", missing, type(missing));

    // for-in walks arrays, strings and object keys.
    for (key in person) {
        print("  \${key} = \${get(person, key)}");
    }
    for (ch in "hi") { print("char:", ch); }

    // Each iteration gets a fresh binding, so closures capture correctly.
    var fns = [];
    for (n in [1, 2, 3]) { push(fns, func() { return n; }); }
    print("captured:", map(fns, func(f) { return f(); }));

    // Seeded randomness is deterministic and repeatable.
    var rng = random(2026);
    var rolls = [];
    for (i in range(5)) { push(rolls, floor(rng() * 6) + 1); }
    print("dice:", rolls);
}`
    }
  ]

  const handleRun = async () => {
    setIsRunning(true)
    setOutput('Running...')

    // A short delay keeps the "Running..." state visible for fast programs
    // too, so the button feedback doesn't feel like it did nothing.
    setTimeout(() => {
      try {
        const { output, errors } = runMRT(code, {
          path: './main.mrt',
          resolveModule: resolvePlaygroundModule,
        })
        if (errors.length > 0) {
          setOutput(errors.join('\n'))
        } else {
          setOutput(output.join('\n') || 'Program executed successfully (no output)')
        }
      } catch (error) {
        setOutput(`Error: ${error}`)
      }
      setIsRunning(false)
    }, 150)
  }

  const handleReset = () => {
    setCode(examples[0].code)
    setOutput('')
  }

  const handleLoadExample = (exampleCode: string) => {
    setCode(exampleCode)
    setOutput('')
  }

  const handleShare = async () => {
    try {
      const shareData = {
        title: 'MRT Code Playground',
        text: 'Check out this MRT code!',
        url: window.location.href
      }
      
      if (navigator.share) {
        await navigator.share(shareData)
      } else {
        // Fallback to copying URL
        await navigator.clipboard.writeText(window.location.href)
        alert('Link copied to clipboard!')
      }
    } catch (err) {
      console.error('Error sharing:', err)
    }
  }

  const handleDownload = () => {
    const blob = new Blob([code], { type: 'text/plain' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = 'playground.mrt'
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
  }

  return (
    <div className="min-h-screen bg-slate-100">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        {/* Header */}
        <div className="mb-8">
          <h1 className="text-3xl font-bold text-slate-900 mb-2">
            MRT Playground
          </h1>
          <p className="text-slate-600">
            Write and run MRT code directly in your browser
          </p>
        </div>

        {/* Toolbar */}
        <div className="bg-white rounded-lg shadow-sm border border-slate-200 p-4 mb-6">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <div className="flex flex-wrap items-center gap-3">
              <button
                onClick={handleRun}
                disabled={isRunning}
                className="flex items-center px-4 py-2 bg-primary-600 text-white rounded-lg hover:bg-primary-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              >
                <Play className="w-4 h-4 mr-2" />
                {isRunning ? 'Running...' : 'Run Code'}
              </button>
              
              <button
                onClick={handleReset}
                className="flex items-center px-4 py-2 bg-slate-600 text-white rounded-lg hover:bg-slate-700 transition-colors"
              >
                <RotateCcw className="w-4 h-4 mr-2" />
                Reset
              </button>
              
              <button
                onClick={handleDownload}
                className="flex items-center px-3 py-2 border border-slate-300 text-slate-700 rounded-lg hover:bg-slate-50 transition-colors"
              >
                <Download className="w-4 h-4 mr-2" />
                Download
              </button>
              
              <button
                onClick={handleShare}
                className="flex items-center px-3 py-2 border border-slate-300 text-slate-700 rounded-lg hover:bg-slate-50 transition-colors"
              >
                <Share className="w-4 h-4 mr-2" />
                Share
              </button>
            </div>
            
            <div className="flex items-center gap-3">
              <button
                onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
                className="flex items-center px-3 py-2 border border-slate-300 text-slate-700 rounded-lg hover:bg-slate-50 transition-colors"
              >
                <Settings className="w-4 h-4 mr-2" />
                {theme === 'dark' ? 'Light' : 'Dark'} Theme
              </button>
            </div>
          </div>
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-4 gap-6">
          {/* Examples Sidebar */}
          <div className="lg:col-span-1">
            <div className="bg-white rounded-lg shadow-sm border border-slate-200 p-4">
              <h3 className="font-semibold text-slate-900 mb-4">Examples</h3>
              <div className="space-y-2">
                {examples.map((example, index) => (
                  <button
                    key={index}
                    onClick={() => handleLoadExample(example.code)}
                    className="w-full text-left px-3 py-2 text-sm text-slate-600 hover:text-slate-900 hover:bg-slate-50 rounded-lg transition-colors"
                  >
                    {example.name}
                  </button>
                ))}
              </div>
            </div>
          </div>

          {/* Main Content */}
          <div className="lg:col-span-3 space-y-6">
            {/* Code Editor */}
            <div className="bg-white rounded-lg shadow-sm border border-slate-200 overflow-hidden">
              <div className="bg-slate-50 px-4 py-2 border-b border-slate-200">
                <h3 className="font-medium text-slate-900">Code Editor</h3>
              </div>
              <div className="h-96">
                <Editor
                  height="100%"
                  defaultLanguage="javascript"
                  theme={theme === 'dark' ? 'vs-dark' : 'light'}
                  value={code}
                  onChange={(value) => setCode(value || '')}
                  options={{
                    minimap: { enabled: false },
                    fontSize: 14,
                    lineNumbers: 'on',
                    roundedSelection: false,
                    scrollBeyondLastLine: false,
                    automaticLayout: true,
                    tabSize: 4,
                    insertSpaces: true,
                    wordWrap: 'on',
                    fontFamily: 'JetBrains Mono, Monaco, Consolas, monospace',
                  }}
                />
              </div>
            </div>

            {/* Output */}
            <div className="bg-white rounded-lg shadow-sm border border-slate-200">
              <div className="bg-slate-50 px-4 py-2 border-b border-slate-200">
                <h3 className="font-medium text-slate-900">Output</h3>
              </div>
              <div className="p-4">
                <pre className="text-sm text-slate-700 font-mono whitespace-pre-wrap min-h-[200px] bg-slate-50 p-4 rounded-lg">
                  {output || 'Click "Run Code" to see the output here...'}
                </pre>
              </div>
            </div>
          </div>
        </div>

        {/* Info Section */}
        <div className="mt-8 bg-blue-50 border border-blue-200 rounded-lg p-6">
          <h3 className="text-lg font-semibold text-blue-900 mb-2">
            About the Playground
          </h3>
          <p className="text-blue-800 mb-4">
            This playground runs a complete MRT interpreter simulation in your browser. 
            It supports functions, variables, arrays, strings, control flow, and all built-in functions.
            For full functionality and to run programs locally, install MRT using:
          </p>
          <code className="bg-blue-100 text-blue-900 px-3 py-1 rounded font-mono text-sm">
            pip install mrt-lang
          </code>
        </div>
      </div>
    </div>
  )
}

export default Playground