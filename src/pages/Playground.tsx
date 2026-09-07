import React from 'react'
import Editor from '@monaco-editor/react'
import { Play, Download, Share, RotateCcw, Settings } from 'lucide-react'
import { runMRT } from '../lib/mrtInterpreter'

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
        const { output, errors } = runMRT(code)
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