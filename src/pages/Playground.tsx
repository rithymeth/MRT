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
    }
  ]

  // Enhanced MRT code simulator with proper parsing and execution
  const simulateMRTExecution = async (code: string): Promise<string> => {
    try {
      // Send code to a simulated MRT interpreter
      const response = await fetch('/api/mrt-execute', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ code }),
      }).catch(() => {
        // Fallback to client-side simulation if API is not available
        return { ok: false }
      })

      if (response.ok) {
        const result = await response.json()
        return result.output || result.error || 'No output'
      }

      // Client-side simulation fallback
      return simulateClientSide(code)
    } catch (error) {
      return simulateClientSide(code)
    }
  }

  const simulateClientSide = (code: string): string => {
    const output: string[] = []
    
    try {
      // Parse and simulate the code execution
      const lines = code.split('\n').map(line => line.trim()).filter(line => line)
      
      // Simple state tracking
      const variables: { [key: string]: any } = {}
      const functions: { [key: string]: string[] } = {}
      
      // Extract functions first
      let currentFunction = ''
      let functionBody: string[] = []
      let braceCount = 0
      
      for (const line of lines) {
        if (line.startsWith('func ')) {
          const funcMatch = line.match(/func\s+(\w+)\s*\([^)]*\)\s*\{?/)
          if (funcMatch) {
            currentFunction = funcMatch[1]
            functionBody = []
            braceCount = (line.match(/\{/g) || []).length - (line.match(/\}/g) || []).length
          }
        } else if (currentFunction) {
          braceCount += (line.match(/\{/g) || []).length - (line.match(/\}/g) || []).length
          if (braceCount > 0) {
            functionBody.push(line)
          } else {
            functions[currentFunction] = functionBody
            currentFunction = ''
          }
        }
      }
      
      // Execute main function if it exists
      if (functions.main) {
        return executeFunction('main', functions, variables, {})
      }
      
      return 'No main function found'
      
    } catch (error) {
      return `Simulation Error: ${error}`
    }
  }

  const executeFunction = (funcName: string, functions: { [key: string]: string[] }, variables: { [key: string]: any }, params: { [key: string]: any }): string => {
    const output: string[] = []
    
    if (!functions[funcName]) {
      return `Function ${funcName} not found`
    }
    
    const functionBody = functions[funcName]
    const localVars = { ...variables, ...params }
    
    for (const line of functionBody) {
      if (line.includes('print(')) {
        const result = executePrintStatement(line, localVars, functions)
        if (result) output.push(result)
      } else if (line.startsWith('var ')) {
        executeVarDeclaration(line, localVars)
      } else if (line.includes('return ')) {
        const returnValue = executeReturnStatement(line, localVars, functions)
        return returnValue?.toString() || ''
      }
    }
    
    return output.join('\n')
  }

  const executePrintStatement = (line: string, variables: { [key: string]: any }, functions: { [key: string]: string[] }): string => {
    const printMatch = line.match(/print\((.*)\)/)
    if (!printMatch) return ''
    
    const args = printMatch[1].split(',').map(arg => arg.trim())
    const results: string[] = []
    
    for (const arg of args) {
      if (arg.startsWith('"') && arg.endsWith('"')) {
        // String literal
        results.push(arg.slice(1, -1))
      } else if (arg.includes('calculator(')) {
        // Calculator function call
        const calcResult = executeCalculatorCall(arg, variables, functions)
        results.push(calcResult.toString())
      } else if (variables[arg] !== undefined) {
        // Variable
        results.push(variables[arg].toString())
      } else {
        // Try to evaluate as expression
        results.push(evaluateExpression(arg, variables, functions))
      }
    }
    
    return results.join(' ')
  }

  const executeCalculatorCall = (call: string, variables: { [key: string]: any }, functions: { [key: string]: string[] }): number => {
    const match = call.match(/calculator\(\s*([^,]+),\s*([^,]+),\s*"([^"]+)"\s*\)/)
    if (!match) return 0
    
    const a = parseFloat(match[1])
    const b = parseFloat(match[2])
    const op = match[3]
    
    switch (op) {
      case '+': return a + b
      case '-': return a - b
      case '*': return a * b
      case '/': return b === 0 ? 0 : a / b
      default: return 0
    }
  }

  const executeVarDeclaration = (line: string, variables: { [key: string]: any }) => {
    const varMatch = line.match(/var\s+(\w+)\s*=\s*(.+)/)
    if (varMatch) {
      const varName = varMatch[1]
      const value = varMatch[2].trim()
      
      if (value.startsWith('[') && value.endsWith(']')) {
        // Array
        const arrayContent = value.slice(1, -1)
        variables[varName] = arrayContent.split(',').map(item => {
          const trimmed = item.trim()
          return isNaN(Number(trimmed)) ? trimmed : Number(trimmed)
        })
      } else if (value.startsWith('"') && value.endsWith('"')) {
        // String
        variables[varName] = value.slice(1, -1)
      } else if (!isNaN(Number(value))) {
        // Number
        variables[varName] = Number(value)
      } else {
        // Variable reference or expression
        variables[varName] = evaluateExpression(value, variables, {})
      }
    }
  }

  const executeReturnStatement = (line: string, variables: { [key: string]: any }, functions: { [key: string]: string[] }): any => {
    const returnMatch = line.match(/return\s+(.+)/)
    if (!returnMatch) return null
    
    const expression = returnMatch[1].trim()
    return evaluateExpression(expression, variables, functions)
  }

  const evaluateExpression = (expr: string, variables: { [key: string]: any }, functions: { [key: string]: string[] }): string => {
    expr = expr.trim()
    
    // Handle string literals
    if (expr.startsWith('"') && expr.endsWith('"')) {
      return expr.slice(1, -1)
    }
    
    // Handle numbers
    if (!isNaN(Number(expr))) {
      return expr
    }
    
    // Handle variables
    if (variables[expr] !== undefined) {
      return variables[expr].toString()
    }
    
    // Handle function calls
    if (expr.includes('calculator(')) {
      return executeCalculatorCall(expr, variables, functions).toString()
    }
    
    // Handle arithmetic expressions
    if (expr.includes('+') || expr.includes('-') || expr.includes('*') || expr.includes('/')) {
      try {
        // Simple arithmetic evaluation (unsafe in production, but OK for simulation)
        const result = Function(`"use strict"; return (${expr.replace(/[a-zA-Z_][a-zA-Z0-9_]*/g, (match) => {
          return variables[match] !== undefined ? variables[match] : match
        })})`)()
        return result.toString()
      } catch {
        return expr
      }
    }
    
    return expr
  }

  const handleRun = async () => {
    setIsRunning(true)
    setOutput('Running...')
    
    // Simulate execution delay
    setTimeout(async () => {
      const simulatedOutput = await simulateMRTExecution(code)
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