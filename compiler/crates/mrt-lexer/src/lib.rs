//! The MRT lexer.
//!
//! A hand-written scanner mirroring `src/lexer.py`, the MRT 1.x reference
//! implementation, token for token. Where the two could differ they must not:
//! the conformance harness feeds every example, test source and parity
//! snippet through both and diffs the result, so a "tidier" decision here is
//! a bug, not an improvement.
//!
//! Three inherited behaviours are deliberate and worth stating, because each
//! one looks like an oversight until you try to change it:
//!
//! * **A token's `line` is the line it *ends* on.** For every token but a
//!   multi-line string that is also the line it starts on, so the difference
//!   only shows up in string literals -- where MRT 1.x reports the closing
//!   quote's line. `span` carries the truth; `line` carries the compatibility.
//! * **There is no exponent notation.** `1e10` is the number `1` followed by
//!   the identifier `e10`. A parity case once relied on `1e10` being a
//!   number, which made the whole snippet a syntax error that both
//!   implementations agreed on, so the case silently tested nothing.
//! * **An unrecognised escape keeps its backslash.** `"\q"` is two
//!   characters, not one; only the escapes in [`ESCAPES`] are special.

use mrt_diagnostics::{Diagnostic, SourceFile, Span, Stage};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum TokenKind {
    // Keywords. `from` and `as` are deliberately absent: they are contextual
    // keywords matched by the parser as ordinary identifiers, so a program
    // may use them as names.
    Func,
    Return,
    If,
    Else,
    While,
    For,
    Print,
    Var,
    True,
    False,
    Break,
    Continue,
    Null,
    Try,
    Catch,
    Finally,
    Throw,
    In,
    Import,
    Export,
    Struct,
    Yield,
    Match,
    Case,
    Default,

    // Literals
    Identifier,
    Number,
    Str,
    /// A string literal containing at least one `${...}` run. Its literal is
    /// a list of parts rather than a single string.
    Template,

    // Operators
    Plus,
    Minus,
    Multiply,
    Divide,
    Modulo,
    PlusAssign,
    MinusAssign,
    MultiplyAssign,
    DivideAssign,
    ModuloAssign,
    Assign,
    Equals,
    NotEquals,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    And,
    Or,
    Not,

    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Dot,
    Ellipsis,
    Colon,

    Eof,
}

impl TokenKind {
    /// The name the Python implementation uses for this token type. The
    /// conformance dump is written in these names so the two sides are
    /// directly comparable.
    pub fn python_name(self) -> &'static str {
        use TokenKind::*;
        match self {
            Func => "FUNC",
            Return => "RETURN",
            If => "IF",
            Else => "ELSE",
            While => "WHILE",
            For => "FOR",
            Print => "PRINT",
            Var => "VAR",
            True => "TRUE",
            False => "FALSE",
            Break => "BREAK",
            Continue => "CONTINUE",
            Null => "NULL",
            Try => "TRY",
            Catch => "CATCH",
            Finally => "FINALLY",
            Throw => "THROW",
            In => "IN",
            Import => "IMPORT",
            Export => "EXPORT",
            Struct => "STRUCT",
            Yield => "YIELD",
            Match => "MATCH",
            Case => "CASE",
            Default => "DEFAULT",
            Identifier => "IDENTIFIER",
            Number => "NUMBER",
            Str => "STRING",
            Template => "TEMPLATE",
            Plus => "PLUS",
            Minus => "MINUS",
            Multiply => "MULTIPLY",
            Divide => "DIVIDE",
            Modulo => "MODULO",
            PlusAssign => "PLUS_ASSIGN",
            MinusAssign => "MINUS_ASSIGN",
            MultiplyAssign => "MULTIPLY_ASSIGN",
            DivideAssign => "DIVIDE_ASSIGN",
            ModuloAssign => "MODULO_ASSIGN",
            Assign => "ASSIGN",
            Equals => "EQUALS",
            NotEquals => "NOT_EQUALS",
            Greater => "GREATER",
            GreaterEqual => "GREATER_EQUAL",
            Less => "LESS",
            LessEqual => "LESS_EQUAL",
            And => "AND",
            Or => "OR",
            Not => "NOT",
            LParen => "LPAREN",
            RParen => "RPAREN",
            LBrace => "LBRACE",
            RBrace => "RBRACE",
            LBracket => "LBRACKET",
            RBracket => "RBRACKET",
            Comma => "COMMA",
            Semicolon => "SEMICOLON",
            Dot => "DOT",
            Ellipsis => "ELLIPSIS",
            Colon => "COLON",
            Eof => "EOF",
        }
    }
}

