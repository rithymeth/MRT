// A small, faithful re-implementation of the MRT language runtime in
// TypeScript, mirroring src/lexer.py, src/parser.py and src/interpreter.py.
//
// This exists so the browser Playground actually runs MRT code -- the
// previous implementation matched source lines against regular
// expressions and never executed the bodies of `if`, `while` or `for`
// statements, so almost every example with a loop or a branch produced
// no output at all. This version tokenizes, parses to a real AST and
// tree-walks it, the same way the Python interpreter does.

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

type TokenType =
  | 'FUNC' | 'RETURN' | 'IF' | 'ELSE' | 'WHILE' | 'FOR' | 'PRINT' | 'VAR'
  | 'TRUE' | 'FALSE' | 'BREAK' | 'CONTINUE'
  | 'IDENTIFIER' | 'NUMBER' | 'STRING'
  | 'PLUS' | 'MINUS' | 'MULTIPLY' | 'DIVIDE' | 'MODULO'
  | 'PLUS_ASSIGN' | 'MINUS_ASSIGN' | 'MULTIPLY_ASSIGN' | 'DIVIDE_ASSIGN' | 'MODULO_ASSIGN'
  | 'ASSIGN' | 'EQUALS' | 'NOT_EQUALS'
  | 'GREATER' | 'GREATER_EQUAL' | 'LESS' | 'LESS_EQUAL'
  | 'AND' | 'OR' | 'NOT'
  | 'LPAREN' | 'RPAREN' | 'LBRACE' | 'RBRACE' | 'LBRACKET' | 'RBRACKET'
  | 'COMMA' | 'SEMICOLON' | 'DOT' | 'COLON' | 'EOF'

export interface Token {
  type: TokenType
  lexeme: string
  literal: unknown
  line: number
}

export class MRTError extends Error {
  line?: number
  constructor(message: string, line?: number) {
    super(line !== undefined ? `${message} [line ${line}]` : message)
    this.line = line
  }
}

// A Map, not a plain object: a plain `{...}` literal inherits
// Object.prototype, so a lookup like `KEYWORDS['toString']` for an MRT
// identifier that happens to be spelled "toString" (or "valueOf",
// "constructor", "hasOwnProperty", ...) would silently return the
// *inherited* Object.prototype method instead of `undefined`, corrupting
// that token's type. (This bit the `toString` built-in's own name during
// development -- see the BUILTINS.toString comment below for the sibling
// bug this caused there.)
const KEYWORDS: Map<string, TokenType> = new Map([
  ['func', 'FUNC'], ['return', 'RETURN'], ['if', 'IF'], ['else', 'ELSE'], ['while', 'WHILE'],
  ['for', 'FOR'], ['print', 'PRINT'], ['var', 'VAR'], ['true', 'TRUE'], ['false', 'FALSE'],
  ['break', 'BREAK'], ['continue', 'CONTINUE'],
])

const ESCAPES: Record<string, string> = { n: '\n', t: '\t', r: '\r', '"': '"', '\\': '\\', '0': '\0' }

class Lexer {
  private start = 0
  private current = 0
  private line = 1
  private tokens: Token[] = []

  constructor(private source: string) {}

  scanTokens(): Token[] {
    while (!this.isAtEnd()) {
      this.start = this.current
      this.scanToken()
    }
    this.tokens.push({ type: 'EOF', lexeme: '', literal: null, line: this.line })
    return this.tokens
  }

  private scanToken() {
    const c = this.advance()
    switch (c) {
      case '(': this.addToken('LPAREN'); break
      case ')': this.addToken('RPAREN'); break
      case '{': this.addToken('LBRACE'); break
      case '}': this.addToken('RBRACE'); break
      case '[': this.addToken('LBRACKET'); break
      case ']': this.addToken('RBRACKET'); break
      case ',': this.addToken('COMMA'); break
      case ';': this.addToken('SEMICOLON'); break
      case '.': this.addToken('DOT'); break
      case ':': this.addToken('COLON'); break
      case '+': this.addToken(this.match('=') ? 'PLUS_ASSIGN' : 'PLUS'); break
      case '-': this.addToken(this.match('=') ? 'MINUS_ASSIGN' : 'MINUS'); break
      case '*': this.addToken(this.match('=') ? 'MULTIPLY_ASSIGN' : 'MULTIPLY'); break
      case '%': this.addToken(this.match('=') ? 'MODULO_ASSIGN' : 'MODULO'); break
      case '/':
        if (this.match('/')) {
          while (this.peek() !== '\n' && !this.isAtEnd()) this.advance()
        } else if (this.match('*')) {
          this.blockComment()
        } else if (this.match('=')) {
          this.addToken('DIVIDE_ASSIGN')
        } else {
          this.addToken('DIVIDE')
        }
        break
      case ' ': case '\r': case '\t': break
      case '\n': this.line++; break
      case '"': this.string(); break
      case '>': this.addToken(this.match('=') ? 'GREATER_EQUAL' : 'GREATER'); break
      case '<': this.addToken(this.match('=') ? 'LESS_EQUAL' : 'LESS'); break
      case '=': this.addToken(this.match('=') ? 'EQUALS' : 'ASSIGN'); break
      case '!': this.addToken(this.match('=') ? 'NOT_EQUALS' : 'NOT'); break
      case '&':
        if (this.match('&')) this.addToken('AND')
        else throw new MRTError("Unexpected character '&' (did you mean '&&'?)", this.line)
        break
      case '|':
        if (this.match('|')) this.addToken('OR')
        else throw new MRTError("Unexpected character '|' (did you mean '||'?)", this.line)
        break
      default:
        if (this.isDigit(c)) this.number()
        else if (this.isAlpha(c)) this.identifier()
        else throw new MRTError(`Unexpected character '${c}'`, this.line)
    }
  }

