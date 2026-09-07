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
  | 'NULL' | 'TRY' | 'CATCH' | 'FINALLY' | 'THROW' | 'IN'
  | 'IMPORT' | 'EXPORT' | 'FROM' | 'AS'
  | 'IDENTIFIER' | 'NUMBER' | 'STRING' | 'TEMPLATE'
  | 'PLUS' | 'MINUS' | 'MULTIPLY' | 'DIVIDE' | 'MODULO'
  | 'PLUS_ASSIGN' | 'MINUS_ASSIGN' | 'MULTIPLY_ASSIGN' | 'DIVIDE_ASSIGN' | 'MODULO_ASSIGN'
  | 'ASSIGN' | 'EQUALS' | 'NOT_EQUALS'
  | 'GREATER' | 'GREATER_EQUAL' | 'LESS' | 'LESS_EQUAL'
  | 'AND' | 'OR' | 'NOT'
  | 'LPAREN' | 'RPAREN' | 'LBRACE' | 'RBRACE' | 'LBRACKET' | 'RBRACKET'
  | 'COMMA' | 'SEMICOLON' | 'DOT' | 'ELLIPSIS' | 'COLON' | 'EOF'

export interface Token {
  type: TokenType
  lexeme: string
  literal: unknown
  line: number
}

/** The closed set of error kinds a `catch` guard can branch on. Mirrors
 * ERROR_KINDS in src/errors.py; the two must stay in step. */
export type ErrorKind =
  | 'TypeError'        // a value of the wrong type
  | 'ArityError'       // wrong number of arguments
  | 'IndexError'       // index outside an array/string, or a non-numeric index
  | 'KeyError'         // object key that isn't present
  | 'NameError'        // undefined variable
  | 'ValueError'       // right type, unusable value (sqrt(-1), a zero step)
  | 'ArithmeticError'  // division or modulo by zero
  | 'RuntimeError'     // anything not covered above

export class MRTError extends Error {
  line?: number
  /** The message without the " [line N]" suffix `message` carries. `catch`
   * hands this to the program, so that a caught error's `.message` reads the
   * same here as in the Python interpreter (which stores the two apart). */
  rawMessage: string
  kind: ErrorKind
  /** The MRT-level call stack, innermost first. Filled in as the error
   * propagates outward through MRTFunction.call rather than captured at the
   * raise site, because only the callers know their own names. */
  mrtStack: string[] = []
  constructor(message: string, line?: number, kind: ErrorKind = 'RuntimeError') {
    super(line !== undefined ? `${message} [line ${line}]` : message)
    this.line = line
    this.rawMessage = message
    this.kind = kind
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
  ['null', 'NULL'], ['try', 'TRY'], ['catch', 'CATCH'], ['finally', 'FINALLY'],
  ['throw', 'THROW'], ['in', 'IN'],
  ['import', 'IMPORT'], ['export', 'EXPORT'], ['from', 'FROM'], ['as', 'AS'],
])

// `\$` escapes an interpolation, so "\${x}" is the literal text "${x}".
const ESCAPES: Record<string, string> = { n: '\n', t: '\t', r: '\r', '"': '"', '\\': '\\', '0': '\0', $: '$' }

