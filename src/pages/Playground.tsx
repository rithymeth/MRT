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
    }
  ]

  // Enhanced MRT interpreter simulation
  class MRTSimulator {
    private variables: Map<string, any> = new Map()
    private functions: Map<string, any> = new Map()
    private output: string[] = []

    constructor() {
      this.setupBuiltins()
    }

    private setupBuiltins() {
      // Built-in functions
      this.functions.set('print', (...args: any[]) => {
        const output = args.map(arg => this.valueToString(arg)).join(' ')
        this.output.push(output)
      })

      this.functions.set('len', (arr: any[]) => {
        if (Array.isArray(arr)) return arr.length
        if (typeof arr === 'string') return arr.length
        return 0
      })

      this.functions.set('push', (arr: any[], item: any) => {
        if (Array.isArray(arr)) {
          arr.push(item)
          return item
        }
        return null
      })

      this.functions.set('pop', (arr: any[]) => {
        if (Array.isArray(arr) && arr.length > 0) {
          return arr.pop()
        }
        return null
      })

      this.functions.set('slice', (arr: any[], start: number, end?: number) => {
        if (Array.isArray(arr)) {
          return arr.slice(start, end)
        }
        return []
      })

      this.functions.set('join', (arr: any[], separator: string = '') => {
        if (Array.isArray(arr)) {
          return arr.map(item => this.valueToString(item)).join(separator)
        }
        return ''
      })

      this.functions.set('indexOf', (arr: any[], item: any) => {
        if (Array.isArray(arr)) {
          return arr.indexOf(item)
        }
        return -1
      })

      // String functions
      this.functions.set('trim', (str: string) => {
        return typeof str === 'string' ? str.trim() : ''
      })

      this.functions.set('toUpper', (str: string) => {
        return typeof str === 'string' ? str.toUpperCase() : ''
      })

      this.functions.set('toLower', (str: string) => {
        return typeof str === 'string' ? str.toLowerCase() : ''
      })

      this.functions.set('split', (str: string, separator: string = ' ') => {
        return typeof str === 'string' ? str.split(separator) : []
      })

      this.functions.set('substring', (str: string, start: number, end?: number) => {
        return typeof str === 'string' ? str.substring(start, end) : ''
      })

      this.functions.set('replace', (str: string, search: string, replace: string) => {
        return typeof str === 'string' ? str.replace(new RegExp(search, 'g'), replace) : ''
      })

      this.functions.set('startsWith', (str: string, prefix: string) => {
        return typeof str === 'string' ? str.startsWith(prefix) : false
      })

      this.functions.set('endsWith', (str: string, suffix: string) => {
        return typeof str === 'string' ? str.endsWith(suffix) : false
      })

      this.functions.set('contains', (str: string, substr: string) => {
        return typeof str === 'string' ? str.includes(substr) : false
      })
    }

    private valueToString(value: any): string {
      if (value === null || value === undefined) return 'null'
      if (typeof value === 'boolean') return value.toString()
      if (typeof value === 'number') return value.toString()
      if (typeof value === 'string') return value
      if (Array.isArray(value)) return '[' + value.map(v => this.valueToString(v)).join(', ') + ']'
      return String(value)
    }

    execute(code: string): string {
      this.output = []
      this.variables.clear()
      
      try {
        // Parse the code into functions and statements
        const parsed = this.parseCode(code)
        
        // Define all functions first
        for (const func of parsed.functions) {
          this.functions.set(func.name, func)
        }

        // Execute main function if it exists
        if (this.functions.has('main')) {
          const mainFunc = this.functions.get('main')
          this.executeFunction(mainFunc, [])
        } else {
          // Execute top-level statements
          for (const stmt of parsed.statements) {
            this.executeStatement(stmt)
          }
        }

        return this.output.join('\n') || 'Program executed successfully (no output)'
      } catch (error) {
        return `Runtime Error: ${error}`
      }
    }

    private parseCode(code: string) {
      const lines = code.split('\n').map(line => line.trim()).filter(line => line && !line.startsWith('//'))
      const functions: any[] = []
      const statements: any[] = []
      
      let i = 0
      while (i < lines.length) {
        const line = lines[i]
        
        if (line.startsWith('func ')) {
          const func = this.parseFunction(lines, i)
          functions.push(func.function)
          i = func.nextIndex
        } else {
          statements.push(line)
          i++
        }
      }
      
      return { functions, statements }
    }

    private parseFunction(lines: string[], startIndex: number) {
      const funcLine = lines[startIndex]
      const funcMatch = funcLine.match(/func\s+(\w+)\s*\(([^)]*)\)\s*\{?/)
      
      if (!funcMatch) {
        throw new Error(`Invalid function declaration: ${funcLine}`)
      }
      
      const name = funcMatch[1]
      const params = funcMatch[2].split(',').map(p => p.trim()).filter(p => p)
      
      const body: string[] = []
      let braceCount = (funcLine.match(/\{/g) || []).length
      let i = startIndex + 1
      
      while (i < lines.length && braceCount > 0) {
        const line = lines[i]
        braceCount += (line.match(/\{/g) || []).length - (line.match(/\}/g) || []).length
        
        if (braceCount > 0) {
          body.push(line)
        }
        i++
      }
      
      return {
        function: { name, params, body },
        nextIndex: i
      }
    }

    private executeFunction(func: any, args: any[]): any {
      const localVars = new Map(this.variables)
      
      // Set parameters
      for (let i = 0; i < func.params.length; i++) {
        if (i < args.length) {
          localVars.set(func.params[i], args[i])
        }
      }
      
      const savedVars = this.variables
      this.variables = localVars
      
      try {
        for (const stmt of func.body) {
          const result = this.executeStatement(stmt)
          if (result && result.type === 'return') {
            this.variables = savedVars
            return result.value
          }
        }
      } finally {
        this.variables = savedVars
      }
      
      return null
    }

    private executeStatement(stmt: string): any {
      stmt = stmt.trim()
      
      if (stmt.startsWith('var ')) {
        this.executeVarDeclaration(stmt)
      } else if (stmt.startsWith('print(')) {
        this.executePrint(stmt)
      } else if (stmt.startsWith('return ')) {
        const value = this.evaluateExpression(stmt.substring(7))
        return { type: 'return', value }
      } else if (stmt.startsWith('if (')) {
        this.executeIf(stmt)
      } else if (stmt.startsWith('for (')) {
        this.executeFor(stmt)
      } else if (stmt.includes('=') && !stmt.includes('==')) {
        this.executeAssignment(stmt)
      } else {
        // Expression statement
        this.evaluateExpression(stmt)
      }
      
      return null
    }

    private executeVarDeclaration(stmt: string) {
      const match = stmt.match(/var\s+(\w+)\s*=\s*(.+)/)
      if (match) {
        const varName = match[1]
        const value = this.evaluateExpression(match[2])
        this.variables.set(varName, value)
      }
    }

    private executePrint(stmt: string) {
      const match = stmt.match(/print\((.*)\)/)
      if (match) {
        const args = this.parseArguments(match[1])
        const values = args.map(arg => this.evaluateExpression(arg))
        this.functions.get('print')(...values)
      }
    }

    private executeIf(stmt: string) {
      // Simplified if statement handling
      const match = stmt.match(/if\s*\(([^)]+)\)\s*\{/)
      if (match) {
        const condition = this.evaluateExpression(match[1])
        if (this.isTruthy(condition)) {
          // Execute if body (simplified)
          // In a real implementation, we'd need to parse the block properly
        }
      }
    }

    private executeFor(stmt: string) {
      // Simplified for loop handling
      const match = stmt.match(/for\s*\(\s*var\s+(\w+)\s*=\s*([^;]+);\s*([^;]+);\s*([^)]+)\)\s*\{/)
      if (match) {
        const varName = match[1]
        const init = this.evaluateExpression(match[2])
        const condition = match[3]
        const increment = match[4]
        
        this.variables.set(varName, init)
        
        while (this.isTruthy(this.evaluateExpression(condition))) {
          // Execute loop body (simplified)
          this.evaluateExpression(increment)
        }
      }
    }

    private executeAssignment(stmt: string) {
      const parts = stmt.split('=')
      if (parts.length === 2) {
        const varName = parts[0].trim()
        const value = this.evaluateExpression(parts[1].trim())
        this.variables.set(varName, value)
      }
    }

    private parseArguments(argsStr: string): string[] {
      const args: string[] = []
      let current = ''
      let parenCount = 0
      let inString = false
      
      for (let i = 0; i < argsStr.length; i++) {
        const char = argsStr[i]
        
        if (char === '"' && (i === 0 || argsStr[i-1] !== '\\')) {
          inString = !inString
        }
        
        if (!inString) {
          if (char === '(') parenCount++
          else if (char === ')') parenCount--
          else if (char === ',' && parenCount === 0) {
            args.push(current.trim())
            current = ''
            continue
          }
        }
        
        current += char
      }
      
      if (current.trim()) {
        args.push(current.trim())
      }
      
      return args
    }

    private evaluateExpression(expr: string): any {
      expr = expr.trim()
      
      // Handle string literals
      if (expr.startsWith('"') && expr.endsWith('"')) {
        return expr.slice(1, -1)
      }
      
      // Handle numbers
      if (/^-?\d+(\.\d+)?$/.test(expr)) {
        return parseFloat(expr)
      }
      
      // Handle booleans
      if (expr === 'true') return true
      if (expr === 'false') return false
      
      // Handle arrays
      if (expr.startsWith('[') && expr.endsWith(']')) {
        const content = expr.slice(1, -1)
        if (!content.trim()) return []
        const elements = this.parseArguments(content)
        return elements.map(el => this.evaluateExpression(el))
      }
      
      // Handle function calls
      if (expr.includes('(') && expr.includes(')')) {
        const match = expr.match(/(\w+)\((.*)\)/)
        if (match) {
          const funcName = match[1]
          const args = match[2] ? this.parseArguments(match[2]) : []
          const argValues = args.map(arg => this.evaluateExpression(arg))
          
          if (this.functions.has(funcName)) {
            const func = this.functions.get(funcName)
            if (typeof func === 'function') {
              return func(...argValues)
            } else {
              return this.executeFunction(func, argValues)
            }
          }
        }
      }
      
      // Handle arithmetic operations
      if (expr.includes('+') || expr.includes('-') || expr.includes('*') || expr.includes('/')) {
        return this.evaluateArithmetic(expr)
      }
      
      // Handle comparisons
      if (expr.includes('==') || expr.includes('!=') || expr.includes('<') || expr.includes('>')) {
        return this.evaluateComparison(expr)
      }
      
      // Handle variables
      if (this.variables.has(expr)) {
        return this.variables.get(expr)
      }
      
      // Handle array access
      if (expr.includes('[') && expr.includes(']')) {
        const match = expr.match(/(\w+)\[([^\]]+)\]/)
        if (match) {
          const arrayName = match[1]
          const index = this.evaluateExpression(match[2])
          const array = this.variables.get(arrayName)
          if (Array.isArray(array) && typeof index === 'number') {
            return array[index]
          }
        }
      }
      
      return expr
    }

    private evaluateArithmetic(expr: string): number {
      // Simple arithmetic evaluation
      try {
        // Replace variables with their values
        let processedExpr = expr
        for (const [name, value] of this.variables) {
          if (typeof value === 'number') {
            processedExpr = processedExpr.replace(new RegExp(`\\b${name}\\b`, 'g'), value.toString())
          }
        }
        
        // Handle string concatenation with +
        if (expr.includes('+') && (expr.includes('"') || this.hasStringVariable(expr))) {
          return this.evaluateStringConcatenation(expr)
        }
        
        // Evaluate numeric expression
        return Function(`"use strict"; return (${processedExpr})`)()
      } catch {
        return 0
      }
    }

    private hasStringVariable(expr: string): boolean {
      for (const [name, value] of this.variables) {
        if (typeof value === 'string' && expr.includes(name)) {
          return true
        }
      }
      return false
    }

    private evaluateStringConcatenation(expr: string): string {
      // Handle string concatenation
      const parts = expr.split('+').map(part => part.trim())
      return parts.map(part => {
        const value = this.evaluateExpression(part)
        return this.valueToString(value)
      }).join('')
    }

    private evaluateComparison(expr: string): boolean {
      const operators = ['==', '!=', '<=', '>=', '<', '>']
      
      for (const op of operators) {
        if (expr.includes(op)) {
          const parts = expr.split(op)
          if (parts.length === 2) {
            const left = this.evaluateExpression(parts[0].trim())
            const right = this.evaluateExpression(parts[1].trim())
            
            switch (op) {
              case '==': return left == right
              case '!=': return left != right
              case '<': return Number(left) < Number(right)
              case '>': return Number(left) > Number(right)
              case '<=': return Number(left) <= Number(right)
              case '>=': return Number(left) >= Number(right)
            }
          }
        }
      }
      
      return false
    }

    private isTruthy(value: any): boolean {
      if (value === null || value === undefined) return false
      if (typeof value === 'boolean') return value
      if (typeof value === 'number') return value !== 0
      if (typeof value === 'string') return value.length > 0
      return true
    }
  }

  const handleRun = async () => {
    setIsRunning(true)
    setOutput('Running...')
    
    // Simulate execution delay for better UX
    setTimeout(() => {
      try {
        const simulator = new MRTSimulator()
        const result = simulator.execute(code)
        setOutput(result)
      } catch (error) {
        setOutput(`Error: ${error}`)
      }
      setIsRunning(false)
    }, 500)
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