  private blockComment() {
    while (!this.isAtEnd()) {
      if (this.peek() === '*' && this.peekNext() === '/') {
        this.advance(); this.advance()
        return
      }
      if (this.peek() === '\n') this.line++
      this.advance()
    }
    throw new MRTError('Unterminated comment', this.line)
  }

  private identifier() {
    while (this.isAlphanumeric(this.peek())) this.advance()
    const text = this.source.slice(this.start, this.current)
    const type = KEYWORDS.get(text) ?? 'IDENTIFIER'
    if (type === 'TRUE') this.addToken(type, true)
    else if (type === 'FALSE') this.addToken(type, false)
    else this.addToken(type)
  }

  private number() {
    while (this.isDigit(this.peek())) this.advance()
    if (this.peek() === '.' && this.isDigit(this.peekNext())) {
      this.advance()
      while (this.isDigit(this.peek())) this.advance()
    }
    this.addToken('NUMBER', parseFloat(this.source.slice(this.start, this.current)))
  }

  private string() {
    const startLine = this.line
    const chars: string[] = []
    while (this.peek() !== '"' && !this.isAtEnd()) {
      const c = this.peek()
      if (c === '\n') this.line++
      if (c === '\\' && ESCAPES[this.peekNext()] !== undefined) {
        this.advance()
        const escape = this.advance()
        chars.push(ESCAPES[escape])
      } else {
        chars.push(this.advance())
      }
    }
    if (this.isAtEnd()) throw new MRTError('Unterminated string', startLine)
    this.advance()
    this.addToken('STRING', chars.join(''))
  }

  private match(expected: string): boolean {
    if (this.isAtEnd() || this.source[this.current] !== expected) return false
    this.current++
    return true
  }

  private peek(): string { return this.isAtEnd() ? '\0' : this.source[this.current] }
  private peekNext(): string { return this.current + 1 >= this.source.length ? '\0' : this.source[this.current + 1] }
  private isAlpha(c: string) { return /[a-zA-Z_]/.test(c) }
  private isDigit(c: string) { return /[0-9]/.test(c) }
  private isAlphanumeric(c: string) { return this.isAlpha(c) || this.isDigit(c) }
  private isAtEnd() { return this.current >= this.source.length }
  private advance(): string { return this.source[this.current++] }

  private addToken(type: TokenType, literal: unknown = null) {
    this.tokens.push({ type, lexeme: this.source.slice(this.start, this.current), literal, line: this.line })
  }
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

type Expr =
  | { kind: 'Binary'; left: Expr; operator: Token; right: Expr }
  | { kind: 'Logical'; left: Expr; operator: Token; right: Expr }
  | { kind: 'Grouping'; expression: Expr }
  | { kind: 'Literal'; value: unknown }
  | { kind: 'Unary'; operator: Token; right: Expr }
  | { kind: 'Variable'; name: Token }
  | { kind: 'Assign'; name: Token; value: Expr }
  | { kind: 'Call'; callee: Expr; paren: Token; arguments: Expr[] }
  | { kind: 'Array'; elements: Expr[] }
  | { kind: 'ArrayAccess'; array: Expr; index: Expr }
  | { kind: 'ArrayAssign'; array: Expr; index: Expr; value: Expr }
  | { kind: 'DictLiteral'; pairs: [Expr, Expr][] }

type Stmt =
  | { kind: 'Expression'; expression: Expr }
  | { kind: 'Function'; name: Token; params: Token[]; body: Stmt[] }
  | { kind: 'If'; condition: Expr; thenBranch: Stmt; elseBranch: Stmt | null }
  | { kind: 'Return'; keyword: Token; value: Expr | null }
  | { kind: 'While'; condition: Expr; body: Stmt }
  | { kind: 'For'; initializer: Stmt | null; condition: Expr | null; increment: Expr | null; body: Stmt }
  | { kind: 'Break' }
  | { kind: 'Continue' }
  | { kind: 'Block'; statements: Stmt[] }
  | { kind: 'Print'; expressions: Expr[] }
  | { kind: 'Var'; name: Token; initializer: Expr | null }

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

// Maps each compound-assignment token to the plain binary operator it
// desugars into, e.g. `x += 1` becomes `x = x + 1`.
const COMPOUND_ASSIGN_OPS: Partial<Record<TokenType, [TokenType, string]>> = {
  PLUS_ASSIGN: ['PLUS', '+'],
  MINUS_ASSIGN: ['MINUS', '-'],
  MULTIPLY_ASSIGN: ['MULTIPLY', '*'],
  DIVIDE_ASSIGN: ['DIVIDE', '/'],
  MODULO_ASSIGN: ['MODULO', '%'],
}

class Parser {
  private current = 0
  errors: MRTError[] = []

  constructor(private tokens: Token[]) {}

  parse(): Stmt[] {
    const statements: Stmt[] = []
    while (!this.isAtEnd()) {
      const stmt = this.declaration()
      if (stmt) statements.push(stmt)
    }
    return statements
  }

  private declaration(): Stmt | null {
    try {
      if (this.match('FUNC')) return this.function_('function')
      if (this.match('VAR')) return this.varDeclaration()
      return this.statement()
    } catch (e) {
      if (e instanceof MRTError) {
        this.errors.push(e)
        this.synchronize()
        return null
      }
      throw e
    }
  }