/** One piece of a `"...${...}..."` template, as produced by the lexer. */
type TemplatePart = { kind: 'str'; value: string } | { kind: 'expr'; source: string; line: number }

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
      case '.':
        // `...` is the rest/spread marker; a single '.' is property access.
        // Two dots is not a token, so `a..b` stays an error.
        if (this.peek() === '.' && this.peekNext() === '.') {
          this.advance(); this.advance()
          this.addToken('ELLIPSIS')
        } else {
          this.addToken('DOT')
        }
        break
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
        else throw new MRTError("Unexpected character '&' (did you mean '&&'?)", this.line, 'RuntimeError')
        break
      case '|':
        if (this.match('|')) this.addToken('OR')
        else throw new MRTError("Unexpected character '|' (did you mean '||'?)", this.line, 'RuntimeError')
        break
      default:
        if (this.isDigit(c)) this.number()
        else if (this.isAlpha(c)) this.identifier()
        else throw new MRTError(`Unexpected character '${c}'`, this.line, 'RuntimeError')
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
    throw new MRTError('Unterminated comment', this.line, 'RuntimeError')
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

  // A `${ ... }` run makes this a *template*: the literal text and the
  // embedded expression sources are collected as alternating parts, and the
  // parser re-lexes each expression source into a real AST (see
  // Parser.interpolation). A string with no `${` is emitted as a plain
  // STRING exactly as before, so nothing about existing programs changes.
  private string() {
    const startLine = this.line
    let chars: string[] = []
    const parts: TemplatePart[] = []

    const flushText = () => {
      if (chars.length) { parts.push({ kind: 'str', value: chars.join('') }); chars = [] }
    }

    while (this.peek() !== '"' && !this.isAtEnd()) {
      const c = this.peek()
      if (c === '\n') this.line++
      if (c === '\\' && ESCAPES[this.peekNext()] !== undefined) {
        this.advance()
        const escape = this.advance()
        chars.push(ESCAPES[escape])
      } else if (c === '$' && this.peekNext() === '{') {
        this.advance()  // '$'
        this.advance()  // '{'
        flushText()
        parts.push(this.interpolatedExpression(startLine))
      } else {
        chars.push(this.advance())
      }
    }
    if (this.isAtEnd()) throw new MRTError('Unterminated string', startLine, 'RuntimeError')
    this.advance()

    if (parts.length === 0) { this.addToken('STRING', chars.join('')); return }
    flushText()
    this.addToken('TEMPLATE', parts)
  }

  /** Consume the source text of a `${ ... }` interpolation, starting just
   * after the `{`. Brace depth is tracked so an object literal nested inside
   * the expression doesn't end it early, and string literals are skipped
   * wholesale so a `}` or quote inside them is treated as text. */
  private interpolatedExpression(stringStartLine: number): TemplatePart {
    const exprLine = this.line
    const start = this.current
    let depth = 1

    while (!this.isAtEnd()) {
      const c = this.peek()
      if (c === '"') {
        this.advance()
        while (this.peek() !== '"' && !this.isAtEnd()) {
          if (this.peek() === '\n') this.line++
          if (this.peek() === '\\') this.advance()
          this.advance()
        }
        if (this.isAtEnd()) throw new MRTError('Unterminated string', stringStartLine, 'RuntimeError')
        this.advance()
        continue
      }
      if (c === '{') depth++
      else if (c === '}') {
        depth--
        if (depth === 0) {
          const source = this.source.slice(start, this.current)
          this.advance()
          return { kind: 'expr', source, line: exprLine }
        }
      } else if (c === '\n') this.line++
      this.advance()
    }

    throw new MRTError("Unterminated interpolation: expected '}'", exprLine, 'RuntimeError')
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

/** One declared parameter. `default` is evaluated at call time in the
 * callee's own scope, so a later default may refer to an earlier parameter
 * (`func f(a, b = a * 2)`). `rest` marks a `...name` parameter, which
 * collects any remaining arguments into an array and must come last. */
interface Param { name: Token; default: Expr | null; rest: boolean }

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
  | { kind: 'FunctionExpr'; params: Param[]; body: Stmt[]; name: Token | null }
  | { kind: 'Spread'; value: Expr; token: Token }
  | { kind: 'Interpolation'; parts: (string | Expr)[] }

/** One `catch (e) { }` clause, optionally guarded by `if (cond)`. The guard
 * is evaluated with `name` already bound to the error, so it can inspect
 * it: `catch (e) if (e.kind == "IndexError") { ... }`. */
interface CatchClause { name: Token; guard: Expr | null; block: Stmt }

type Stmt =
  | { kind: 'Expression'; expression: Expr }
  | { kind: 'Function'; name: Token; params: Param[]; body: Stmt[] }
  | { kind: 'If'; condition: Expr; thenBranch: Stmt; elseBranch: Stmt | null }
  | { kind: 'Return'; keyword: Token; value: Expr | null }
  | { kind: 'While'; condition: Expr; body: Stmt }
  | { kind: 'For'; initializer: Stmt | null; condition: Expr | null; increment: Expr | null; body: Stmt }
  | { kind: 'Break' }
  | { kind: 'Continue' }
  | { kind: 'Block'; statements: Stmt[] }
  | { kind: 'Print'; expressions: Expr[] }
  | { kind: 'Var'; name: Token; initializer: Expr | null }
  | { kind: 'ForIn'; name: Token; iterable: Expr; body: Stmt }
  | { kind: 'Throw'; keyword: Token; value: Expr }
  | { kind: 'Try'; tryBlock: Stmt; catches: CatchClause[]; finallyBlock: Stmt | null }
  | { kind: 'Import'; names: [Token, Token][]; specifier: Token; keyword: Token }
  | { kind: 'Export'; declaration: Stmt; name: Token }

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
  /** `import`/`export` are only meaningful at the top level of a file, so
   * the parser tracks how deep into blocks it currently is. */
  private blockDepth = 0

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
      if (this.match('IMPORT')) return this.importStatement()
      if (this.match('EXPORT')) return this.exportDeclaration()
      // `func name(...)` is a declaration; a bare `func(...)` in statement
      // position is an anonymous function *expression* and falls through to
      // expressionStatement below.
      if (this.check('FUNC') && this.checkNext('IDENTIFIER')) { this.advance(); return this.function_('function') }
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

  private importStatement(): Stmt {
    const keyword = this.previous()
    if (this.blockDepth > 0) {
      throw this.error(keyword, "'import' is only allowed at the top level of a file.")
    }

    this.consume('LBRACE', "Expect '{' after 'import'.")
    const names: [Token, Token][] = []
    if (!this.check('RBRACE')) {
      do {
        const exported = this.consume('IDENTIFIER', 'Expect an imported name.')
        let local = exported
        if (this.match('AS')) local = this.consume('IDENTIFIER', "Expect a local name after 'as'.")
        names.push([exported, local])
      } while (this.match('COMMA'))
    }
    this.consume('RBRACE', "Expect '}' after imported names.")

    this.consume('FROM', "Expect 'from' after imported names.")
    const specifier = this.consume('STRING', "Expect a module path string after 'from'.")
    this.consumeStatementEnd()
    return { kind: 'Import', names, specifier, keyword }
  }

  private exportDeclaration(): Stmt {
    const keyword = this.previous()
    if (this.blockDepth > 0) {
      throw this.error(keyword, "'export' is only allowed at the top level of a file.")
    }

    if (this.check('FUNC') && this.checkNext('IDENTIFIER')) {
      this.advance()
      const declaration = this.function_('function') as Extract<Stmt, { kind: 'Function' }>
      return { kind: 'Export', declaration, name: declaration.name }
    }
    if (this.match('VAR')) {
      const declaration = this.varDeclaration() as Extract<Stmt, { kind: 'Var' }>
      return { kind: 'Export', declaration, name: declaration.name }
    }
    throw this.error(this.peek(), "Expect a 'func' or 'var' declaration after 'export'.")
  }

  private function_(kind: string): Stmt {
    const name = this.consume('IDENTIFIER', `Expect ${kind} name.`)
    const params = this.parameterList(`Expect '(' after ${kind} name.`)
    this.consume('LBRACE', `Expect '{' before ${kind} body.`)
    const body = this.block()
    return { kind: 'Function', name, params, body }
  }

  /** Parse `(a, b = expr, ...rest)`. Two shape rules are enforced here
   * rather than at run time, because they are always mistakes: a rest
   * parameter must be last, and a required parameter may not follow a
   * defaulted one (which would make it unreachable by position). */
  private parameterList(lparenMessage: string): Param[] {
    this.consume('LPAREN', lparenMessage)
    const params: Param[] = []
    let seenDefault = false
    let seenRest = false

    if (!this.check('RPAREN')) {
      do {
        if (seenRest) throw this.error(this.peek(), 'A rest parameter must be the last parameter.')

        const isRest = this.match('ELLIPSIS')
        const name = this.consume('IDENTIFIER', 'Expect parameter name.')

        let def: Expr | null = null
        if (isRest) {
          seenRest = true
          if (this.check('ASSIGN')) {
            throw this.error(this.peek(), "A rest parameter can't have a default value.")
          }
        } else if (this.match('ASSIGN')) {
          def = this.expression()
          seenDefault = true
        } else if (seenDefault) {
          throw this.error(name, "A required parameter can't follow one with a default value.")
        }

        params.push({ name, default: def, rest: isRest })
      } while (this.match('COMMA'))
    }

    this.consume('RPAREN', "Expect ')' after parameters.")
    return params
  }

  /** An argument, array element or print value, which may be `...expr`. */
  private spreadOrExpression(): Expr {
    if (this.match('ELLIPSIS')) {
      const token = this.previous()
      return { kind: 'Spread', value: this.expression(), token }
    }
    return this.expression()
  }

  /** An anonymous `func(a, b) { ... }` in expression position. An optional
   * name is accepted purely so the function has something to print as. */
  private functionExpression(): Expr {
    let name: Token | null = null
    if (this.check('IDENTIFIER')) name = this.advance()
    const params = this.parameterList("Expect '(' after 'func'.")
    this.consume('LBRACE', "Expect '{' before function body.")
    const body = this.block()
    return { kind: 'FunctionExpr', params, body, name }
  }

  private statement(): Stmt {
    if (this.match('FOR')) return this.forStatement()
    if (this.match('IF')) return this.ifStatement()
    if (this.match('RETURN')) return this.returnStatement()
    if (this.match('WHILE')) return this.whileStatement()
    if (this.match('BREAK')) { this.consumeStatementEnd(); return { kind: 'Break' } }
    if (this.match('CONTINUE')) { this.consumeStatementEnd(); return { kind: 'Continue' } }
    if (this.match('TRY')) return this.tryStatement()
    if (this.match('THROW')) return this.throwStatement()
    if (this.match('LBRACE')) return { kind: 'Block', statements: this.block() }
    if (this.match('PRINT')) return this.printStatement()
    return this.expressionStatement()
  }

  private forStatement(): Stmt {
    this.consume('LPAREN', "Expect '(' after 'for'.")

    // `for (x in xs)` / `for (var x in xs)` -- decided by lookahead so the
    // C-style three-clause form below is untouched.
    if ((this.check('IDENTIFIER') && this.checkNext('IN')) ||
        (this.check('VAR') && this.checkNext('IDENTIFIER'))) {
      const saved = this.current
      this.match('VAR')
      if (this.check('IDENTIFIER') && this.checkNext('IN')) {
        const name = this.advance()
        this.advance()  // 'in'
        const iterable = this.expression()
        this.consume('RPAREN', "Expect ')' after for-in iterable.")
        const body = this.statement()
        return { kind: 'ForIn', name, iterable, body }
      }
      this.current = saved
    }

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

  private tryStatement(): Stmt {
    const keyword = this.previous()
    this.consume('LBRACE', "Expect '{' after 'try'.")
    const tryBlock: Stmt = { kind: 'Block', statements: this.block() }

    const catches: CatchClause[] = []
    while (this.match('CATCH')) {
      this.consume('LPAREN', "Expect '(' after 'catch'.")
      const name = this.consume('IDENTIFIER', 'Expect variable name in catch.')
      this.consume('RPAREN', "Expect ')' after catch variable.")

      // An optional guard: `catch (e) if (cond) { ... }`.
      let guard: Expr | null = null
      if (this.match('IF')) {
        this.consume('LPAREN', "Expect '(' after 'if' in catch guard.")
        guard = this.expression()
        this.consume('RPAREN', "Expect ')' after catch guard.")
      }

      this.consume('LBRACE', "Expect '{' after catch.")
      catches.push({ name, guard, block: { kind: 'Block', statements: this.block() } })
    }

    let finallyBlock: Stmt | null = null
    if (this.match('FINALLY')) {
      this.consume('LBRACE', "Expect '{' after 'finally'.")
      finallyBlock = { kind: 'Block', statements: this.block() }
    }

    if (catches.length === 0 && finallyBlock === null) {
      throw this.error(keyword, "Expect 'catch' or 'finally' after 'try' block.")
    }

    return { kind: 'Try', tryBlock, catches, finallyBlock }
  }

  private throwStatement(): Stmt {
    const keyword = this.previous()
    const value = this.expression()
    this.consumeStatementEnd()
    return { kind: 'Throw', keyword, value }
  }

  private block(): Stmt[] {
    const statements: Stmt[] = []
    this.blockDepth++
    try {
      while (!this.check('RBRACE') && !this.isAtEnd()) {
        const stmt = this.declaration()
        if (stmt) statements.push(stmt)
      }
    } finally {
      this.blockDepth--
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
      values.push(this.spreadOrExpression())
      while (this.match('COMMA')) values.push(this.spreadOrExpression())
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
      do { args.push(this.spreadOrExpression()) } while (this.match('COMMA'))
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
    if (this.match('NULL')) return { kind: 'Literal', value: null }
    if (this.match('NUMBER', 'STRING')) return { kind: 'Literal', value: this.previous().literal }
    if (this.match('TEMPLATE')) return this.interpolation(this.previous())
    if (this.match('FUNC')) return this.functionExpression()
    if (this.match('IDENTIFIER')) return { kind: 'Variable', name: this.previous() }
    if (this.match('LPAREN')) {
      const expr = this.expression()
      this.consume('RPAREN', "Expect ')' after expression.")
      return { kind: 'Grouping', expression: expr }
    }
    if (this.match('LBRACKET')) {
      const elements: Expr[] = []
      if (!this.check('RBRACKET')) {
        do { elements.push(this.spreadOrExpression()) } while (this.match('COMMA'))
      }
      this.consume('RBRACKET', "Expect ']' after array elements.")
      return { kind: 'Array', elements }
    }
    if (this.match('LBRACE')) {
      const pairs: [Expr, Expr][] = []
      if (!this.check('RBRACE')) {
        do {
          // A bareword key is shorthand for the *string* of that name:
          // `{name: "Ada"}` means `{"name": "Ada"}`. To use a variable's
          // value as the key instead, parenthesise it: `{(k): v}`.
          const key: Expr = this.check('IDENTIFIER') && this.checkNext('COLON')
            ? { kind: 'Literal', value: this.advance().lexeme }
            : this.expression()
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

  /** Turn a TEMPLATE token's parts into an `Interpolation` node. Each
   * `${ ... }` part arrives from the lexer as raw source text, which is
   * lexed and parsed here as a self-contained expression; line numbers from
   * that sub-lex are shifted onto the line the fragment appeared on. */
  private interpolation(token: Token): Expr {
    const parts: (string | Expr)[] = []
    for (const part of token.literal as TemplatePart[]) {
      if (part.kind === 'str') { parts.push(part.value); continue }

      if (part.source.trim() === '') {
        throw this.error(token, "Empty interpolation: expected an expression inside '${}'.")
      }

      let subTokens: Token[]
      try {
        subTokens = new Lexer(part.source).scanTokens()
      } catch (e) {
        throw new MRTError(e instanceof MRTError ? e.message.replace(/ \[line \d+\]$/, '') : String(e), part.line)
      }
      for (const t of subTokens) t.line += part.line - 1

      const subParser = new Parser(subTokens)
      const expr = subParser.expression()
      if (!subParser.isAtEnd()) {
        throw this.error(subParser.peek(), 'Unexpected trailing tokens in interpolation.')
      }
      parts.push(expr)
    }
    return { kind: 'Interpolation', parts }
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
  /** One token of lookahead past `peek()`, for constructs that can't be
   * identified from their first token alone (`func name` vs `func(`,
   * `for (x in` vs `for (x =`, a bareword object key vs an expression). */
  private checkNext(type: TokenType): boolean {
    return this.current + 1 < this.tokens.length && this.tokens[this.current + 1].type === type
  }
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
    return new MRTError(`Error at ${where}: ${message}`, token.line, 'RuntimeError')
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
  // A built-in (a host-language function, not an MRTFunction, whose
  // toString below yields "<function name>"). Without this, String(fn)
  // would dump the JavaScript source text, disagreeing with the reference
  // interpreter -- which would otherwise print a Python repr complete with
  // a memory address.
  if (typeof value === 'function') return '<builtin>'
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

/** A callable closure. Built from either a `func name(...)` declaration or
 * an anonymous `func(...)` expression -- the two are the same thing at
 * runtime, differing only in whether `name` is set. */
class MRTFunction {
  constructor(
    public params: Param[],
    public body: Stmt[],
    public closure: Environment,
    public name: string | null = null,
  ) {}

  /** How this function's accepted argument count reads in an error. */
  arityDescription(): string {
    const positional = this.params.filter((p) => !p.rest)
    const required = positional.filter((p) => p.default === null).length
    if (this.params.some((p) => p.rest)) return `at least ${required}`
    if (required === positional.length) return String(required)
    return `between ${required} and ${positional.length}`
  }

  accepts(count: number): boolean {
    const positional = this.params.filter((p) => !p.rest)
    const required = positional.filter((p) => p.default === null).length
    if (count < required) return false
    return this.params.some((p) => p.rest) || count <= positional.length
  }

  /** Bind arguments to parameters in a fresh scope. Defaults are evaluated
   * here, at call time and in the callee's own scope, so a default may
   * refer to a parameter to its left. A rest parameter always binds -- to
   * an empty array when nothing is left over. */
  bind(interpreter: Interpreter, args: unknown[]): Environment {
    const environment = new Environment(this.closure)
    const positional = this.params.filter((p) => !p.rest)

    const previous = interpreter.environment
    try {
      interpreter.environment = environment
      positional.forEach((param, i) => {
        const value = i < args.length ? args[i] : interpreter.evaluate(param.default as Expr)
        environment.define(param.name.lexeme, value)
      })
    } finally {
      interpreter.environment = previous
    }

    for (const param of this.params) {
      if (param.rest) environment.define(param.name.lexeme, args.slice(positional.length))
    }

    return environment
  }

  call(interpreter: Interpreter, args: unknown[]): unknown {
    const environment = this.bind(interpreter, args)
    const frame = this.name ?? '<anonymous>'
    try {
      interpreter.executeBlock(this.body, environment)
      return null
    } catch (e) {
      if (e instanceof ReturnSignal) return e.value
      // Build the trace on the way out: a raise site knows nothing about
      // who called it, but every frame the error passes through knows its
      // own name. Innermost first.
      if (e instanceof MRTError) e.mrtStack.push(frame)
      else if (e instanceof MRTThrow) e.stack.push(frame)
      throw e
    }
  }

  toString() { return this.name ? `<function ${this.name}>` : '<function>' }
}

/** A value thrown by `throw`, unwinding until a `try`/`catch` catches it.
 * Distinct from MRTError: this carries an arbitrary MRT *value*, whereas
 * MRTError is the interpreter's own failure. Built-in runtime errors become
 * catchable by being converted into the standard error object below. */
class MRTThrow {
  stack: string[] = []
  constructor(public value: unknown) {}
}

/** The object a `catch` block receives for an interpreter-raised error: a
 * message, the line it happened on, a coarse `kind` to branch on, and the
 * MRT call stack innermost-first. */
function makeErrorValue(error: MRTError): Map<unknown, unknown> {
  return new Map<unknown, unknown>([
    ['message', error.rawMessage],
    ['line', error.line === undefined ? null : error.line],
    ['kind', error.kind],
    ['stack', [...error.mrtStack]],
  ])
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
    throw new MRTError(`Undefined variable '${name.lexeme}'.`, name.line, 'NameError')
  }

  assign(name: Token, value: unknown) {
    if (this.values.has(name.lexeme)) { this.values.set(name.lexeme, value); return }
    if (this.enclosing) { this.enclosing.assign(name, value); return }
    throw new MRTError(`Undefined variable '${name.lexeme}'.`, name.line, 'NameError')
  }
}

function isNumber(v: unknown): v is number { return typeof v === 'number' }

const BUILTINS: Record<string, (...args: unknown[]) => unknown> = {
  len: (...a) => {
    if (a.length !== 1) throw new MRTError('len() takes exactly one argument.', undefined, 'ArityError')
    if (typeof a[0] === 'string' || Array.isArray(a[0])) return (a[0] as string | unknown[]).length
    if (a[0] instanceof Map) return a[0].size
    throw new MRTError('len() argument must be an array, object, or string.', undefined, 'TypeError')
  },
  push: (...a) => {
    if (a.length !== 2 || !Array.isArray(a[0])) throw new MRTError('push() takes an array and a value.', undefined, 'ArityError')
    ;(a[0] as unknown[]).push(a[1])
    return a[1]
  },
  pop: (...a) => {
    if (a.length !== 1 || !Array.isArray(a[0])) throw new MRTError('pop() takes exactly one array argument.', undefined, 'ArityError')
    const arr = a[0] as unknown[]
    if (arr.length === 0) throw new MRTError('Cannot pop from empty array.', undefined, 'ValueError')
    return arr.pop()
  },
  slice: (...a) => {
    if (!Array.isArray(a[0])) throw new MRTError('First argument to slice() must be an array.', undefined, 'TypeError')
    const arr = a[0] as unknown[]
    let start = isNumber(a[1]) ? Math.trunc(a[1]) : 0
    let end = a.length > 2 && isNumber(a[2]) ? Math.trunc(a[2] as number) : arr.length
    if (start < 0) start = arr.length + start
    if (end < 0) end = arr.length + end
    return arr.slice(start, end)
  },
  join: (...a) => {
    if (!Array.isArray(a[0])) throw new MRTError('First argument to join() must be an array.', undefined, 'TypeError')
    const sep = a.length > 1 ? String(a[1]) : ''
    return (a[0] as unknown[]).map(stringify).join(sep)
  },
  indexOf: (...a) => {
    if (!Array.isArray(a[0])) throw new MRTError('First argument to indexOf() must be an array.', undefined, 'TypeError')
    const arr = a[0] as unknown[]
    return arr.findIndex((v) => valuesEqual(v, a[1]))
  },
  split: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('First argument to split() must be a string.', undefined, 'TypeError')
    const sep = a.length > 1 ? String(a[1]) : ' '
    return a[0].split(sep)
  },
  substring: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('First argument to substring() must be a string.', undefined, 'TypeError')
    const text = a[0]
    let start = isNumber(a[1]) ? Math.trunc(a[1]) : 0
    let end = a.length > 2 && isNumber(a[2]) ? Math.trunc(a[2] as number) : text.length
    if (start < 0) start = text.length + start
    if (end < 0) end = text.length + end
    return text.slice(start, end)
  },
  toUpper: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('toUpper() argument must be a string.', undefined, 'TypeError')
    return a[0].toUpperCase()
  },
  toLower: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('toLower() argument must be a string.', undefined, 'TypeError')
    return a[0].toLowerCase()
  },
  trim: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('trim() argument must be a string.', undefined, 'TypeError')
    return a[0].trim()
  },
  replace: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('First argument to replace() must be a string.', undefined, 'TypeError')
    return a[0].split(String(a[1])).join(String(a[2]))
  },
  startsWith: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('First argument to startsWith() must be a string.', undefined, 'TypeError')
    return a[0].startsWith(String(a[1]))
  },
  endsWith: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('First argument to endsWith() must be a string.', undefined, 'TypeError')
    return a[0].endsWith(String(a[1]))
  },
  contains: (...a) => {
    if (typeof a[0] !== 'string') throw new MRTError('First argument to contains() must be a string.', undefined, 'TypeError')
    return a[0].includes(String(a[1]))
  },

  // -- Type / conversion --
  type: (...a) => {
    if (a.length !== 1) throw new MRTError('type() takes exactly one argument.', undefined, 'ArityError')
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
    if (a.length !== 1) throw new MRTError('toNumber() takes exactly one argument.', undefined, 'ArityError')
    const v = a[0]
    if (typeof v === 'boolean') return v ? 1 : 0
    if (typeof v === 'number') return v
    if (typeof v === 'string') {
      const n = Number(v.trim())
      if (Number.isNaN(n) || v.trim() === '') throw new MRTError(`Cannot convert '${v}' to a number.`, undefined, 'ValueError')
      return n
    }
    throw new MRTError('toNumber() argument must be a string, number, or boolean.', undefined, 'TypeError')
  },
  // -- Math --
  abs: (...a) => {
    if (a.length !== 1) throw new MRTError('abs() takes exactly one argument.', undefined, 'ArityError')
    return Math.abs(numArg(a[0], 'abs'))
  },
  min: (...a) => {
    const values = a.length === 1 && Array.isArray(a[0]) ? (a[0] as unknown[]) : a
    if (values.length === 0) throw new MRTError('min() requires at least one argument.', undefined, 'ArityError')
    return Math.min(...values.map((v) => numArg(v, 'min')))
  },
  max: (...a) => {
    const values = a.length === 1 && Array.isArray(a[0]) ? (a[0] as unknown[]) : a
    if (values.length === 0) throw new MRTError('max() requires at least one argument.', undefined, 'ArityError')
    return Math.max(...values.map((v) => numArg(v, 'max')))
  },
  round: (...a) => {
    if (a.length < 1 || a.length > 2) throw new MRTError('round() takes 1 or 2 arguments.', undefined, 'ArityError')
    const value = numArg(a[0], 'round')
    const digits = a.length === 2 ? Math.trunc(numArg(a[1], 'round')) : 0
    const factor = 10 ** digits
    return Math.round(value * factor) / factor
  },
  floor: (...a) => {
    if (a.length !== 1) throw new MRTError('floor() takes exactly one argument.', undefined, 'ArityError')
    return Math.floor(numArg(a[0], 'floor'))
  },
  ceil: (...a) => {
    if (a.length !== 1) throw new MRTError('ceil() takes exactly one argument.', undefined, 'ArityError')
    return Math.ceil(numArg(a[0], 'ceil'))
  },
  sqrt: (...a) => {
    if (a.length !== 1) throw new MRTError('sqrt() takes exactly one argument.', undefined, 'ArityError')
    const value = numArg(a[0], 'sqrt')
    if (value < 0) throw new MRTError('sqrt() argument must not be negative.', undefined, 'ValueError')
    return Math.sqrt(value)
  },
  pow: (...a) => {
    if (a.length !== 2) throw new MRTError('pow() takes exactly 2 arguments.', undefined, 'ArityError')
    return numArg(a[0], 'pow') ** numArg(a[1], 'pow')
  },

  // -- Objects (dicts, represented as Map so keys can be numbers/booleans too) --
  keys: (...a) => {
    if (a.length !== 1 || !(a[0] instanceof Map)) throw new MRTError('keys() takes exactly one object argument.', undefined, 'ArityError')
    return Array.from(a[0].keys())
  },
  values: (...a) => {
    if (a.length !== 1 || !(a[0] instanceof Map)) throw new MRTError('values() takes exactly one object argument.', undefined, 'ArityError')
    return Array.from(a[0].values())
  },
  has: (...a) => {
    if (a.length !== 2) throw new MRTError('has() takes exactly 2 arguments.', undefined, 'ArityError')
    const [container, key] = a
    if (container instanceof Map) return container.has(key)
    if (Array.isArray(container)) return container.some((v) => valuesEqual(v, key))
    throw new MRTError('First argument to has() must be an array or object.', undefined, 'TypeError')
  },
  get: (...a) => {
    if (a.length < 2 || a.length > 3) throw new MRTError('get() takes 2 or 3 arguments.', undefined, 'ArityError')
    const [container, key] = a
    const fallback = a.length === 3 ? a[2] : null
    if (container instanceof Map) return container.has(key) ? container.get(key) : fallback
    if (Array.isArray(container) || typeof container === 'string') {
      if (typeof key === 'number') {
        const i = Math.trunc(key)
        if (i >= 0 && i < container.length) return container[i]
      }
      return fallback
    }
    // Anything else has no keys at all, so the fallback is the answer.
    // get() is the total, never-raising accessor -- throwing here would
    // break its whole contract, and would make the natural `catch` guard
    // `get(e, "kind", "")` blow up on a thrown string or number.
    return fallback
  },
  // -- Sequences --
  reverse: (...a) => {
    if (a.length !== 1) throw new MRTError('reverse() takes exactly one argument.', undefined, 'ArityError')
    if (typeof a[0] === 'string') return [...a[0]].reverse().join('')
    if (Array.isArray(a[0])) return [...a[0]].reverse()
    throw new MRTError('reverse() argument must be an array or string.', undefined, 'TypeError')
  },
  unique: (...a) => {
    if (a.length !== 1 || !Array.isArray(a[0])) throw new MRTError('unique() takes exactly one array argument.', undefined, 'ArityError')
    const result: unknown[] = []
    for (const item of a[0]) if (!result.some((seen) => valuesEqual(item, seen))) result.push(item)
    return result
  },
  flatten: (...a) => {
    if (a.length < 1 || a.length > 2 || !Array.isArray(a[0])) {
      throw new MRTError('flatten() takes an array and an optional depth.', undefined, 'ArityError')
    }
    const depth = a.length === 1 ? 1 : Math.trunc(numArg(a[1], 'flatten'))
    if (depth < 0) throw new MRTError('flatten() depth must not be negative.', undefined, 'ValueError')
    const go = (items: unknown[], d: number): unknown[] => {
      const out: unknown[] = []
      for (const item of items) {
        if (Array.isArray(item) && d > 0) out.push(...go(item, d - 1))
        else out.push(item)
      }
      return out
    }
    return go(a[0], depth)
  },
  zip: (...a) => {
    if (a.length !== 2 || !Array.isArray(a[0]) || !Array.isArray(a[1])) {
      throw new MRTError('zip() takes exactly two array arguments.', undefined, 'ArityError')
    }
    const n = Math.min(a[0].length, a[1].length)
    const out: unknown[] = []
    for (let i = 0; i < n; i++) out.push([a[0][i], a[1][i]])
    return out
  },
  enumerate: (...a) => {
    if (a.length !== 1 || !Array.isArray(a[0])) throw new MRTError('enumerate() takes exactly one array argument.', undefined, 'ArityError')
    return a[0].map((v, i) => [i, v])
  },
  count: (...a) => {
    if (a.length !== 2 || !Array.isArray(a[0])) throw new MRTError('count() takes an array and a value.', undefined, 'ArityError')
    return a[0].filter((item) => valuesEqual(item, a[1])).length
  },
  sum: (...a) => {
    if (a.length !== 1 || !Array.isArray(a[0])) throw new MRTError('sum() takes exactly one array argument.', undefined, 'ArityError')
    let total = 0
    for (const item of a[0]) total += numArg(item, 'sum')
    return total
  },
  range: (...a) => {
    if (a.length < 1 || a.length > 3) throw new MRTError('range() takes one to three number arguments.', undefined, 'ArityError')
    const nums = a.map((v) => numArg(v, 'range'))
    let start = 0, end = 0, step = 1
    if (nums.length === 1) { end = nums[0] }
    else if (nums.length === 2) { start = nums[0]; end = nums[1] }
    else { start = nums[0]; end = nums[1]; step = nums[2] }
    if (step === 0) throw new MRTError('range() step must not be zero.', undefined, 'ValueError')
    const out: number[] = []
    for (let c = start; step > 0 ? c < end : c > end; c += step) out.push(c)
    return out
  },
  // -- More strings --
  repeat: (...a) => {
    if (a.length !== 2 || typeof a[0] !== 'string') throw new MRTError('repeat() takes a string and a count.', undefined, 'ArityError')
    const n = Math.trunc(numArg(a[1], 'repeat'))
    if (n < 0) throw new MRTError('repeat() count must not be negative.', undefined, 'ValueError')
    return a[0].repeat(n)
  },
  padStart: (...a) => pad(a, 'padStart', true),
  padEnd: (...a) => pad(a, 'padEnd', false),
  // -- Deterministic, seeded randomness --
  random: (...a) => {
    if (a.length !== 1) throw new MRTError('random() takes exactly one seed argument.', undefined, 'ArityError')
    const seed = Math.trunc(numArg(a[0], 'random')) >>> 0
    let state = seed !== 0 ? seed : 1
    return (...callArgs: unknown[]) => {
      if (callArgs.length) throw new MRTError('A random generator takes no arguments.', undefined, 'ArityError')
      // xorshift32 over uint32 state -- identical arithmetic to the Python
      // reference implementation, so a seeded program prints the same
      // numbers in the browser as it does on the command line.
      state ^= (state << 13); state >>>= 0
      state ^= (state >>> 17); state >>>= 0
      state ^= (state << 5); state >>>= 0
      return state / 4294967296
    }
  },
}

