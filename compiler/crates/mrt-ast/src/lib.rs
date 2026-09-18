//! The MRT abstract syntax tree.
//!
//! A direct translation of `mrt/ast.py`, with two differences that are worth
//! stating because they are the reason this crate exists at all:
//!
//! * **Every node carries a [`Span`]**, not just a line number. That is what
//!   lets a later stage say "expected a number, found a string" and underline
//!   the string.
//! * **Sum types are enums**, so a malformed tree is unrepresentable rather
//!   than merely unlikely. The Python version leans on dataclasses and
//!   `isinstance`; the TypeScript one on a discriminated union.
//!
//! The shapes themselves are deliberately identical to the reference
//! implementation's. Anywhere the two could differ they must not: the AST is
//! compared node for node against the Python parser by
//! `scripts/check-frontend-conformance.py`, so a tidier arrangement here is a
//! conformance failure, not an improvement.

pub mod dump;

use mrt_diagnostics::Span;
use mrt_lexer::{Token, TokenKind};

/// A name, kept with the span it was written at so later stages can point at
/// the *use* of a variable rather than at its declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct Name {
    pub text: String,
    pub span: Span,
    /// The line MRT 1.x would report for this name.
    pub line: u32,
}

impl Name {
    pub fn from_token(token: &Token) -> Self {
        Name {
            text: token.lexeme.clone(),
            span: token.span,
            line: token.line,
        }
    }
}

/// Binary and unary operators, kept as their own type rather than as raw
/// tokens so the tree does not depend on lexer detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

impl BinOp {
    pub fn from_kind(kind: TokenKind) -> Option<Self> {
        use TokenKind as T;
        Some(match kind {
            T::Plus => BinOp::Add,
            T::Minus => BinOp::Sub,
            T::Multiply => BinOp::Mul,
            T::Divide => BinOp::Div,
            T::Modulo => BinOp::Mod,
            T::Equals => BinOp::Equal,
            T::NotEquals => BinOp::NotEqual,
            T::Less => BinOp::Less,
            T::LessEqual => BinOp::LessEqual,
            T::Greater => BinOp::Greater,
            T::GreaterEqual => BinOp::GreaterEqual,
            _ => return None,
        })
    }

    /// The spelling the reference implementation records on the token, which
    /// the AST dump prints so both sides agree.
    pub fn lexeme(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Equal => "==",
            BinOp::NotEqual => "!=",
            BinOp::Less => "<",
            BinOp::LessEqual => "<=",
            BinOp::Greater => ">",
            BinOp::GreaterEqual => ">=",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

impl UnOp {
    pub fn lexeme(self) -> &'static str {
        match self {
            UnOp::Neg => "-",
            UnOp::Not => "!",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
}

impl LogicalOp {
    pub fn lexeme(self) -> &'static str {
        match self {
            LogicalOp::And => "&&",
            LogicalOp::Or => "||",
        }
    }
}

/// A literal value as written in source.
#[derive(Clone, Debug, PartialEq)]
pub enum LitValue {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
}

// -- Binding patterns --------------------------------------------------------
//
// The left-hand side of a binding: a plain name, or a shape to take apart.
// The same nodes serve `var`, parameters, `for`-`in`, `catch` and
// destructuring assignment, so destructuring works identically everywhere a
// name can be bound.

#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    /// `name`, optionally `name = default`.
    Name {
        name: Name,
        default: Option<Box<Expr>>,
    },
    /// `[a, b]`, `[a, ...rest]`, `[a = 1]`, and nested patterns within.
    Array {
        elements: Vec<Pattern>,
        rest: Option<Name>,
        default: Option<Box<Expr>>,
        span: Span,
        line: u32,
    },
    /// `{name, age}`, `{name: local}`, `{name = "anon"}`, `{a, ...others}`.
    Object {
        entries: Vec<(String, Pattern)>,
        rest: Option<Name>,
        default: Option<Box<Expr>>,
        span: Span,
        line: u32,
    },
}

impl Pattern {
    pub fn default(&self) -> Option<&Expr> {
        match self {
            Pattern::Name { default, .. }
            | Pattern::Array { default, .. }
            | Pattern::Object { default, .. } => default.as_deref(),
        }
    }

    pub fn take_default(&mut self) -> Option<Box<Expr>> {
        match self {
            Pattern::Name { default, .. }
            | Pattern::Array { default, .. }
            | Pattern::Object { default, .. } => default.take(),
        }
    }

    pub fn set_default(&mut self, value: Option<Box<Expr>>) {
        match self {
            Pattern::Name { default, .. }
            | Pattern::Array { default, .. }
            | Pattern::Object { default, .. } => *default = value,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Pattern::Name { name, .. } => name.span,
            Pattern::Array { span, .. } | Pattern::Object { span, .. } => *span,
        }
    }
}

/// One declared parameter. Any default lives on the pattern and is evaluated
/// at call time in the callee's own scope, so a later default may refer to an
/// earlier parameter. `rest` marks `...name`, which must come last.
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub pattern: Pattern,
    pub rest: bool,
}

// -- Match patterns ----------------------------------------------------------
//
// Deliberately separate from binding patterns: a binding pattern takes a value
// apart and fails loudly if it cannot, whereas a match pattern *tests* a value
// and binds only if it fits. One node set would mean every node carrying both
// a default and a literal.