  private function_(kind: string): Stmt {
    const name = this.consume('IDENTIFIER', `Expect ${kind} name.`)
    this.consume('LPAREN', `Expect '(' after ${kind} name.`)
    const params: Token[] = []
    if (!this.check('RPAREN')) {
      do {
        params.push(this.consume('IDENTIFIER', 'Expect parameter name.'))
      } while (this.match('COMMA'))
    }
    this.consume('RPAREN', "Expect ')' after parameters.")
    this.consume('LBRACE', `Expect '{' before ${kind} body.`)
    const body = this.block()
    return { kind: 'Function', name, params, body }
  }

  private statement(): Stmt {
    if (this.match('FOR')) return this.forStatement()
    if (this.match('IF')) return this.ifStatement()
    if (this.match('RETURN')) return this.returnStatement()
    if (this.match('WHILE')) return this.whileStatement()
    if (this.match('BREAK')) { this.consumeStatementEnd(); return { kind: 'Break' } }
    if (this.match('CONTINUE')) { this.consumeStatementEnd(); return { kind: 'Continue' } }
    if (this.match('LBRACE')) return { kind: 'Block', statements: this.block() }
    if (this.match('PRINT')) return this.printStatement()
    return this.expressionStatement()
  }

  private forStatement(): Stmt {
    this.consume('LPAREN', "Expect '(' after 'for'.")
    let initializer: Stmt | null
    if (this.match('SEMICOLON')) initializer = null
    else if (this.match('VAR')) initializer = this.varDeclaration()
    else initializer = this.expressionStatement()

    let condition: Expr | null = null
    if (!this.check('SEMICOLON')) condition = this.expression()
    this.consume('SEMICOLON', "Expect ';' after loop condition.")

    let increment: Expr | null = null
    if (!this.check('RPAREN')) increment = this.expression()
    this.consume('RPAREN', "Expect ')' after for clauses.")

    const body = this.statement()
    return { kind: 'For', initializer, condition, increment, body }
  }

  private ifStatement(): Stmt {
    this.consume('LPAREN', "Expect '(' after 'if'.")
    const condition = this.expression()
    this.consume('RPAREN', "Expect ')' after if condition.")
    const thenBranch = this.statement()
    let elseBranch: Stmt | null = null
    if (this.match('ELSE')) elseBranch = this.statement()
    return { kind: 'If', condition, thenBranch, elseBranch }
  }

  private returnStatement(): Stmt {
    const keyword = this.previous()
    let value: Expr | null = null
    if (!this.check('SEMICOLON') && !this.check('RBRACE') && !this.isAtEnd() && this.peek().line === keyword.line) {
      value = this.expression()
    }
    this.consumeStatementEnd()
    return { kind: 'Return', keyword, value }
  }

  private whileStatement(): Stmt {
    this.consume('LPAREN', "Expect '(' after 'while'.")
    const condition = this.expression()
    this.consume('RPAREN', "Expect ')' after condition.")
    const body = this.statement()
    return { kind: 'While', condition, body }
  }

  private block(): Stmt[] {
    const statements: Stmt[] = []
    while (!this.check('RBRACE') && !this.isAtEnd()) {
      const stmt = this.declaration()
      if (stmt) statements.push(stmt)
    }
    this.consume('RBRACE', "Expect '}' after block.")
    return statements
  }

  private expressionStatement(): Stmt {
    const expr = this.expression()
    this.consumeStatementEnd()
    return { kind: 'Expression', expression: expr }
  }

  private printStatement(): Stmt {
    this.consume('LPAREN', "Expect '(' after 'print'.")
    const values: Expr[] = []
    if (!this.check('RPAREN')) {
      values.push(this.expression())
      while (this.match('COMMA')) values.push(this.expression())
    }
    this.consume('RPAREN', "Expect ')' after print arguments.")
    this.consumeStatementEnd()
    return { kind: 'Print', expressions: values }
  }

  private expression(): Expr { return this.assignment() }

  private assignment(): Expr {
    const expr = this.orExpression()

    if (this.match('ASSIGN')) {
      const equals = this.previous()
      const value = this.assignment()
      return this.makeAssignTarget(expr, equals, value)
    }

    const compoundTypes = Object.keys(COMPOUND_ASSIGN_OPS) as TokenType[]
    if (this.match(...compoundTypes)) {
      const opToken = this.previous()
      const [baseType, baseLexeme] = COMPOUND_ASSIGN_OPS[opToken.type]!
      const value = this.assignment()
      // Desugar `target += value` into `target = target + value`. For an
      // ArrayAssign target this evaluates the array/index sub-expressions
      // twice (once to read, once to write) -- fine for the simple
      // variable/literal indices idiomatic MRT code uses.
      const syntheticOperator: Token = { type: baseType, lexeme: baseLexeme, literal: null, line: opToken.line }
      const combined: Expr = { kind: 'Binary', left: expr, operator: syntheticOperator, right: value }
      return this.makeAssignTarget(expr, opToken, combined)
    }

    return expr
  }

  private makeAssignTarget(target: Expr, errorToken: Token, value: Expr): Expr {
    if (target.kind === 'Variable') return { kind: 'Assign', name: target.name, value }
    if (target.kind === 'ArrayAccess') return { kind: 'ArrayAssign', array: target.array, index: target.index, value }
    throw this.error(errorToken, 'Invalid assignment target.')
  }

  private orExpression(): Expr {
    let expr = this.andExpression()
    while (this.match('OR')) {
      const operator = this.previous()
      expr = { kind: 'Logical', left: expr, operator, right: this.andExpression() }
    }
    return expr
  }

