//! Static resolution of names to frame slots.
//!
//! MRT 1.x resolves every name at run time by walking a chain of
//! `Environment` dictionaries, so reading a variable costs a hash lookup per
//! enclosing scope. This pass does that work once, at compile time: each name
//! becomes either a numbered slot in the current frame, an index into that
//! frame's capture list, or a global looked up by name.
//!
//! It is the stage that makes a bytecode backend possible, and it is useful
//! before one exists: the same analysis would let the existing tree-walker
//! replace its dictionary chain with a flat array.
//!
//! # This pass reports no errors, on purpose
//!
//! MRT's semantics are dynamic: a name the resolver cannot find is not a
//! mistake, it is a global, and whether it exists is decided when the program
//! runs. Reporting "undefined variable" here would reject programs the
//! reference implementation accepts, which is exactly the divergence the
//! conformance harness exists to prevent. So this pass classifies and never
//! rejects.
//!
//! # What it has to mirror
//!
//! * **The module scope is global, not a frame.** A top-level `var` is stored
//!   in the interpreter's `globals`, so it resolves as a global even from the
//!   same file.
//! * **Blocks are scopes but not frames.** A `{ }` block, a `for` header, a
//!   `catch` clause and each `match` case introduce a scope; all of them draw
//!   slots from the enclosing *function's* single slot space.
//! * **`for`-`in` binds afresh on each iteration.** A closure made in the body
//!   captures that iteration's value, so its loop variable is flagged
//!   `per_iteration`: a backend must give each turn of the loop its own cell
//!   rather than reusing one slot.
//! * **A method's `this`** is bound by a wrapper scope around the method's
//!   closure, and is modelled here as a local of the method.

use std::collections::{BTreeSet, HashMap};

use mrt_ast::*;
use mrt_diagnostics::Span;

/// Where a name lives once resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// A numbered slot in the current function's frame.
    Local { slot: u32 },
    /// An entry in the current function's capture list.
    Capture { index: u32 },
    /// Not found in any enclosing function: looked up by name at run time.
    /// Built-ins and everything declared at the top level of a module land
    /// here.
    Global,
}

/// Where a captured name comes from, one level up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureFrom {
    ParentLocal(u32),
    ParentCapture(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capture {
    pub name: String,
    pub from: CaptureFrom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    pub slot: u32,
    pub name: String,
    /// Some inner function closes over this local, so it cannot live in a
    /// plain frame slot that dies with the call.
    pub captured: bool,
    /// Bound afresh on each turn of a `for`-`in` loop.
    pub per_iteration: bool,
}

/// One function's frame layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionScope {
    pub name: Option<String>,
    pub span: Span,
    pub slot_count: u32,
    pub locals: Vec<Local>,
    pub captures: Vec<Capture>,
    /// Index into `Resolved::functions` of the enclosing function, if any.
    pub parent: Option<usize>,
    pub uses: Vec<Use>,
}

/// One resolved mention of a name, in traversal order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Use {
    pub name: String,
    pub span: Span,
    pub resolution: Resolution,
}

#[derive(Clone, Debug, Default)]
pub struct Resolved {
    pub functions: Vec<FunctionScope>,
    /// Every resolved name mention, keyed by the span of the name itself.
    /// Spans are unique per occurrence in a file, which is what makes them
    /// usable as identity here.
    pub by_span: HashMap<Span, Resolution>,
    /// Name mentions at module level, which are always globals.
    pub module_uses: Vec<Use>,
}

pub fn resolve(program: &Program) -> Resolved {
    let mut r = Resolver::default();
    for stmt in &program.statements {
        r.stmt(stmt);
    }
    let mut out = r.out;
    out.module_uses = r.module_uses;
    out
}

#[derive(Default)]
struct Frame {
    /// Block scopes within this function, innermost last.
    scopes: Vec<HashMap<String, u32>>,
    slot_count: u32,
    locals: Vec<Local>,
    captures: Vec<Capture>,
    capture_index: HashMap<String, u32>,
    captured_slots: BTreeSet<u32>,
    per_iteration_slots: BTreeSet<u32>,
    uses: Vec<Use>,
    name: Option<String>,
    span: Span,
    parent: Option<usize>,
    /// Index reserved in `Resolved::functions` before the body is walked, so
    /// nested functions can name this one as their parent.
    index: usize,
}