function pad(args: unknown[], who: string, atStart: boolean): string {
  if (args.length < 2 || args.length > 3 || typeof args[0] !== 'string') {
    throw new MRTError(`${who}() takes a string, a width, and an optional pad string.`, undefined, 'ArityError')
  }
  const text = args[0]
  const width = Math.trunc(numArg(args[1], who))
  const filler = args.length === 3 ? args[2] : ' '
  if (typeof filler !== 'string') throw new MRTError(`${who}() pad argument must be a string.`, undefined, 'TypeError')
  if (filler === '') throw new MRTError(`${who}() pad string must not be empty.`, undefined, 'ValueError')
  if (text.length >= width) return text
  const needed = width - text.length
  const padding = filler.repeat(Math.floor(needed / filler.length) + 1).slice(0, needed)
  return atStart ? padding + text : text + padding
}

function numArg(value: unknown, who: string): number {
  if (typeof value !== 'number') throw new MRTError(`${who}() argument must be a number.`, undefined, 'TypeError')
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
  if (a.length !== 1) throw new MRTError('toString() takes exactly one argument.', undefined, 'ArityError')
  return stringify(a[0])
}
// A literal `.toString` (or even `['toString']`) assignment is still typed
// against the inherited `Object.prototype.toString(): string` member, not
// the Record index signature -- so the key has to be a non-literal `string`
// to force TypeScript through the index signature instead.
const toStringKey: string = 'toString'
BUILTINS[toStringKey] = toStringBuiltin

