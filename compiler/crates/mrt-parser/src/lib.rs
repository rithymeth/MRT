//! The MRT parser: a recursive-descent parser mirroring `mrt/parser.py`.
//!
//! Like the reference implementation it recovers from errors rather than
//! stopping at the first one -- a file with three mistakes reports three --
//! and it makes the same decisions in the same places. The interesting ones,
//! all inherited:
//!
//! * **`func` is a declaration only when followed by an identifier.** A bare
//!   `func(` in statement position is an anonymous function expression.
//! * **`for (` and a leading `[`/`{` are resolved by speculative parsing**,
//!   not lookahead, because a pattern can be arbitrarily long. The parser
//!   rewinds when the guess does not pan out, which is what keeps `{ x = 1; }`
//!   a block and `[1, 2][0];` an expression.
//! * **A leading `[` or `{` only opens a pattern when what follows could
//!   plausibly be one.** Without that, `func main( {` -- a missing paren --
//!   reports its error wherever the brace's contents happen to start.
//! * **`yield` produces a value in exactly four statement shapes.** One flag,
//!   cleared on entry to every nested expression, enforces it, so `f(yield 1)`
//!   is a parse error rather than something the runtime must reject.

use mrt_ast::*;
use mrt_diagnostics::{Diagnostic, SourceFile, Span, Stage};
use mrt_lexer::{lex, Literal, TemplatePart, Token, TokenKind};

type PResult<T> = Result<T, Diagnostic>;

pub struct Parsed {
    pub program: Program,
    pub errors: Vec<Diagnostic>,
}