  private andExpression(): Expr {
    let expr = this.equality()
    while (this.match('AND')) {
      const operator = this.previous()
      expr = { kind: 'Logical', left: expr, operator, right: this.equality() }
    }
    return expr
  }

  private equality(): Expr {
    let expr = this.comparison()
    while (this.match('NOT_EQUALS', 'EQUALS')) {
      const operator = this.previous()
      expr = { kind: 'Binary', left: expr, operator, right: this.comparison() }
    }
    return expr
  }

  private comparison(): Expr {
    let expr = this.term()
    while (this.match('GREATER', 'GREATER_EQUAL', 'LESS', 'LESS_EQUAL')) {
      const operator = this.previous()
      expr = { kind: 'Binary', left: expr, operator, right: this.term() }
    }
    return expr
  }

  private term(): Expr {
    let expr = this.factor()
    while (this.match('PLUS', 'MINUS')) {
      const operator = this.previous()
      expr = { kind: 'Binary', left: expr, operator, right: this.factor() }
    }
    return expr
  }

  private factor(): Expr {
    let expr = this.unary()
    while (this.match('MULTIPLY', 'DIVIDE', 'MODULO')) {
      const operator = this.previous()
      expr = { kind: 'Binary', left: expr, operator, right: this.unary() }
    }
    return expr
  }

  private unary(): Expr {
    if (this.match('MINUS', 'NOT')) {
      const operator = this.previous()
      return { kind: 'Unary', operator, right: this.unary() }
    }
    return this.call()
  }

  private call(): Expr {
    let expr = this.primary()
    for (;;) {
      if (this.match('LPAREN')) expr = this.finishCall(expr)
      else if (this.match('LBRACKET')) expr = this.arrayAccess(expr)
      else if (this.match('DOT')) {
        const name = this.consume('IDENTIFIER', "Expect property name after '.'.")
        // `obj.name` is sugar for `obj["name"]`.
        expr = { kind: 'ArrayAccess', array: expr, index: { kind: 'Literal', value: name.lexeme } }
      } else break
    }
    return expr
  }

  private finishCall(callee: Expr): Expr {
    const args: Expr[] = []
    if (!this.check('RPAREN')) {
      do { args.push(this.expression()) } while (this.match('COMMA'))
    }
    const paren = this.consume('RPAREN', "Expect ')' after arguments.")
    return { kind: 'Call', callee, paren, arguments: args }
  }

  private arrayAccess(expr: Expr): Expr {
    const index = this.expression()
    this.consume('RBRACKET', "Expect ']' after array index.")
    return { kind: 'ArrayAccess', array: expr, index }
  }

  private primary(): Expr {
    if (this.match('TRUE')) return { kind: 'Literal', value: true }
    if (this.match('FALSE')) return { kind: 'Literal', value: false }
    if (this.match('NUMBER', 'STRING')) return { kind: 'Literal', value: this.previous().literal }
    if (this.match('IDENTIFIER')) return { kind: 'Variable', name: this.previous() }
    if (this.match('LPAREN')) {
      const expr = this.expression()
      this.consume('RPAREN', "Expect ')' after expression.")
      return { kind: 'Grouping', expression: expr }
    }
    if (this.match('LBRACKET')) {
      const elements: Expr[] = []
      if (!this.check('RBRACKET')) {
        do { elements.push(this.expression()) } while (this.match('COMMA'))
      }
      this.consume('RBRACKET', "Expect ']' after array elements.")
      return { kind: 'Array', elements }
    }
    if (this.match('LBRACE')) {
      const pairs: [Expr, Expr][] = []
      if (!this.check('RBRACE')) {
        do {
          const key = this.expression()
          this.consume('COLON', "Expect ':' after dictionary key.")
          const value = this.expression()
          pairs.push([key, value])
        } while (this.match('COMMA'))
      }
      this.consume('RBRACE', "Expect '}' after dictionary literal.")
      return { kind: 'DictLiteral', pairs }
    }
    throw this.error(this.peek(), 'Expect expression.')
  }

  private varDeclaration(): Stmt {
    const name = this.consume('IDENTIFIER', 'Expect variable name.')
    let initializer: Expr | null = null
    if (this.match('ASSIGN')) initializer = this.expression()
    this.consumeStatementEnd()
    return { kind: 'Var', name, initializer }
  }

  private match(...types: TokenType[]): boolean {
    for (const type of types) {
      if (this.check(type)) { this.advance(); return true }
    }
    return false
  }

  private check(type: TokenType): boolean { return !this.isAtEnd() && this.peek().type === type }
  private advance(): Token { if (!this.isAtEnd()) this.current++; return this.previous() }
  private isAtEnd(): boolean { return this.peek().type === 'EOF' }
  private peek(): Token { return this.tokens[this.current] }
  private previous(): Token { return this.tokens[this.current - 1] }

  private consume(type: TokenType, message: string): Token {
    if (this.check(type)) return this.advance()
    throw this.error(this.peek(), message)
  }

  /** Statement terminators are optional in MRT: consume a trailing ';' if
   * present, but never error when it's missing. */
  private consumeStatementEnd() { this.match('SEMICOLON') }

  private error(token: Token, message: string): MRTError {
    const where = token.type === 'EOF' ? 'end of file' : `'${token.lexeme}'`
    return new MRTError(`Error at ${where}: ${message}`, token.line)
  }