/** Resolves an import specifier to a module's canonical path and source.
 * Returning null means "no such module". The browser has no filesystem, so
 * module loading is injected rather than assumed: the Playground can supply
 * a map of virtual files, a Node host can read from disk, and a page that
 * supplies nothing simply has no imports. */
export type ModuleResolver =
  (specifier: string, fromPath: string) => { path: string; source: string } | null

export interface RunOptions {
  /** Path of the entry file, used to resolve relative imports against. */
  path?: string
  resolveModule?: ModuleResolver
}

class Interpreter {
  globals = new Environment()
  environment = this.globals
  output: string[] = []

  modulePath: string | null
  resolveModule: ModuleResolver | null
  private moduleExports = new Map<string, Map<string, unknown>>()
  private moduleLoading: string[] = []
  private currentExports = new Map<string, unknown>()

  constructor(options: RunOptions = {}) {
    this.modulePath = options.path ?? null
    this.resolveModule = options.resolveModule ?? null
    for (const [name, fn] of Object.entries(BUILTINS)) this.globals.define(name, fn)
    // Higher-order built-ins are defined here rather than in BUILTINS
    // because they have to call back into user code (`this.callValue`),
    // which a free-standing function has no handle on.
    this.globals.define('map', (...a: unknown[]) => this.builtinMap(a))
    this.globals.define('filter', (...a: unknown[]) => this.builtinFilter(a))
    this.globals.define('reduce', (...a: unknown[]) => this.builtinReduce(a))
    this.globals.define('find', (...a: unknown[]) => this.builtinFind(a))
    this.globals.define('some', (...a: unknown[]) => this.builtinSome(a))
    this.globals.define('every', (...a: unknown[]) => this.builtinEvery(a))
    this.globals.define('sort', (...a: unknown[]) => this.builtinSort(a))
  }