/// One `${ ... }` run, or the literal text between runs.
#[derive(Clone, PartialEq, Debug)]
pub enum TemplatePart {
    Text(String),
    /// The raw source of an embedded expression. The parser re-lexes it, which
    /// is why it is kept as text rather than parsed here -- the lexer has no
    /// business knowing what an expression is.
    Expr {
        source: String,
        /// The line the run opened on, used to shift the sub-lex's line
        /// numbers onto the outer file.
        line: u32,
        span: Span,
    },
}

#[derive(Clone, PartialEq, Debug)]
pub enum Literal {
    None,
    Number(f64),
    Str(String),
    Bool(bool),
    Template(Vec<TemplatePart>),
}

#[derive(Clone, PartialEq, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub literal: Literal,
    /// The line MRT 1.x stamps on this token: the line it *ends* on.
    pub line: u32,
    pub span: Span,
}

/// The escapes recognised inside a string literal. Anything else keeps its
/// backslash, so `"\q"` is a backslash followed by `q`.
pub const ESCAPES: &[(char, char)] = &[
    ('n', '\n'),
    ('t', '\t'),
    ('r', '\r'),
    ('"', '"'),
    ('\\', '\\'),
    ('0', '\0'),
    // `\$` escapes an interpolation, so "\${x}" is literal text.
    ('$', '$'),
];

fn escape_for(c: char) -> Option<char> {
    ESCAPES.iter().find(|(k, _)| *k == c).map(|(_, v)| *v)
}

fn keyword_for(word: &str) -> Option<TokenKind> {
    use TokenKind::*;
    Some(match word {
        "func" => Func,
        "return" => Return,
        "if" => If,
        "else" => Else,
        "while" => While,
        "for" => For,
        "print" => Print,
        "var" => Var,
        "true" => True,
        "false" => False,
        "break" => Break,
        "continue" => Continue,
        "null" => Null,
        "try" => Try,
        "catch" => Catch,
        "finally" => Finally,
        "throw" => Throw,
        "in" => In,
        "struct" => Struct,
        "yield" => Yield,
        "match" => Match,
        "case" => Case,
        "default" => Default,
        "import" => Import,
        "export" => Export,
        _ => return None,
    })
}