pub fn parse(file: &SourceFile) -> Parsed {
    match lex(file) {
        Ok(tokens) => Parser::new(tokens).parse(),
        Err(diagnostic) => Parsed {
            program: Program { statements: vec![] },
            errors: vec![diagnostic],
        },
    }
}

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
    errors: Vec<Diagnostic>,
    /// `import`/`export` are only meaningful at the top level of a file, so
    /// the parser tracks how deep into blocks it currently is.
    block_depth: u32,
    /// One flag per function body being parsed, set when a `yield` is seen, so
    /// a function knows at parse time whether it is a generator.
    function_yields: Vec<bool>,
    /// True only while parsing the outermost expression of a statement, which
    /// is the one place a `yield` may produce a value.
    statement_rhs: bool,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            current: 0,
            errors: Vec::new(),
            block_depth: 0,
            function_yields: Vec::new(),
            statement_rhs: false,
        }
    }

    pub fn parse(mut self) -> Parsed {
        let mut statements = Vec::new();
        while !self.is_at_end() {
            match self.declaration() {
                Ok(Some(stmt)) => statements.push(stmt),
                Ok(None) => {}
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize();
                }
            }
        }
        Parsed {
            program: Program { statements },
            errors: self.errors,
        }
    }

    // -- declarations -------------------------------------------------------

    fn declaration(&mut self) -> PResult<Option<Stmt>> {
        if self.matches(&[TokenKind::Import]) {
            return self.import_statement().map(Some);
        }
        if self.matches(&[TokenKind::Export]) {
            return self.export_declaration().map(Some);
        }
        // `func name(...)` is a declaration; a bare `func(...)` in statement
        // position is an anonymous function expression.
        if self.check(TokenKind::Func) && self.check_next(TokenKind::Identifier) {
            self.advance();
            return self.function("function").map(Some);
        }
        if self.matches(&[TokenKind::Struct]) {
            return self.struct_declaration().map(Some);
        }
        if self.matches(&[TokenKind::Var]) {
            return self.var_declaration().map(Some);
        }
        self.statement().map(Some)
    }

    fn import_statement(&mut self) -> PResult<Stmt> {
        let keyword = self.previous().clone();
        if self.block_depth > 0 {
            return Err(self.error(
                &keyword,
                "'import' is only allowed at the top level of a file.",
            ));
        }

        if self.matches(&[TokenKind::Multiply]) {
            self.consume_word("as", "Expect 'as' after '*' in an import.")?;
            let alias = self.consume(TokenKind::Identifier, "Expect a name after 'as'.")?;
            self.consume_word("from", "Expect 'from' after the import name.")?;
            let specifier =
                self.consume(TokenKind::Str, "Expect a module path string after 'from'.")?;
            self.consume_statement_end();
            let span = keyword.span.to(specifier.span);
            return Ok(Stmt {
                kind: StmtKind::Import {
                    names: vec![],
                    namespace: Some(Name::from_token(&alias)),
                    specifier: string_literal(&specifier),
                },
                span,
                line: keyword.line,
            });
        }

        self.consume(TokenKind::LBrace, "Expect '{' after 'import'.")?;
        let mut names = Vec::new();
        if !self.check(TokenKind::RBrace) {
            loop {
                let exported = self.consume(TokenKind::Identifier, "Expect an imported name.")?;
                let local = if self.matches_word("as") {
                    self.consume(TokenKind::Identifier, "Expect a local name after 'as'.")?
                } else {
                    exported.clone()
                };
                names.push((Name::from_token(&exported), Name::from_token(&local)));
                if !self.matches(&[TokenKind::Comma]) {
                    break;
                }
            }
        }
        self.consume(TokenKind::RBrace, "Expect '}' after imported names.")?;
        self.consume_word("from", "Expect 'from' after imported names.")?;
        let specifier =
            self.consume(TokenKind::Str, "Expect a module path string after 'from'.")?;
        self.consume_statement_end();
        let span = keyword.span.to(specifier.span);
        Ok(Stmt {
            kind: StmtKind::Import {
                names,
                namespace: None,
                specifier: string_literal(&specifier),
            },
            span,
            line: keyword.line,
        })
    }

    fn export_declaration(&mut self) -> PResult<Stmt> {
        let keyword = self.previous().clone();
        if self.block_depth > 0 {
            return Err(self.error(
                &keyword,
                "'export' is only allowed at the top level of a file.",
            ));
        }

        if self.check(TokenKind::LBrace) {
            self.advance();
            let mut names = Vec::new();
            if !self.check(TokenKind::RBrace) {
                loop {
                    let local = self.consume(TokenKind::Identifier, "Expect an exported name.")?;
                    let exported = if self.matches_word("as") {
                        self.consume(TokenKind::Identifier, "Expect a name after 'as'.")?
                    } else {
                        local.clone()
                    };
                    names.push((Name::from_token(&local), Name::from_token(&exported)));
                    if !self.matches(&[TokenKind::Comma]) {
                        break;
                    }
                }
            }
            let close = self.consume(TokenKind::RBrace, "Expect '}' after exported names.")?;
            let mut specifier = None;
            let mut end = close.span;
            if self.matches_word("from") {
                let token =
                    self.consume(TokenKind::Str, "Expect a module path string after 'from'.")?;
                end = token.span;
                specifier = Some(string_literal(&token));
            }
            self.consume_statement_end();
            return Ok(Stmt {
                kind: StmtKind::ExportNames { names, specifier },
                span: keyword.span.to(end),
                line: keyword.line,
            });
        }

        if self.matches(&[TokenKind::Struct]) {
            let declaration = self.struct_declaration()?;
            let StmtKind::Struct { ref name, .. } = declaration.kind else {
                unreachable!("struct_declaration returns a Struct")
            };
            let name = name.clone();
            let span = keyword.span.to(declaration.span);
            return Ok(Stmt {
                kind: StmtKind::Export {
                    declaration: Box::new(declaration),
                    name,
                },
                span,
                line: keyword.line,
            });
        }

        if self.check(TokenKind::Func) && self.check_next(TokenKind::Identifier) {
            self.advance();
            let declaration = self.function("function")?;
            let StmtKind::Function { ref name, .. } = declaration.kind else {
                unreachable!("function returns a Function")
            };
            let name = name.clone();
            let span = keyword.span.to(declaration.span);
            return Ok(Stmt {
                kind: StmtKind::Export {
                    declaration: Box::new(declaration),
                    name,
                },
                span,
                line: keyword.line,
            });
        }

        if self.matches(&[TokenKind::Var]) {
            let declaration = self.var_declaration()?;
            let StmtKind::Var { ref pattern, .. } = declaration.kind else {
                unreachable!("var_declaration returns a Var")
            };
            let Pattern::Name { name, .. } = pattern else {
                return Err(self.error(
                    &keyword,
                    "Only a plain `var name` can be exported, not a destructuring one.",
                ));
            };
            let name = name.clone();
            let span = keyword.span.to(declaration.span);
            return Ok(Stmt {
                kind: StmtKind::Export {
                    declaration: Box::new(declaration),
                    name,
                },
                span,
                line: keyword.line,
            });
        }

        let peek = self.peek().clone();
        Err(self.error(
            &peek,
            "Expect a 'func', 'var' or 'struct' declaration, or '{ names }', after 'export'.",
        ))
    }

    fn struct_declaration(&mut self) -> PResult<Stmt> {
        let name_token = self.consume(TokenKind::Identifier, "Expect struct name.")?;
        self.consume(TokenKind::LBrace, "Expect '{' before struct body.")?;

        let mut fields: Vec<Param> = Vec::new();
        let mut methods: Vec<Stmt> = Vec::new();
        let mut seen_default = false;

        self.block_depth += 1;
        let result = (|| -> PResult<()> {
            while !self.check(TokenKind::RBrace) && !self.is_at_end() {
                if self.check(TokenKind::Func) {
                    self.advance();
                    let method = self.function("method")?;
                    methods.push(method);
                    continue;
                }
                loop {
                    let field = self.consume(
                        TokenKind::Identifier,
                        "Expect a field name or a 'func' method.",
                    )?;
                    let mut default = None;
                    if self.matches(&[TokenKind::Assign]) {
                        default = Some(Box::new(self.expression()?));
                        seen_default = true;
                    } else if seen_default {
                        return Err(self.error(
                            &field,
                            "A field without a default can't follow one with a default value.",
                        ));
                    }
                    fields.push(Param {
                        pattern: Pattern::Name {
                            name: Name::from_token(&field),
                            default,
                        },
                        rest: false,
                    });
                    if !self.matches(&[TokenKind::Comma]) {
                        break;
                    }
                }
                self.consume_statement_end();
            }
            Ok(())
        })();
        self.block_depth -= 1;
        result?;

        let close = self.consume(TokenKind::RBrace, "Expect '}' after struct body.")?;

        let field_names: Vec<&str> = fields
            .iter()
            .map(|f| match &f.pattern {
                Pattern::Name { name, .. } => name.text.as_str(),
                _ => unreachable!("struct fields are always plain names"),
            })
            .collect();
        if has_duplicate(&field_names) {
            return Err(self.error(
                &name_token,
                format!("Struct '{}' has a duplicate field name.", name_token.lexeme),
            ));
        }
        let method_names: Vec<&str> = methods
            .iter()
            .map(|m| match &m.kind {
                StmtKind::Function { name, .. } => name.text.as_str(),
                _ => unreachable!("methods are always functions"),
            })
            .collect();
        if has_duplicate(&method_names) {
            return Err(self.error(
                &name_token,
                format!(
                    "Struct '{}' has a duplicate method name.",
                    name_token.lexeme
                ),
            ));
        }
        let mut clash: Vec<&str> = field_names
            .iter()
            .filter(|f| method_names.contains(f))
            .copied()
            .collect();
        if !clash.is_empty() {
            clash.sort_unstable();
            return Err(self.error(
                &name_token,
                format!(
                    "Struct '{}' has a field and a method both named '{}'.",
                    name_token.lexeme, clash[0]
                ),
            ));
        }

        Ok(Stmt {
            kind: StmtKind::Struct {
                name: Name::from_token(&name_token),
                fields,
                methods,
            },
            span: name_token.span.to(close.span),
            line: name_token.line,
        })
    }

    fn function(&mut self, kind: &str) -> PResult<Stmt> {
        let name = self.consume(TokenKind::Identifier, format!("Expect {kind} name."))?;
        let params = self.parameter_list(format!("Expect '(' after {kind} name."))?;
        self.consume(
            TokenKind::LBrace,
            format!("Expect '{{' before {kind} body."),
        )?;
        self.function_yields.push(false);
        let body = self.block();
        let is_generator = self.function_yields.pop().unwrap_or(false);
        let (body, close) = body?;
        Ok(Stmt {
            kind: StmtKind::Function {
                name: Name::from_token(&name),
                params,
                body,
                is_generator,
            },
            span: name.span.to(close),
            line: name.line,
        })
    }

    // -- binding patterns ---------------------------------------------------

    fn binding_pattern(&mut self, what: &str, allow_yield: bool) -> PResult<Pattern> {
        if self.starts_pattern() {
            return if self.check(TokenKind::LBracket) {
                self.array_pattern(allow_yield)
            } else {
                self.object_pattern(allow_yield)
            };
        }
        let name = self.consume(TokenKind::Identifier, format!("Expect {what}."))?;
        let default = self.pattern_default(allow_yield)?;
        Ok(Pattern::Name {
            name: Name::from_token(&name),
            default,
        })
    }

    /// Whether the next tokens really open a destructuring pattern. A bare `[`
    /// or `{` is not enough: `func main( {` is a missing paren, not an object
    /// pattern, and treating it as one drags the error onto whatever line the
    /// brace's contents start on.
    fn starts_pattern(&self) -> bool {
        if self.check(TokenKind::LBracket) {
            return self.check_next(TokenKind::Identifier)
                || self.check_next(TokenKind::RBracket)
                || self.check_next(TokenKind::Ellipsis)
                || self.check_next(TokenKind::LBracket)
                || self.check_next(TokenKind::LBrace);
        }
        if self.check(TokenKind::LBrace) {
            return self.check_next(TokenKind::Identifier)
                || self.check_next(TokenKind::RBrace)
                || self.check_next(TokenKind::Ellipsis);
        }
        false
    }

    fn pattern_default(&mut self, allow_yield: bool) -> PResult<Option<Box<Expr>>> {
        if !self.matches(&[TokenKind::Assign]) {
            return Ok(None);
        }
        if allow_yield && self.check(TokenKind::Yield) {
            return Ok(Some(Box::new(self.yield_expression()?)));
        }
        Ok(Some(Box::new(self.expression()?)))
    }

    fn array_pattern(&mut self, allow_yield: bool) -> PResult<Pattern> {
        let open = self.consume(TokenKind::LBracket, "Expect '[' to start a pattern.")?;
        let mut elements = Vec::new();
        let mut rest = None;
        if !self.check(TokenKind::RBracket) {
            loop {
                if self.matches(&[TokenKind::Ellipsis]) {
                    let name = self.consume(
                        TokenKind::Identifier,
                        "Expect a name after '...' in a pattern.",
                    )?;
                    rest = Some(Name::from_token(&name));
                    break;
                }
                elements.push(self.binding_pattern("a name in the pattern", false)?);
                if !self.matches(&[TokenKind::Comma]) {
                    break;
                }
            }
        }
        let close = self.consume(TokenKind::RBracket, "Expect ']' after array pattern.")?;
        let default = self.pattern_default(allow_yield)?;
        Ok(Pattern::Array {
            elements,
            rest,
            default,
            span: open.span.to(close.span),
            line: open.line,
        })
    }

    fn object_pattern(&mut self, allow_yield: bool) -> PResult<Pattern> {
        let open = self.consume(TokenKind::LBrace, "Expect '{' to start a pattern.")?;
        let mut entries = Vec::new();
        let mut rest = None;
        if !self.check(TokenKind::RBrace) {
            loop {
                if self.matches(&[TokenKind::Ellipsis]) {
                    let name = self.consume(
                        TokenKind::Identifier,
                        "Expect a name after '...' in a pattern.",
                    )?;
                    rest = Some(Name::from_token(&name));
                    break;
                }
                let key =
                    self.consume(TokenKind::Identifier, "Expect a key name in the pattern.")?;
                if self.matches(&[TokenKind::Colon]) {
                    let sub = self.binding_pattern("a name in the pattern", false)?;
                    entries.push((key.lexeme.clone(), sub));
                } else {
                    let default = self.pattern_default(false)?;
                    entries.push((
                        key.lexeme.clone(),
                        Pattern::Name {
                            name: Name::from_token(&key),
                            default,
                        },
                    ));
                }
                if !self.matches(&[TokenKind::Comma]) {
                    break;
                }
            }
        }
        let close = self.consume(TokenKind::RBrace, "Expect '}' after object pattern.")?;
        let default = self.pattern_default(allow_yield)?;
        Ok(Pattern::Object {
            entries,
            rest,
            default,
            span: open.span.to(close.span),
            line: open.line,
        })
    }

    /// Parse `(a, b = expr, ...rest)`. Two shape rules are enforced here
    /// rather than at run time, because they are always mistakes: a rest
    /// parameter must be last, and a required parameter may not follow a
    /// defaulted one.
    fn parameter_list(&mut self, lparen_message: String) -> PResult<Vec<Param>> {
        self.consume(TokenKind::LParen, lparen_message)?;
        let mut params: Vec<Param> = Vec::new();
        let mut seen_default = false;
        let mut seen_rest = false;

        if !self.check(TokenKind::RParen) {
            loop {
                if params.len() >= 255 {
                    let peek = self.peek().clone();
                    let e = self.error(&peek, "Can't have more than 255 parameters.");
                    self.errors.push(e);
                }
                if seen_rest {
                    let peek = self.peek().clone();
                    return Err(self.error(&peek, "A rest parameter must be the last parameter."));
                }
                if self.matches(&[TokenKind::Ellipsis]) {
                    seen_rest = true;
                    let name = self.consume(TokenKind::Identifier, "Expect parameter name.")?;
                    if self.check(TokenKind::Assign) {
                        let peek = self.peek().clone();
                        return Err(
                            self.error(&peek, "A rest parameter can't have a default value.")
                        );
                    }
                    params.push(Param {
                        pattern: Pattern::Name {
                            name: Name::from_token(&name),
                            default: None,
                        },
                        rest: true,
                    });
                } else {
                    let start = self.peek().clone();
                    let pattern = self.binding_pattern("parameter name", false)?;
                    if pattern.default().is_some() {
                        seen_default = true;
                    } else if seen_default {
                        return Err(self.error(
                            &start,
                            "A required parameter can't follow one with a default value.",
                        ));
                    }
                    params.push(Param {
                        pattern,
                        rest: false,
                    });
                }
                if !self.matches(&[TokenKind::Comma]) {
                    break;
                }
            }
        }
        self.consume(TokenKind::RParen, "Expect ')' after parameters.")?;
        Ok(params)
    }

    fn function_expression(&mut self) -> PResult<Expr> {
        let start = self.previous().span;
        let line = self.previous().line;
        let name = if self.check(TokenKind::Identifier) {
            Some(Name::from_token(&self.advance().clone()))
        } else {
            None
        };
        let params = self.parameter_list("Expect '(' after 'func'.".to_string())?;
        self.consume(TokenKind::LBrace, "Expect '{' before function body.")?;
        self.function_yields.push(false);
        let body = self.block();
        let is_generator = self.function_yields.pop().unwrap_or(false);
        let (body, close) = body?;
        Ok(Expr {
            kind: ExprKind::Function {
                name,
                params,
                body,
                is_generator,
            },
            span: start.to(close),
            line,
        })
    }

    // -- statements ---------------------------------------------------------

    fn statement(&mut self) -> PResult<Stmt> {
        if let Some(stmt) = self.try_destructuring_assignment()? {
            return Ok(stmt);
        }
        if self.matches(&[TokenKind::For]) {
            return self.for_statement();
        }
        if self.matches(&[TokenKind::If]) {
            return self.if_statement();
        }
        if self.matches(&[TokenKind::Return]) {
            return self.return_statement();
        }
        if self.matches(&[TokenKind::While]) {
            return self.while_statement();
        }
        if self.matches(&[TokenKind::Break]) {
            let keyword = self.previous().clone();
            self.consume_statement_end();
            return Ok(Stmt {
                kind: StmtKind::Break,
                span: keyword.span,
                line: keyword.line,
            });
        }
        if self.matches(&[TokenKind::Continue]) {
            let keyword = self.previous().clone();
            self.consume_statement_end();
            return Ok(Stmt {
                kind: StmtKind::Continue,
                span: keyword.span,
                line: keyword.line,
            });
        }
        if self.matches(&[TokenKind::Match]) {
            return self.match_statement();
        }
        if self.matches(&[TokenKind::Try]) {
            return self.try_statement();
        }
        if self.matches(&[TokenKind::Throw]) {
            let keyword = self.previous().clone();
            let value = self.expression()?;
            self.consume_statement_end();
            let span = keyword.span.to(value.span);
            return Ok(Stmt {
                kind: StmtKind::Throw(value),
                span,
                line: keyword.line,
            });
        }
        if self.matches(&[TokenKind::Yield]) {
            return self.yield_statement();
        }
        if self.matches(&[TokenKind::LBrace]) {
            let open = self.previous().span;
            let (statements, close) = self.block()?;
            return Ok(Stmt {
                kind: StmtKind::Block(statements),
                span: open.to(close),
                line: self.line_of(open),
            });
        }
        if self.matches(&[TokenKind::Print]) {
            return self.print_statement();
        }
        self.expression_statement()
    }

    /// Attempt to parse `<pattern> = expr;`. Speculative: a leading `{` is far
    /// more often a block and `[` an array literal, so when there is no
    /// `= ...` the tokens are handed back untouched.
    fn try_destructuring_assignment(&mut self) -> PResult<Option<Stmt>> {
        if !self.starts_pattern() {
            return Ok(None);
        }
        let saved = self.current;
        let saved_errors = self.errors.len();
        let start = self.peek().clone();

        let mut pattern = match self.binding_pattern("a name in the pattern", true) {
            Ok(pattern) => pattern,
            Err(_) => {
                self.current = saved;
                self.errors.truncate(saved_errors);
                return Ok(None);
            }
        };
        let Some(value) = pattern.take_default() else {
            self.current = saved;
            self.errors.truncate(saved_errors);
            return Ok(None);
        };
        self.consume_statement_end();
        let span = start.span.to(value.span);
        Ok(Some(Stmt {
            kind: StmtKind::DestructureAssign {
                pattern,
                value: *value,
            },
            span,
            line: start.line,
        }))
    }

    fn for_statement(&mut self) -> PResult<Stmt> {
        let keyword = self.previous().clone();
        self.consume(TokenKind::LParen, "Expect '(' after 'for'.")?;

        // `for (<pattern> in xs)` -- parsed speculatively, since a pattern can
        // start with `[` or `{`, which no C-style initializer does.
        let saved = self.current;
        let saved_errors = self.errors.len();
        if let Some(for_in) = self.try_for_in(&keyword)? {
            return Ok(for_in);
        }
        self.current = saved;
        self.errors.truncate(saved_errors);

        let initializer = if self.matches(&[TokenKind::Semicolon]) {
            None
        } else if self.matches(&[TokenKind::Var]) {
            Some(Box::new(self.var_declaration()?))
        } else {
            Some(Box::new(self.expression_statement()?))
        };

        let condition = if self.check(TokenKind::Semicolon) {
            None
        } else {
            Some(self.expression()?)
        };
        self.consume(TokenKind::Semicolon, "Expect ';' after loop condition.")?;

        let increment = if self.check(TokenKind::RParen) {
            None
        } else {
            Some(self.expression()?)
        };
        self.consume(TokenKind::RParen, "Expect ')' after for clauses.")?;

        let body = self.statement()?;
        let span = keyword.span.to(body.span);
        Ok(Stmt {
            kind: StmtKind::For {
                initializer,
                condition,
                increment,
                body: Box::new(body),
            },
            span,
            line: keyword.line,
        })
    }

    fn try_for_in(&mut self, keyword: &Token) -> PResult<Option<Stmt>> {
        self.matches(&[TokenKind::Var]);
        if !(self.check(TokenKind::Identifier) || self.starts_pattern()) {
            return Ok(None);
        }
        let Ok(pattern) = self.binding_pattern("loop variable name", false) else {
            return Ok(None);
        };
        if pattern.default().is_some() || !self.matches(&[TokenKind::In]) {
            return Ok(None);
        }
        let iterable = self.expression()?;
        self.consume(TokenKind::RParen, "Expect ')' after for-in iterable.")?;
        let body = self.statement()?;
        let span = keyword.span.to(body.span);
        Ok(Some(Stmt {
            kind: StmtKind::ForIn {
                pattern,
                iterable,
                body: Box::new(body),
            },
            span,
            line: keyword.line,
        }))
    }

    fn if_statement(&mut self) -> PResult<Stmt> {
        let keyword = self.previous().clone();
        self.consume(TokenKind::LParen, "Expect '(' after 'if'.")?;
        let condition = self.expression()?;
        self.consume(TokenKind::RParen, "Expect ')' after if condition.")?;
        let then_branch = self.statement()?;
        let mut end = then_branch.span;
        let else_branch = if self.matches(&[TokenKind::Else]) {
            let branch = self.statement()?;
            end = branch.span;
            Some(Box::new(branch))
        } else {
            None
        };
        Ok(Stmt {
            kind: StmtKind::If {
                condition,
                then_branch: Box::new(then_branch),
                else_branch,
            },
            span: keyword.span.to(end),
            line: keyword.line,
        })
    }

    fn return_statement(&mut self) -> PResult<Stmt> {
        let keyword = self.previous().clone();
        let mut end = keyword.span;
        // A bare `return` is only ambiguous with `return <expr>` when there is
        // no semicolon; require the value to start on the same line, so
        // `return\nfoo()` is two statements.
        let value = if !self.check(TokenKind::Semicolon)
            && !self.check(TokenKind::RBrace)
            && !self.is_at_end()
            && self.peek().line == keyword.line
        {
            let expr = self.expression()?;
            end = expr.span;
            Some(expr)
        } else {
            None
        };
        self.consume_statement_end();
        Ok(Stmt {
            kind: StmtKind::Return(value),
            span: keyword.span.to(end),
            line: keyword.line,
        })
    }

    fn while_statement(&mut self) -> PResult<Stmt> {
        let keyword = self.previous().clone();
        self.consume(TokenKind::LParen, "Expect '(' after 'while'.")?;
        let condition = self.expression()?;
        self.consume(TokenKind::RParen, "Expect ')' after condition.")?;
        let body = self.statement()?;
        let span = keyword.span.to(body.span);
        Ok(Stmt {
            kind: StmtKind::While {
                condition,
                body: Box::new(body),
            },
            span,
            line: keyword.line,
        })
    }

    // -- match --------------------------------------------------------------

    fn match_statement(&mut self) -> PResult<Stmt> {
        let keyword = self.previous().clone();
        self.consume(TokenKind::LParen, "Expect '(' after 'match'.")?;
        let subject = self.expression()?;
        self.consume(TokenKind::RParen, "Expect ')' after the match subject.")?;
        self.consume(TokenKind::LBrace, "Expect '{' before match cases.")?;

        let mut cases: Vec<MatchCase> = Vec::new();
        let mut seen_default = false;
        self.block_depth += 1;
        let result = (|| -> PResult<()> {
            while !self.check(TokenKind::RBrace) && !self.is_at_end() {
                if self.matches(&[TokenKind::Default]) {
                    let case_keyword = self.previous().clone();
                    if seen_default {
                        return Err(
                            self.error(&case_keyword, "A match can only have one 'default'.")
                        );
                    }
                    seen_default = true;
                    self.consume(TokenKind::Colon, "Expect ':' after 'default'.")?;
                    let body = self.case_body()?;
                    cases.push(MatchCase {
                        pattern: None,
                        guard: None,
                        body,
                        line: case_keyword.line,
                    });
                    continue;
                }
                self.consume(TokenKind::Case, "Expect 'case' or 'default' in a match.")?;
                let case_keyword = self.previous().clone();
                if seen_default {
                    return Err(self.error(
                        &case_keyword,
                        "'default' must be the last clause of a match.",
                    ));
                }
                let pattern = self.match_pattern()?;
                let guard = self.case_guard()?;
                self.consume(TokenKind::Colon, "Expect ':' after the case pattern.")?;
                let body = self.case_body()?;
                cases.push(MatchCase {
                    pattern: Some(pattern),
                    guard,
                    body,
                    line: case_keyword.line,
                });
            }
            Ok(())
        })();
        self.block_depth -= 1;
        result?;

        let close = self.consume(TokenKind::RBrace, "Expect '}' after match cases.")?;
        if cases.is_empty() {
            return Err(self.error(&keyword, "A match needs at least one case."));
        }
        Ok(Stmt {
            kind: StmtKind::Match { subject, cases },
            span: keyword.span.to(close.span),
            line: keyword.line,
        })
    }

    fn match_expression(&mut self) -> PResult<Expr> {
        let keyword = self.previous().clone();
        self.consume(TokenKind::LParen, "Expect '(' after 'match'.")?;
        let subject = self.expression()?;
        self.consume(TokenKind::RParen, "Expect ')' after the match subject.")?;
        self.consume(TokenKind::LBrace, "Expect '{' before match cases.")?;

        let mut arms: Vec<MatchArm> = Vec::new();
        let mut seen_default = false;
        while !self.check(TokenKind::RBrace) && !self.is_at_end() {
            if self.matches(&[TokenKind::Default]) {
                let arm_keyword = self.previous().clone();
                if seen_default {
                    return Err(self.error(&arm_keyword, "A match can only have one 'default'."));
                }
                seen_default = true;
                self.consume(TokenKind::Colon, "Expect ':' after 'default'.")?;
                let value = self.expression()?;
                arms.push(MatchArm {
                    pattern: None,
                    guard: None,
                    value,
                    line: arm_keyword.line,
                });
            } else {
                self.consume(TokenKind::Case, "Expect 'case' or 'default' in a match.")?;
                let arm_keyword = self.previous().clone();
                if seen_default {
                    return Err(self.error(
                        &arm_keyword,
                        "'default' must be the last clause of a match.",
                    ));
                }
                let pattern = self.match_pattern()?;
                let guard = self.case_guard()?;
                self.consume(TokenKind::Colon, "Expect ':' after the case pattern.")?;
                let value = self.expression()?;
                arms.push(MatchArm {
                    pattern: Some(pattern),
                    guard,
                    value,
                    line: arm_keyword.line,
                });
            }
            if !self.matches(&[TokenKind::Comma]) {
                break;
            }
        }

        let close = self.consume(TokenKind::RBrace, "Expect '}' after match cases.")?;
        if arms.is_empty() {
            return Err(self.error(&keyword, "A match needs at least one case."));
        }
        Ok(Expr {
            kind: ExprKind::Match {
                subject: Box::new(subject),
                arms,
            },
            span: keyword.span.to(close.span),
            line: keyword.line,
        })
    }

    fn case_guard(&mut self) -> PResult<Option<Expr>> {
        if !self.matches(&[TokenKind::If]) {
            return Ok(None);
        }
        self.consume(TokenKind::LParen, "Expect '(' after 'if' in a case guard.")?;
        let guard = self.expression()?;
        self.consume(TokenKind::RParen, "Expect ')' after the case guard.")?;
        Ok(Some(guard))
    }

    /// Statements up to the next `case`/`default`/`}`. There is no
    /// fall-through, so a case ends where the next one begins.
    fn case_body(&mut self) -> PResult<Vec<Stmt>> {
        let mut statements = Vec::new();
        while !(self.check(TokenKind::Case)
            || self.check(TokenKind::Default)
            || self.check(TokenKind::RBrace)
            || self.is_at_end())
        {
            match self.declaration() {
                Ok(Some(stmt)) => statements.push(stmt),
                Ok(None) => {}
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize();
                }
            }
        }
        Ok(statements)
    }

    fn match_pattern(&mut self) -> PResult<MatchPattern> {
        if self.matches(&[TokenKind::Number, TokenKind::Str]) {
            let token = self.previous().clone();
            return Ok(MatchPattern::Literal {
                value: literal_value(&token),
                span: token.span,
            });
        }
        if self.matches(&[TokenKind::True]) {
            let t = self.previous().clone();
            return Ok(MatchPattern::Literal {
                value: LitValue::Bool(true),
                span: t.span,
            });
        }
        if self.matches(&[TokenKind::False]) {
            let t = self.previous().clone();
            return Ok(MatchPattern::Literal {
                value: LitValue::Bool(false),
                span: t.span,
            });
        }
        if self.matches(&[TokenKind::Null]) {
            let t = self.previous().clone();
            return Ok(MatchPattern::Literal {
                value: LitValue::Null,
                span: t.span,
            });
        }
        if self.matches(&[TokenKind::Minus]) {
            let minus = self.previous().clone();
            let number =
                self.consume(TokenKind::Number, "Expect a number after '-' in a pattern.")?;
            let Literal::Number(n) = number.literal else {
                unreachable!("a NUMBER token carries a number")
            };
            return Ok(MatchPattern::Literal {
                value: LitValue::Number(-n),
                span: minus.span.to(number.span),
            });
        }
        if self.check(TokenKind::LBracket) {
            return self.array_match();
        }
        if self.check(TokenKind::LBrace) {
            return self.object_match();
        }
        if self.check(TokenKind::Identifier) {
            let name = self.advance().clone();
            if self.matches(&[TokenKind::LParen]) {
                let mut elements = Vec::new();
                if !self.check(TokenKind::RParen) {
                    loop {
                        elements.push(self.match_pattern()?);
                        if !self.matches(&[TokenKind::Comma]) {
                            break;
                        }
                    }
                }
                let close =
                    self.consume(TokenKind::RParen, "Expect ')' after struct pattern fields.")?;
                return Ok(MatchPattern::Struct {
                    name: Name::from_token(&name),
                    elements,
                    span: name.span.to(close.span),
                });
            }
            return Ok(MatchPattern::Bind {
                name: Name::from_token(&name),
            });
        }
        let peek = self.peek().clone();
        Err(self.error(&peek, "Expect a pattern after 'case'."))
    }

    fn array_match(&mut self) -> PResult<MatchPattern> {
        let open = self.consume(TokenKind::LBracket, "Expect '[' to start a pattern.")?;
        let mut elements = Vec::new();
        let mut rest = None;
        if !self.check(TokenKind::RBracket) {
            loop {
                if self.matches(&[TokenKind::Ellipsis]) {
                    let name = self.consume(
                        TokenKind::Identifier,
                        "Expect a name after '...' in a pattern.",
                    )?;
                    rest = Some(Name::from_token(&name));
                    break;
                }
                elements.push(self.match_pattern()?);
                if !self.matches(&[TokenKind::Comma]) {
                    break;
                }
            }
        }
        let close = self.consume(TokenKind::RBracket, "Expect ']' after array pattern.")?;
        Ok(MatchPattern::Array {
            elements,
            rest,
            span: open.span.to(close.span),
            line: open.line,
        })
    }

    fn object_match(&mut self) -> PResult<MatchPattern> {
        let open = self.consume(TokenKind::LBrace, "Expect '{' to start a pattern.")?;
        let mut entries = Vec::new();
        if !self.check(TokenKind::RBrace) {
            loop {
                let key =
                    self.consume(TokenKind::Identifier, "Expect a key name in the pattern.")?;
                if self.matches(&[TokenKind::Colon]) {
                    let sub = self.match_pattern()?;
                    entries.push((key.lexeme.clone(), sub));
                } else {
                    entries.push((
                        key.lexeme.clone(),
                        MatchPattern::Bind {
                            name: Name::from_token(&key),
                        },
                    ));
                }
                if !self.matches(&[TokenKind::Comma]) {
                    break;
                }
            }
        }
        let close = self.consume(TokenKind::RBrace, "Expect '}' after object pattern.")?;
        Ok(MatchPattern::Object {
            entries,
            span: open.span.to(close.span),
            line: open.line,
        })
    }

    // -- try / yield --------------------------------------------------------

    fn try_statement(&mut self) -> PResult<Stmt> {
        let keyword = self.previous().clone();
        self.consume(TokenKind::LBrace, "Expect '{' after 'try'.")?;
        let (body, mut end) = self.block()?;

        let mut catches = Vec::new();
        while self.matches(&[TokenKind::Catch]) {
            self.consume(TokenKind::LParen, "Expect '(' after 'catch'.")?;
            let pattern = self.binding_pattern("variable name in catch", false)?;
            self.consume(TokenKind::RParen, "Expect ')' after catch variable.")?;
            let guard = if self.matches(&[TokenKind::If]) {
                self.consume(TokenKind::LParen, "Expect '(' after 'if' in catch guard.")?;
                let g = self.expression()?;
                self.consume(TokenKind::RParen, "Expect ')' after catch guard.")?;
                Some(g)
            } else {
                None
            };
            self.consume(TokenKind::LBrace, "Expect '{' after catch.")?;
            let (block, close) = self.block()?;
            end = close;
            catches.push(CatchClause {
                pattern,
                guard,
                body: block,
            });
        }

        let mut finally = None;
        if self.matches(&[TokenKind::Finally]) {
            self.consume(TokenKind::LBrace, "Expect '{' after 'finally'.")?;
            let (block, close) = self.block()?;
            end = close;
            finally = Some(block);
        }

        if catches.is_empty() && finally.is_none() {
            return Err(self.error(&keyword, "Expect 'catch' or 'finally' after 'try' block."));
        }

        Ok(Stmt {
            kind: StmtKind::Try {
                body,
                catches,
                finally,
            },
            span: keyword.span.to(end),
            line: keyword.line,
        })
    }

    fn yield_statement(&mut self) -> PResult<Stmt> {
        let keyword = self.previous().clone();
        if self.function_yields.is_empty() {
            return Err(self.error(&keyword, "'yield' is only allowed inside a function."));
        }
        *self.function_yields.last_mut().expect("checked above") = true;
        let delegate = self.matches(&[TokenKind::Multiply]);
        let value = self.expression()?;
        self.consume_statement_end();
        let span = keyword.span.to(value.span);
        Ok(Stmt {
            kind: StmtKind::Yield { value, delegate },
            span,
            line: keyword.line,
        })
    }

    fn yield_expression(&mut self) -> PResult<Expr> {
        let keyword = self.consume(TokenKind::Yield, "Expect 'yield'.")?;
        if self.function_yields.is_empty() {
            return Err(self.error(&keyword, "'yield' is only allowed inside a function."));
        }
        *self.function_yields.last_mut().expect("checked above") = true;
        if self.check(TokenKind::Multiply) {
            let peek = self.peek().clone();
            return Err(self.error(
                &peek,
                "'yield*' re-yields a whole sequence and has no value of its own; \
                 use it as a statement.",
            ));
        }
        let value = self.expression()?;
        let span = keyword.span.to(value.span);
        Ok(Expr {
            kind: ExprKind::Yield(Box::new(value)),
            span,
            line: keyword.line,
        })
    }

    /// Returns the statements and the closing brace's span.
    fn block(&mut self) -> PResult<(Vec<Stmt>, Span)> {
        let mut statements = Vec::new();
        self.block_depth += 1;
        while !self.check(TokenKind::RBrace) && !self.is_at_end() {
            match self.declaration() {
                Ok(Some(stmt)) => statements.push(stmt),
                Ok(None) => {}
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize();
                }
            }
        }
        self.block_depth -= 1;
        let close = self.consume(TokenKind::RBrace, "Expect '}' after block.")?;
        Ok((statements, close.span))
    }

    fn expression_statement(&mut self) -> PResult<Stmt> {
        self.statement_rhs = true;
        let expr = self.expression();
        self.statement_rhs = false;
        let expr = expr?;
        self.consume_statement_end();
        let (span, line) = (expr.span, expr.line);
        Ok(Stmt {
            kind: StmtKind::Expression(expr),
            span,
            line,
        })
    }

    fn print_statement(&mut self) -> PResult<Stmt> {
        let keyword = self.previous().clone();
        self.consume(TokenKind::LParen, "Expect '(' after 'print'.")?;
        let mut values = Vec::new();
        if !self.check(TokenKind::RParen) {
            values.push(self.spread_or_expression()?);
            while self.matches(&[TokenKind::Comma]) {
                values.push(self.spread_or_expression()?);
            }
        }
        let close = self.consume(TokenKind::RParen, "Expect ')' after print arguments.")?;
        self.consume_statement_end();
        Ok(Stmt {
            kind: StmtKind::Print(values),
            span: keyword.span.to(close.span),
            line: keyword.line,
        })
    }

    fn var_declaration(&mut self) -> PResult<Stmt> {
        let keyword = self.previous().clone();
        let mut pattern = self.binding_pattern("variable name", true)?;

        // `var [a, b] = ...` parses its own `=` as the pattern's default slot,
        // so only take another initializer when the pattern did not.
        let mut initializer = pattern.take_default().map(|b| *b);
        if initializer.is_none() && self.matches(&[TokenKind::Assign]) {
            initializer = Some(if self.check(TokenKind::Yield) {
                self.yield_expression()?
            } else {
                self.expression()?
            });
        }

        if initializer.is_none() && !matches!(pattern, Pattern::Name { .. }) {
            let prev = self.previous().clone();
            return Err(self.error(&prev, "A destructuring declaration needs an initializer."));
        }

        self.consume_statement_end();
        let end = initializer
            .as_ref()
            .map(|e| e.span)
            .unwrap_or_else(|| pattern.span());
        Ok(Stmt {
            kind: StmtKind::Var {
                pattern,
                initializer,
            },
            span: keyword.span.to(end),
            line: keyword.line,
        })
    }

    // -- expressions --------------------------------------------------------

    fn expression(&mut self) -> PResult<Expr> {
        self.assignment()
    }

    fn assignment(&mut self) -> PResult<Expr> {
        // Consumed here rather than read where it is needed: descending into
        // any sub-expression must clear it, and every sub-expression comes
        // back through this method.
        let statement_rhs = self.statement_rhs;
        self.statement_rhs = false;

        let expr = self.or_expression()?;

        if self.matches(&[TokenKind::Assign]) {
            let equals = self.previous().clone();
            let value = if statement_rhs && self.check(TokenKind::Yield) {
                self.yield_expression()?
            } else {
                self.assignment()?
            };
            return self.make_assign_target(expr, &equals, value);
        }

        const COMPOUND: &[(TokenKind, BinOp)] = &[
            (TokenKind::PlusAssign, BinOp::Add),
            (TokenKind::MinusAssign, BinOp::Sub),
            (TokenKind::MultiplyAssign, BinOp::Mul),
            (TokenKind::DivideAssign, BinOp::Div),
            (TokenKind::ModuloAssign, BinOp::Mod),
        ];
        for (kind, op) in COMPOUND {
            if self.matches(&[*kind]) {
                let op_token = self.previous().clone();
                let value = self.assignment()?;
                // Desugar `target op= value` into `target = target op value`.
                let span = expr.span.to(value.span);
                let line = expr.line;
                let combined = Expr {
                    kind: ExprKind::Binary {
                        left: Box::new(expr.clone()),
                        op: *op,
                        right: Box::new(value),
                    },
                    span,
                    line,
                };
                return self.make_assign_target(expr, &op_token, combined);
            }
        }

        Ok(expr)
    }

    fn make_assign_target(
        &mut self,
        target: Expr,
        error_token: &Token,
        value: Expr,
    ) -> PResult<Expr> {
        let span = target.span.to(value.span);
        let line = target.line;
        match target.kind {
            ExprKind::Variable(name) => Ok(Expr {
                kind: ExprKind::Assign {
                    name,
                    value: Box::new(value),
                },
                span,
                line,
            }),
            ExprKind::Index { target, index } => Ok(Expr {
                kind: ExprKind::IndexAssign {
                    target,
                    index,
                    value: Box::new(value),
                },
                span,
                line,
            }),
            _ => Err(self.error(error_token, "Invalid assignment target.")),
        }
    }

    fn or_expression(&mut self) -> PResult<Expr> {
        let mut expr = self.and_expression()?;
        while self.matches(&[TokenKind::Or]) {
            let right = self.and_expression()?;
            let (span, line) = (expr.span.to(right.span), expr.line);
            expr = Expr {
                kind: ExprKind::Logical {
                    left: Box::new(expr),
                    op: LogicalOp::Or,
                    right: Box::new(right),
                },
                span,
                line,
            };
        }
        Ok(expr)
    }

    fn and_expression(&mut self) -> PResult<Expr> {
        let mut expr = self.equality()?;
        while self.matches(&[TokenKind::And]) {
            let right = self.equality()?;
            let (span, line) = (expr.span.to(right.span), expr.line);
            expr = Expr {
                kind: ExprKind::Logical {
                    left: Box::new(expr),
                    op: LogicalOp::And,
                    right: Box::new(right),
                },
                span,
                line,
            };
        }
        Ok(expr)
    }

    fn binary_level(
        &mut self,
        kinds: &[TokenKind],
        next: fn(&mut Self) -> PResult<Expr>,
    ) -> PResult<Expr> {
        let mut expr = next(self)?;
        while self.matches(kinds) {
            let op = BinOp::from_kind(self.previous().kind).expect("kinds are binary operators");
            let right = next(self)?;
            let (span, line) = (expr.span.to(right.span), expr.line);
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
                span,
                line,
            };
        }
        Ok(expr)
    }

    fn equality(&mut self) -> PResult<Expr> {
        self.binary_level(&[TokenKind::NotEquals, TokenKind::Equals], Self::comparison)
    }

    fn comparison(&mut self) -> PResult<Expr> {
        self.binary_level(
            &[
                TokenKind::Greater,
                TokenKind::GreaterEqual,
                TokenKind::Less,
                TokenKind::LessEqual,
            ],
            Self::term,
        )
    }

    fn term(&mut self) -> PResult<Expr> {
        self.binary_level(&[TokenKind::Plus, TokenKind::Minus], Self::factor)
    }

    fn factor(&mut self) -> PResult<Expr> {
        self.binary_level(
            &[TokenKind::Multiply, TokenKind::Divide, TokenKind::Modulo],
            Self::unary,
        )
    }

    fn unary(&mut self) -> PResult<Expr> {
        if self.matches(&[TokenKind::Minus, TokenKind::Not]) {
            let operator = self.previous().clone();
            let op = if operator.kind == TokenKind::Minus {
                UnOp::Neg
            } else {
                UnOp::Not
            };
            let right = self.unary()?;
            let span = operator.span.to(right.span);
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op,
                    right: Box::new(right),
                },
                span,
                line: operator.line,
            });
        }
        self.call()
    }

    fn call(&mut self) -> PResult<Expr> {
        let mut expr = self.primary()?;
        loop {
            if self.matches(&[TokenKind::LParen]) {
                expr = self.finish_call(expr)?;
            } else if self.matches(&[TokenKind::LBracket]) {
                let index = self.expression()?;
                let close = self.consume(TokenKind::RBracket, "Expect ']' after array index.")?;
                let (span, line) = (expr.span.to(close.span), expr.line);
                expr = Expr {
                    kind: ExprKind::Index {
                        target: Box::new(expr),
                        index: Box::new(index),
                    },
                    span,
                    line,
                };
            } else if self.matches(&[TokenKind::Dot]) {
                let name =
                    self.consume(TokenKind::Identifier, "Expect property name after '.'.")?;
                // `obj.name` is sugar for `obj["name"]`.
                let (span, line) = (expr.span.to(name.span), expr.line);
                expr = Expr {
                    kind: ExprKind::Index {
                        target: Box::new(expr),
                        index: Box::new(Expr {
                            kind: ExprKind::Literal(LitValue::Str(name.lexeme.clone())),
                            span: name.span,
                            line: name.line,
                        }),
                    },
                    span,
                    line,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn finish_call(&mut self, callee: Expr) -> PResult<Expr> {
        let mut args = Vec::new();
        if !self.check(TokenKind::RParen) {
            loop {
                if args.len() >= 255 {
                    let peek = self.peek().clone();
                    let e = self.error(&peek, "Can't have more than 255 arguments.");
                    self.errors.push(e);
                }
                args.push(self.spread_or_expression()?);
                if !self.matches(&[TokenKind::Comma]) {
                    break;
                }
            }
        }
        let paren = self.consume(TokenKind::RParen, "Expect ')' after arguments.")?;
        let (span, line) = (callee.span.to(paren.span), callee.line);
        Ok(Expr {
            kind: ExprKind::Call {
                callee: Box::new(callee),
                args,
            },
            span,
            line,
        })
    }

    fn spread_or_expression(&mut self) -> PResult<Expr> {
        if self.matches(&[TokenKind::Ellipsis]) {
            let token = self.previous().clone();
            let value = self.expression()?;
            let span = token.span.to(value.span);
            return Ok(Expr {
                kind: ExprKind::Spread(Box::new(value)),
                span,
                line: token.line,
            });
        }
        self.expression()
    }

    fn primary(&mut self) -> PResult<Expr> {
        if self.matches(&[TokenKind::True]) {
            return Ok(self.literal_expr(LitValue::Bool(true)));
        }
        if self.matches(&[TokenKind::False]) {
            return Ok(self.literal_expr(LitValue::Bool(false)));
        }
        if self.matches(&[TokenKind::Null]) {
            return Ok(self.literal_expr(LitValue::Null));
        }
        if self.matches(&[TokenKind::Number, TokenKind::Str]) {
            let token = self.previous().clone();
            return Ok(self.literal_expr(literal_value(&token)));
        }
        if self.matches(&[TokenKind::Template]) {
            let token = self.previous().clone();
            return self.interpolation(&token);
        }
        if self.matches(&[TokenKind::Func]) {
            return self.function_expression();
        }
        if self.matches(&[TokenKind::Match]) {
            return self.match_expression();
        }
        if self.matches(&[TokenKind::Identifier]) {
            let token = self.previous().clone();
            return Ok(Expr {
                kind: ExprKind::Variable(Name::from_token(&token)),
                span: token.span,
                line: token.line,
            });
        }
        if self.matches(&[TokenKind::LParen]) {
            let open = self.previous().clone();
            let expr = self.expression()?;
            let close = self.consume(TokenKind::RParen, "Expect ')' after expression.")?;
            return Ok(Expr {
                kind: ExprKind::Grouping(Box::new(expr)),
                span: open.span.to(close.span),
                line: open.line,
            });
        }
        if self.matches(&[TokenKind::LBracket]) {
            let open = self.previous().clone();
            let mut elements = Vec::new();
            if !self.check(TokenKind::RBracket) {
                loop {
                    elements.push(self.spread_or_expression()?);
                    if !self.matches(&[TokenKind::Comma]) {
                        break;
                    }
                }
            }
            let close = self.consume(TokenKind::RBracket, "Expect ']' after array elements.")?;
            return Ok(Expr {
                kind: ExprKind::Array(elements),
                span: open.span.to(close.span),
                line: open.line,
            });
        }
        if self.matches(&[TokenKind::LBrace]) {
            let open = self.previous().clone();
            let mut pairs = Vec::new();
            if !self.check(TokenKind::RBrace) {
                loop {
                    // A bareword key is shorthand for the *string* of that
                    // name: `{name: "Ada"}` means `{"name": "Ada"}`. To key by
                    // a variable's value, parenthesise it: `{(k): v}`.
                    let key =
                        if self.check(TokenKind::Identifier) && self.check_next(TokenKind::Colon) {
                            let token = self.advance().clone();
                            Expr {
                                kind: ExprKind::Literal(LitValue::Str(token.lexeme.clone())),
                                span: token.span,
                                line: token.line,
                            }
                        } else {
                            self.expression()?
                        };
                    self.consume(TokenKind::Colon, "Expect ':' after dictionary key.")?;
                    let value = self.expression()?;
                    pairs.push((key, value));
                    if !self.matches(&[TokenKind::Comma]) {
                        break;
                    }
                }
            }
            let close = self.consume(TokenKind::RBrace, "Expect '}' after dictionary literal.")?;
            return Ok(Expr {
                kind: ExprKind::Dict(pairs),
                span: open.span.to(close.span),
                line: open.line,
            });
        }

        let peek = self.peek().clone();
        Err(self.error(&peek, "Expect expression."))
    }

    /// Turn a TEMPLATE token's parts into an interpolation node.
    ///
    /// Each `${ ... }` part arrives from the lexer as raw source text, which is
    /// lexed and parsed here as a self-contained expression. Spans and lines
    /// from that sub-parse are shifted onto the outer file, so a mistake inside
    /// an interpolation is still pointed at where it actually appears.
    fn interpolation(&mut self, token: &Token) -> PResult<Expr> {
        let Literal::Template(template_parts) = &token.literal else {
            unreachable!("a TEMPLATE token carries template parts")
        };

        let mut parts = Vec::new();
        for part in template_parts {
            match part {
                TemplatePart::Text(text) => parts.push(InterpPart::Text(text.clone())),
                TemplatePart::Expr { source, line, span } => {
                    if source.trim().is_empty() {
                        return Err(self.error(
                            token,
                            "Empty interpolation: expected an expression inside '${}'.",
                        ));
                    }
                    let fragment = SourceFile::new("<interpolation>", source);
                    let mut sub_tokens = match lex(&fragment) {
                        Ok(tokens) => tokens,
                        Err(mut e) => {
                            e.legacy_line = *line;
                            e.span = *span;
                            return Err(e);
                        }
                    };
                    // Shift the sub-lex onto the outer file's coordinates.
                    for t in &mut sub_tokens {
                        t.line += line - 1;
                        t.span = Span::new(t.span.start + span.start, t.span.end + span.start);
                    }
                    let mut sub = Parser::new(sub_tokens);
                    // A `yield` inside an interpolation belongs to no function
                    // body the sub-parser can see, so inherit the stack.
                    sub.function_yields = self.function_yields.clone();
                    let expr = sub.expression()?;
                    if !sub.is_at_end() {
                        let peek = sub.peek().clone();
                        return Err(
                            sub.error(&peek, "Unexpected trailing tokens in interpolation.")
                        );
                    }
                    if let Some(true) = sub.function_yields.last() {
                        if let Some(last) = self.function_yields.last_mut() {
                            *last = true;
                        }
                    }
                    self.errors.extend(sub.errors);
                    parts.push(InterpPart::Expr(expr));
                }
            }
        }

        Ok(Expr {
            kind: ExprKind::Interpolation(parts),
            span: token.span,
            line: token.line,
        })
    }

    fn literal_expr(&self, value: LitValue) -> Expr {
        let token = self.previous();
        Expr {
            kind: ExprKind::Literal(value),
            span: token.span,
            line: token.line,
        }
    }

    // -- cursor -------------------------------------------------------------

    fn matches(&mut self, kinds: &[TokenKind]) -> bool {
        for kind in kinds {
            if self.check(*kind) {
                self.advance();
                return true;
            }
        }
        false
    }

    /// `from` and `as` are contextual keywords: the lexer leaves them as
    /// identifiers, so a program may use them as names.
    fn matches_word(&mut self, word: &str) -> bool {
        if self.check(TokenKind::Identifier) && self.peek().lexeme == word {
            self.advance();
            return true;
        }
        false
    }

    fn consume_word(&mut self, word: &str, message: &str) -> PResult<Token> {
        if self.matches_word(word) {
            return Ok(self.previous().clone());
        }
        let peek = self.peek().clone();
        Err(self.error(&peek, message))
    }

    fn check(&self, kind: TokenKind) -> bool {
        !self.is_at_end() && self.peek().kind == kind
    }

    /// One token of lookahead past `peek`, for constructs that cannot be
    /// identified from their first token alone.
    fn check_next(&self, kind: TokenKind) -> bool {
        self.tokens
            .get(self.current + 1)
            .is_some_and(|t| t.kind == kind)
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        self.previous()
    }

    fn is_at_end(&self) -> bool {
        self.peek().kind == TokenKind::Eof
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.current - 1]
    }

    fn consume(&mut self, kind: TokenKind, message: impl Into<String>) -> PResult<Token> {
        if self.check(kind) {
            return Ok(self.advance().clone());
        }
        let peek = self.peek().clone();
        Err(self.error(&peek, message))
    }

    /// Statement terminators are optional in MRT: consume a trailing `;` if
    /// one is present, but do not error when it is missing.
    fn consume_statement_end(&mut self) {
        self.matches(&[TokenKind::Semicolon]);
    }

    fn error(&self, token: &Token, message: impl Into<String>) -> Diagnostic {
        let where_ = if token.kind == TokenKind::Eof {
            "end of file".to_string()
        } else {
            format!("'{}'", token.lexeme)
        };
        Diagnostic::error(
            Stage::Parse,
            format!("Error at {where_}: {}", message.into()),
            token.span,
            token.line,
        )
    }

    fn line_of(&self, span: Span) -> u32 {
        self.tokens
            .iter()
            .find(|t| t.span.start == span.start)
            .map(|t| t.line)
            .unwrap_or(1)
    }

    fn synchronize(&mut self) {
        self.advance();
        while !self.is_at_end() {
            if self.previous().kind == TokenKind::Semicolon {
                return;
            }
            match self.peek().kind {
                TokenKind::Func
                | TokenKind::If
                | TokenKind::Return
                | TokenKind::While
                | TokenKind::Var
                | TokenKind::For
                | TokenKind::Try
                | TokenKind::Throw
                | TokenKind::Import
                | TokenKind::Export
                | TokenKind::Struct
                | TokenKind::Match
                | TokenKind::Yield => return,
                _ => {}
            }
            self.advance();
        }
    }
}

fn literal_value(token: &Token) -> LitValue {
    match &token.literal {
        Literal::Number(n) => LitValue::Number(*n),
        Literal::Str(s) => LitValue::Str(s.clone()),
        Literal::Bool(b) => LitValue::Bool(*b),
        other => unreachable!("token {:?} carries {:?}", token.kind, other),
    }
}

fn string_literal(token: &Token) -> String {
    match &token.literal {
        Literal::Str(s) => s.clone(),
        other => unreachable!("expected a string literal, got {other:?}"),
    }
}

fn has_duplicate(names: &[&str]) -> bool {
    for (i, name) in names.iter().enumerate() {
        if names[i + 1..].contains(name) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(src: &str) -> Program {
        let file = SourceFile::new("t.mrt", src);
        let parsed = parse(&file);
        assert!(
            parsed.errors.is_empty(),
            "unexpected errors: {:?}",
            parsed.errors
        );
        parsed.program
    }

    fn errors(src: &str) -> Vec<Diagnostic> {
        let file = SourceFile::new("t.mrt", src);
        let parsed = parse(&file);
        assert!(!parsed.errors.is_empty(), "expected errors, got none");
        parsed.errors
    }

    /// The text a node's span covers -- the thing conformance against the
    /// Python parser cannot check, because that parser has no spans.
    fn spanned(src: &str, pick: impl Fn(&Program) -> Span) -> String {
        let file = SourceFile::new("t.mrt", src);
        let parsed = parse(&file);
        assert!(
            parsed.errors.is_empty(),
            "unexpected errors: {:?}",
            parsed.errors
        );
        file.slice(pick(&parsed.program))
    }

    #[test]
    fn a_statement_span_covers_the_whole_statement() {
        let src = "func f() { var total = 1 + 2; }";
        let text = spanned(src, |p| {
            let StmtKind::Function { body, .. } = &p.statements[0].kind else {
                panic!()
            };
            body[0].span
        });
        assert_eq!(text, "var total = 1 + 2");
    }

    #[test]
    fn a_binary_span_covers_both_operands() {
        let src = "func f() { return alpha + beta * gamma; }";
        let text = spanned(src, |p| {
            let StmtKind::Function { body, .. } = &p.statements[0].kind else {
                panic!()
            };
            let StmtKind::Return(Some(e)) = &body[0].kind else {
                panic!()
            };
            let ExprKind::Binary { right, .. } = &e.kind else {
                panic!()
            };
            // The right operand is the whole `beta * gamma`, because `*`
            // binds tighter than `+`.
            right.span
        });
        assert_eq!(text, "beta * gamma");
    }

    #[test]
    fn a_span_inside_an_interpolation_points_into_the_outer_file() {
        // The sub-parse happens on a detached fragment, so its spans have to
        // be shifted back onto the real file or every diagnostic inside a
        // template would point at the wrong place.
        let src = r#"func f() { print("total ${count + 1}!"); }"#;
        let text = spanned(src, |p| {
            let StmtKind::Function { body, .. } = &p.statements[0].kind else {
                panic!()
            };
            let StmtKind::Print(values) = &body[0].kind else {
                panic!()
            };
            let ExprKind::Interpolation(parts) = &values[0].kind else {
                panic!()
            };
            let InterpPart::Expr(e) = &parts[1] else {
                panic!("expected an expression part")
            };
            let ExprKind::Binary { left, .. } = &e.kind else {
                panic!()
            };
            left.span
        });
        assert_eq!(text, "count");
    }

    #[test]
    fn errors_carry_a_span_pointing_at_the_offending_token() {
        let src = "func f() {\n    var x = ;\n}";
        let errs = errors(src);
        let file = SourceFile::new("t.mrt", src);
        assert_eq!(errs.len(), 1);
        assert_eq!(file.slice(errs[0].span), ";");
        assert_eq!(errs[0].legacy_line, 2);
    }

    #[test]
    fn the_parser_reports_several_errors_rather_than_stopping_at_the_first() {
        let errs = errors("func f() { var = 1; }\nfunc g() { print(); }\nfunc h() { var = 2; }");
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert_eq!(errs[0].legacy_line, 1);
        assert_eq!(errs[1].legacy_line, 3);
    }

    #[test]
    fn dot_access_desugars_to_a_string_index() {
        let p = program("func f() { return a.b; }");
        let StmtKind::Function { body, .. } = &p.statements[0].kind else {
            panic!()
        };
        let StmtKind::Return(Some(e)) = &body[0].kind else {
            panic!()
        };
        let ExprKind::Index { index, .. } = &e.kind else {
            panic!("expected an index")
        };
        assert_eq!(index.kind, ExprKind::Literal(LitValue::Str("b".into())));
    }

    #[test]
    fn compound_assignment_desugars_to_the_binary_form() {
        let p = program("func f() { x += 1; }");
        let StmtKind::Function { body, .. } = &p.statements[0].kind else {
            panic!()
        };
        let StmtKind::Expression(e) = &body[0].kind else {
            panic!()
        };
        let ExprKind::Assign { value, .. } = &e.kind else {
            panic!("expected an assignment")
        };
        assert!(matches!(
            value.kind,
            ExprKind::Binary { op: BinOp::Add, .. }
        ));
    }

    #[test]
    fn a_leading_brace_is_a_block_unless_an_equals_follows() {
        let p = program("func f() { { x = 1; } {x, y} = o; }");
        let StmtKind::Function { body, .. } = &p.statements[0].kind else {
            panic!()
        };
        assert!(matches!(body[0].kind, StmtKind::Block(_)));
        assert!(matches!(body[1].kind, StmtKind::DestructureAssign { .. }));
    }

    #[test]
    fn a_leading_bracket_is_an_array_unless_an_equals_follows() {
        let p = program("func f() { [1, 2][0]; [a, b] = pair; }");
        let StmtKind::Function { body, .. } = &p.statements[0].kind else {
            panic!()
        };
        assert!(matches!(body[0].kind, StmtKind::Expression(_)));
        assert!(matches!(body[1].kind, StmtKind::DestructureAssign { .. }));
    }

    #[test]
    fn for_in_and_c_style_for_are_told_apart() {
        let p = program("func f() { for (var i = 0; i < 1; i += 1) { } for (x in xs) { } }");
        let StmtKind::Function { body, .. } = &p.statements[0].kind else {
            panic!()
        };
        assert!(matches!(body[0].kind, StmtKind::For { .. }));
        assert!(matches!(body[1].kind, StmtKind::ForIn { .. }));
    }

    #[test]
    fn a_yield_marks_only_its_own_function_as_a_generator() {
        let p = program("func outer() { var inner = func() { yield 1; }; return inner; }");
        let StmtKind::Function {
            is_generator, body, ..
        } = &p.statements[0].kind
        else {
            panic!()
        };
        assert!(!is_generator, "the outer function is not a generator");
        let StmtKind::Var { initializer, .. } = &body[0].kind else {
            panic!()
        };
        let ExprKind::Function { is_generator, .. } = &initializer.as_ref().unwrap().kind else {
            panic!()
        };
        assert!(is_generator, "the inner function is");
    }

    #[test]
    fn yield_is_an_expression_only_in_the_four_allowed_shapes() {
        for src in [
            "func f() { var x = yield 1; }",
            "func f() { x = yield 1; }",
            "func f() { o.k = yield 1; }",
            "func f() { [a, b] = yield 1; }",
        ] {
            let file = SourceFile::new("t.mrt", src);
            assert!(parse(&file).errors.is_empty(), "should parse: {src}");
        }
        for src in [
            "func f() { print(yield 1); }",
            "func f() { return 1 + yield 2; }",
            "func f() { var xs = [yield 1]; }",
            "func f() { x += yield 1; }",
            "func f() { var x = yield* g(); }",
        ] {
            let file = SourceFile::new("t.mrt", src);
            assert!(!parse(&file).errors.is_empty(), "should not parse: {src}");
        }
    }

    #[test]
    fn match_is_a_statement_at_statement_level_and_an_expression_elsewhere() {
        let p = program(
            "func f(v) { match (v) { case 1: print(1); default: print(2); } \
             return match (v) { case 1: \"a\", default: \"b\" }; }",
        );
        let StmtKind::Function { body, .. } = &p.statements[0].kind else {
            panic!()
        };
        assert!(matches!(body[0].kind, StmtKind::Match { .. }));
        let StmtKind::Return(Some(e)) = &body[1].kind else {
            panic!()
        };
        assert!(matches!(e.kind, ExprKind::Match { .. }));
    }

    #[test]
    fn precedence_binds_tighter_operators_first() {
        let p = program("func f() { return 1 + 2 * 3; }");
        let StmtKind::Function { body, .. } = &p.statements[0].kind else {
            panic!()
        };
        let StmtKind::Return(Some(e)) = &body[0].kind else {
            panic!()
        };
        let ExprKind::Binary { op, right, .. } = &e.kind else {
            panic!()
        };
        assert_eq!(*op, BinOp::Add);
        assert!(matches!(
            right.kind,
            ExprKind::Binary { op: BinOp::Mul, .. }
        ));
    }

    #[test]
    fn from_and_as_stay_usable_as_names() {
        let p = program(
            "struct Trip { from, to }\nfunc fare(from, as) { return from; }\n\
             func f() { var from = 1; var o = {from: 1, as: 2}; }",
        );
        assert!(matches!(p.statements[0].kind, StmtKind::Struct { .. }));
        assert!(matches!(p.statements[1].kind, StmtKind::Function { .. }));
    }

    #[test]
    fn imports_and_exports_only_parse_at_the_top_level() {
        assert!(
            parse(&SourceFile::new("t.mrt", "import { a } from \"./m.mrt\";"))
                .errors
                .is_empty()
        );
        let errs = errors("func f() { import { a } from \"./m.mrt\"; }");
        assert!(errs[0].message.contains("only allowed at the top level"));
    }
}