#[derive(Default)]
struct Resolver {
    frames: Vec<Frame>,
    out: Resolved,
    module_uses: Vec<Use>,
}

impl Resolver {
    // -- scope plumbing ----------------------------------------------------

    fn in_function(&self) -> bool {
        !self.frames.is_empty()
    }

    fn push_function(&mut self, name: Option<String>, span: Span) {
        let parent = self.frames.last().map(|f| f.index);
        let index = self.out.functions.len();
        // Reserve the slot so nested functions can refer to this one.
        self.out.functions.push(FunctionScope {
            name: name.clone(),
            span,
            slot_count: 0,
            locals: Vec::new(),
            captures: Vec::new(),
            parent,
            uses: Vec::new(),
        });
        self.frames.push(Frame {
            scopes: vec![HashMap::new()],
            name,
            span,
            parent,
            index,
            ..Frame::default()
        });
    }

    fn pop_function(&mut self) {
        let frame = self.frames.pop().expect("push/pop are balanced");
        let locals = frame
            .locals
            .into_iter()
            .map(|mut l| {
                l.captured = frame.captured_slots.contains(&l.slot);
                l.per_iteration = frame.per_iteration_slots.contains(&l.slot);
                l
            })
            .collect();
        self.out.functions[frame.index] = FunctionScope {
            name: frame.name,
            span: frame.span,
            slot_count: frame.slot_count,
            locals,
            captures: frame.captures,
            parent: frame.parent,
            uses: frame.uses,
        };
    }

    fn push_scope(&mut self) {
        if let Some(frame) = self.frames.last_mut() {
            frame.scopes.push(HashMap::new());
        }
    }

    fn pop_scope(&mut self) {
        if let Some(frame) = self.frames.last_mut() {
            frame.scopes.pop();
        }
    }

    /// Declare a name in the innermost scope. At module level there is no
    /// frame, so the name is simply a global and nothing is recorded.
    fn declare(&mut self, name: &str) -> Option<u32> {
        let frame = self.frames.last_mut()?;
        let slot = frame.slot_count;
        frame.slot_count += 1;
        frame
            .scopes
            .last_mut()
            .expect("a frame always has one scope")
            .insert(name.to_string(), slot);
        frame.locals.push(Local {
            slot,
            name: name.to_string(),
            captured: false,
            per_iteration: false,
        });
        Some(slot)
    }

    fn mark_per_iteration(&mut self, slot: u32) {
        if let Some(frame) = self.frames.last_mut() {
            frame.per_iteration_slots.insert(slot);
        }
    }

