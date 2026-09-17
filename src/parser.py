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
        # One flag per function body being parsed, set when a `yield` is
        # seen, so a function knows at parse time whether it is a generator.
        self.function_yields: List[bool] = []
        # True only while parsing the outermost expression of a statement,
        # which is the one place a `yield` may produce a value (see
        # `yield_expression`). Cleared on the way into any nested expression,
        # so `f(yield v)` is rejected while `x = yield v;` is not.
        self.statement_rhs = False

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
            if self.match(TokenType.STRUCT):
                return self.struct_declaration()
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

        # `import * as name from "..."` -- one object holding every export.
        if self.match(TokenType.MULTIPLY):
            self.consume_word("as", "Expect 'as' after '*' in an import.")
            alias = self.consume(TokenType.IDENTIFIER, "Expect a name after 'as'.")
            self.consume_word("from", "Expect 'from' after the import name.")
            specifier = self.consume(TokenType.STRING,
                                     "Expect a module path string after 'from'.")
            self.consume_statement_end("Expect ';' after import.")
            return Import([], specifier, keyword, alias)

        self.consume(TokenType.LBRACE, "Expect '{' after 'import'.")
        names: List[tuple] = []
        if not self.check(TokenType.RBRACE):
            while True:
                exported = self.consume(TokenType.IDENTIFIER, "Expect an imported name.")
                local = exported
                if self.match_word("as"):
                    local = self.consume(TokenType.IDENTIFIER, "Expect a local name after 'as'.")
                names.append((exported, local))
                if not self.match(TokenType.COMMA):
                    break
        self.consume(TokenType.RBRACE, "Expect '}' after imported names.")

        self.consume_word("from", "Expect 'from' after imported names.")
        specifier = self.consume(TokenType.STRING, "Expect a module path string after 'from'.")
        self.consume_statement_end("Expect ';' after import.")
        return Import(names, specifier, keyword)

    def export_declaration(self) -> Export:
        keyword = self.previous()
        if self.block_depth > 0:
            raise self.error(keyword, "'export' is only allowed at the top level of a file.")

        # `export { a, b as c };` or `export { a } from "./m.mrt";`
        if self.check(TokenType.LBRACE):
            self.advance()
            names: List[tuple] = []
            if not self.check(TokenType.RBRACE):
                while True:
                    local = self.consume(TokenType.IDENTIFIER, "Expect an exported name.")
                    exported = local
                    if self.match_word("as"):
                        exported = self.consume(TokenType.IDENTIFIER,
                                                "Expect a name after 'as'.")
                    names.append((local, exported))
                    if not self.match(TokenType.COMMA):
                        break
            self.consume(TokenType.RBRACE, "Expect '}' after exported names.")

            specifier = None
            if self.match_word("from"):
                specifier = self.consume(TokenType.STRING,
                                         "Expect a module path string after 'from'.")
            self.consume_statement_end("Expect ';' after export.")
            return ExportNames(names, specifier, keyword)

        if self.match(TokenType.STRUCT):
            declaration = self.struct_declaration()
            return Export(declaration, declaration.name)

        if self.check(TokenType.FUNC) and self.check_next(TokenType.IDENTIFIER):
            self.advance()
            declaration = self.function("function")
            return Export(declaration, declaration.name)
        if self.match(TokenType.VAR):
            declaration = self.var_declaration()
            if not isinstance(declaration.pattern, NamePattern):
                raise self.error(keyword,
                                 "Only a plain `var name` can be exported, not a destructuring one.")
            return Export(declaration, declaration.pattern.name)
        raise self.error(self.peek(),
                         "Expect a 'func', 'var' or 'struct' declaration, "
                         "or '{ names }', after 'export'.")

    def struct_declaration(self) -> StructDecl:
        """`struct Name { fieldList; methods... }`.

        Fields and methods may be interleaved; a member starting with `func`
        is a method and anything else is a comma-separated run of field
        names, each optionally with a default."""
        name = self.consume(TokenType.IDENTIFIER, "Expect struct name.")
        self.consume(TokenType.LBRACE, "Expect '{' before struct body.")

        fields: List[Param] = []
        methods: List[Function] = []
        seen_default = False

        self.block_depth += 1
        try:
            while not self.check(TokenType.RBRACE) and not self.is_at_end():
                if self.check(TokenType.FUNC):
                    self.advance()
                    methods.append(self.function("method"))
                    continue

                while True:
                    field = self.consume(TokenType.IDENTIFIER,
                                         "Expect a field name or a 'func' method.")
                    default = None
                    if self.match(TokenType.ASSIGN):
                        default = self.expression()
                        seen_default = True
                    elif seen_default:
                        raise self.error(
                            field,
                            "A field without a default can't follow one with a default value.")
                    fields.append(Param(NamePattern(field, default), False))
                    if not self.match(TokenType.COMMA):
                        break
                self.consume_statement_end("Expect ';' after struct fields.")
        finally:
            self.block_depth -= 1

        self.consume(TokenType.RBRACE, "Expect '}' after struct body.")

        names = [f.pattern.name.lexeme for f in fields]
        if len(set(names)) != len(names):
            raise self.error(name, f"Struct '{name.lexeme}' has a duplicate field name.")
        method_names = [m.name.lexeme for m in methods]
        if len(set(method_names)) != len(method_names):
            raise self.error(name, f"Struct '{name.lexeme}' has a duplicate method name.")
        clash = set(names) & set(method_names)
        if clash:
            raise self.error(
                name,
                f"Struct '{name.lexeme}' has a field and a method both named "
                f"'{sorted(clash)[0]}'.")

        return StructDecl(name, fields, methods)

    def function(self, kind: str) -> Function:
        name = self.consume(TokenType.IDENTIFIER, f"Expect {kind} name.")
        parameters = self.parameter_list(f"Expect '(' after {kind} name.")
        self.consume(TokenType.LBRACE, f"Expect '{{' before {kind} body.")
        self.function_yields.append(False)
        try:
            body = self.block()
            is_generator = self.function_yields[-1]
        finally:
            self.function_yields.pop()
        return Function(name, parameters, body, is_generator)

    # -- Binding patterns --------------------------------------------------

    def binding_pattern(self, what: str, allow_yield: bool = False) -> Pattern:
        """Parse the left-hand side of a binding: a name, `[...]` or `{...}`.

        `what` names the construct for error messages ("variable name",
        "parameter name", ...) so a malformed pattern still reads like the
        error the simple case would have produced.

        `allow_yield` permits `yield` as the pattern's own trailing default,
        which is how `var x = yield 1;` is parsed -- a `var` declaration's
        initializer arrives through that same slot. It is never passed down
        to nested patterns, so `var [a = yield 1] = xs;` stays an error."""
        if self.starts_pattern():
            if self.check(TokenType.LBRACKET):
                return self.array_pattern(allow_yield)
            return self.object_pattern(allow_yield)
        name = self.consume(TokenType.IDENTIFIER, f"Expect {what}.")
        return NamePattern(name, self.pattern_default(allow_yield))

    def starts_pattern(self) -> bool:
        """Whether the next tokens really open a destructuring pattern.

        A bare `[` or `{` isn't enough: `func main( {` is a missing paren, not
        an object pattern, and treating it as one would drag the syntax error
        onto whatever line the brace's contents happen to start on. Requiring
        a plausible first element keeps that error where the mistake is."""
        if self.check(TokenType.LBRACKET):
            return (self.check_next(TokenType.IDENTIFIER)
                    or self.check_next(TokenType.RBRACKET)
                    or self.check_next(TokenType.ELLIPSIS)
                    or self.check_next(TokenType.LBRACKET)
                    or self.check_next(TokenType.LBRACE))
        if self.check(TokenType.LBRACE):
            return (self.check_next(TokenType.IDENTIFIER)
                    or self.check_next(TokenType.RBRACE)
                    or self.check_next(TokenType.ELLIPSIS))
        return False

    def pattern_default(self, allow_yield: bool = False) -> Optional[Expr]:
        if self.match(TokenType.ASSIGN):
            if allow_yield and self.check(TokenType.YIELD):
                return self.yield_expression()
            return self.expression()
        return None

    def array_pattern(self, allow_yield: bool = False) -> ArrayPattern:
        token = self.consume(TokenType.LBRACKET, "Expect '[' to start a pattern.")
        elements: List[Pattern] = []
        rest = None
        if not self.check(TokenType.RBRACKET):
            while True:
                if self.match(TokenType.ELLIPSIS):
                    rest = self.consume(TokenType.IDENTIFIER,
                                        "Expect a name after '...' in a pattern.")
                    break
                elements.append(self.binding_pattern("a name in the pattern"))
                if not self.match(TokenType.COMMA):
                    break
        self.consume(TokenType.RBRACKET, "Expect ']' after array pattern.")
        return ArrayPattern(elements, rest, self.pattern_default(allow_yield), token)

    def object_pattern(self, allow_yield: bool = False) -> ObjectPattern:
        token = self.consume(TokenType.LBRACE, "Expect '{' to start a pattern.")
        entries: List[tuple] = []
        rest = None
        if not self.check(TokenType.RBRACE):
            while True:
                if self.match(TokenType.ELLIPSIS):
                    rest = self.consume(TokenType.IDENTIFIER,
                                        "Expect a name after '...' in a pattern.")
                    break
                key = self.consume(TokenType.IDENTIFIER, "Expect a key name in the pattern.")
                if self.match(TokenType.COLON):
                    # `{key: <pattern>}` -- bind the key to something else.
                    entries.append((key.lexeme, self.binding_pattern("a name in the pattern")))
                else:
                    # `{key}` shorthand, optionally `{key = default}`.
                    entries.append((key.lexeme, NamePattern(key, self.pattern_default())))
                if not self.match(TokenType.COMMA):
                    break
        self.consume(TokenType.RBRACE, "Expect '}' after object pattern.")
        return ObjectPattern(entries, rest, self.pattern_default(allow_yield), token)

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
                if is_rest:
                    seen_rest = True
                    name = self.consume(TokenType.IDENTIFIER, "Expect parameter name.")
                    if self.check(TokenType.ASSIGN):
                        raise self.error(self.peek(),
                                         "A rest parameter can't have a default value.")
                    parameters.append(Param(NamePattern(name), True))
                else:
                    start = self.peek()
                    pattern = self.binding_pattern("parameter name")
                    if pattern.default is not None:
                        seen_default = True
                    elif seen_default:
                        raise self.error(start,
                                         "A required parameter can't follow one with a default value.")
                    parameters.append(Param(pattern, False))
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
        self.function_yields.append(False)
        try:
            body = self.block()
            is_generator = self.function_yields[-1]
        finally:
            self.function_yields.pop()
        return FunctionExpr(parameters, body, name, is_generator)

    def statement(self) -> Stmt:
        destructured = self.try_destructuring_assignment()
        if destructured is not None:
            return destructured
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
        if self.match(TokenType.MATCH):
            return self.match_statement()
        if self.match(TokenType.TRY):
            return self.try_statement()
        if self.match(TokenType.THROW):
            return self.throw_statement()
        if self.match(TokenType.YIELD):
            return self.yield_statement()
        if self.match(TokenType.LBRACE):
            return Block(self.block())
        if self.match(TokenType.PRINT):
            return self.print_statement()
        return self.expression_statement()

    def try_destructuring_assignment(self) -> Optional[Stmt]:
        """Attempt to parse `<pattern> = expr;` -- assignment to existing
        variables through an array or object pattern.

        Only tried at the start of a statement, and speculatively: a leading
        `{` is far more often a block, and `[` an array literal. Both forms
        parse as a binding pattern whose trailing default *is* the assigned
        value, so when there is no `= ...` this was something else and the
        tokens are handed back untouched."""
        if not self.starts_pattern():
            return None

        saved = self.current
        token = self.peek()
        try:
            pattern = self.binding_pattern("a name in the pattern", allow_yield=True)
        except ParseError:
            self.current = saved
            return None

        if pattern.default is None:
            self.current = saved
            return None

        value = pattern.default
        pattern.default = None
        self.consume_statement_end("Expect ';' after assignment.")
        return DestructureAssign(pattern, value, token)

    def for_statement(self) -> Stmt:
        self.consume(TokenType.LPAREN, "Expect '(' after 'for'.")

        # `for (x in xs)` / `for (var x in xs)` -- decided by lookahead so the
        # C-style three-clause form below is untouched. Both spellings mean
        # the same thing; `var` is allowed because it reads naturally and
        # because the loop variable really is a fresh binding each time.
        saved = self.current
        for_in = self.try_for_in()
        if for_in is not None:
            return for_in
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

    def try_for_in(self) -> Optional[ForIn]:
        """Attempt to parse the `for (<pattern> in expr)` form.

        Returns None (with the caller restoring the token position) when this
        is really the C-style three-clause loop. A pattern can start with `[`
        or `{`, which no C-style initializer does, so speculative parsing is
        simpler here than deeper lookahead -- and cheap, since it bails at the
        first token that doesn't fit."""
        keyword = self.previous()
        self.match(TokenType.VAR)
        if not (self.check(TokenType.IDENTIFIER) or self.starts_pattern()):
            return None
        try:
            pattern = self.binding_pattern("loop variable name")
        except ParseError:
            return None
        if pattern.default is not None or not self.match(TokenType.IN):
            return None
        iterable = self.expression()
        self.consume(TokenType.RPAREN, "Expect ')' after for-in iterable.")
        body = self.statement()
        return ForIn(pattern, iterable, keyword, body)

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

    # -- match --------------------------------------------------------------

    def match_statement(self) -> Match:
        keyword = self.previous()
        self.consume(TokenType.LPAREN, "Expect '(' after 'match'.")
        subject = self.expression()
        self.consume(TokenType.RPAREN, "Expect ')' after the match subject.")
        self.consume(TokenType.LBRACE, "Expect '{' before match cases.")

        cases: List[MatchCase] = []
        seen_default = False
        self.block_depth += 1
        try:
            while not self.check(TokenType.RBRACE) and not self.is_at_end():
                if self.match(TokenType.DEFAULT):
                    case_keyword = self.previous()
                    if seen_default:
                        raise self.error(case_keyword, "A match can only have one 'default'.")
                    seen_default = True
                    self.consume(TokenType.COLON, "Expect ':' after 'default'.")
                    cases.append(MatchCase(None, None, self.case_body(), case_keyword))
                    continue

                self.consume(TokenType.CASE, "Expect 'case' or 'default' in a match.")
                case_keyword = self.previous()
                if seen_default:
                    raise self.error(case_keyword, "'default' must be the last clause of a match.")
                pattern = self.match_pattern()
                guard = None
                if self.match(TokenType.IF):
                    self.consume(TokenType.LPAREN, "Expect '(' after 'if' in a case guard.")
                    guard = self.expression()
                    self.consume(TokenType.RPAREN, "Expect ')' after the case guard.")
                self.consume(TokenType.COLON, "Expect ':' after the case pattern.")
                cases.append(MatchCase(pattern, guard, self.case_body(), case_keyword))
        finally:
            self.block_depth -= 1

        self.consume(TokenType.RBRACE, "Expect '}' after match cases.")
        if not cases:
            raise self.error(keyword, "A match needs at least one case.")
        return Match(subject, cases, keyword)

    def match_expression(self) -> MatchExpr:
        """`match (subject) { case <pattern>: <expr>, default: <expr> }`.

        Arms are separated by commas and each one is a single expression, so
        the whole construct reads as the value it produces. The statement
        form above is what `match` means in statement position; this one is
        reached only from `primary`, where a statement can't start."""
        keyword = self.previous()
        self.consume(TokenType.LPAREN, "Expect '(' after 'match'.")
        subject = self.expression()
        self.consume(TokenType.RPAREN, "Expect ')' after the match subject.")
        self.consume(TokenType.LBRACE, "Expect '{' before match cases.")

        arms: List[MatchArm] = []
        seen_default = False
        while not self.check(TokenType.RBRACE) and not self.is_at_end():
            if self.match(TokenType.DEFAULT):
                arm_keyword = self.previous()
                if seen_default:
                    raise self.error(arm_keyword, "A match can only have one 'default'.")
                seen_default = True
                self.consume(TokenType.COLON, "Expect ':' after 'default'.")
                arms.append(MatchArm(None, None, self.expression(), arm_keyword))
            else:
                self.consume(TokenType.CASE, "Expect 'case' or 'default' in a match.")
                arm_keyword = self.previous()
                if seen_default:
                    raise self.error(arm_keyword,
                                     "'default' must be the last clause of a match.")
                pattern = self.match_pattern()
                guard = None
                if self.match(TokenType.IF):
                    self.consume(TokenType.LPAREN, "Expect '(' after 'if' in a case guard.")
                    guard = self.expression()
                    self.consume(TokenType.RPAREN, "Expect ')' after the case guard.")
                self.consume(TokenType.COLON, "Expect ':' after the case pattern.")
                arms.append(MatchArm(pattern, guard, self.expression(), arm_keyword))

            if not self.match(TokenType.COMMA):
                break

        self.consume(TokenType.RBRACE, "Expect '}' after match cases.")
        if not arms:
            raise self.error(keyword, "A match needs at least one case.")
        return MatchExpr(subject, arms, keyword)

    def case_body(self) -> List[Stmt]:
        """Statements up to the next `case`/`default`/`}`.

        There is no fall-through, so a case ends where the next one begins and
        needs no `break`."""
        statements: List[Stmt] = []
        while not (self.check(TokenType.CASE) or self.check(TokenType.DEFAULT)
                   or self.check(TokenType.RBRACE) or self.is_at_end()):
            stmt = self.declaration()
            if stmt:
                statements.append(stmt)
        return statements

    def match_pattern(self) -> MatchPattern:
        # Literals match by value.
        if self.match(TokenType.NUMBER, TokenType.STRING):
            return LiteralMatch(self.previous().literal)
        if self.match(TokenType.TRUE):
            return LiteralMatch(True)
        if self.match(TokenType.FALSE):
            return LiteralMatch(False)
        if self.match(TokenType.NULL):
            return LiteralMatch(None)
        if self.match(TokenType.MINUS):
            number = self.consume(TokenType.NUMBER, "Expect a number after '-' in a pattern.")
            return LiteralMatch(-number.literal)

        if self.check(TokenType.LBRACKET):
            return self.array_match()
        if self.check(TokenType.LBRACE):
            return self.object_match()

        if self.check(TokenType.IDENTIFIER):
            name = self.advance()
            if self.match(TokenType.LPAREN):
                # `Point(x, y)` -- an instance of that struct.
                elements: List[MatchPattern] = []
                if not self.check(TokenType.RPAREN):
                    while True:
                        elements.append(self.match_pattern())
                        if not self.match(TokenType.COMMA):
                            break
                self.consume(TokenType.RPAREN, "Expect ')' after struct pattern fields.")
                return StructMatch(name, elements)
            return BindMatch(name)

        raise self.error(self.peek(), "Expect a pattern after 'case'.")

    def array_match(self) -> ArrayMatch:
        token = self.consume(TokenType.LBRACKET, "Expect '[' to start a pattern.")
        elements: List[MatchPattern] = []
        rest = None
        if not self.check(TokenType.RBRACKET):
            while True:
                if self.match(TokenType.ELLIPSIS):
                    rest = self.consume(TokenType.IDENTIFIER,
                                        "Expect a name after '...' in a pattern.")
                    break
                elements.append(self.match_pattern())
                if not self.match(TokenType.COMMA):
                    break
        self.consume(TokenType.RBRACKET, "Expect ']' after array pattern.")
        return ArrayMatch(elements, rest, token)

    def object_match(self) -> ObjectMatch:
        token = self.consume(TokenType.LBRACE, "Expect '{' to start a pattern.")
        entries: List[tuple] = []
        if not self.check(TokenType.RBRACE):
            while True:
                key = self.consume(TokenType.IDENTIFIER, "Expect a key name in the pattern.")
                if self.match(TokenType.COLON):
                    entries.append((key.lexeme, self.match_pattern()))
                else:
                    entries.append((key.lexeme, BindMatch(key)))
                if not self.match(TokenType.COMMA):
                    break
        self.consume(TokenType.RBRACE, "Expect '}' after object pattern.")
        return ObjectMatch(entries, token)

    def try_statement(self) -> Try:
        keyword = self.previous()
        self.consume(TokenType.LBRACE, "Expect '{' after 'try'.")
        try_block = Block(self.block())

        catches: List[Catch] = []
        while self.match(TokenType.CATCH):
            self.consume(TokenType.LPAREN, "Expect '(' after 'catch'.")
            pattern = self.binding_pattern("variable name in catch")
            self.consume(TokenType.RPAREN, "Expect ')' after catch variable.")

            # An optional guard: `catch (e) if (cond) { ... }`.
            guard = None
            if self.match(TokenType.IF):
                self.consume(TokenType.LPAREN, "Expect '(' after 'if' in catch guard.")
                guard = self.expression()
                self.consume(TokenType.RPAREN, "Expect ')' after catch guard.")

            self.consume(TokenType.LBRACE, "Expect '{' after catch.")
            catches.append(Catch(pattern, guard, Block(self.block())))

        finally_block = None
        if self.match(TokenType.FINALLY):
            self.consume(TokenType.LBRACE, "Expect '{' after 'finally'.")
            finally_block = Block(self.block())

        if not catches and finally_block is None:
            raise self.error(keyword, "Expect 'catch' or 'finally' after 'try' block.")

        return Try(try_block, catches, finally_block)

    def yield_statement(self) -> Yield:
        keyword = self.previous()
        if not self.function_yields:
            raise self.error(keyword, "'yield' is only allowed inside a function.")
        self.function_yields[-1] = True
        # `yield* other` re-yields every item of another iterable. Spelled
        # with the existing `*` token rather than a keyword of its own, so
        # the lexer stays untouched.
        delegate = self.match(TokenType.MULTIPLY)
        value = self.expression()
        self.consume_statement_end("Expect ';' after the yielded value.")
        return Yield(keyword, value, delegate)

    def yield_expression(self) -> Expr:
        """`yield expr` used for its value, which the parser only ever calls
        in the two places that can hold one: a `var` initializer and the
        right-hand side of an assignment."""
        keyword = self.consume(TokenType.YIELD, "Expect 'yield'.")
        if not self.function_yields:
            raise self.error(keyword, "'yield' is only allowed inside a function.")
        self.function_yields[-1] = True
        if self.check(TokenType.MULTIPLY):
            raise self.error(
                self.peek(),
                "'yield*' re-yields a whole sequence and has no value of its own; "
                "use it as a statement.")
        return YieldExpr(keyword, self.expression())

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
        # The outermost expression of a statement is the one place an
        # assignment may take a `yield` as its value.
        self.statement_rhs = True
        try:
            expr = self.expression()
        finally:
            self.statement_rhs = False
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
        # Consumed here rather than read where it is needed: descending into
        # any sub-expression must clear it, and every sub-expression comes
        # back through this method.
        statement_rhs = self.statement_rhs
        self.statement_rhs = False

        expr = self.or_expression()

        if self.match(TokenType.ASSIGN):
            equals = self.previous()
            if statement_rhs and self.check(TokenType.YIELD):
                value = self.yield_expression()
            else:
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
        if self.match(TokenType.MATCH):
            return self.match_expression()
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
        pattern = self.binding_pattern("variable name", allow_yield=True)

        # `var [a, b] = ...` parses its own `=` as part of the pattern's
        # default slot, so only take another initializer when the pattern
        # didn't already consume one.
        initializer = pattern.default
        if initializer is None and self.match(TokenType.ASSIGN):
            if self.check(TokenType.YIELD):
                initializer = self.yield_expression()
            else:
                initializer = self.expression()
        if initializer is not None and pattern.default is not None:
            pattern.default = None

        if initializer is None and not isinstance(pattern, NamePattern):
            raise self.error(self.previous(),
                             "A destructuring declaration needs an initializer.")

        self.consume_statement_end("Expect ';' after variable declaration.")
        return Var(pattern, initializer)

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

    def match_word(self, word: str) -> bool:
        """Consume an identifier spelled exactly `word`.

        `from` and `as` are *contextual* keywords: they only mean anything
        inside an import or export clause, so the lexer leaves them as
        ordinary identifiers and a program is free to use them as a variable,
        a struct field or a bareword object key."""
        if self.check(TokenType.IDENTIFIER) and self.peek().lexeme == word:
            self.advance()
            return True
        return False

    def consume_word(self, word: str, message: str) -> Token:
        if not self.match_word(word):
            raise self.error(self.peek(), message)
        return self.previous()

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
                      | TokenType.IMPORT | TokenType.EXPORT | TokenType.STRUCT
                      | TokenType.MATCH | TokenType.YIELD):
                    return

            self.advance()

class ParseError(MRTSyntaxError):
    pass
