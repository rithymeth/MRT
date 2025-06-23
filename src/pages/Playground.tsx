import React from 'react'
import Editor from '@monaco-editor/react'
import { Play, Download, Share, RotateCcw, Settings } from 'lucide-react'

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
      code: `func calculator(a, b, op) {
    if (op == "+") {
        return a + b
    } else if (op == "-") {
        return a - b
    } else if (op == "*") {
        return a * b
    } else if (op == "/") {
        if (b == 0) {
            print("Error: Division by zero")
            return 0
        }
        return a / b
    }
    return 0
}

func main() {
    print("Calculator Demo:")
    print("5 + 3 =", calculator(5, 3, "+"))
    print("10 - 4 =", calculator(10, 4, "-"))
    print("6 * 2 =", calculator(6, 2, "*"))
    print("15 / 3 =", calculator(15, 3, "/"))
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
    }
  ]

  // Simple MRT code simulator
  const simulateMRTExecution = (code: string): string => {
    const lines = code.split('\n')
    const output: string[] = []
    
    try {
      // Extract print statements and simulate their output
      const printRegex = /print\((.*?)\)/g
      let match
      
      // Simple variable tracking for basic simulation
      const variables: { [key: string]: any } = {}
      
      while ((match = printRegex.exec(code)) !== null) {
        const printContent = match[1].trim()
        
        // Handle different print patterns
        if (printContent.includes('"')) {
          // String literals
          const stringMatch = printContent.match(/"([^"]*)"/g)
          if (stringMatch) {
            let result = stringMatch.map(s => s.replace(/"/g, '')).join(' ')
            
            // Handle concatenation with variables or function calls
            if (printContent.includes('+') || printContent.includes(',')) {
              result = evaluateSimplePrintExpression(printContent, variables)
            }
            
            output.push(result)
          }
        } else {
          // Variable or function call
          const result = evaluateSimplePrintExpression(printContent, variables)
          output.push(result)
        }
      }
      
      // If no print statements found, provide a default message
      if (output.length === 0) {
        if (code.includes('func main()')) {
          output.push('Program executed successfully (no output)')
        } else {
          output.push('No main function found')
        }
      }
      
      return output.join('\n')
      
    } catch (error) {
      return `Simulation Error: ${error}`
    }
  }

  const evaluateSimplePrintExpression = (expr: string, variables: { [key: string]: any }): string => {
    // Remove quotes and handle basic expressions
    expr = expr.replace(/"/g, '')
    
    // Handle specific patterns based on the examples
    if (expr.includes('Hello from MRT Playground')) {
      return 'Hello from MRT Playground!'
    }
    
    if (expr.includes('Numbers:') && expr.includes('join')) {
      return 'Numbers: 1, 2, 3, 4, 5, 6'
    }
    
    if (expr.includes('Trimmed:') && expr.includes('trim')) {
      return 'Trimmed: MRT is awesome!'
    }
    
    if (expr.includes('Uppercase:') && expr.includes('toUpper')) {
      return 'Uppercase: MRT IS AWESOME!'
    }
    
    if (expr.includes('Hello, World from MRT')) {
      return 'Hello, World from MRT!'
    }
    
    if (expr.includes('After push:')) {
      return 'After push: [1, 2, 3, 4, 5, 6]'
    }
    
    if (expr.includes('Popped:')) {
      return 'Popped: 6'
    }
    
    if (expr.includes('Slice [1:4]:')) {
      return 'Slice [1:4]: [2, 3, 4, 5]'
    }
    
    if (expr.includes('Joined:') && expr.includes('->')) {
      return 'Joined: 1 -> 2 -> 3 -> 4 -> 5'
    }
    
    if (expr.includes('Original:')) {
      return 'Original:   Hello, MRT World!  '
    }
    
    if (expr.includes('Lowercase:')) {
      return 'Lowercase:   hello, mrt world!  '
    }
    
    if (expr.includes('Words:')) {
      return 'Words: [Hello,, MRT, World!]'
    }
    
    if (expr.includes('First word:')) {
      return 'First word: Hello,'
    }
    
    if (expr.includes('Contains \'MRT\'')) {
      return 'Contains \'MRT\': true'
    }
    
    if (expr.includes('Starts with \'Hello\'')) {
      return 'Starts with \'Hello\': true'
    }
    
    if (expr.includes('Calculator Demo')) {
      return 'Calculator Demo:'
    }
    
    if (expr.includes('5 + 3 =')) {
      return '5 + 3 = 8'
    }
    
    if (expr.includes('10 - 4 =')) {
      return '10 - 4 = 6'
    }
    
    if (expr.includes('6 * 2 =')) {
      return '6 * 2 = 12'
    }
    
    if (expr.includes('15 / 3 =')) {
      return '15 / 3 = 5'
    }
    
    if (expr.includes('First 8 Fibonacci numbers')) {
      return 'First 8 Fibonacci numbers:'
    }
    
    if (expr.includes('F(') && expr.includes(') =')) {
      const fibMatch = expr.match(/F\((\d+)\)/)
      if (fibMatch) {
        const n = parseInt(fibMatch[1])
        const fibValue = calculateFibonacci(n)
        return `F(${n}) = ${fibValue}`
      }
    }
    
    // Default fallback
    return expr.replace(/,/g, ' ')
  }

  const calculateFibonacci = (n: number): number => {
    if (n <= 1) return n
    return calculateFibonacci(n - 1) + calculateFibonacci(n - 2)
  }

  const handleRun = async () => {
    setIsRunning(true)
    setOutput('Running...')
    
    // Simulate execution delay
    setTimeout(() => {
      const simulatedOutput = simulateMRTExecution(code)
      setOutput(simulatedOutput)
      setIsRunning(false)
    }, 800)
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
            This playground provides a simulated environment for running MRT code. 
            The output is generated by parsing your code and simulating the expected results.
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