  private synchronize() {
    this.advance()
    while (!this.isAtEnd()) {
      if (this.previous().type === 'SEMICOLON') return
      switch (this.peek().type) {
        case 'FUNC': case 'IF': case 'RETURN': case 'WHILE': case 'VAR': case 'FOR':
          return
      }
      this.advance()
    }
  }
}

// ---------------------------------------------------------------------------
// Interpreter
// ---------------------------------------------------------------------------

export function stringify(value: unknown): string {
  if (value === null || value === undefined) return 'null'
  if (typeof value === 'boolean') return value ? 'true' : 'false'
  if (typeof value === 'number') return String(value)
  if (Array.isArray(value)) return '[' + value.map(stringify).join(', ') + ']'
  if (value instanceof Map) {
    const parts: string[] = []
    value.forEach((v, k) => parts.push(`${stringify(k)}: ${stringify(v)}`))
    return '{' + parts.join(', ') + '}'
  }
  return String(value)
}

/** Structural equality between two MRT values: arrays/objects compare
 * element-wise, recursively, rather than by reference (so `[1,2] == [1,2]`
 * is true even though they're two distinct array instances) -- matching
 * Python's native `==` on lists/dicts, which the reference interpreter
 * relies on. Shared by `Interpreter`'s `==`/`!=` and by the `has()`/
 * `indexOf()` built-ins, which need the same "does this array contain a
 * value equal to X" semantics. */
function valuesEqual(a: unknown, b: unknown): boolean {
  if (a === null && b === null) return true
  if (a === null || b === null) return false
  if (typeof a === 'boolean' !== (typeof b === 'boolean')) return false
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((v, i) => valuesEqual(v, b[i]))
  }
  if (a instanceof Map && b instanceof Map) {
    if (a.size !== b.size) return false
    for (const [k, v] of a) {
      if (!b.has(k) || !valuesEqual(v, b.get(k))) return false
    }
    return true
  }
  if (Array.isArray(a) !== Array.isArray(b)) return false
  if (a instanceof Map !== b instanceof Map) return false
  return a === b
}

class MRTFunction {
  constructor(public declaration: Extract<Stmt, { kind: 'Function' }>, public closure: Environment) {}

  call(interpreter: Interpreter, args: unknown[]): unknown {
    const environment = new Environment(this.closure)
    this.declaration.params.forEach((param, i) => environment.define(param.lexeme, args[i]))
    try {
      interpreter.executeBlock(this.declaration.body, environment)
      return null
    } catch (e) {
      if (e instanceof ReturnSignal) return e.value
      throw e
    }
  }

  toString() { return `<function ${this.declaration.name.lexeme}>` }
}

class ReturnSignal { constructor(public value: unknown) {} }
class BreakSignal {}
class ContinueSignal {}

class Environment {
  private values = new Map<string, unknown>()
  constructor(public enclosing: Environment | null = null) {}

  define(name: string, value: unknown) { this.values.set(name, value) }

  get(name: Token): unknown {
    if (this.values.has(name.lexeme)) return this.values.get(name.lexeme)
    if (this.enclosing) return this.enclosing.get(name)
    throw new MRTError(`Undefined variable '${name.lexeme}'.`, name.line)
  }

  assign(name: Token, value: unknown) {
    if (this.values.has(name.lexeme)) { this.values.set(name.lexeme, value); return }
    if (this.enclosing) { this.enclosing.assign(name, value); return }
    throw new MRTError(`Undefined variable '${name.lexeme}'.`, name.line)
  }
}

function isNumber(v: unknown): v is number { return typeof v === 'number' }

