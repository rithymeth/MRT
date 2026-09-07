from typing import List, Optional
from .lexer import Token, TokenType
from .ast import *
from .errors import MRTSyntaxError

# Maps each compound-assignment token to the plain binary operator it
# desugars into, e.g. `x += 1` becomes `x = x + 1`.
COMPOUND_ASSIGN_OPS = {
    TokenType.PLUS_ASSIGN: (TokenType.PLUS, '+'),
    TokenType.MINUS_ASSIGN: (TokenType.MINUS, '-'),
    TokenType.MULTIPLY_ASSIGN: (TokenType.MULTIPLY, '*'),
    TokenType.DIVIDE_ASSIGN: (TokenType.DIVIDE, '/'),
    TokenType.MODULO_ASSIGN: (TokenType.MODULO, '%'),
}

class Parser:
    def __init__(self, tokens: List[Token]):
        self.tokens = tokens
        self.current = 0
        self.errors: List["ParseError"] = []
        # `import`/`export` are only meaningful at the top level of a file,
        # so the parser tracks how deep into blocks it currently is.
        self.block_depth = 0

    def parse(self) -> List[Stmt]:
        statements = []
        while not self.is_at_end():
            stmt = self.declaration()
            if stmt:
                statements.append(stmt)
        return statements

    def declaration(self) -> Optional[Stmt]:
        try:
            if self.match(TokenType.IMPORT):
                return self.import_statement()
            if self.match(TokenType.EXPORT):
                return self.export_declaration()
            # `func name(...)` is a declaration; a bare `func(...)` in
            # statement position is an anonymous function *expression* and
            # falls through to expression_statement below.
            if self.check(TokenType.FUNC) and self.check_next(TokenType.IDENTIFIER):
                self.advance()
                return self.function("function")
            if self.match(TokenType.VAR):
                return self.var_declaration()
            return self.statement()
        except ParseError as e:
            self.errors.append(e)
            self.synchronize()
            return None

    def import_statement(self) -> Import:
        keyword = self.previous()
        if self.block_depth > 0:
            raise self.error(keyword, "'import' is only allowed at the top level of a file.")

        self.consume(TokenType.LBRACE, "Expect '{' after 'import'.")
        names: List[tuple] = []
        if not self.check(TokenType.RBRACE):
            while True:
                exported = self.consume(TokenType.IDENTIFIER, "Expect an imported name.")
                local = exported
                if self.match(TokenType.AS):
                    local = self.consume(TokenType.IDENTIFIER, "Expect a local name after 'as'.")
                names.append((exported, local))
                if not self.match(TokenType.COMMA):
                    break
        self.consume(TokenType.RBRACE, "Expect '}' after imported names.")

        self.consume(TokenType.FROM, "Expect 'from' after imported names.")
        specifier = self.consume(TokenType.STRING, "Expect a module path string after 'from'.")
        self.consume_statement_end("Expect ';' after import.")
        return Import(names, specifier, keyword)

    def export_declaration(self) -> Export:
        keyword = self.previous()
        if self.block_depth > 0:
            raise self.error(keyword, "'export' is only allowed at the top level of a file.")

        if self.check(TokenType.FUNC) and self.check_next(TokenType.IDENTIFIER):
            self.advance()
            declaration = self.function("function")
            return Export(declaration, declaration.name)
        if self.match(TokenType.VAR):
            declaration = self.var_declaration()
            return Export(declaration, declaration.name)
        raise self.error(self.peek(), "Expect a 'func' or 'var' declaration after 'export'.")

    def function(self, kind: str) -> Function:
        name = self.consume(TokenType.IDENTIFIER, f"Expect {kind} name.")
        parameters = self.parameter_list(f"Expect '(' after {kind} name.")
        self.consume(TokenType.LBRACE, f"Expect '{{' before {kind} body.")
        body = self.block()
        return Function(name, parameters, body)

    def parameter_list(self, lparen_message: str) -> List[Param]:
        """Parse `(a, b = expr, ...rest)`.

        Two shape rules are enforced here rather than at run time, because
        they are always mistakes: a rest parameter must be last, and a
        required parameter may not follow a defaulted one (which would make
        it unreachable by position)."""
        self.consume(TokenType.LPAREN, lparen_message)
        parameters: List[Param] = []
        seen_default = False
        seen_rest = False

        if not self.check(TokenType.RPAREN):
            while True:
                if len(parameters) >= 255:
                    self.error(self.peek(), "Can't have more than 255 parameters.")

                if seen_rest:
                    raise self.error(self.peek(),
                                     "A rest parameter must be the last parameter.")

                is_rest = self.match(TokenType.ELLIPSIS)
                name = self.consume(TokenType.IDENTIFIER, "Expect parameter name.")

                default = None
                if is_rest:
                    seen_rest = True
                    if self.check(TokenType.ASSIGN):
                        raise self.error(self.peek(),
                                         "A rest parameter can't have a default value.")
                elif self.match(TokenType.ASSIGN):
                    default = self.expression()
                    seen_default = True
                elif seen_default:
                    raise self.error(name,
                                     "A required parameter can't follow one with a default value.")

                parameters.append(Param(name, default, is_rest))
                if not self.match(TokenType.COMMA):
                    break

        self.consume(TokenType.RPAREN, "Expect ')' after parameters.")
        return parameters

    def function_expression(self) -> FunctionExpr:
        """An anonymous `func(a, b) { ... }` in expression position. An
        optional name is accepted (`func fact(n) { ... }` as a value) purely
        so the function has something to print as."""
        name = None
        if self.check(TokenType.IDENTIFIER):
            name = self.advance()
        parameters = self.parameter_list("Expect '(' after 'func'.")
        self.consume(TokenType.LBRACE, "Expect '{' before function body.")
        body = self.block()
        return FunctionExpr(parameters, body, name)

    def statement(self) -> Stmt:
        if self.match(TokenType.FOR):
            return self.for_statement()
        if self.match(TokenType.IF):
            return self.if_statement()
        if self.match(TokenType.RETURN):
            return self.return_statement()
        if self.match(TokenType.WHILE):
            return self.while_statement()
        if self.match(TokenType.BREAK):
            return self.break_statement()
        if self.match(TokenType.CONTINUE):
            return self.continue_statement()
        if self.match(TokenType.TRY):
            return self.try_statement()
        if self.match(TokenType.THROW):
            return self.throw_statement()
        if self.match(TokenType.LBRACE):
            return Block(self.block())
        if self.match(TokenType.PRINT):
            return self.print_statement()
        return self.expression_statement()

    def for_statement(self) -> Stmt:
        self.consume(TokenType.LPAREN, "Expect '(' after 'for'.")

        # `for (x in xs)` / `for (var x in xs)` -- decided by lookahead so the
        # C-style three-clause form below is untouched. Both spellings mean
        # the same thing; `var` is allowed because it reads naturally and
        # because the loop variable really is a fresh binding each time.
        if (self.check(TokenType.IDENTIFIER) and self.check_next(TokenType.IN)) or \
           (self.check(TokenType.VAR) and self.check_next(TokenType.IDENTIFIER)):
            saved = self.current
            self.match(TokenType.VAR)
            if self.check(TokenType.IDENTIFIER) and self.check_next(TokenType.IN):
                name = self.advance()
                self.advance()  # consume 'in'
                iterable = self.expression()
                self.consume(TokenType.RPAREN, "Expect ')' after for-in iterable.")
                body = self.statement()
                return ForIn(name, iterable, body)
            self.current = saved

        # Initializer
        initializer = None
        if self.match(TokenType.SEMICOLON):
            initializer = None
        elif self.match(TokenType.VAR):
            initializer = self.var_declaration()
        else:
            initializer = self.expression_statement()

        # Condition
        condition = None
        if not self.check(TokenType.SEMICOLON):
            condition = self.expression()
        self.consume(TokenType.SEMICOLON, "Expect ';' after loop condition.")

        # Increment
        increment = None
        if not self.check(TokenType.RPAREN):
            increment = self.expression()
        self.consume(TokenType.RPAREN, "Expect ')' after for clauses.")

        body = self.statement()

        # Kept as a dedicated node (rather than desugared into `while`) so
        # that `continue` inside the body still runs the increment step
        # before re-checking the condition.
        return For(initializer, condition, increment, body)

    def if_statement(self) -> If:
        self.consume(TokenType.LPAREN, "Expect '(' after 'if'.")
        condition = self.expression()
        self.consume(TokenType.RPAREN, "Expect ')' after if condition.")

        then_branch = self.statement()
        else_branch = None
        if self.match(TokenType.ELSE):
            else_branch = self.statement()

        return If(condition, then_branch, else_branch)

    def return_statement(self) -> Return:
        keyword = self.previous()
        value = None
        # A bare `return` is only ambiguous with `return <expr>` when there is
        # no semicolon; require the value to start on the same line (like
        # JavaScript's ASI rule for `return`) so `return\nfoo()` is parsed as
        # two statements rather than swallowing `foo()` as the return value.
        if (not self.check(TokenType.SEMICOLON)
                and not self.check(TokenType.RBRACE)
                and not self.is_at_end()
                and self.peek().line == keyword.line):
            value = self.expression()

        self.consume_statement_end("Expect ';' after return value.")
        return Return(keyword, value)

    def while_statement(self) -> While:
        self.consume(TokenType.LPAREN, "Expect '(' after 'while'.")
        condition = self.expression()
        self.consume(TokenType.RPAREN, "Expect ')' after condition.")
        body = self.statement()

        return While(condition, body)

    def break_statement(self) -> Break:
        keyword = self.previous()
        self.consume_statement_end("Expect ';' after 'break'.")
        return Break(keyword)

    def continue_statement(self) -> Continue:
        keyword = self.previous()
        self.consume_statement_end("Expect ';' after 'continue'.")
        return Continue(keyword)

    def try_statement(self) -> Try:
        keyword = self.previous()
        self.consume(TokenType.LBRACE, "Expect '{' after 'try'.")
        try_block = Block(self.block())

        catches: List[Catch] = []
        while self.match(TokenType.CATCH):
            self.consume(TokenType.LPAREN, "Expect '(' after 'catch'.")
            name = self.consume(TokenType.IDENTIFIER, "Expect variable name in catch.")
            self.consume(TokenType.RPAREN, "Expect ')' after catch variable.")

            # An optional guard: `catch (e) if (cond) { ... }`.
            guard = None
            if self.match(TokenType.IF):
                self.consume(TokenType.LPAREN, "Expect '(' after 'if' in catch guard.")
                guard = self.expression()
                self.consume(TokenType.RPAREN, "Expect ')' after catch guard.")

            self.consume(TokenType.LBRACE, "Expect '{' after catch.")
            catches.append(Catch(name, guard, Block(self.block())))

        finally_block = None
        if self.match(TokenType.FINALLY):
            self.consume(TokenType.LBRACE, "Expect '{' after 'finally'.")
            finally_block = Block(self.block())

        if not catches and finally_block is None:
            raise self.error(keyword, "Expect 'catch' or 'finally' after 'try' block.")

        return Try(try_block, catches, finally_block)

    def throw_statement(self) -> Throw:
        keyword = self.previous()
        value = self.expression()
        self.consume_statement_end("Expect ';' after thrown value.")
        return Throw(keyword, value)

    def block(self) -> List[Stmt]:
        statements = []
        self.block_depth += 1
        try:
            while not self.check(TokenType.RBRACE) and not self.is_at_end():
                stmt = self.declaration()
                if stmt:
                    statements.append(stmt)
        finally:
            self.block_depth -= 1

        self.consume(TokenType.RBRACE, "Expect '}' after block.")
        return statements

    def expression_statement(self) -> Stmt:
        expr = self.expression()
        self.consume_statement_end("Expect ';' after expression.")
        return Expression(expr)

    def print_statement(self) -> Print:
        # `print` takes a parenthesized, comma-separated argument list, e.g.
        # `print("label:", value)` -- matching every example in the docs and
        # the bundled .mrt programs, several of which rely on multiple
        # arguments in a single print call.
        self.consume(TokenType.LPAREN, "Expect '(' after 'print'.")
        values = []
        if not self.check(TokenType.RPAREN):
            values.append(self.spread_or_expression())
            while self.match(TokenType.COMMA):
                values.append(self.spread_or_expression())
        self.consume(TokenType.RPAREN, "Expect ')' after print arguments.")
        self.consume_statement_end("Expect ';' after value.")
        return Print(values)

    def expression(self) -> Expr:
        return self.assignment()

    def assignment(self) -> Expr:
        expr = self.or_expression()

        if self.match(TokenType.ASSIGN):
            equals = self.previous()
            value = self.assignment()
            return self._make_assign_target(expr, equals, value)

        if self.match(*COMPOUND_ASSIGN_OPS.keys()):
            op_token = self.previous()
            base_type, base_lexeme = COMPOUND_ASSIGN_OPS[op_token.type]
            value = self.assignment()
            # Desugar `target += value` into `target = target + value`. For
            # an ArrayAssign target this evaluates the array/index
            # sub-expressions twice (once to read, once to write) -- fine
            # for the simple variable/literal indices idiomatic MRT code
            # uses, but not side-effect-safe for an index with a function
            # call in it.
            synthetic_operator = Token(base_type, base_lexeme, None, op_token.line)
            combined = Binary(expr, synthetic_operator, value)
            return self._make_assign_target(expr, op_token, combined)

        return expr

    def _make_assign_target(self, target: Expr, error_token: Token, value: Expr) -> Expr:
        if isinstance(target, Variable):
            return Assign(target.name, value)
        if isinstance(target, ArrayAccess):
            return ArrayAssign(target.array, target.index, value)
        raise self.error(error_token, "Invalid assignment target.")

    def or_expression(self) -> Expr:
        expr = self.and_expression()

        while self.match(TokenType.OR):
            operator = self.previous()
            right = self.and_expression()
            expr = Logical(expr, operator, right)

        return expr

    def and_expression(self) -> Expr:
        expr = self.equality()

        while self.match(TokenType.AND):
            operator = self.previous()
            right = self.equality()
            expr = Logical(expr, operator, right)

        return expr

    def equality(self) -> Expr:
        expr = self.comparison()

        while self.match(TokenType.NOT_EQUALS, TokenType.EQUALS):
            operator = self.previous()
            right = self.comparison()
            expr = Binary(expr, operator, right)

        return expr

    def comparison(self) -> Expr:
        expr = self.term()

        while self.match(TokenType.GREATER, TokenType.GREATER_EQUAL, TokenType.LESS, TokenType.LESS_EQUAL):
            operator = self.previous()
            right = self.term()
            expr = Binary(expr, operator, right)

        return expr

    def term(self) -> Expr:
        expr = self.factor()

        while self.match(TokenType.PLUS, TokenType.MINUS):
            operator = self.previous()
            right = self.factor()
            expr = Binary(expr, operator, right)

        return expr

    def factor(self) -> Expr:
        expr = self.unary()

        while self.match(TokenType.MULTIPLY, TokenType.DIVIDE, TokenType.MODULO):
            operator = self.previous()
            right = self.unary()
            expr = Binary(expr, operator, right)

        return expr

    def unary(self) -> Expr:
        if self.match(TokenType.MINUS, TokenType.NOT):
            operator = self.previous()
            right = self.unary()
            return Unary(operator, right)

        return self.call()

    def call(self) -> Expr:
        expr = self.primary()

        while True:
            if self.match(TokenType.LPAREN):
                expr = self.finish_call(expr)
            elif self.match(TokenType.LBRACKET):
                expr = self.array_access(expr)
            elif self.match(TokenType.DOT):
                name = self.consume(TokenType.IDENTIFIER, "Expect property name after '.'.")
                # `obj.name` is sugar for `obj["name"]`.
                expr = ArrayAccess(expr, Literal(name.lexeme))
            else:
                break

        return expr

    def finish_call(self, callee: Expr) -> Expr:
        arguments = []
        if not self.check(TokenType.RPAREN):
            while True:
                if len(arguments) >= 255:
                    self.error(self.peek(), "Can't have more than 255 arguments.")
                arguments.append(self.spread_or_expression())
                if not self.match(TokenType.COMMA):
                    break

        paren = self.consume(TokenType.RPAREN, "Expect ')' after arguments.")
        return Call(callee, paren, arguments)

    def spread_or_expression(self) -> Expr:
        """An argument or array element, which may be `...expr`."""
        if self.match(TokenType.ELLIPSIS):
            token = self.previous()
            return Spread(self.expression(), token)
        return self.expression()

    def array_access(self, expr: Expr) -> Expr:
        index = self.expression()
        self.consume(TokenType.RBRACKET, "Expect ']' after array index.")
        return ArrayAccess(expr, index)

    def primary(self) -> Expr:
        if self.match(TokenType.TRUE):
            return Literal(True)
        if self.match(TokenType.FALSE):
            return Literal(False)
        if self.match(TokenType.NULL):
            return Literal(None)
        if self.match(TokenType.NUMBER, TokenType.STRING):
            return Literal(self.previous().literal)
        if self.match(TokenType.TEMPLATE):
            return self.interpolation(self.previous())
        if self.match(TokenType.FUNC):
            return self.function_expression()
        if self.match(TokenType.IDENTIFIER):
            return Variable(self.previous())
        if self.match(TokenType.LPAREN):
            expr = self.expression()
            self.consume(TokenType.RPAREN, "Expect ')' after expression.")
            return Grouping(expr)
        if self.match(TokenType.LBRACKET):
            elements = []
            if not self.check(TokenType.RBRACKET):
                while True:
                    elements.append(self.spread_or_expression())
                    if not self.match(TokenType.COMMA):
                        break
            self.consume(TokenType.RBRACKET, "Expect ']' after array elements.")
            return Array(elements)
        if self.match(TokenType.LBRACE):
            pairs: List[tuple] = []
            if not self.check(TokenType.RBRACE):
                while True:
                    # A bareword key is shorthand for the *string* of that
                    # name: `{name: "Ada"}` means `{"name": "Ada"}`. To use a
                    # variable's value as the key instead, parenthesise it:
                    # `{(k): v}`.
                    if self.check(TokenType.IDENTIFIER) and self.check_next(TokenType.COLON):
                        key = Literal(self.advance().lexeme)
                    else:
                        key = self.expression()
                    self.consume(TokenType.COLON, "Expect ':' after dictionary key.")
                    value = self.expression()
                    pairs.append((key, value))
                    if not self.match(TokenType.COMMA):
                        break
            self.consume(TokenType.RBRACE, "Expect '}' after dictionary literal.")
            return DictLiteral(pairs)

        raise self.error(self.peek(), "Expect expression.")

    def interpolation(self, token: Token) -> Expr:
        """Turn a TEMPLATE token's parts into an `Interpolation` node.

        Each `${ ... }` part arrives from the lexer as raw source text, which
        is lexed and parsed here as a self-contained expression. Line numbers
        from that sub-lex are relative to the start of the fragment, so they
        are shifted onto the line the fragment actually appeared on and any
        error is re-raised against the outer program."""
        from .lexer import Lexer

        parts: List[Any] = []
        for part in token.literal:
            if part[0] == 'str':
                parts.append(part[1])
                continue

            _, source, line = part
            if not source.strip():
                raise self.error(token, "Empty interpolation: expected an expression inside '${}'.")

            try:
                sub_tokens = Lexer(source).scan_tokens()
            except MRTSyntaxError as e:
                raise MRTSyntaxError(e.message, line) from None

            for t in sub_tokens:
                t.line += line - 1

            sub_parser = Parser(sub_tokens)
            expr = sub_parser.expression()
            if not sub_parser.is_at_end():
                raise self.error(sub_parser.peek(),
                                 "Unexpected trailing tokens in interpolation.")
            parts.append(expr)

        return Interpolation(parts)

    def var_declaration(self) -> Var:
        name = self.consume(TokenType.IDENTIFIER, "Expect variable name.")

        initializer = None
        if self.match(TokenType.ASSIGN):
            initializer = self.expression()

        self.consume_statement_end("Expect ';' after variable declaration.")
        return Var(name, initializer)

    def match(self, *types: TokenType) -> bool:
        for type in types:
            if self.check(type):
                self.advance()
                return True
        return False

    def check(self, type: TokenType) -> bool:
        if self.is_at_end():
            return False
        return self.peek().type == type

    def check_next(self, type: TokenType) -> bool:
        """One token of lookahead past `peek()`, used where a construct can't
        be identified from its first token alone (`func name` vs `func(`,
        `for (x in` vs `for (x =`, a bareword object key vs an expression)."""
        if self.current + 1 >= len(self.tokens):
            return False
        return self.tokens[self.current + 1].type == type

    def advance(self) -> Token:
        if not self.is_at_end():
            self.current += 1
        return self.previous()

    def is_at_end(self) -> bool:
        return self.peek().type == TokenType.EOF

    def peek(self) -> Token:
        return self.tokens[self.current]

    def previous(self) -> Token:
        return self.tokens[self.current - 1]

    def consume(self, type: TokenType, message: str) -> Token:
        if self.check(type):
            return self.advance()
        raise self.error(self.peek(), message)

    def consume_statement_end(self, message: str):
        """Statement terminators are optional in MRT: consume a trailing
        ';' if one is present, but don't error when it's missing."""
        self.match(TokenType.SEMICOLON)

    def error(self, token: Token, message: str):
        if token.type == TokenType.EOF:
            where = "end of file"
        else:
            where = f"'{token.lexeme}'"
        return ParseError(f"Error at {where}: {message}", token.line)

    def synchronize(self):
        self.advance()

        while not self.is_at_end():
            if self.previous().type == TokenType.SEMICOLON:
                return

            match self.peek().type:
                case (TokenType.FUNC | TokenType.IF | TokenType.RETURN | TokenType.WHILE
                      | TokenType.VAR | TokenType.FOR | TokenType.TRY | TokenType.THROW
                      | TokenType.IMPORT | TokenType.EXPORT):
                    return

            self.advance()

class ParseError(MRTSyntaxError):
    pass