  private arrayArg(value: unknown, who: string): unknown[] {
    if (!Array.isArray(value)) throw new MRTError(`First argument to ${who}() must be an array.`, undefined, 'TypeError')
    return value
  }

  private builtinMap(a: unknown[]): unknown {
    if (a.length !== 2) throw new MRTError('map() takes an array and a function.', undefined, 'ArityError')
    return this.arrayArg(a[0], 'map').map((item) => this.callValue(a[1], [item]))
  }

  private builtinFilter(a: unknown[]): unknown {
    if (a.length !== 2) throw new MRTError('filter() takes an array and a function.', undefined, 'ArityError')
    return this.arrayArg(a[0], 'filter').filter((item) => this.isTruthy(this.callValue(a[1], [item])))
  }

  private builtinReduce(a: unknown[]): unknown {
    if (a.length < 2 || a.length > 3) {
      throw new MRTError('reduce() takes an array, a function, and an optional initial value.', undefined, 'ArityError')
    }
    const items = this.arrayArg(a[0], 'reduce')
    let accumulator: unknown
    let rest: unknown[]
    if (a.length === 3) {
      accumulator = a[2]
      rest = items
    } else {
      if (items.length === 0) throw new MRTError('reduce() of an empty array needs an initial value.', undefined, 'ValueError')
      accumulator = items[0]
      rest = items.slice(1)
    }
    for (const item of rest) accumulator = this.callValue(a[1], [accumulator, item])
    return accumulator
  }