const BUILTINS: Record<string, (...args: unknown[]) => unknown> = {
  len: (...a) => {
    if (a.length !== 1) throw new MRTError('len() takes exactly one argument.')
    if (typeof a[0] === 'string' || Array.isArray(a[0])) return (a[0] as string | unknown[]).length
    if (a[0] instanceof Map) return a[0].size
    throw new MRTError('len() argument must be an array, object, or string.')
  },
  push: (...a) => {
    if (a.length !== 2 || !Array.isArray(a[0])) throw new MRTError('push() takes an array and a value.')
    ;(a[0] as unknown[]).push(a[1])
    return a[1]
  },
  pop: (...a) => {
    if (a.length !== 1 || !Array.isArray(a[0])) throw new MRTError('pop() takes exactly one array argument.')
    const arr = a[0] as unknown[]
    if (arr.length === 0) throw new MRTError('Cannot pop from empty array.')
    return arr.pop()
  },
  slice: (...a) => {
    if (!Array.isArray(a[0])) throw new MRTError('First argument to slice() must be an array.')
    const arr = a[0] as unknown[]
    let start = isNumber(a[1]) ? Math.trunc(a[1]) : 0
    let end = a.length > 2 && isNumber(a[2]) ? Math.trunc(a[2] as number) : arr.length
    if (start < 0) start = arr.length + start
    if (end < 0) end = arr.length + end
    return arr.slice(start, end)
  },
  join: (...a) => {
    if (!Array.isArray(a[0])) throw new MRTError('First argument to join() must be an array.')
    const sep = a.length > 1 ? String(a[1]) : ''
    return (a[0] as unknown[]).map(stringify).join(sep)
  },
  indexOf: (...a) => {
    if (!Array.isArray(a[0])) throw new MRTError('First argument to indexOf() must be an array.')
    const arr = a[0] as unknown[]
    return arr.findIndex((v) => valuesEqual(v, a[1]))
  },
  split: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('First argument to split() must be a string.')
    const sep = a.length > 1 ? String(a[1]) : ' '
    return a[0].split(sep)
  },
  substring: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('First argument to substring() must be a string.')
    const text = a[0]
    let start = isNumber(a[1]) ? Math.trunc(a[1]) : 0
    let end = a.length > 2 && isNumber(a[2]) ? Math.trunc(a[2] as number) : text.length
    if (start < 0) start = text.length + start
    if (end < 0) end = text.length + end
    return text.slice(start, end)
  },
  toUpper: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('toUpper() argument must be a string.')
    return a[0].toUpperCase()
  },
  toLower: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('toLower() argument must be a string.')
    return a[0].toLowerCase()
  },
  trim: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('trim() argument must be a string.')
    return a[0].trim()
  },
  replace: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('First argument to replace() must be a string.')
    return a[0].split(String(a[1])).join(String(a[2]))
  },
  startsWith: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('First argument to startsWith() must be a string.')
    return a[0].startsWith(String(a[1]))
  },
  endsWith: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('First argument to endsWith() must be a string.')
    return a[0].endsWith(String(a[1]))
  },
  contains: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('First argument to contains() must be a string.')
    return a[0].includes(String(a[1]))
  },

  // -- Type / conversion --
  type: (...a) => {
    if (a.length !== 1) throw new MRTError('type() takes exactly one argument.')
    const v = a[0]
    if (v === null || v === undefined) return 'null'
    if (typeof v === 'boolean') return 'boolean'
    if (typeof v === 'number') return 'number'
    if (typeof v === 'string') return 'string'
    if (Array.isArray(v)) return 'array'
    if (v instanceof Map) return 'object'
    if (v instanceof MRTFunction || typeof v === 'function') return 'function'
    return 'unknown'
  },
  toNumber: (...a) => {
    if (a.length !== 1) throw new MRTError('toNumber() takes exactly one argument.')
    const v = a[0]
    if (typeof v === 'boolean') return v ? 1 : 0
    if (typeof v === 'number') return v
    if (typeof v === 'string') {
      const n = Number(v.trim())
      if (Number.isNaN(n) || v.trim() === '') throw new MRTError(`Cannot convert '${v}' to a number.`)
      return n
    }
    throw new MRTError('toNumber() argument must be a string, number, or boolean.')
  },
  // -- Math --
  abs: (...a) => {
    if (a.length !== 1) throw new MRTError('abs() takes exactly one argument.')
    return Math.abs(numArg(a[0], 'abs'))
  },
  min: (...a) => {
    const values = a.length === 1 && Array.isArray(a[0]) ? (a[0] as unknown[]) : a
    if (values.length === 0) throw new MRTError('min() requires at least one argument.')
    return Math.min(...values.map((v) => numArg(v, 'min')))
  },
  max: (...a) => {
    const values = a.length === 1 && Array.isArray(a[0]) ? (a[0] as unknown[]) : a
    if (values.length === 0) throw new MRTError('max() requires at least one argument.')
    return Math.max(...values.map((v) => numArg(v, 'max')))
  },
  round: (...a) => {
    if (a.length < 1 || a.length > 2) throw new MRTError('round() takes 1 or 2 arguments.')
    const value = numArg(a[0], 'round')
    const digits = a.length === 2 ? Math.trunc(numArg(a[1], 'round')) : 0
    const factor = 10 ** digits
    return Math.round(value * factor) / factor
  },
  floor: (...a) => {
    if (a.length !== 1) throw new MRTError('floor() takes exactly one argument.')
    return Math.floor(numArg(a[0], 'floor'))
  },
  ceil: (...a) => {
    if (a.length !== 1) throw new MRTError('ceil() takes exactly one argument.')
    return Math.ceil(numArg(a[0], 'ceil'))
  },
  sqrt: (...a) => {
    if (a.length !== 1) throw new MRTError('sqrt() takes exactly one argument.')
    const value = numArg(a[0], 'sqrt')
    if (value < 0) throw new MRTError('sqrt() argument must not be negative.')
    return Math.sqrt(value)
  },
  pow: (...a) => {
    if (a.length !== 2) throw new MRTError('pow() takes exactly 2 arguments.')
    return numArg(a[0], 'pow') ** numArg(a[1], 'pow')
  },

  // -- Objects (dicts, represented as Map so keys can be numbers/booleans too) --
  keys: (...a) => {
    if (a.length !== 1 || !(a[0] instanceof Map)) throw new MRTError('keys() takes exactly one object argument.')
    return Array.from(a[0].keys())
  },
  values: (...a) => {
    if (a.length !== 1 || !(a[0] instanceof Map)) throw new MRTError('values() takes exactly one object argument.')
    return Array.from(a[0].values())
  },
  has: (...a) => {
    if (a.length !== 2) throw new MRTError('has() takes exactly 2 arguments.')
    const [container, key] = a
    if (container instanceof Map) return container.has(key)
    if (Array.isArray(container)) return container.some((v) => valuesEqual(v, key))
    throw new MRTError('First argument to has() must be an array or object.')
  },
  get: (...a) => {
    if (a.length < 2 || a.length > 3) throw new MRTError('get() takes 2 or 3 arguments.')
    const [container, key] = a
    const fallback = a.length === 3 ? a[2] : null
    if (container instanceof Map) return container.has(key) ? container.get(key) : fallback
    if (Array.isArray(container)) {
      if (typeof key === 'number') {
        const i = Math.trunc(key)
        if (i >= 0 && i < container.length) return container[i]
      }
      return fallback
    }
    throw new MRTError('First argument to get() must be an array or object.')
  },
}

function numArg(value: unknown, who: string): number {
  if (typeof value !== 'number') throw new MRTError(`${who}() argument must be a number.`)
  return value
}