    fn lookup_local(frame: &Frame, name: &str) -> Option<u32> {
        frame
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    /// Thread a capture from wherever `name` lives down to frame `fi`.
    fn resolve_capture(&mut self, fi: usize, name: &str) -> Option<u32> {
        if fi == 0 {
            // Above the outermost function is the module scope, which is
            // global -- nothing to capture.
            return None;
        }
        let parent = fi - 1;
        if let Some(slot) = Self::lookup_local(&self.frames[parent], name) {
            self.frames[parent].captured_slots.insert(slot);
            return Some(self.add_capture(fi, name, CaptureFrom::ParentLocal(slot)));
        }
        let index = self.resolve_capture(parent, name)?;
        Some(self.add_capture(fi, name, CaptureFrom::ParentCapture(index)))
    }

    fn add_capture(&mut self, fi: usize, name: &str, from: CaptureFrom) -> u32 {
        let frame = &mut self.frames[fi];
        if let Some(existing) = frame.capture_index.get(name) {
            return *existing;
        }
        let index = frame.captures.len() as u32;
        frame.captures.push(Capture {
            name: name.to_string(),
            from,
        });
        frame.capture_index.insert(name.to_string(), index);
        index
    }

    fn resolve_name(&mut self, name: &Name) {
        let resolution = if self.frames.is_empty() {
            Resolution::Global
        } else {
            let last = self.frames.len() - 1;
            if let Some(slot) = Self::lookup_local(&self.frames[last], &name.text) {
                Resolution::Local { slot }
            } else if let Some(index) = self.resolve_capture(last, &name.text) {
                Resolution::Capture { index }
            } else {
                Resolution::Global
            }
        };

        let use_ = Use {
            name: name.text.clone(),
            span: name.span,
            resolution,
        };
        self.out.by_span.insert(name.span, resolution);
        match self.frames.last_mut() {
            Some(frame) => frame.uses.push(use_),
            None => self.module_uses.push(use_),
        }
    }

    // -- traversal ---------------------------------------------------------

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Expression(e) => self.expr(e),
            StmtKind::Print(values) => values.iter().for_each(|e| self.expr(e)),
            StmtKind::Var {
                pattern,
                initializer,
            } => {
                // The initializer is evaluated before the binding exists, so
                // it is resolved first -- `var x = x;` reads the outer `x`.
                if let Some(e) = initializer {
                    self.expr(e);
                }
                self.declare_pattern(pattern, false);
            }
            StmtKind::DestructureAssign { pattern, value } => {
                self.expr(value);
                // Assignment, not declaration: every leaf is an existing name.
                self.use_pattern(pattern);
            }
            StmtKind::Block(statements) => {
                self.push_scope();
                statements.iter().for_each(|s| self.stmt(s));
                self.pop_scope();
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                self.stmt(then_branch);
                if let Some(e) = else_branch {
                    self.stmt(e);
                }
            }
            StmtKind::While { condition, body } => {
                self.expr(condition);
                self.stmt(body);
            }
            StmtKind::For {
                initializer,
                condition,
                increment,
                body,
            } => {
                // The header gets its own scope so a `var` in the initializer
                // does not leak into the surrounding block.
                self.push_scope();
                if let Some(s) = initializer {
                    self.stmt(s);
                }
                if let Some(e) = condition {
                    self.expr(e);
                }
                if let Some(e) = increment {
                    self.expr(e);
                }
                self.stmt(body);
                self.pop_scope();
            }
            StmtKind::ForIn {
                pattern,
                iterable,
                body,
            } => {
                self.expr(iterable);
                self.push_scope();
                self.declare_pattern(pattern, true);
                self.stmt(body);
                self.pop_scope();
            }
            StmtKind::Function {
                name, params, body, ..
            } => {
                // The name binds in the enclosing scope; the body is its own
                // frame. Declared before the body is walked so the function
                // can call itself.
                self.declare(&name.text);
                self.resolve_function(Some(name.text.clone()), s.span, params, body, false);
            }
            StmtKind::Return(value) => {
                if let Some(e) = value {
                    self.expr(e);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Throw(e) => self.expr(e),
            StmtKind::Yield { value, .. } => self.expr(value),
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.push_scope();
                body.iter().for_each(|s| self.stmt(s));
                self.pop_scope();
                for clause in catches {
                    self.push_scope();
                    self.declare_pattern(&clause.pattern, false);
                    if let Some(g) = &clause.guard {
                        self.expr(g);
                    }
                    clause.body.iter().for_each(|s| self.stmt(s));
                    self.pop_scope();
                }
                if let Some(block) = finally {
                    self.push_scope();
                    block.iter().for_each(|s| self.stmt(s));
                    self.pop_scope();
                }
            }
            StmtKind::Match { subject, cases } => {
                self.expr(subject);
                for case in cases {
                    self.push_scope();
                    if let Some(p) = &case.pattern {
                        self.declare_match_pattern(p);
                    }
                    if let Some(g) = &case.guard {
                        self.expr(g);
                    }
                    case.body.iter().for_each(|s| self.stmt(s));
                    self.pop_scope();
                }
            }
            StmtKind::Struct {
                name,
                fields,
                methods,
            } => {
                self.declare(&name.text);
                // Field defaults are evaluated at construction time in the
                // instance's own scope, alongside the fields to their left.
                self.push_scope();
                for field in fields {
                    if let Some(default) = field.pattern.default() {
                        self.expr(default);
                    }
                    self.declare_pattern(&field.pattern, false);
                }
                self.pop_scope();
                for method in methods {
                    let StmtKind::Function {
                        name: mname,
                        params,
                        body,
                        ..
                    } = &method.kind
                    else {
                        continue;
                    };
                    self.resolve_function(
                        Some(mname.text.clone()),
                        method.span,
                        params,
                        body,
                        true,
                    );
                }
            }
            StmtKind::Import {
                names, namespace, ..
            } => {
                for (_, local) in names {
                    self.declare(&local.text);
                }
                if let Some(ns) = namespace {
                    self.declare(&ns.text);
                }
            }
            StmtKind::Export { declaration, .. } => self.stmt(declaration),
            StmtKind::ExportNames { names, specifier } => {
                // A re-export with a source binds nothing locally; without
                // one, the names must already exist here.
                if specifier.is_none() {
                    for (local, _) in names {
                        self.resolve_name(local);
                    }
                }
            }
        }
    }