pub struct Lexer<'a> {
    file: &'a SourceFile,
    src: &'a [char],
    start: u32,
    current: u32,
    line: u32,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    pub fn new(file: &'a SourceFile) -> Self {
        Lexer {
            file,
            src: file.chars(),
            start: 0,
            current: 0,
            line: 1,
            tokens: Vec::new(),
        }
    }

    /// Scan the whole file.
    ///
    /// Lexical errors are fatal, exactly as in MRT 1.x: the scanner has no
    /// way to guess what a stray `&` was meant to be, and guessing would make
    /// every later token suspect. The parser, by contrast, recovers and
    /// reports several errors at once.
    pub fn scan(mut self) -> Result<Vec<Token>, Diagnostic> {
        while !self.is_at_end() {
            self.start = self.current;
            self.scan_token()?;
        }
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            lexeme: String::new(),
            literal: Literal::None,
            line: self.line,
            span: Span::empty(self.current),
        });
        Ok(self.tokens)
    }

    fn scan_token(&mut self) -> Result<(), Diagnostic> {
        use TokenKind::*;
        let c = self.advance();
        match c {
            '(' => self.add(LParen),
            ')' => self.add(RParen),
            '{' => self.add(LBrace),
            '}' => self.add(RBrace),
            '[' => self.add(LBracket),
            ']' => self.add(RBracket),
            ',' => self.add(Comma),
            ';' => self.add(Semicolon),
            ':' => self.add(Colon),
            '.' => {
                // `...` is the rest/spread marker; a single `.` is property
                // access. Two dots is not a token, so `a..b` stays an error.
                if self.peek() == '.' && self.peek_next() == '.' {
                    self.advance();
                    self.advance();
                    self.add(Ellipsis);
                } else {
                    self.add(Dot);
                }
            }
            '+' => {
                let k = if self.matches('=') { PlusAssign } else { Plus };
                self.add(k);
            }
            '-' => {
                let k = if self.matches('=') {
                    MinusAssign
                } else {
                    Minus
                };
                self.add(k);
            }
            '*' => {
                let k = if self.matches('=') {
                    MultiplyAssign
                } else {
                    Multiply
                };
                self.add(k);
            }
            '%' => {
                let k = if self.matches('=') {
                    ModuloAssign
                } else {
                    Modulo
                };
                self.add(k);
            }
            '/' => {
                if self.matches('/') {
                    while self.peek() != '\n' && !self.is_at_end() {
                        self.advance();
                    }
                } else if self.matches('*') {
                    self.block_comment()?;
                } else if self.matches('=') {
                    self.add(DivideAssign);
                } else {
                    self.add(Divide);
                }
            }
            ' ' | '\r' | '\t' => {}
            '\n' => self.line += 1,
            '"' => self.string()?,
            '>' => {
                let k = if self.matches('=') {
                    GreaterEqual
                } else {
                    Greater
                };
                self.add(k);
            }
            '<' => {
                let k = if self.matches('=') { LessEqual } else { Less };
                self.add(k);
            }
            '=' => {
                let k = if self.matches('=') { Equals } else { Assign };
                self.add(k);
            }
            '!' => {
                let k = if self.matches('=') { NotEquals } else { Not };
                self.add(k);
            }
            '&' => {
                if self.matches('&') {
                    self.add(And);
                } else {
                    return Err(self.error(
                        "Unexpected character '&' (did you mean '&&'?)",
                        self.char_span(),
                        self.line,
                    ));
                }
            }
            '|' => {
                if self.matches('|') {
                    self.add(Or);
                } else {
                    return Err(self.error(
                        "Unexpected character '|' (did you mean '||'?)",
                        self.char_span(),
                        self.line,
                    ));
                }
            }
            _ => {
                if c.is_ascii_digit() {
                    self.number();
                } else if is_alpha(c) {
                    self.identifier();
                } else {
                    return Err(self.error(
                        format!("Unexpected character '{c}'"),
                        self.char_span(),
                        self.line,
                    ));
                }
            }
        }
        Ok(())
    }

    fn block_comment(&mut self) -> Result<(), Diagnostic> {
        let open_line = self.line;
        while !self.is_at_end() {
            if self.peek() == '*' && self.peek_next() == '/' {
                self.advance();
                self.advance();
                return Ok(());
            }
            if self.peek() == '\n' {
                self.line += 1;
            }
            self.advance();
        }
        // MRT 1.x reports the line the scan *reached*, not the one the
        // comment opened on, so an unterminated comment blames the end of the
        // file. Kept for conformance; the span points at the opener, which is
        // what the rich renderer shows.
        Err(self
            .error(
                "Unterminated comment",
                Span::new(self.start, self.start + 2),
                self.line,
            )
            .with_note(format!("the comment opened on line {open_line}")))
    }

    fn identifier(&mut self) {
        while is_alphanumeric(self.peek()) {
            self.advance();
        }
        let text: String = self.current_text();
        match keyword_for(&text) {
            Some(TokenKind::True) => self.add_with(TokenKind::True, Literal::Bool(true)),
            Some(TokenKind::False) => self.add_with(TokenKind::False, Literal::Bool(false)),
            Some(kind) => self.add(kind),
            None => self.add(TokenKind::Identifier),
        }
    }

    fn number(&mut self) {
        while self.peek().is_ascii_digit() {
            self.advance();
        }
        // A decimal point only continues the number when a digit follows, so
        // `1.` is the number 1 then a DOT, and `arr.0` still works. There is
        // no exponent form -- see the module docs.
        if self.peek() == '.' && self.peek_next().is_ascii_digit() {
            self.advance();
            while self.peek().is_ascii_digit() {
                self.advance();
            }
        }
        let text = self.current_text();
        let value: f64 = text
            .parse()
            .expect("digits with at most one dot parse as f64");
        self.add_with(TokenKind::Number, Literal::Number(value));
    }

    fn string(&mut self) -> Result<(), Diagnostic> {
        let open = self.current - 1;
        let start_line = self.line;
        let mut text = String::new();
        let mut parts: Vec<TemplatePart> = Vec::new();

        while self.peek() != '"' && !self.is_at_end() {
            let c = self.peek();
            if c == '\n' {
                self.line += 1;
            }

            if c == '\\' && escape_for(self.peek_next()).is_some() {
                self.advance();
                let escaped = self.advance();
                text.push(escape_for(escaped).expect("checked above"));
            } else if c == '$' && self.peek_next() == '{' {
                self.advance();
                self.advance();
                if !text.is_empty() {
                    parts.push(TemplatePart::Text(std::mem::take(&mut text)));
                }
                parts.push(self.interpolation(open, start_line)?);
            } else {
                text.push(self.advance());
            }
        }

        if self.is_at_end() {
            return Err(self.error(
                "Unterminated string",
                Span::new(open, self.current),
                start_line,
            ));
        }

        self.advance(); // closing quote

        if parts.is_empty() {
            self.add_with(TokenKind::Str, Literal::Str(text));
        } else {
            if !text.is_empty() {
                parts.push(TemplatePart::Text(text));
            }
            self.add_with(TokenKind::Template, Literal::Template(parts));
        }
        Ok(())
    }

    /// Consume the source of a `${ ... }` run, starting just after the `{`.
    ///
    /// Brace depth is tracked so an object literal or block nested inside the
    /// expression does not end it early, and nested string literals are
    /// skipped wholesale so a `}` or quote inside one is treated as text.
    fn interpolation(
        &mut self,
        string_open: u32,
        string_start_line: u32,
    ) -> Result<TemplatePart, Diagnostic> {
        let expr_line = self.line;
        let start = self.current;
        let mut depth = 1usize;

        while !self.is_at_end() {
            let c = self.peek();
            if c == '"' {
                self.advance();
                while self.peek() != '"' && !self.is_at_end() {
                    if self.peek() == '\n' {
                        self.line += 1;
                    }
                    if self.peek() == '\\' {
                        // A backslash escapes the next character, so an
                        // escaped quote does not look like the terminator.
                        // Guarded against running off the end, which the
                        // Python implementation does not do.
                        self.advance();
                        if self.is_at_end() {
                            break;
                        }
                    }
                    self.advance();
                }
                if self.is_at_end() {
                    return Err(self.error(
                        "Unterminated string",
                        Span::new(string_open, self.current),
                        string_start_line,
                    ));
                }
                self.advance(); // closing quote
                continue;
            }
            if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    let span = Span::new(start, self.current);
                    let source: String = self.src[start as usize..self.current as usize]
                        .iter()
                        .collect();
                    self.advance(); // closing '}'
                    return Ok(TemplatePart::Expr {
                        source,
                        line: expr_line,
                        span,
                    });
                }
            } else if c == '\n' {
                self.line += 1;
            }
            self.advance();
        }

        Err(self.error(
            "Unterminated interpolation: expected '}'",
            Span::new(start.saturating_sub(2), self.current),
            expr_line,
        ))
    }

    // -- cursor ------------------------------------------------------------

    fn matches(&mut self, expected: char) -> bool {
        if self.is_at_end() || self.src[self.current as usize] != expected {
            return false;
        }
        self.current += 1;
        true
    }

    fn peek(&self) -> char {
        if self.is_at_end() {
            '\0'
        } else {
            self.src[self.current as usize]
        }
    }

    fn peek_next(&self) -> char {
        self.src
            .get(self.current as usize + 1)
            .copied()
            .unwrap_or('\0')
    }

    fn is_at_end(&self) -> bool {
        self.current as usize >= self.src.len()
    }

    fn advance(&mut self) -> char {
        let c = self.src[self.current as usize];
        self.current += 1;
        c
    }

    fn current_text(&self) -> String {
        self.src[self.start as usize..self.current as usize]
            .iter()
            .collect()
    }

    fn current_span(&self) -> Span {
        Span::new(self.start, self.current)
    }

    /// The single character just consumed, for "unexpected character".
    fn char_span(&self) -> Span {
        Span::new(self.current.saturating_sub(1), self.current)
    }

    fn add(&mut self, kind: TokenKind) {
        self.add_with(kind, Literal::None);
    }

    fn add_with(&mut self, kind: TokenKind, literal: Literal) {
        self.tokens.push(Token {
            kind,
            lexeme: self.current_text(),
            literal,
            line: self.line,
            span: self.current_span(),
        });
    }

    fn error(&self, message: impl Into<String>, span: Span, line: u32) -> Diagnostic {
        let _ = self.file;
        Diagnostic::error(Stage::Lex, message, span, line)
    }
}

