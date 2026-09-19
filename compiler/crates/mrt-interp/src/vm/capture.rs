//! Which names a function body's *nested* functions mention.
//!
//! A variable can live in a frame slot only if nothing outlives the frame.
//! MRT closures capture by reference -- `var n = 1; var f = func(){return n;};
//! n = 2;` makes `f()` answer 2 -- so a captured variable has to stay in the
//! shared `Env` where both the frame and the closure see the same cell.
//!
//! The analysis is deliberately conservative: *any* mention of a name inside
//! *any* nested function disqualifies it, even where the nested function
//! declares its own variable of that name and could not possibly be
//! referring to the outer one. Being wrong in this direction costs a slot;
//! being wrong in the other direction would make a closure read a stale copy,
//! which is a silent wrong answer.

use std::collections::HashSet;

use mrt_ast::*;

/// Every identifier mentioned anywhere inside a nested function of `body`.
pub fn captured_names(body: &[Stmt]) -> HashSet<String> {
    let mut found = HashSet::new();
    for stmt in body {
        scan_stmt(stmt, false, &mut found);
    }
    found
}

/// `inside` is true once the walk has descended into a nested function, at
/// which point every identifier counts.
fn scan_stmt(stmt: &Stmt, inside: bool, found: &mut HashSet<String>) {
    match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => scan_expr(e, inside, found),
        StmtKind::Print(items) => items.iter().for_each(|e| scan_expr(e, inside, found)),
        StmtKind::Var {
            pattern,
            initializer,
        } => {
            scan_pattern(pattern, inside, found);
            if let Some(e) = initializer {
                scan_expr(e, inside, found);
            }
        }
        StmtKind::DestructureAssign { pattern, value } => {
            scan_pattern(pattern, inside, found);
            scan_expr(value, inside, found);
        }
        StmtKind::Block(body) => body.iter().for_each(|s| scan_stmt(s, inside, found)),
        StmtKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr(condition, inside, found);
            scan_stmt(then_branch, inside, found);
            if let Some(s) = else_branch {
                scan_stmt(s, inside, found);
            }
        }
        StmtKind::While { condition, body } => {
            scan_expr(condition, inside, found);
            scan_stmt(body, inside, found);
        }
        StmtKind::For {
            initializer,
            condition,
            increment,
            body,
        } => {
            if let Some(s) = initializer {
                scan_stmt(s, inside, found);
            }
            if let Some(e) = condition {
                scan_expr(e, inside, found);
            }
            if let Some(e) = increment {
                scan_expr(e, inside, found);
            }
            scan_stmt(body, inside, found);
        }
        StmtKind::ForIn {
            pattern,
            iterable,
            body,
        } => {
            scan_pattern(pattern, inside, found);
            scan_expr(iterable, inside, found);
            scan_stmt(body, inside, found);
        }
        // The body of a nested function: everything below here counts.
        StmtKind::Function { params, body, .. } => {
            params
                .iter()
                .for_each(|p| scan_pattern(&p.pattern, true, found));
            body.iter().for_each(|s| scan_stmt(s, true, found));
        }
        StmtKind::Return(value) => {
            if let Some(e) = value {
                scan_expr(e, inside, found);
            }
        }
        StmtKind::Yield { value, .. } => scan_expr(value, inside, found),
        StmtKind::Try {
            body,
            catches,
            finally,
        } => {
            body.iter().for_each(|s| scan_stmt(s, inside, found));
            for clause in catches {
                scan_pattern(&clause.pattern, inside, found);
                clause.body.iter().for_each(|s| scan_stmt(s, inside, found));
            }
            if let Some(block) = finally {
                block.iter().for_each(|s| scan_stmt(s, inside, found));
            }
        }
        StmtKind::Match { subject, cases } => {
            scan_expr(subject, inside, found);
            for case in cases {
                if let Some(guard) = &case.guard {
                    scan_expr(guard, inside, found);
                }
                case.body.iter().for_each(|s| scan_stmt(s, inside, found));
            }
        }
        // A method body is a nested function body.
        StmtKind::Struct {
            fields, methods, ..
        } => {
            fields
                .iter()
                .for_each(|p| scan_pattern(&p.pattern, true, found));
            methods.iter().for_each(|s| scan_stmt(s, true, found));
        }
        StmtKind::Export { declaration, .. } => scan_stmt(declaration, inside, found),
        StmtKind::Break
        | StmtKind::Continue
        | StmtKind::Import { .. }
        | StmtKind::ExportNames { .. } => {}
    }
}

fn scan_expr(expr: &Expr, inside: bool, found: &mut HashSet<String>) {
    match &expr.kind {
        ExprKind::Literal(_) => {}
        ExprKind::Variable(name) => {
            if inside {
                found.insert(name.text.clone());
            }
        }
        ExprKind::Assign { name, value } => {
            if inside {
                found.insert(name.text.clone());
            }
            scan_expr(value, inside, found);
        }
        ExprKind::Binary { left, right, .. } | ExprKind::Logical { left, right, .. } => {
            scan_expr(left, inside, found);
            scan_expr(right, inside, found);
        }
        ExprKind::Unary { right, .. } => scan_expr(right, inside, found),
        ExprKind::Grouping(e) | ExprKind::Spread(e) | ExprKind::Yield(e) => {
            scan_expr(e, inside, found)
        }
        ExprKind::Call { callee, args } => {
            scan_expr(callee, inside, found);
            args.iter().for_each(|e| scan_expr(e, inside, found));
        }
        ExprKind::Array(items) => items.iter().for_each(|e| scan_expr(e, inside, found)),
        ExprKind::Index { target, index } => {
            scan_expr(target, inside, found);
            scan_expr(index, inside, found);
        }
        ExprKind::IndexAssign {
            target,
            index,
            value,
        } => {
            scan_expr(target, inside, found);
            scan_expr(index, inside, found);
            scan_expr(value, inside, found);
        }
        ExprKind::Dict(pairs) => {
            for (key, value) in pairs {
                scan_expr(key, inside, found);
                scan_expr(value, inside, found);
            }
        }
        ExprKind::Interpolation(parts) => {
            for part in parts {
                if let InterpPart::Expr(e) = part {
                    scan_expr(e, inside, found);
                }
            }
        }
        // A function literal: everything inside it counts.
        ExprKind::Function { params, body, .. } => {
            params
                .iter()
                .for_each(|p| scan_pattern(&p.pattern, true, found));
            body.iter().for_each(|s| scan_stmt(s, true, found));
        }
        ExprKind::Match { subject, arms } => {
            scan_expr(subject, inside, found);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    scan_expr(guard, inside, found);
                }
                scan_expr(&arm.value, inside, found);
            }
        }
    }
}

/// A pattern's own defaults can mention names, and a pattern inside a nested
/// function binds names that must not be slotted outside it.
fn scan_pattern(pattern: &Pattern, inside: bool, found: &mut HashSet<String>) {
    match pattern {
        Pattern::Name { name, default } => {
            if inside {
                found.insert(name.text.clone());
            }
            if let Some(e) = default {
                scan_expr(e, inside, found);
            }
        }
        Pattern::Array { elements, rest, .. } => {
            for element in elements.iter() {
                scan_pattern(element, inside, found);
            }
            if let (true, Some(rest)) = (inside, rest) {
                found.insert(rest.text.clone());
            }
        }
        Pattern::Object { entries, rest, .. } => {
            for (_, value) in entries {
                scan_pattern(value, inside, found);
            }
            if let (true, Some(rest)) = (inside, rest) {
                found.insert(rest.text.clone());
            }
        }
    }
}