    fn resolve_function(
        &mut self,
        name: Option<String>,
        span: Span,
        params: &[Param],
        body: &[Stmt],
        is_method: bool,
    ) {
        self.push_function(name, span);
        if is_method {
            // `this` is bound by a wrapper scope around the method's closure;
            // modelled here as a local of the method itself.
            self.declare("this");
        }
        for param in params {
            // A default may refer to a parameter to its left, so resolve it
            // before declaring this one.
            if let Some(default) = param.pattern.default() {
                self.expr(default);
            }
            self.declare_pattern(&param.pattern, false);
        }
        body.iter().for_each(|s| self.stmt(s));
        self.pop_function();
    }

    fn declare_pattern(&mut self, pattern: &Pattern, per_iteration: bool) {
        match pattern {
            Pattern::Name { name, default } => {
                if let Some(d) = default {
                    self.expr(d);
                }
                if let Some(slot) = self.declare(&name.text) {
                    if per_iteration {
                        self.mark_per_iteration(slot);
                    }
                }
            }
            Pattern::Array {
                elements,
                rest,
                default,
                ..
            } => {
                if let Some(d) = default {
                    self.expr(d);
                }
                for element in elements {
                    self.declare_pattern(element, per_iteration);
                }
                if let Some(rest) = rest {
                    if let Some(slot) = self.declare(&rest.text) {
                        if per_iteration {
                            self.mark_per_iteration(slot);
                        }
                    }
                }
            }
            Pattern::Object {
                entries,
                rest,
                default,
                ..
            } => {
                if let Some(d) = default {
                    self.expr(d);
                }
                for (_, sub) in entries {
                    self.declare_pattern(sub, per_iteration);
                }
                if let Some(rest) = rest {
                    if let Some(slot) = self.declare(&rest.text) {
                        if per_iteration {
                            self.mark_per_iteration(slot);
                        }
                    }
                }
            }
        }
    }

    /// A destructuring *assignment*: every leaf names something that already
    /// exists, so each is a use rather than a declaration.
    fn use_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Name { name, default } => {
                if let Some(d) = default {
                    self.expr(d);
                }
                self.resolve_name(name);
            }
            Pattern::Array {
                elements,
                rest,
                default,
                ..
            } => {
                if let Some(d) = default {
                    self.expr(d);
                }
                elements.iter().for_each(|e| self.use_pattern(e));
                if let Some(rest) = rest {
                    self.resolve_name(rest);
                }
            }
            Pattern::Object {
                entries,
                rest,
                default,
                ..
            } => {
                if let Some(d) = default {
                    self.expr(d);
                }
                entries.iter().for_each(|(_, sub)| self.use_pattern(sub));
                if let Some(rest) = rest {
                    self.resolve_name(rest);
                }
            }
        }
    }

    fn declare_match_pattern(&mut self, pattern: &MatchPattern) {
        match pattern {
            MatchPattern::Literal { .. } => {}
            MatchPattern::Bind { name } => {
                self.declare(&name.text);
            }
            MatchPattern::Array { elements, rest, .. } => {
                elements.iter().for_each(|e| self.declare_match_pattern(e));
                if let Some(rest) = rest {
                    self.declare(&rest.text);
                }
            }
            MatchPattern::Object { entries, .. } => {
                entries
                    .iter()
                    .for_each(|(_, sub)| self.declare_match_pattern(sub));
            }
            MatchPattern::Struct { name, elements, .. } => {
                // The struct's name is read, not bound.
                self.resolve_name(name);
                elements.iter().for_each(|e| self.declare_match_pattern(e));
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Literal(_) => {}
            ExprKind::Variable(name) => self.resolve_name(name),
            ExprKind::Assign { name, value } => {
                self.expr(value);
                self.resolve_name(name);
            }
            ExprKind::Binary { left, right, .. } | ExprKind::Logical { left, right, .. } => {
                self.expr(left);
                self.expr(right);
            }
            ExprKind::Unary { right, .. } => self.expr(right),
            ExprKind::Grouping(inner) => self.expr(inner),
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                args.iter().for_each(|a| self.expr(a));
            }
            ExprKind::Array(elements) => elements.iter().for_each(|a| self.expr(a)),
            ExprKind::Index { target, index } => {
                self.expr(target);
                self.expr(index);
            }
            ExprKind::IndexAssign {
                target,
                index,
                value,
            } => {
                self.expr(target);
                self.expr(index);
                self.expr(value);
            }
            ExprKind::Dict(pairs) => {
                for (key, value) in pairs {
                    self.expr(key);
                    self.expr(value);
                }
            }
            ExprKind::Spread(value) => self.expr(value),
            ExprKind::Function {
                name, params, body, ..
            } => {
                self.resolve_function(
                    name.as_ref().map(|n| n.text.clone()),
                    e.span,
                    params,
                    body,
                    false,
                );
            }
            ExprKind::Interpolation(parts) => {
                for part in parts {
                    if let InterpPart::Expr(inner) = part {
                        self.expr(inner);
                    }
                }
            }
            ExprKind::Match { subject, arms } => {
                self.expr(subject);
                for arm in arms {
                    self.push_scope();
                    if let Some(p) = &arm.pattern {
                        self.declare_match_pattern(p);
                    }
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    self.expr(&arm.value);
                    self.pop_scope();
                }
            }
            ExprKind::Yield(value) => self.expr(value),
        }
        let _ = self.in_function();
    }
}