  private builtinFind(a: unknown[]): unknown {
    if (a.length !== 2) throw new MRTError('find() takes an array and a function.', undefined, 'ArityError')
    for (const item of this.arrayArg(a[0], 'find')) {
      if (this.isTruthy(this.callValue(a[1], [item]))) return item
    }
    return null
  }

  private builtinSome(a: unknown[]): unknown {
    if (a.length !== 2) throw new MRTError('some() takes an array and a function.', undefined, 'ArityError')
    return this.arrayArg(a[0], 'some').some((item) => this.isTruthy(this.callValue(a[1], [item])))
  }

  private builtinEvery(a: unknown[]): unknown {
    if (a.length !== 2) throw new MRTError('every() takes an array and a function.', undefined, 'ArityError')
    return this.arrayArg(a[0], 'every').every((item) => this.isTruthy(this.callValue(a[1], [item])))
  }

  /** `sort(arr)` or `sort(arr, compare)`. Returns a new array; the input is
   * left alone. Without a comparator the array must be all numbers or all
   * strings -- there is no cross-type default order, and notably *not*
   * JavaScript's default of coercing everything to a string. Array.prototype
   * .sort is stable (ES2019+), matching Python's sorted(). */
  private builtinSort(a: unknown[]): unknown {
    if (a.length < 1 || a.length > 2) {
      throw new MRTError('sort() takes an array and an optional compare function.', undefined, 'ArityError')
    }
    const items = [...this.arrayArg(a[0], 'sort')]

    if (a.length === 2) {
      return items.sort((x, y) => {
        const result = this.callValue(a[1], [x, y])
        if (typeof result !== 'number') throw new MRTError('sort() compare function must return a number.', undefined, 'TypeError')
        return result < 0 ? -1 : result > 0 ? 1 : 0
      })
    }

    if (items.every((i) => typeof i === 'number')) {
      return items.sort((x, y) => (x as number) - (y as number))
    }
    if (items.every((i) => typeof i === 'string')) {
      return items.sort((x, y) => ((x as string) < (y as string) ? -1 : (x as string) > (y as string) ? 1 : 0))
    }
    throw new MRTError('sort() without a compare function needs an array of all numbers or all strings.', undefined, 'ValueError')
  }