// Assigned outside the BUILTINS object literal, with an explicit type
// annotation, rather than as a `toString:` property inside it. TypeScript
// contextually types anything assigned to a property named `toString`
// against the inherited `Object.prototype.toString(): string` -- a
// zero-argument signature -- which made the rest parameter below get
// inferred as an empty tuple (even via `BUILTINS.toString = (...) => ...`
// after the literal). Giving the function its own explicit type up front
// sidesteps contextual inference entirely.
const toStringBuiltin: (...args: unknown[]) => unknown = (...a) => {
  if (a.length !== 1) throw new MRTError('toString() takes exactly one argument.')
  return stringify(a[0])
}
// A literal `.toString` (or even `['toString']`) assignment is still typed
// against the inherited `Object.prototype.toString(): string` member, not
// the Record index signature -- so the key has to be a non-literal `string`
// to force TypeScript through the index signature instead.
const toStringKey: string = 'toString'
BUILTINS[toStringKey] = toStringBuiltin

class Interpreter {
  globals = new Environment()
  environment = this.globals
  output: string[] = []

  constructor() {
    for (const [name, fn] of Object.entries(BUILTINS)) this.globals.define(name, fn)
  }

  private printValues(values: unknown[]) {
    this.output.push(values.map(stringify).join(' '))
  }

  interpret(statements: Stmt[]) {
    try {
      for (const s of statements) if (s.kind === 'Function') this.execute(s)

      let mainFn: unknown = null
      try {
        mainFn = this.environment.get({ type: 'IDENTIFIER', lexeme: 'main', literal: null, line: 1 })
      } catch {
        // no main() defined -- fall through to running top-level statements
      }

      if (mainFn instanceof MRTFunction) {
        mainFn.call(this, [])
      } else {
        for (const s of statements) if (s.kind !== 'Function') this.execute(s)
      }
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      this.output.push(`Runtime Error: ${message}`)
    }
  }

  execute(stmt: Stmt) {
    switch (stmt.kind) {
      case 'Block':
        this.executeBlock(stmt.statements, new Environment(this.environment))
        return
      case 'Break':
        throw new BreakSignal()
      case 'Continue':
        throw new ContinueSignal()
      case 'Expression':
        this.evaluate(stmt.expression)
        return
      case 'For':
        this.executeFor(stmt)
        return
      case 'Function':
        this.environment.define(stmt.name.lexeme, new MRTFunction(stmt, this.environment))
        return
      case 'If':
        if (this.isTruthy(this.evaluate(stmt.condition))) this.execute(stmt.thenBranch)
        else if (stmt.elseBranch) this.execute(stmt.elseBranch)
        return
      case 'Print':
        this.printValues(stmt.expressions.map((e) => this.evaluate(e)))
        return
      case 'Return': {
        const value = stmt.value ? this.evaluate(stmt.value) : null
        throw new ReturnSignal(value)
      }
      case 'Var':
        this.environment.define(stmt.name.lexeme, stmt.initializer ? this.evaluate(stmt.initializer) : null)
        return
      case 'While':
        while (this.isTruthy(this.evaluate(stmt.condition))) {
          try {
            this.execute(stmt.body)
          } catch (e) {
            if (e instanceof BreakSignal) break
            if (e instanceof ContinueSignal) continue
            throw e
          }
        }
        return
    }
  }

  private executeFor(stmt: Extract<Stmt, { kind: 'For' }>) {
    const previous = this.environment
    this.environment = new Environment(previous)
    try {
      if (stmt.initializer) this.execute(stmt.initializer)
      while (stmt.condition === null || this.isTruthy(this.evaluate(stmt.condition))) {
        try {
          this.execute(stmt.body)
        } catch (e) {
          if (e instanceof BreakSignal) break
          if (e instanceof ContinueSignal) { /* fall through to increment */ }
          else throw e
        }
        if (stmt.increment !== null) this.evaluate(stmt.increment)
      }
    } finally {
      this.environment = previous
    }
  }

  executeBlock(statements: Stmt[], environment: Environment) {
    const previous = this.environment
    try {
      this.environment = environment
      for (const s of statements) this.execute(s)
    } finally {
      this.environment = previous
    }
  }

  evaluate(expr: Expr): unknown {
    switch (expr.kind) {
      case 'Array':
        return expr.elements.map((e) => this.evaluate(e))
      case 'ArrayAccess':
        return this.evaluateIndexGet(expr)
      case 'ArrayAssign':
        return this.evaluateIndexSet(expr)
      case 'Assign': {
        const value = this.evaluate(expr.value)
        this.environment.assign(expr.name, value)
        return value
      }
      case 'Binary':
        return this.evaluateBinary(expr)
      case 'DictLiteral': {
        const result = new Map<unknown, unknown>()
        for (const [keyExpr, valueExpr] of expr.pairs) {
          const key = this.evaluate(keyExpr)
          this.checkHashableKey(key)
          result.set(key, this.evaluate(valueExpr))
        }
        return result
      }
      case 'Call': {
        const callee = this.evaluate(expr.callee)
        const args = expr.arguments.map((a) => this.evaluate(a))
        if (callee instanceof MRTFunction) {
          if (args.length !== callee.declaration.params.length) {
            throw new MRTError(
              `Expected ${callee.declaration.params.length} arguments but got ${args.length}.`,
              expr.paren.line,
            )
          }
          return callee.call(this, args)
        }
        if (typeof callee === 'function') {
          try {
            return callee(...args)
          } catch (e) {
            if (e instanceof MRTError) throw new MRTError(e.message, expr.paren.line)
            throw e
          }
        }
        throw new MRTError('Can only call functions.', expr.paren.line)
      }
      case 'Grouping':
        return this.evaluate(expr.expression)
      case 'Literal':
        return expr.value
      case 'Logical': {
        const left = this.evaluate(expr.left)
        if (expr.operator.type === 'OR') { if (this.isTruthy(left)) return left }
        else { if (!this.isTruthy(left)) return left }
        return this.evaluate(expr.right)
      }
      case 'Unary': {
        const right = this.evaluate(expr.right)
        if (expr.operator.type === 'MINUS') {
          if (typeof right !== 'number') throw new MRTError("Operand of '-' must be a number.", expr.operator.line)
          return -right
        }
        if (expr.operator.type === 'NOT') return !this.isTruthy(right)
        return null
      }
      case 'Variable':
        return this.environment.get(expr.name)
    }
  }