/// Check the resolution's internal consistency.
///
/// There is no Python counterpart to this pass, so the conformance harness
/// cannot check it the way it checks tokens and trees. What it can do is run
/// the resolver over every real program in the corpus and require these
/// invariants to hold, which catches the mistakes that matter: a slot or
/// capture index pointing at nothing, or a capture chain that does not line up
/// with the parent frame it names.
pub fn validate(resolved: &Resolved) -> Result<(), String> {
    for (i, f) in resolved.functions.iter().enumerate() {
        let name = f.name.clone().unwrap_or_else(|| "<anonymous>".into());

        if f.locals.len() as u32 != f.slot_count {
            return Err(format!(
                "function {name:?} (#{i}) declares {} locals but reserves {} slots",
                f.locals.len(),
                f.slot_count
            ));
        }
        for (expected, local) in f.locals.iter().enumerate() {
            if local.slot != expected as u32 {
                return Err(format!(
                    "function {name:?} (#{i}) local {:?} has slot {} at position {expected}",
                    local.name, local.slot
                ));
            }
        }

        // Each captured name must appear exactly once. Duplicates are
        // individually well-formed -- every one names a real parent local --
        // so nothing else here would notice, but a backend would allocate a
        // separate cell per entry and closures sharing a variable would stop
        // sharing it. Found by mutation-testing this very function.
        for (index, capture) in f.captures.iter().enumerate() {
            if let Some(earlier) = f.captures[..index]
                .iter()
                .position(|c| c.name == capture.name)
            {
                return Err(format!(
                    "function {name:?} (#{i}) captures {:?} twice, at {earlier} and {index}",
                    capture.name
                ));
            }
        }

        for (index, capture) in f.captures.iter().enumerate() {
            let Some(parent_index) = f.parent else {
                return Err(format!(
                    "function {name:?} (#{i}) captures {:?} but has no parent",
                    capture.name
                ));
            };
            let parent = &resolved.functions[parent_index];
            match capture.from {
                CaptureFrom::ParentLocal(slot) => {
                    let Some(local) = parent.locals.iter().find(|l| l.slot == slot) else {
                        return Err(format!(
                            "function {name:?} (#{i}) capture {index} names parent local {slot}, \
                             which does not exist"
                        ));
                    };
                    if !local.captured {
                        return Err(format!(
                            "function {name:?} (#{i}) captures parent local {slot} ({:?}) \
                             but that local is not flagged captured",
                            local.name
                        ));
                    }
                    if local.name != capture.name {
                        return Err(format!(
                            "function {name:?} (#{i}) capture {index} is named {:?} but parent \
                             local {slot} is named {:?}",
                            capture.name, local.name
                        ));
                    }
                }
                CaptureFrom::ParentCapture(idx) => {
                    let Some(outer) = parent.captures.get(idx as usize) else {
                        return Err(format!(
                            "function {name:?} (#{i}) capture {index} names parent capture \
                             {idx}, which does not exist"
                        ));
                    };
                    if outer.name != capture.name {
                        return Err(format!(
                            "function {name:?} (#{i}) capture {index} is named {:?} but parent \
                             capture {idx} is named {:?}",
                            capture.name, outer.name
                        ));
                    }
                }
            }
        }

        for use_ in &f.uses {
            match use_.resolution {
                Resolution::Local { slot } if slot >= f.slot_count => {
                    return Err(format!(
                        "function {name:?} (#{i}) resolves {:?} to slot {slot}, out of {} slots",
                        use_.name, f.slot_count
                    ));
                }
                Resolution::Capture { index } if index as usize >= f.captures.len() => {
                    return Err(format!(
                        "function {name:?} (#{i}) resolves {:?} to capture {index}, out of {}",
                        use_.name,
                        f.captures.len()
                    ));
                }
                _ => {}
            }
        }
    }

    for use_ in &resolved.module_uses {
        if use_.resolution != Resolution::Global {
            return Err(format!(
                "module-level use of {:?} resolved to {:?}, but the module scope is global",
                use_.name, use_.resolution
            ));
        }
    }

    Ok(())
}