  private printValues(values: unknown[]) {
    this.output.push(values.map(stringify).join(' '))
  }

  interpret(statements: Stmt[]) {
    try {
      // Function declarations are registered first (so they can refer to
      // each other in any order), then the remaining top-level statements
      // run in source order -- imports among them, which is why they can no
      // longer be skipped when a main() exists.
      this.runTopLevel(statements)

      let mainFn: unknown = null
      try {
        mainFn = this.environment.get({ type: 'IDENTIFIER', lexeme: 'main', literal: null, line: 1 })
      } catch {
        // no main() defined -- the top-level statements above were the program
      }

      if (mainFn instanceof MRTFunction) mainFn.call(this, [])
    } catch (e) {
      // Nothing caught it, so it halts the program like any other runtime
      // failure -- but reports the thrown value, since that's what the
      // program chose to say.
      if (e instanceof MRTThrow) {
        this.output.push(`Runtime Error: Uncaught ${stringify(e.value)}`)
        return
      }
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
        this.environment.define(
          stmt.name.lexeme,
          new MRTFunction(stmt.params, stmt.body, this.environment, stmt.name.lexeme),
        )
        return
      case 'ForIn':
        this.executeForIn(stmt)
        return
      case 'Import':
        this.executeImport(stmt)
        return
      case 'Export':
        this.execute(stmt.declaration)
        this.currentExports.set(stmt.name.lexeme, this.environment.get(stmt.name))
        return
      case 'Throw':
        throw new MRTThrow(this.evaluate(stmt.value))
      case 'Try':
        this.executeTry(stmt)
        return
      case 'If':
        if (this.isTruthy(this.evaluate(stmt.condition))) this.execute(stmt.thenBranch)
        else if (stmt.elseBranch) this.execute(stmt.elseBranch)
        return
      case 'Print':
        this.printValues(this.evaluateSpreadList(stmt.expressions))
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

  /** Invoke an MRT value with arguments. Shared by the `Call` expression and
   * by the higher-order built-ins (map, filter, sort, ...), which need to
   * call back into user code. */
  /** Evaluate an argument list or array literal, splicing `...expr`
   * elements in place. Spreading anything but an array is an error --
   * there is no implicit iteration of strings or objects here, which keeps
   * `f(...x)` from silently meaning something different depending on what
   * `x` happens to hold. */
  evaluateSpreadList(items: Expr[]): unknown[] {
    const values: unknown[] = []
    for (const item of items) {
      if (item.kind === 'Spread') {
        const spread = this.evaluate(item.value)
        if (!Array.isArray(spread)) {
          throw new MRTError("Can only spread an array with '...'.", item.token.line, 'TypeError')
        }
        values.push(...spread)
      } else {
        values.push(this.evaluate(item))
      }
    }
    return values
  }

  callValue(callee: unknown, args: unknown[], line?: number): unknown {
    if (callee instanceof MRTFunction) {
      if (!callee.accepts(args.length)) {
        throw new MRTError(
          `Expected ${callee.arityDescription()} arguments but got ${args.length}.`,
          line, 'ArityError')
      }
      return callee.call(this, args)
    }
    if (typeof callee === 'function') {
      try {
        return (callee as (...a: unknown[]) => unknown)(...args)
      } catch (e) {
        // Only attach a line if the error doesn't already carry one --
        // MRTError bakes the line into its message, so re-wrapping an
        // already-located error would append a second "[line N]".
        if (e instanceof MRTError && e.line === undefined && line !== undefined) {
          // Re-raised only to attach a call-site line; the original
          // classification and any stack collected so far are the error's
          // own and must survive.
          const located = new MRTError(e.rawMessage, line, e.kind)
          located.mrtStack = e.mrtStack
          throw located
        }
        throw e
      }
    }
    throw new MRTError('Can only call functions.', line, 'TypeError')
  }

  // -- Modules ------------------------------------------------------------

  /** Evaluate a module once and return its export table. Modules are cached
   * by resolved path, so importing the same file from two places runs it
   * once and shares the result -- which matters, since a module's top-level
   * code can have side effects. */
  private loadModule(specifier: string, line?: number): Map<string, unknown> {
    if (!specifier.startsWith('./') && !specifier.startsWith('../')) {
      throw new MRTError(
        `Module path ${JSON.stringify(specifier)} must start with './' or '../'.`,
        line, 'ValueError')
    }
    if (this.resolveModule === null || this.modulePath === null) {
      throw new MRTError(
        'Imports need a file to resolve against; run this program from a file.',
        line, 'RuntimeError')
    }

    const resolved = this.resolveModule(specifier, this.modulePath)
    if (resolved === null) {
      throw new MRTError(`Cannot find module ${JSON.stringify(specifier)}.`, line, 'ValueError')
    }

    const cached = this.moduleExports.get(resolved.path)
    if (cached) return cached

    if (this.moduleLoading.includes(resolved.path)) {
      const cycle = [...this.moduleLoading, resolved.path]
        .map((p) => p.split('/').pop())
        .join(' -> ')
      throw new MRTError(`Circular import: ${cycle}.`, line, 'RuntimeError')
    }

    const parser = new Parser(new Lexer(resolved.source).scanTokens())
    const statements = parser.parse()
    if (parser.errors.length > 0) {
      throw new MRTError(
        `Module ${JSON.stringify(specifier)} has syntax errors: ${parser.errors[0].rawMessage}`,
        line, 'RuntimeError')
    }

    const previousEnv = this.environment
    const previousPath = this.modulePath
    const previousExports = this.currentExports

    this.moduleLoading.push(resolved.path)
    this.environment = new Environment(this.globals)
    this.modulePath = resolved.path
    this.currentExports = new Map()
    let exports: Map<string, unknown>
    try {
      this.runTopLevel(statements)
      exports = this.currentExports
    } finally {
      this.moduleLoading.pop()
      this.environment = previousEnv
      this.modulePath = previousPath
      this.currentExports = previousExports
    }

    this.moduleExports.set(resolved.path, exports)
    return exports
  }

  private executeImport(stmt: Extract<Stmt, { kind: 'Import' }>) {
    const specifier = stmt.specifier.literal as string
    const exports = this.loadModule(specifier, stmt.keyword.line)
    for (const [exported, local] of stmt.names) {
      if (!exports.has(exported.lexeme)) {
        throw new MRTError(
          `Module ${JSON.stringify(specifier)} has no export named '${exported.lexeme}'.`,
          exported.line, 'NameError')
      }
      this.environment.define(local.lexeme, exports.get(exported.lexeme))
    }
  }

  /** Run a file's top-level statements: function declarations first, so they
   * can refer to each other regardless of order, then everything else in
   * source order. */
  runTopLevel(statements: Stmt[]) {
    const isFunctionDecl = (s: Stmt) =>
      s.kind === 'Function' || (s.kind === 'Export' && s.declaration.kind === 'Function')
    for (const s of statements) if (isFunctionDecl(s)) this.execute(s)
    for (const s of statements) if (!isFunctionDecl(s)) this.execute(s)
  }

  private executeForIn(stmt: Extract<Stmt, { kind: 'ForIn' }>) {
    const iterable = this.evaluate(stmt.iterable)

    let items: unknown[]
    if (Array.isArray(iterable)) items = [...iterable]
    else if (typeof iterable === 'string') items = [...iterable]
    else if (iterable instanceof Map) items = [...iterable.keys()]
    else throw new MRTError('Can only iterate over an array, string, or object.', stmt.name.line, 'TypeError')

    const previous = this.environment
    try {
      for (const item of items) {
        // A fresh scope per iteration, so a closure made in the body captures
        // this item rather than sharing one slot with every other iteration.
        this.environment = new Environment(previous)
        this.environment.define(stmt.name.lexeme, item)
        try {
          this.execute(stmt.body)
        } catch (e) {
          if (e instanceof BreakSignal) break
          if (e instanceof ContinueSignal) continue
          throw e
        }
      }
    } finally {
      this.environment = previous
    }
  }

  private executeTry(stmt: Extract<Stmt, { kind: 'Try' }>) {
    try {
      try {
        this.execute(stmt.tryBlock)
      } catch (e) {
        if (e instanceof BreakSignal || e instanceof ContinueSignal || e instanceof ReturnSignal) throw e
        if (e instanceof MRTThrow) {
          if (!this.runCatch(stmt, e.value)) throw e
        } else if (e instanceof MRTError) {
          // An interpreter-raised failure (bad index, division by zero, ...)
          // is catchable too: it reaches the program as the standard error
          // object, carrying `kind` for guards to branch on.
          if (!this.runCatch(stmt, makeErrorValue(e))) throw e
        } else {
          throw e
        }
      }
    } finally {
      // Runs on every path out of the try -- normal completion, a caught or
      // uncaught throw, and a return/break/continue unwinding through it.
      if (stmt.finallyBlock !== null) this.execute(stmt.finallyBlock)
    }
  }

  /** Run the first `catch` clause that matches, returning whether one did.
   * A clause with no guard always matches; a guarded one is tried with the
   * error already bound, so the guard can inspect it. If none match, the
   * error keeps propagating (and `finally` still runs). */
  private runCatch(stmt: Extract<Stmt, { kind: 'Try' }>, value: unknown): boolean {
    for (const clause of stmt.catches) {
      const environment = new Environment(this.environment)
      environment.define(clause.name.lexeme, value)

      if (clause.guard !== null) {
        const previous = this.environment
        let matched: boolean
        try {
          this.environment = environment
          matched = this.isTruthy(this.evaluate(clause.guard))
        } finally {
          this.environment = previous
        }
        if (!matched) continue
      }

      this.executeBlock((clause.block as Extract<Stmt, { kind: 'Block' }>).statements, environment)
      return true
    }
    return false
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
        return this.evaluateSpreadList(expr.elements)
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
        const args = this.evaluateSpreadList(expr.arguments)
        return this.callValue(callee, args, expr.paren.line)
      }
      case 'Spread':
        throw new MRTError(
          "'...' is only allowed in a call's arguments or an array literal.",
          expr.token.line, 'TypeError')
      case 'FunctionExpr':
        return new MRTFunction(expr.params, expr.body, this.environment,
          expr.name ? expr.name.lexeme : null)
      case 'Interpolation':
        return expr.parts
          .map((part) => (typeof part === 'string' ? part : stringify(this.evaluate(part))))
          .join('')
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
          if (typeof right !== 'number') throw new MRTError("Operand of '-' must be a number.", expr.operator.line, 'TypeError')
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
      if (!target.has(index)) throw new MRTError(`Key ${JSON.stringify(stringify(index))} not found in object.`, undefined, 'KeyError')
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
    throw new MRTError('Can only index into arrays, objects, or strings.', undefined, 'TypeError')
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
      throw new MRTError('Strings are immutable; cannot assign to a character index.', undefined, 'TypeError')
    }
    throw new MRTError('Can only assign into arrays or objects.', undefined, 'TypeError')
  }

