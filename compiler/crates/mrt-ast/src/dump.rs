//! The canonical AST dump.
//!
//! One node per line, two spaces of indentation per level. A node line is its
//! name plus scalar attributes; a child group is a `.name` line followed by
//! that group's children, indented one further. Groups are always emitted,
//! even when empty, so the shape of the output never depends on whether an
//! optional part happens to be present.
//!
//! ```text
//! Program
//!   .statements
//!     Function name="main" generator=false
//!       .params
//!       .body
//!         Print
//!           .values
//!             Literal S:"hello"
//! ```
//!
//! `scripts/dump_ast.py` produces byte-identical output from the Python
//! parser's tree. That equality is the only evidence that the two parsers
//! agree about what MRT means, so the format is kept rigidly regular rather
//! than pretty: every difference in output should be a real difference in the
//! tree.
//!
//! Numbers are written as raw IEEE-754 bit patterns, for the same reason as in
//! the token dump: float formatting is where two languages quietly disagree.

use mrt_diagnostics::quote;

use crate::*;

pub fn dump_program(program: &Program) -> String {
    let mut out = Dumper::default();
    out.node("Program", &[]);
    out.indent(|out| out.group("statements", &program.statements, Dumper::stmt));
    out.text
}

#[derive(Default)]
struct Dumper {
    text: String,
    depth: usize,
}

impl Dumper {
    fn node(&mut self, name: &str, attrs: &[(&str, String)]) {
        self.text.push_str(&"  ".repeat(self.depth));
        self.text.push_str(name);
        for (key, value) in attrs {
            self.text.push_str(&format!(" {key}={value}"));
        }
        self.text.push('\n');
    }

    fn indent(&mut self, f: impl FnOnce(&mut Self)) {
        self.depth += 1;
        f(self);
        self.depth -= 1;
    }

    fn label(&mut self, name: &str) {
        self.text.push_str(&"  ".repeat(self.depth));
        self.text.push('.');
        self.text.push_str(name);
        self.text.push('\n');
    }

    fn group<T>(&mut self, name: &str, items: &[T], mut f: impl FnMut(&mut Self, &T)) {
        self.label(name);
        self.indent(|out| {
            for item in items {
                f(out, item);
            }
        });
    }

    fn single<T>(&mut self, name: &str, item: Option<&T>, f: impl FnOnce(&mut Self, &T)) {
        self.label(name);
        self.indent(|out| {
            if let Some(item) = item {
                f(out, item);
            }
        });
    }