/// A readable rendering of the resolution, for `mrt-check --dump-scopes`.
pub fn dump(resolved: &Resolved, file: &mrt_diagnostics::SourceFile) -> String {
    let mut out = String::from("module\n");
    for use_ in &resolved.module_uses {
        let at = file.line_col(use_.span.start);
        out.push_str(&format!("  use {:?} {at} -> global\n", use_.name));
    }
    let roots: Vec<usize> = (0..resolved.functions.len())
        .filter(|i| resolved.functions[*i].parent.is_none())
        .collect();
    for index in roots {
        render(resolved, file, index, 1, &mut out);
    }
    out
}

fn render(
    resolved: &Resolved,
    file: &mrt_diagnostics::SourceFile,
    index: usize,
    depth: usize,
    out: &mut String,
) {
    let f = &resolved.functions[index];
    let pad = "  ".repeat(depth);
    let name = f.name.clone().unwrap_or_else(|| "<anonymous>".into());
    out.push_str(&format!(
        "{pad}function {name:?} slots={} captures={}\n",
        f.slot_count,
        f.captures.len()
    ));
    for local in &f.locals {
        let mut flags = String::new();
        if local.captured {
            flags.push_str(" captured");
        }
        if local.per_iteration {
            flags.push_str(" per-iteration");
        }
        out.push_str(&format!(
            "{pad}  slot {} {:?}{flags}\n",
            local.slot, local.name
        ));
    }
    for (i, capture) in f.captures.iter().enumerate() {
        let from = match capture.from {
            CaptureFrom::ParentLocal(slot) => format!("parent local {slot}"),
            CaptureFrom::ParentCapture(idx) => format!("parent capture {idx}"),
        };
        out.push_str(&format!(
            "{pad}  capture {i} {:?} <- {from}\n",
            capture.name
        ));
    }
    for use_ in &f.uses {
        let at = file.line_col(use_.span.start);
        let where_ = match use_.resolution {
            Resolution::Local { slot } => format!("local {slot}"),
            Resolution::Capture { index } => format!("capture {index}"),
            Resolution::Global => "global".to_string(),
        };
        out.push_str(&format!("{pad}  use {:?} {at} -> {where_}\n", use_.name));
    }
    for child in 0..resolved.functions.len() {
        if resolved.functions[child].parent == Some(index) {
            render(resolved, file, child, depth + 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mrt_diagnostics::SourceFile;

    fn resolved(src: &str) -> Resolved {
        let file = SourceFile::new("t.mrt", src);
        let parsed = mrt_parser::parse(&file);
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        let r = resolve(&parsed.program);
        validate(&r).expect("invariants hold");
        r
    }

    fn function<'a>(r: &'a Resolved, name: &str) -> &'a FunctionScope {
        r.functions
            .iter()
            .find(|f| f.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no function {name:?}"))
    }

    fn uses_of<'a>(f: &'a FunctionScope, name: &str) -> Vec<&'a Use> {
        f.uses.iter().filter(|u| u.name == name).collect()
    }

    #[test]
    fn parameters_and_locals_get_slots_in_declaration_order() {
        let r = resolved("func f(a, b) { var c = 1; var d = 2; }");
        let f = function(&r, "f");
        assert_eq!(f.slot_count, 4);
        let names: Vec<&str> = f.locals.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn the_module_scope_is_global_not_a_frame() {
        // A top-level `var` is stored in the interpreter's globals, so even a
        // reference from the same file is a global.
        let r = resolved("var top = 1;\nfunc f() { return top; }");
        let f = function(&r, "f");
        assert_eq!(uses_of(f, "top")[0].resolution, Resolution::Global);
        // And no frame was created for the module itself.
        assert!(r.functions.iter().all(|f| f.name.as_deref() != Some("")));
    }

    #[test]
    fn builtins_resolve_as_globals() {
        let r = resolved("func f() { print(len([1])); }");
        let f = function(&r, "f");
        assert_eq!(uses_of(f, "len")[0].resolution, Resolution::Global);
    }

    #[test]
    fn a_closure_captures_and_flags_the_parent_local() {
        let r = resolved(
            "func makeCounter() { var count = 0; return func() { count += 1; return count; }; }",
        );
        let outer = function(&r, "makeCounter");
        let count = outer.locals.iter().find(|l| l.name == "count").unwrap();
        assert!(count.captured, "the parent local must be flagged");

        let inner = r
            .functions
            .iter()
            .find(|f| f.parent == Some(0) && f.name.is_none())
            .expect("the anonymous inner function");
        assert_eq!(inner.captures.len(), 1);
        assert_eq!(inner.captures[0].name, "count");
        assert_eq!(inner.captures[0].from, CaptureFrom::ParentLocal(count.slot));
        assert!(uses_of(inner, "count")
            .iter()
            .all(|u| u.resolution == Resolution::Capture { index: 0 }));
    }

    #[test]
    fn a_name_is_captured_once_however_often_it_is_used() {
        // `count += 1` desugars to `count = count + 1`, so this body mentions
        // `count` three times and must still produce one capture.
        let r =
            resolved("func f() { var count = 0; return func() { count += 1; return count; }; }");
        let inner = r.functions.iter().find(|f| f.parent == Some(0)).unwrap();
        assert_eq!(inner.captures.len(), 1);
        assert_eq!(uses_of(inner, "count").len(), 3);
    }

    #[test]
    fn a_capture_threads_through_intermediate_functions() {
        let r = resolved("func a() { var x = 1; return func() { return func() { return x; }; }; }");
        let middle = r.functions.iter().find(|f| f.parent == Some(0)).unwrap();
        let middle_index = r
            .functions
            .iter()
            .position(|f| std::ptr::eq(f, middle))
            .unwrap();
        let inner = r
            .functions
            .iter()
            .find(|f| f.parent == Some(middle_index))
            .unwrap();
        // The middle function does not mention `x`, but must still carry it.
        assert_eq!(middle.captures.len(), 1);
        assert_eq!(middle.captures[0].from, CaptureFrom::ParentLocal(0));
        assert_eq!(inner.captures.len(), 1);
        assert_eq!(inner.captures[0].from, CaptureFrom::ParentCapture(0));
    }

    #[test]
    fn a_for_in_loop_variable_is_flagged_per_iteration() {
        // MRT binds the loop variable afresh each turn, so a closure made in
        // the body captures that turn's value. A backend has to know.
        let r = resolved(
            "func f(items) { var out = []; for (x in items) { push(out, func() { return x; }); } }",
        );
        let f = function(&r, "f");
        let x = f.locals.iter().find(|l| l.name == "x").unwrap();
        assert!(x.per_iteration, "for-in binds afresh each iteration");
        assert!(x.captured);
        // A C-style loop variable is not per-iteration: it is one slot.
        let r = resolved("func g() { for (var i = 0; i < 2; i += 1) { } }");
        let g = function(&r, "g");
        let i = g.locals.iter().find(|l| l.name == "i").unwrap();
        assert!(!i.per_iteration);
    }

    #[test]
    fn block_scopes_shadow_but_share_the_functions_slot_space() {
        let r = resolved("func f() { var a = 1; { var a = 2; print(a); } print(a); }");
        let f = function(&r, "f");
        assert_eq!(f.slot_count, 2, "two distinct slots for two distinct `a`s");
        let uses = uses_of(f, "a");
        assert_eq!(uses[0].resolution, Resolution::Local { slot: 1 }, "inner");
        assert_eq!(uses[1].resolution, Resolution::Local { slot: 0 }, "outer");
    }

    #[test]
    fn a_var_initializer_sees_the_outer_binding() {
        // `var x = x;` reads the enclosing `x`, because the initializer runs
        // before the new binding exists.
        let r = resolved("func f() { var x = 1; { var x = x; } }");
        let f = function(&r, "f");
        let uses = uses_of(f, "x");
        assert_eq!(uses[0].resolution, Resolution::Local { slot: 0 });
    }

    #[test]
    fn a_parameter_default_sees_parameters_to_its_left() {
        let r = resolved("func f(a, b = a) { }");
        let f = function(&r, "f");
        assert_eq!(uses_of(f, "a")[0].resolution, Resolution::Local { slot: 0 });
    }

    #[test]
    fn a_method_binds_this_as_a_local() {
        let r = resolved("struct P { x; func m() { return this.x; } }");
        let m = function(&r, "m");
        assert_eq!(m.locals[0].name, "this");
        assert_eq!(
            uses_of(m, "this")[0].resolution,
            Resolution::Local { slot: 0 }
        );
    }

    #[test]
    fn catch_and_match_bindings_are_scoped_to_their_clause() {
        let r = resolved(
            "func f(v) { try { } catch (e) { print(e); } \
             match (v) { case n: print(n); default: print(1); } }",
        );
        let f = function(&r, "f");
        assert_eq!(uses_of(f, "e")[0].resolution, Resolution::Local { slot: 1 });
        assert_eq!(uses_of(f, "n")[0].resolution, Resolution::Local { slot: 2 });
    }

    #[test]
    fn destructuring_assignment_resolves_existing_names() {
        // The leaves are uses, not declarations: no new slots.
        let r = resolved("func f() { var a = 1; var b = 2; [a, b] = [b, a]; }");
        let f = function(&r, "f");
        assert_eq!(f.slot_count, 2);
        let uses = uses_of(f, "a");
        assert!(uses
            .iter()
            .all(|u| u.resolution == Resolution::Local { slot: 0 }));
    }

    #[test]
    fn a_function_can_refer_to_itself() {
        let r = resolved("func f() { var fact = 0; return f(); }");
        // `f` is declared at module level, so from inside it is a global.
        let f = function(&r, "f");
        assert_eq!(uses_of(f, "f")[0].resolution, Resolution::Global);

        // Nested, it is a local of the enclosing function.
        let r = resolved("func outer() { func inner() { return inner(); } }");
        let inner = function(&r, "inner");
        assert_eq!(
            uses_of(inner, "inner")[0].resolution,
            Resolution::Capture { index: 0 }
        );
    }

    #[test]
    fn interpolated_expressions_are_resolved_too() {
        let r = resolved(r#"func f() { var name = "x"; print("hi ${name}"); }"#);
        let f = function(&r, "f");
        assert_eq!(
            uses_of(f, "name")[0].resolution,
            Resolution::Local { slot: 0 }
        );
    }

    #[test]
    fn imports_bind_locally_when_inside_a_function_and_globally_at_top_level() {
        // `import` is only legal at the top level, where it binds a global.
        let r = resolved("import { a } from \"./m.mrt\";\nfunc f() { return a; }");
        let f = function(&r, "f");
        assert_eq!(uses_of(f, "a")[0].resolution, Resolution::Global);
    }

    #[test]
    fn validate_rejects_a_capture_chain_that_does_not_line_up() {
        let mut r = resolved("func f() { var x = 1; return func() { return x; }; }");
        // Point a capture at a parent local that does not exist.
        let inner = r.functions.iter_mut().find(|f| f.parent.is_some()).unwrap();
        inner.captures[0].from = CaptureFrom::ParentLocal(99);
        let err = validate(&r).expect_err("should be rejected");
        assert!(err.contains("does not exist"), "{err}");
    }

    #[test]
    fn validate_rejects_duplicate_captures() {
        let mut r = resolved("func f() { var x = 1; return func() { return x; }; }");
        let inner = r.functions.iter_mut().find(|f| f.parent.is_some()).unwrap();
        let dup = inner.captures[0].clone();
        inner.captures.push(dup);
        let err = validate(&r).expect_err("should be rejected");
        assert!(err.contains("twice"), "{err}");
    }
}