  private evaluateIndexGet(expr: Extract<Expr, { kind: 'ArrayAccess' }>): unknown {
    const target = this.evaluate(expr.array)
    const index = this.evaluate(expr.index)

    if (target instanceof Map) {
      this.checkHashableKey(index)
      if (!target.has(index)) throw new MRTError(`Key ${JSON.stringify(stringify(index))} not found in object.`)
      return target.get(index)
    }
    if (Array.isArray(target)) {
      const i = this.requireArrayIndex(index, target.length)
      return target[i]
    }
    if (typeof target === 'string') {
      const i = this.requireArrayIndex(index, target.length)
      return target[i]
    }
    throw new MRTError('Can only index into arrays, objects, or strings.')
  }

  private evaluateIndexSet(expr: Extract<Expr, { kind: 'ArrayAssign' }>): unknown {
    const target = this.evaluate(expr.array)
    const index = this.evaluate(expr.index)
    const value = this.evaluate(expr.value)

    if (target instanceof Map) {
      this.checkHashableKey(index)
      target.set(index, value)
      return value
    }
    if (Array.isArray(target)) {
      const i = this.requireArrayIndex(index, target.length)
      target[i] = value
      return value
    }
    if (typeof target === 'string') {
      throw new MRTError('Strings are immutable; cannot assign to a character index.')
    }
    throw new MRTError('Can only assign into arrays or objects.')
  }

  private requireArrayIndex(index: unknown, length: number): number {
    if (typeof index !== 'number') throw new MRTError('Array index must be a number.')
    const i = Math.trunc(index)
    if (i < 0 || i >= length) throw new MRTError(`Array index ${i} out of bounds for array of length ${length}.`)
    return i
  }

  private checkHashableKey(key: unknown) {
    if (Array.isArray(key) || key instanceof Map) {
      throw new MRTError('Object keys must be numbers, strings, or booleans (not arrays or objects).')
    }
  }

  private evaluateBinary(expr: Extract<Expr, { kind: 'Binary' }>): unknown {
    const left = this.evaluate(expr.left)
    const right = this.evaluate(expr.right)
    const op = expr.operator.type
    const line = expr.operator.line

    const checkNumbers = (opSym: string) => {
      if (typeof left !== 'number' || typeof right !== 'number') {
        throw new MRTError(`Operands of '${opSym}' must be numbers.`, line)
      }
    }

    switch (op) {
      case 'PLUS':
        if (typeof left === 'string' || typeof right === 'string') return stringify(left) + stringify(right)
        checkNumbers('+')
        return (left as number) + (right as number)
      case 'MINUS':
        checkNumbers('-')
        return (left as number) - (right as number)
      case 'MULTIPLY':
        checkNumbers('*')
        return (left as number) * (right as number)
      case 'DIVIDE':
        checkNumbers('/')
        if ((right as number) === 0) throw new MRTError('Division by zero.', line)
        return (left as number) / (right as number)
      case 'MODULO':
        checkNumbers('%')
        if ((right as number) === 0) throw new MRTError('Modulo by zero.', line)
        return (left as number) % (right as number)
      case 'EQUALS':
        return this.isEqual(left, right)
      case 'NOT_EQUALS':
        return !this.isEqual(left, right)
      case 'GREATER':
        return this.compare(left, right, line) > 0
      case 'GREATER_EQUAL':
        return this.compare(left, right, line) >= 0
      case 'LESS':
        return this.compare(left, right, line) < 0
      case 'LESS_EQUAL':
        return this.compare(left, right, line) <= 0
      default:
        throw new MRTError(`Unknown binary operator '${expr.operator.lexeme}'.`, line)
    }
  }

  private compare(left: unknown, right: unknown, line: number): number {
    if (typeof left === 'string' && typeof right === 'string') return left < right ? -1 : left > right ? 1 : 0
    if (typeof left === 'number' && typeof right === 'number') return left < right ? -1 : left > right ? 1 : 0
    throw new MRTError('Comparison operators require two numbers or two strings.', line)
  }

  private isEqual(a: unknown, b: unknown): boolean {
    return valuesEqual(a, b)
  }

  private isTruthy(value: unknown): boolean {
    if (value === null || value === undefined) return false
    if (typeof value === 'boolean') return value
    return true
  }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export interface RunResult {
  output: string[]
  errors: string[]
}

export function runMRT(source: string): RunResult {
  try {
    const tokens = new Lexer(source).scanTokens()
    const parser = new Parser(tokens)
    const statements = parser.parse()

    if (parser.errors.length > 0) {
      return { output: [], errors: parser.errors.map((e) => `Syntax Error: ${e.message}`) }
    }

    const interpreter = new Interpreter()
    interpreter.interpret(statements)
    return { output: interpreter.output, errors: [] }
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    return { output: [], errors: [`Syntax Error: ${message}`] }
  }
}