#[derive(Clone, Debug, PartialEq)]
pub enum MatchPattern {
    /// `case 0:` / `case "x":` -- matches by structural equality.
    Literal { value: LitValue, span: Span },
    /// `case n:` -- matches anything and binds it.
    Bind { name: Name },
    /// `case [a, b]:` / `case [head, ...tail]:`. Unlike destructuring, the
    /// length must match exactly unless a rest is given.
    Array {
        elements: Vec<MatchPattern>,
        rest: Option<Name>,
        span: Span,
        line: u32,
    },
    /// `case {kind: "circle", radius: r}:` -- a *partial* match.
    Object {
        entries: Vec<(String, MatchPattern)>,
        span: Span,
        line: u32,
    },
    /// `case Point(x, y):` -- an instance of that exact struct.
    Struct {
        name: Name,
        elements: Vec<MatchPattern>,
        span: Span,
    },
}

/// One arm of a `match` *statement*.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchCase {
    /// `None` for `default:`.
    pub pattern: Option<MatchPattern>,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
    pub line: u32,
}

/// One arm of a `match` *expression*, whose body is a single expression.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchArm {
    pub pattern: Option<MatchPattern>,
    pub guard: Option<Expr>,
    pub value: Expr,
    pub line: u32,
}

/// One `catch (e) { }` clause, optionally guarded by `if (cond)`.
#[derive(Clone, Debug, PartialEq)]
pub struct CatchClause {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
}

/// A piece of a string template: literal text, or an embedded expression.
#[derive(Clone, Debug, PartialEq)]
pub enum InterpPart {
    Text(String),
    Expr(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    /// The line MRT 1.x would report for this node.
    pub line: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Literal(LitValue),
    Variable(Name),
    Assign {
        name: Name,
        value: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },
    Logical {
        left: Box<Expr>,
        op: LogicalOp,
        right: Box<Expr>,
    },
    Unary {
        op: UnOp,
        right: Box<Expr>,
    },
    Grouping(Box<Expr>),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Array(Vec<Expr>),
    /// `target[index]`, and `target.name` desugared to `target["name"]`.
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
    },
    /// `target[index] = value`, and `target.name = value`.
    IndexAssign {
        target: Box<Expr>,
        index: Box<Expr>,
        value: Box<Expr>,
    },
    /// A `{key: value}` literal. Keys are expressions evaluated at
    /// construction time; a bareword key is a string literal by the time it
    /// gets here.
    Dict(Vec<(Expr, Expr)>),
    /// `...expr` in a call's arguments, an array literal or `print`. Not a
    /// general-purpose expression: evaluating one elsewhere is an error.
    Spread(Box<Expr>),
    Function {
        name: Option<Name>,
        params: Vec<Param>,
        body: Vec<Stmt>,
        is_generator: bool,
    },
    Interpolation(Vec<InterpPart>),
    /// `match (subject) { case p: expr, default: expr }` used as a value.
    Match {
        subject: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    /// `yield expr` in the one position where it produces a value: the entire
    /// right-hand side of a declaration or an assignment.
    Yield(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
    pub line: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Expression(Expr),
    Print(Vec<Expr>),
    Var {
        pattern: Pattern,
        initializer: Option<Expr>,
    },
    /// `[a, b] = pair;` -- assignment through a pattern to existing variables.
    DestructureAssign {
        pattern: Pattern,
        value: Expr,
    },
    Block(Vec<Stmt>),
    If {
        condition: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While {
        condition: Expr,
        body: Box<Stmt>,
    },
    /// The C-style three-clause loop. Kept distinct from `while` so `continue`
    /// still runs the increment.
    For {
        initializer: Option<Box<Stmt>>,
        condition: Option<Expr>,
        increment: Option<Expr>,
        body: Box<Stmt>,
    },
    /// `for (x in iterable) { }`. The loop variable is bound afresh each
    /// iteration, so a closure made in the body captures that iteration.
    ForIn {
        pattern: Pattern,
        iterable: Expr,
        body: Box<Stmt>,
    },
    Function {
        name: Name,
        params: Vec<Param>,
        body: Vec<Stmt>,
        is_generator: bool,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Throw(Expr),
    /// `yield expr;` and `yield* other;`.
    Yield {
        value: Expr,
        delegate: bool,
    },
    Try {
        body: Vec<Stmt>,
        catches: Vec<CatchClause>,
        finally: Option<Vec<Stmt>>,
    },
    Match {
        subject: Expr,
        cases: Vec<MatchCase>,
    },
    Struct {
        name: Name,
        fields: Vec<Param>,
        methods: Vec<Stmt>,
    },
    /// `import { a, b as c } from "./mod.mrt";`, or `import * as ns from ...`.
    Import {
        names: Vec<(Name, Name)>,
        namespace: Option<Name>,
        specifier: String,
    },
    /// `export` in front of a declaration.
    Export {
        declaration: Box<Stmt>,
        name: Name,
    },
    /// `export { a, b as c };` and `export { a } from "./m.mrt";`.
    ExportNames {
        names: Vec<(Name, Name)>,
        specifier: Option<String>,
    },
}

/// A whole parsed file.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub statements: Vec<Stmt>,
}