fn is_alpha(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_alphanumeric(c: char) -> bool {
    is_alpha(c) || c.is_ascii_digit()
}

/// Convenience: lex a string of source.
pub fn lex(file: &SourceFile) -> Result<Vec<Token>, Diagnostic> {
    Lexer::new(file).scan()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        let file = SourceFile::new("t.mrt", src);
        lex(&file).expect("lexes").iter().map(|t| t.kind).collect()
    }

    fn tokens(src: &str) -> Vec<Token> {
        let file = SourceFile::new("t.mrt", src);
        lex(&file).expect("lexes")
    }

    fn error(src: &str) -> Diagnostic {
        let file = SourceFile::new("t.mrt", src);
        lex(&file).expect_err("should not lex")
    }

    #[test]
    fn there_is_no_exponent_notation() {
        use TokenKind::*;
        // `1e10` is the number 1 followed by the identifier `e10`. Changing
        // this would be a language change, not a bug fix.
        assert_eq!(kinds("1e10"), vec![Number, Identifier, Eof]);
        assert_eq!(kinds("1.5e3"), vec![Number, Identifier, Eof]);
    }

    #[test]
    fn a_dot_continues_a_number_only_before_a_digit() {
        use TokenKind::*;
        assert_eq!(kinds("1.5"), vec![Number, Eof]);
        assert_eq!(kinds("1."), vec![Number, Dot, Eof]);
        assert_eq!(kinds(".5"), vec![Dot, Number, Eof]);
        assert_eq!(kinds("a.0"), vec![Identifier, Dot, Number, Eof]);
    }

    #[test]
    fn a_token_is_stamped_with_the_line_it_ends_on() {
        // The only place this is observable is a multi-line string, which
        // MRT 1.x reports at its closing quote.
        let toks = tokens("var s = \"a\nb\";\nvar t = 1;");
        let string = toks.iter().find(|t| t.kind == TokenKind::Str).unwrap();
        assert_eq!(string.line, 2, "string ends on line 2");
        // The span still tells the truth about where it started.
        assert_eq!(string.span.start, 8);
    }

    #[test]
    fn known_escapes_are_translated_and_unknown_ones_keep_their_backslash() {
        let toks = tokens(r#""a\nb\tc\"d\\e\0f\$g" "\q""#);
        let Literal::Str(s) = &toks[0].literal else {
            panic!("expected a string literal")
        };
        assert_eq!(s, "a\nb\tc\"d\\e\0f$g");
        let Literal::Str(unknown) = &toks[1].literal else {
            panic!("expected a string literal")
        };
        assert_eq!(unknown, "\\q", "an unrecognised escape keeps its backslash");
    }

    #[test]
    fn an_escaped_dollar_does_not_start_an_interpolation() {
        let toks = tokens(r#""\${x}""#);
        assert_eq!(toks[0].kind, TokenKind::Str);
        let Literal::Str(s) = &toks[0].literal else {
            panic!()
        };
        assert_eq!(s, "${x}");
    }

    #[test]
    fn interpolation_splits_into_parts() {
        let toks = tokens(r#""a${b}c""#);
        assert_eq!(toks[0].kind, TokenKind::Template);
        let Literal::Template(parts) = &toks[0].literal else {
            panic!()
        };
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], TemplatePart::Text("a".into()));
        match &parts[1] {
            TemplatePart::Expr { source, line, .. } => {
                assert_eq!(source, "b");
                assert_eq!(*line, 1);
            }
            other => panic!("expected an expression part, got {other:?}"),
        }
        assert_eq!(parts[2], TemplatePart::Text("c".into()));
    }

    #[test]
    fn interpolation_survives_nested_braces_and_strings() {
        let toks = tokens(r#""${ {a: 1} }${ f("}") }""#);
        let Literal::Template(parts) = &toks[0].literal else {
            panic!()
        };
        let sources: Vec<&str> = parts
            .iter()
            .filter_map(|p| match p {
                TemplatePart::Expr { source, .. } => Some(source.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(sources, vec![" {a: 1} ", r#" f("}") "#]);
    }

    #[test]
    fn a_string_with_no_interpolation_stays_a_plain_string() {
        assert_eq!(kinds(r#""plain""#), vec![TokenKind::Str, TokenKind::Eof]);
    }

    #[test]
    fn ellipsis_needs_all_three_dots() {
        use TokenKind::*;
        assert_eq!(kinds("..."), vec![Ellipsis, Eof]);
        assert_eq!(kinds(".."), vec![Dot, Dot, Eof]);
        assert_eq!(kinds("...."), vec![Ellipsis, Dot, Eof]);
    }

    #[test]
    fn from_and_as_are_ordinary_identifiers() {
        // They are contextual keywords the parser matches by spelling, so a
        // program may use them as names.
        assert_eq!(
            kinds("from as"),
            vec![TokenKind::Identifier, TokenKind::Identifier, TokenKind::Eof]
        );
    }

    #[test]
    fn booleans_carry_their_value() {
        let toks = tokens("true false");
        assert_eq!(toks[0].literal, Literal::Bool(true));
        assert_eq!(toks[1].literal, Literal::Bool(false));
    }

    #[test]
    fn comments_are_skipped_and_count_their_lines() {
        let toks = tokens("// one\n/* two\nthree */ var a = 1;");
        assert_eq!(toks[0].kind, TokenKind::Var);
        assert_eq!(toks[0].line, 3, "the var is on line 3");
    }

    #[test]
    fn block_comments_do_not_nest() {
        use TokenKind::*;
        // The first `*/` closes it, so `var` survives but the trailing `*/`
        // would be a stray token -- exactly as in MRT 1.x.
        assert_eq!(
            kinds("/* outer /* inner */ var a = 1;"),
            vec![Var, Identifier, Assign, Number, Semicolon, Eof]
        );
    }

    #[test]
    fn lexical_errors_carry_a_span_and_the_legacy_line() {
        let d = error("var a = 1;\nvar b = &;");
        assert_eq!(d.message, "Unexpected character '&' (did you mean '&&'?)");
        assert_eq!(d.legacy_line, 2);
        assert_eq!(d.span, Span::new(19, 20));

        assert_eq!(error("\"abc").message, "Unterminated string");
        assert_eq!(error("/* x").message, "Unterminated comment");
        assert_eq!(
            error("\"${1 + 2").message,
            "Unterminated interpolation: expected '}'"
        );
    }

    #[test]
    fn an_unterminated_string_is_blamed_on_its_opening_line() {
        let d = error("var a = 1;\nvar s = \"oops\nand more");
        assert_eq!(d.legacy_line, 2, "the line the string opened on");
    }

    #[test]
    fn spans_count_chars_so_non_ascii_does_not_shift_them() {
        let toks = tokens("\"héllo\" x");
        let x = toks
            .iter()
            .find(|t| t.kind == TokenKind::Identifier)
            .unwrap();
        assert_eq!(x.span, Span::new(8, 9));
    }

    #[test]
    fn an_empty_file_is_just_eof() {
        assert_eq!(kinds(""), vec![TokenKind::Eof]);
        assert_eq!(kinds("  \n\t\n "), vec![TokenKind::Eof]);
    }

    #[test]
    fn every_operator_and_delimiter_round_trips() {
        use TokenKind::*;
        assert_eq!(
            kinds("+= -= *= /= %= == != <= >= && || ! = < > + - * / % ( ) { } [ ] , ; . : ..."),
            vec![
                PlusAssign,
                MinusAssign,
                MultiplyAssign,
                DivideAssign,
                ModuloAssign,
                Equals,
                NotEquals,
                LessEqual,
                GreaterEqual,
                And,
                Or,
                Not,
                Assign,
                Less,
                Greater,
                Plus,
                Minus,
                Multiply,
                Divide,
                Modulo,
                LParen,
                RParen,
                LBrace,
                RBrace,
                LBracket,
                RBracket,
                Comma,
                Semicolon,
                Dot,
                Colon,
                Ellipsis,
                Eof,
            ]
        );
    }

    #[test]
    fn every_keyword_maps_to_its_own_kind() {
        use TokenKind::*;
        assert_eq!(
            kinds(
                "func return if else while for print var true false break continue \
                   null try catch finally throw in import export struct yield match \
                   case default"
            ),
            vec![
                Func, Return, If, Else, While, For, Print, Var, True, False, Break, Continue, Null,
                Try, Catch, Finally, Throw, In, Import, Export, Struct, Yield, Match, Case,
                Default, Eof,
            ]
        );
    }
}