  private requireArrayIndex(index: unknown, length: number): number {
    if (typeof index !== 'number') throw new MRTError('Array index must be a number.', undefined, 'IndexError')
    const i = Math.trunc(index)
    if (i < 0 || i >= length) throw new MRTError(`Array index ${i} out of bounds for array of length ${length}.`, undefined, 'IndexError')
    return i
  }

  private checkHashableKey(key: unknown) {
    if (Array.isArray(key) || key instanceof Map) {
      throw new MRTError('Object keys must be numbers, strings, or booleans (not arrays or objects).', undefined, 'TypeError')
    }
  }

  private evaluateBinary(expr: Extract<Expr, { kind: 'Binary' }>): unknown {
    const left = this.evaluate(expr.left)
    const right = this.evaluate(expr.right)
    const op = expr.operator.type
    const line = expr.operator.line

    const checkNumbers = (opSym: string) => {
      if (typeof left !== 'number' || typeof right !== 'number') {
        throw new MRTError(`Operands of '${opSym}' must be numbers.`, line, 'TypeError')
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
        if ((right as number) === 0) throw new MRTError('Division by zero.', line, 'ArithmeticError')
        return (left as number) / (right as number)
      case 'MODULO':
        checkNumbers('%')
        if ((right as number) === 0) throw new MRTError('Modulo by zero.', line, 'ArithmeticError')
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
        throw new MRTError(`Unknown binary operator '${expr.operator.lexeme}'.`, line, 'RuntimeError')
    }
  }

  private compare(left: unknown, right: unknown, line: number): number {
    if (typeof left === 'string' && typeof right === 'string') return left < right ? -1 : left > right ? 1 : 0
    if (typeof left === 'number' && typeof right === 'number') return left < right ? -1 : left > right ? 1 : 0
    throw new MRTError('Comparison operators require two numbers or two strings.', line, 'TypeError')
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

export function runMRT(source: string, options: RunOptions = {}): RunResult {
  try {
    const tokens = new Lexer(source).scanTokens()
    const parser = new Parser(tokens)
    const statements = parser.parse()

    if (parser.errors.length > 0) {
      return { output: [], errors: parser.errors.map((e) => `Syntax Error: ${e.message}`) }
    }

    const interpreter = new Interpreter(options)
    interpreter.interpret(statements)
    return { output: interpreter.output, errors: [] }
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    return { output: [], errors: [`Syntax Error: ${message}`] }
  }
}