    // -- expressions -------------------------------------------------------

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Literal(v) => self.node("Literal", &[("value", lit(v))]),
            ExprKind::Variable(n) => self.node("Variable", &[("name", quote(&n.text))]),
            ExprKind::Assign { name, value } => {
                self.node("Assign", &[("name", quote(&name.text))]);
                self.indent(|out| out.single("value", Some(&**value), Dumper::expr));
            }
            ExprKind::Binary { left, op, right } => {
                self.node("Binary", &[("op", quote(op.lexeme()))]);
                self.indent(|out| {
                    out.single("left", Some(&**left), Dumper::expr);
                    out.single("right", Some(&**right), Dumper::expr);
                });
            }
            ExprKind::Logical { left, op, right } => {
                self.node("Logical", &[("op", quote(op.lexeme()))]);
                self.indent(|out| {
                    out.single("left", Some(&**left), Dumper::expr);
                    out.single("right", Some(&**right), Dumper::expr);
                });
            }
            ExprKind::Unary { op, right } => {
                self.node("Unary", &[("op", quote(op.lexeme()))]);
                self.indent(|out| out.single("right", Some(&**right), Dumper::expr));
            }
            ExprKind::Grouping(inner) => {
                self.node("Grouping", &[]);
                self.indent(|out| out.single("expression", Some(&**inner), Dumper::expr));
            }
            ExprKind::Call { callee, args } => {
                self.node("Call", &[]);
                self.indent(|out| {
                    out.single("callee", Some(&**callee), Dumper::expr);
                    out.group("arguments", args, Dumper::expr);
                });
            }
            ExprKind::Array(elements) => {
                self.node("Array", &[]);
                self.indent(|out| out.group("elements", elements, Dumper::expr));
            }
            ExprKind::Index { target, index } => {
                self.node("Index", &[]);
                self.indent(|out| {
                    out.single("target", Some(&**target), Dumper::expr);
                    out.single("index", Some(&**index), Dumper::expr);
                });
            }
            ExprKind::IndexAssign {
                target,
                index,
                value,
            } => {
                self.node("IndexAssign", &[]);
                self.indent(|out| {
                    out.single("target", Some(&**target), Dumper::expr);
                    out.single("index", Some(&**index), Dumper::expr);
                    out.single("value", Some(&**value), Dumper::expr);
                });
            }
            ExprKind::Dict(pairs) => {
                self.node("Dict", &[]);
                self.indent(|out| {
                    out.group("pairs", pairs, |out, (key, value)| {
                        out.node("Pair", &[]);
                        out.indent(|out| {
                            out.single("key", Some(key), Dumper::expr);
                            out.single("value", Some(value), Dumper::expr);
                        });
                    })
                });
            }
            ExprKind::Spread(value) => {
                self.node("Spread", &[]);
                self.indent(|out| out.single("value", Some(&**value), Dumper::expr));
            }
            ExprKind::Function {
                name,
                params,
                body,
                is_generator,
            } => {
                self.node(
                    "FunctionExpr",
                    &[
                        ("name", opt_name(name.as_ref())),
                        ("generator", bool_attr(*is_generator)),
                    ],
                );
                self.indent(|out| {
                    out.group("params", params, Dumper::param);
                    out.group("body", body, Dumper::stmt);
                });
            }
            ExprKind::Interpolation(parts) => {
                self.node("Interpolation", &[]);
                self.indent(|out| {
                    out.group("parts", parts, |out, part| match part {
                        InterpPart::Text(text) => out.node("Text", &[("value", quote(text))]),
                        InterpPart::Expr(e) => out.expr(e),
                    })
                });
            }
            ExprKind::Match { subject, arms } => {
                self.node("MatchExpr", &[]);
                self.indent(|out| {
                    out.single("subject", Some(&**subject), Dumper::expr);
                    out.group("arms", arms, |out, arm| {
                        out.node("Arm", &[]);
                        out.indent(|out| {
                            out.single("pattern", arm.pattern.as_ref(), Dumper::match_pattern);
                            out.single("guard", arm.guard.as_ref(), Dumper::expr);
                            out.single("value", Some(&arm.value), Dumper::expr);
                        });
                    });
                });
            }
            ExprKind::Yield(value) => {
                self.node("YieldExpr", &[]);
                self.indent(|out| out.single("value", Some(&**value), Dumper::expr));
            }
        }
    }

    // -- statements --------------------------------------------------------

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Expression(e) => {
                self.node("Expression", &[]);
                self.indent(|out| out.single("expression", Some(e), Dumper::expr));
            }
            StmtKind::Print(values) => {
                self.node("Print", &[]);
                self.indent(|out| out.group("values", values, Dumper::expr));
            }
            StmtKind::Var {
                pattern,
                initializer,
            } => {
                self.node("Var", &[]);
                self.indent(|out| {
                    out.single("pattern", Some(pattern), Dumper::pattern);
                    out.single("initializer", initializer.as_ref(), Dumper::expr);
                });
            }
            StmtKind::DestructureAssign { pattern, value } => {
                self.node("DestructureAssign", &[]);
                self.indent(|out| {
                    out.single("pattern", Some(pattern), Dumper::pattern);
                    out.single("value", Some(value), Dumper::expr);
                });
            }
            StmtKind::Block(statements) => {
                self.node("Block", &[]);
                self.indent(|out| out.group("statements", statements, Dumper::stmt));
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.node("If", &[]);
                self.indent(|out| {
                    out.single("condition", Some(condition), Dumper::expr);
                    out.single("then", Some(&**then_branch), Dumper::stmt);
                    out.single("else", else_branch.as_deref(), Dumper::stmt);
                });
            }
            StmtKind::While { condition, body } => {
                self.node("While", &[]);
                self.indent(|out| {
                    out.single("condition", Some(condition), Dumper::expr);
                    out.single("body", Some(&**body), Dumper::stmt);
                });
            }
            StmtKind::For {
                initializer,
                condition,
                increment,
                body,
            } => {
                self.node("For", &[]);
                self.indent(|out| {
                    out.single("initializer", initializer.as_deref(), Dumper::stmt);
                    out.single("condition", condition.as_ref(), Dumper::expr);
                    out.single("increment", increment.as_ref(), Dumper::expr);
                    out.single("body", Some(&**body), Dumper::stmt);
                });
            }
            StmtKind::ForIn {
                pattern,
                iterable,
                body,
            } => {
                self.node("ForIn", &[]);
                self.indent(|out| {
                    out.single("pattern", Some(pattern), Dumper::pattern);
                    out.single("iterable", Some(iterable), Dumper::expr);
                    out.single("body", Some(&**body), Dumper::stmt);
                });
            }
            StmtKind::Function {
                name,
                params,
                body,
                is_generator,
            } => {
                self.node(
                    "Function",
                    &[
                        ("name", quote(&name.text)),
                        ("generator", bool_attr(*is_generator)),
                    ],
                );
                self.indent(|out| {
                    out.group("params", params, Dumper::param);
                    out.group("body", body, Dumper::stmt);
                });
            }
            StmtKind::Return(value) => {
                self.node("Return", &[]);
                self.indent(|out| out.single("value", value.as_ref(), Dumper::expr));
            }
            StmtKind::Break => self.node("Break", &[]),
            StmtKind::Continue => self.node("Continue", &[]),
            StmtKind::Throw(value) => {
                self.node("Throw", &[]);
                self.indent(|out| out.single("value", Some(value), Dumper::expr));
            }
            StmtKind::Yield { value, delegate } => {
                self.node("Yield", &[("delegate", bool_attr(*delegate))]);
                self.indent(|out| out.single("value", Some(value), Dumper::expr));
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.node("Try", &[]);
                self.indent(|out| {
                    out.group("body", body, Dumper::stmt);
                    out.group("catches", catches, |out, c| {
                        out.node("Catch", &[]);
                        out.indent(|out| {
                            out.single("pattern", Some(&c.pattern), Dumper::pattern);
                            out.single("guard", c.guard.as_ref(), Dumper::expr);
                            out.group("body", &c.body, Dumper::stmt);
                        });
                    });
                    out.label("finally");
                    out.indent(|out| {
                        if let Some(block) = finally {
                            for stmt in block {
                                out.stmt(stmt);
                            }
                        }
                    });
                });
            }
            StmtKind::Match { subject, cases } => {
                self.node("Match", &[]);
                self.indent(|out| {
                    out.single("subject", Some(subject), Dumper::expr);
                    out.group("cases", cases, |out, case| {
                        out.node("Case", &[]);
                        out.indent(|out| {
                            out.single("pattern", case.pattern.as_ref(), Dumper::match_pattern);
                            out.single("guard", case.guard.as_ref(), Dumper::expr);
                            out.group("body", &case.body, Dumper::stmt);
                        });
                    });
                });
            }
            StmtKind::Struct {
                name,
                fields,
                methods,
            } => {
                self.node("Struct", &[("name", quote(&name.text))]);
                self.indent(|out| {
                    out.group("fields", fields, Dumper::param);
                    out.group("methods", methods, Dumper::stmt);
                });
            }
            StmtKind::Import {
                names,
                namespace,
                specifier,
            } => {
                self.node(
                    "Import",
                    &[
                        ("specifier", quote(specifier)),
                        ("namespace", opt_name(namespace.as_ref())),
                    ],
                );
                self.indent(|out| {
                    out.group("names", names, |out, (exported, local)| {
                        out.node(
                            "ImportName",
                            &[
                                ("exported", quote(&exported.text)),
                                ("local", quote(&local.text)),
                            ],
                        );
                    })
                });
            }
            StmtKind::Export { declaration, name } => {
                self.node("Export", &[("name", quote(&name.text))]);
                self.indent(|out| out.single("declaration", Some(&**declaration), Dumper::stmt));
            }
            StmtKind::ExportNames { names, specifier } => {
                self.node(
                    "ExportNames",
                    &[(
                        "specifier",
                        specifier.as_ref().map(|s| quote(s)).unwrap_or("-".into()),
                    )],
                );
                self.indent(|out| {
                    out.group("names", names, |out, (local, exported)| {
                        out.node(
                            "ExportName",
                            &[
                                ("local", quote(&local.text)),
                                ("exported", quote(&exported.text)),
                            ],
                        );
                    })
                });
            }
        }
    }

    // -- patterns ----------------------------------------------------------

    fn param(&mut self, p: &Param) {
        self.node("Param", &[("rest", bool_attr(p.rest))]);
        self.indent(|out| out.single("pattern", Some(&p.pattern), Dumper::pattern));
    }

    fn pattern(&mut self, p: &Pattern) {
        match p {
            Pattern::Name { name, default } => {
                self.node("NamePattern", &[("name", quote(&name.text))]);
                self.indent(|out| out.single("default", default.as_deref(), Dumper::expr));
            }
            Pattern::Array {
                elements,
                rest,
                default,
                ..
            } => {
                self.node("ArrayPattern", &[("rest", opt_name(rest.as_ref()))]);
                self.indent(|out| {
                    out.group("elements", elements, Dumper::pattern);
                    out.single("default", default.as_deref(), Dumper::expr);
                });
            }
            Pattern::Object {
                entries,
                rest,
                default,
                ..
            } => {
                self.node("ObjectPattern", &[("rest", opt_name(rest.as_ref()))]);
                self.indent(|out| {
                    out.group("entries", entries, |out, (key, sub)| {
                        out.node("Entry", &[("key", quote(key))]);
                        out.indent(|out| out.single("pattern", Some(sub), Dumper::pattern));
                    });
                    out.single("default", default.as_deref(), Dumper::expr);
                });
            }
        }
    }

    fn match_pattern(&mut self, p: &MatchPattern) {
        match p {
            MatchPattern::Literal { value, .. } => {
                self.node("LiteralMatch", &[("value", lit(value))])
            }
            MatchPattern::Bind { name } => self.node("BindMatch", &[("name", quote(&name.text))]),
            MatchPattern::Array { elements, rest, .. } => {
                self.node("ArrayMatch", &[("rest", opt_name(rest.as_ref()))]);
                self.indent(|out| out.group("elements", elements, Dumper::match_pattern));
            }
            MatchPattern::Object { entries, .. } => {
                self.node("ObjectMatch", &[]);
                self.indent(|out| {
                    out.group("entries", entries, |out, (key, sub)| {
                        out.node("Entry", &[("key", quote(key))]);
                        out.indent(|out| out.single("pattern", Some(sub), Dumper::match_pattern));
                    })
                });
            }
            MatchPattern::Struct { name, elements, .. } => {
                self.node("StructMatch", &[("name", quote(&name.text))]);
                self.indent(|out| out.group("elements", elements, Dumper::match_pattern));
            }
        }
    }
}

fn lit(value: &LitValue) -> String {
    match value {
        LitValue::Null => "null".to_string(),
        LitValue::Bool(b) => format!("B:{b}"),
        LitValue::Number(n) => format!("N:{:016x}", n.to_bits()),
        LitValue::Str(s) => format!("S:{}", quote(s)),
    }
}

fn bool_attr(b: bool) -> String {
    if b {
        "true".into()
    } else {
        "false".into()
    }
}

fn opt_name(name: Option<&Name>) -> String {
    name.map(|n| quote(&n.text)).unwrap_or("-".into())
}
