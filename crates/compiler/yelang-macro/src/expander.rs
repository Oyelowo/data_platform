use yelang_ast::{
    ArrayAccess, AssignEqExpr, AssignOpExpr, BinaryExpr, BlockExpr, Expr, ExprKind, ForLoopExpr,
    IfExpr, ImplItemKind, Item, ItemKind, LambdaExpr, LoopExpr, MacroInvocation, MatchExpr, MemberAccess,
    Program, RangeExpr, Stmt, StmtKind, TernaryExpr, UnaryExpr, WhileExpr,
};
use yelang_interner::Interner;

use crate::builtin_decorators::{
    apply_decorator, BuiltinDecorator, DecoratorResult,
};
use crate::builtin_macros::expand_builtin_macro;
use crate::hygiene::ExpnId;

/// Error encountered during macro expansion.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpandError {
    UnknownMacro { path: String, span: yelang_lexer::Span },
    MalformedMacroArgs { reason: String, span: yelang_lexer::Span },
    DecoratorError { reason: String, span: yelang_lexer::Span },
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpandError::UnknownMacro { path, .. } => {
                write!(f, "unknown macro: {}", path)
            }
            ExpandError::MalformedMacroArgs { reason, .. } => {
                write!(f, "malformed macro arguments: {}", reason)
            }
            ExpandError::DecoratorError { reason, .. } => {
                write!(f, "decorator error: {}", reason)
            }
        }
    }
}

impl std::error::Error for ExpandError {}

/// Result of expanding a program.
pub struct ExpandResult {
    pub program: Program,
    pub errors: Vec<ExpandError>,
}

/// The main macro expansion engine.
///
/// Walks the AST, expands macro invocations, and applies decorators.
/// Operates iteratively until no more macro invocations remain.
pub struct MacroExpander<'a> {
    interner: &'a Interner,
    /// Errors accumulated during expansion.
    errors: Vec<ExpandError>,
    /// Current expansion ID for hygiene tracking.
    _expn_id: ExpnId,
}

impl<'a> MacroExpander<'a> {
    pub fn new(interner: &'a Interner) -> Self {
        Self {
            interner,
            errors: vec![],
            _expn_id: ExpnId::fresh(),
        }
    }

    /// Expand all macros in a program.
    pub fn expand(&mut self, program: &Program) -> ExpandResult {
        let mut items = vec![];
        for item in &program.items {
            match self.expand_item(item.clone()) {
                Ok(expanded) => items.extend(expanded),
                Err(e) => self.errors.push(e),
            }
        }

        // Iterative expansion: expanded output may contain new macro invocations.
        // We loop until no more changes are made (or max iterations reached to prevent infinite loops).
        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 100;
        loop {
            iterations += 1;
            if iterations > MAX_ITERATIONS {
                self.errors.push(ExpandError::UnknownMacro {
                    path: "(expansion loop)".to_string(),
                    span: yelang_lexer::Span::default(),
                });
                break;
            }

            let mut changed = false;
            let mut new_items = vec![];
            for item in items {
                let (expanded, item_changed) = self.expand_item_deep(item);
                new_items.push(expanded);
                changed |= item_changed;
            }
            items = new_items;

            if !changed {
                break;
            }
        }

        ExpandResult {
            program: Program { items, span: yelang_lexer::Span::default() },
            errors: self.errors.clone(),
        }
    }

    /// Expand a single item, applying decorators and any top-level macros.
    ///
    /// Returns a vec because decorators such as `@derive` may generate
    /// additional items (e.g. `impl` blocks) alongside the original item.
    pub fn expand_item(&mut self, mut item: Item) -> Result<Vec<Item>, ExpandError> {
        // Apply decorators first.
        let attrs = std::mem::take(&mut item.attributes);
        let mut decorator_errors = vec![];
        let mut current_items = vec![item];

        for attr in &attrs {
            if let Some(decorator) = BuiltinDecorator::from_attribute(attr, self.interner) {
                // Each decorator is applied to the *first* item produced by the
                // previous decorator.  Generated side-items are accumulated.
                let target = current_items.remove(0);
                let result = apply_decorator(decorator, attr, &target, self.interner);
                if result.items.is_empty() && !result.errors.is_empty() {
                    for err in &result.errors {
                        decorator_errors.push(ExpandError::DecoratorError {
                            reason: err.clone(),
                            span: attr.span,
                        });
                    }
                    // Restore target on error so expansion can continue.
                    current_items.insert(0, target);
                } else {
                    current_items = result.items;
                }
            }
        }

        // Re-attach non-built-in decorators to the primary item (first in vec).
        if let Some(first) = current_items.first_mut() {
            first.attributes = attrs
                .into_iter()
                .filter(|attr| BuiltinDecorator::from_attribute(attr, self.interner).is_none())
                .collect();
        }

        self.errors.extend(decorator_errors);

        // Expand any macro invocations inside every item.
        let mut expanded_items = vec![];
        for it in current_items {
            let (expanded, _) = self.expand_item_deep(it);
            expanded_items.push(expanded);
        }
        Ok(expanded_items)
    }

    /// Deeply expand all macro invocations inside an item.
    /// Returns (expanded_item, whether_anything_changed).
    fn expand_item_deep(&mut self, mut item: Item) -> (Item, bool) {
        let mut changed = false;

        match &mut item.kind {
            ItemKind::Fn(func) => {
                let (new_body, body_changed) = self.expand_block_expr(&func.body);
                func.body = new_body;
                changed |= body_changed;
            }
            ItemKind::Const(c) => {
                let (new_expr, expr_changed) = self.expand_expr(&c.value);
                c.value = new_expr;
                changed |= expr_changed;
            }
            ItemKind::Static(s) => {
                let (new_expr, expr_changed) = self.expand_expr(&s.value);
                s.value = new_expr;
                changed |= expr_changed;
            }
            ItemKind::Impl(i) => {
                for item in &mut i.items {
                    if let yelang_ast::ImplItemKind::Method(m) = &mut item.item {
                        let (new_body, body_changed) = self.expand_block_expr(&m.body);
                        m.body = new_body;
                        changed |= body_changed;
                    }
                }
            }
            ItemKind::Trait(t) => {
                for item in &mut t.items {
                    if let yelang_ast::TraitItemKind::Method(m) = &mut item.item {
                        if let Some(body) = &mut m.body {
                            let (new_body, body_changed) = self.expand_block_expr(body);
                            *body = new_body;
                            changed |= body_changed;
                        }
                    }
                }
            }
            ItemKind::Module(m) => {
                if let yelang_ast::ModKind::Inline { items: mod_items } = &mut m.kind {
                    let mut new_items = vec![];
                    for mi in std::mem::take(mod_items) {
                        let (expanded, ci) = self.expand_item_deep(mi);
                        new_items.push(expanded);
                        changed |= ci;
                    }
                    *mod_items = new_items;
                }
            }
            _ => {}
        }

        (item, changed)
    }

    /// Expand all macros in a block expression.
    fn expand_block_expr(&mut self, block: &BlockExpr) -> (BlockExpr, bool) {
        let mut new_stmts = vec![];
        let mut changed = false;

        for stmt in &block.statements {
            let (new_stmt, stmt_changed) = self.expand_stmt(stmt);
            new_stmts.push(new_stmt);
            changed |= stmt_changed;
        }

        (
            BlockExpr {
                label: block.label.clone(),
                statements: new_stmts,
            },
            changed,
        )
    }

    /// Expand all macros in a statement.
    fn expand_stmt(&mut self, stmt: &Stmt) -> (Stmt, bool) {
        match &stmt.kind {
            StmtKind::Expr(expr) => {
                let (new_expr, changed) = self.expand_expr(expr);
                (
                    Stmt {
                        kind: StmtKind::Expr(Box::new(new_expr)),
                        span: stmt.span,
                    },
                    changed,
                )
            }
            StmtKind::TermExpr(expr) => {
                let (new_expr, changed) = self.expand_expr(expr);
                (
                    Stmt {
                        kind: StmtKind::TermExpr(Box::new(new_expr)),
                        span: stmt.span,
                    },
                    changed,
                )
            }
            StmtKind::Let(let_stmt) => {
                let (new_init, init_changed) = if let Some(init) = &let_stmt.init {
                    let (e, c) = self.expand_expr(init);
                    (Some(Box::new(e)), c)
                } else {
                    (None, false)
                };
                (
                    Stmt {
                        kind: StmtKind::Let(Box::new(yelang_ast::LetStmt {
                            pattern: let_stmt.pattern.clone(),
                            ty: let_stmt.ty.clone(),
                            init: new_init,
                            span: let_stmt.span,
                            attrs: let_stmt.attrs.clone(),
                        })),
                        span: stmt.span,
                    },
                    init_changed,
                )
            }
            StmtKind::Item(item) => {
                match self.expand_item(*item.clone()) {
                    Ok(mut expanded) => {
                        if expanded.len() > 1 {
                            // Decorators that generate side-items are not supported
                            // inside statement position.  Emit an error and keep only
                            // the primary item.
                            self.errors.push(ExpandError::DecoratorError {
                                reason: "decorator produced multiple items in statement position".to_string(),
                                span: stmt.span,
                            });
                        }
                        let primary = expanded.into_iter().next().unwrap_or_else(|| *item.clone());
                        (
                            Stmt {
                                kind: StmtKind::Item(Box::new(primary)),
                                span: stmt.span,
                            },
                            true,
                        )
                    }
                    Err(e) => {
                        self.errors.push(e);
                        (stmt.clone(), false)
                    }
                }
            }
            StmtKind::Empty => (stmt.clone(), false),
        }
    }

    /// Expand all macros in an expression, recursively.
    /// Returns (expanded_expr, whether_anything_changed).
    fn expand_expr(&mut self, expr: &Expr) -> (Expr, bool) {
        match &expr.kind {
            ExprKind::MacroInvocation(inv) => {
                if let Some(expanded) = expand_builtin_macro(inv, self.interner) {
                    return (expanded, true);
                }
                // Unknown macro — emit error and keep as-is.
                let path_name = if inv.path.segments.len() == 1 {
                    self.interner.resolve(&inv.path.segments[0].ident.symbol).to_string()
                } else {
                    "(qualified)".to_string()
                };
                self.errors.push(ExpandError::UnknownMacro {
                    path: path_name,
                    span: inv.span,
                });
                (expr.clone(), false)
            }
            ExprKind::Binary(bin) => {
                let (left, left_changed) = self.expand_expr(&bin.left);
                let (right, right_changed) = self.expand_expr(&bin.right);
                (
                    Expr {
                        kind: ExprKind::Binary(BinaryExpr {
                            left: Box::new(left),
                            op: bin.op,
                            right: Box::new(right),
                        }),
                        span: expr.span,
                    },
                    left_changed || right_changed,
                )
            }
            ExprKind::Unary(un) => {
                let (inner, changed) = self.expand_expr(&un.expr);
                (
                    Expr {
                        kind: ExprKind::Unary(UnaryExpr {
                            op: un.op,
                            expr: Box::new(inner),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::If(if_expr) => {
                let (cond, cond_changed) = self.expand_expr(&if_expr.condition);
                let (then_block, then_changed) = self.expand_block_expr(&if_expr.then_block);
                let (else_expr, else_changed) = if let Some(e) = &if_expr.else_expr {
                    let (exp, ch) = self.expand_expr(e);
                    (Some(exp), ch)
                } else {
                    (None, false)
                };
                (
                    Expr {
                        kind: ExprKind::If(IfExpr {
                            condition: Box::new(cond),
                            then_block,
                            else_expr: else_expr.map(Box::new),
                        }),
                        span: expr.span,
                    },
                    cond_changed || then_changed || else_changed,
                )
            }
            ExprKind::Block(block) => {
                let (new_block, changed) = self.expand_block_expr(block);
                (
                    Expr {
                        kind: ExprKind::Block(new_block),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Call(call) => {
                let (callee, callee_changed) = self.expand_expr(&call.callee);
                let mut args = vec![];
                let mut args_changed = false;
                for arg in &call.args {
                    let (new_arg, arg_changed) = match arg {
                        yelang_ast::CallArgument::Positional(e) => {
                            let (ne, nc) = self.expand_expr(e);
                            (yelang_ast::CallArgument::Positional(ne), nc)
                        }
                        yelang_ast::CallArgument::Named(id, e) => {
                            let (ne, nc) = self.expand_expr(e);
                            (yelang_ast::CallArgument::Named(id.clone(), ne), nc)
                        }
                    };
                    args.push(new_arg);
                    args_changed |= arg_changed;
                }
                (
                    Expr {
                        kind: ExprKind::Call(yelang_ast::CallExpr {
                            callee: Box::new(callee),
                            args,
                        }),
                        span: expr.span,
                    },
                    callee_changed || args_changed,
                )
            }
            ExprKind::Match(match_expr) => {
                let (scrutinee, scrut_changed) = self.expand_expr(&match_expr.scrutinee);
                let mut arms = vec![];
                let mut arms_changed = false;
                for arm in &match_expr.arms {
                    let (body, body_changed) = self.expand_expr(&arm.body);
                    let (guard, guard_changed) = if let Some(g) = &arm.guard {
                        let (ng, nc) = self.expand_expr(g);
                        (Some(ng), nc)
                    } else {
                        (None, false)
                    };
                    arms.push(yelang_ast::MatchArm {
                        pattern: arm.pattern.clone(),
                        guard: guard.map(Box::new),
                        body: Box::new(body),
                        span: arm.span,
                    });
                    arms_changed |= body_changed || guard_changed;
                }
                (
                    Expr {
                        kind: ExprKind::Match(Box::new(yelang_ast::MatchExpr {
                            scrutinee: Box::new(scrutinee),
                            arms,
                        })),
                        span: expr.span,
                    },
                    scrut_changed || arms_changed,
                )
            }
            ExprKind::Lambda(lambda) => {
                let (body, changed) = self.expand_expr(&lambda.body);
                (
                    Expr {
                        kind: ExprKind::Lambda(yelang_ast::LambdaExpr {
                            header_span: lambda.header_span,
                            fn_sig: lambda.fn_sig.clone(),
                            body: Box::new(body),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Return(ret) => {
                if let Some(e) = ret {
                    let (ne, changed) = self.expand_expr(e);
                    (
                        Expr {
                            kind: ExprKind::Return(Some(Box::new(ne))),
                            span: expr.span,
                        },
                        changed,
                    )
                } else {
                    (expr.clone(), false)
                }
            }
            ExprKind::Break(break_expr) => {
                let (value, changed) = if let Some(v) = &break_expr.value {
                    let (nv, nc) = self.expand_expr(v);
                    (Some(nv), nc)
                } else {
                    (None, false)
                };
                (
                    Expr {
                        kind: ExprKind::Break(yelang_ast::BreakExpr {
                            label: break_expr.label.clone(),
                            value: value.map(Box::new),
                            span: break_expr.span,
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::AssignEq(assign) => {
                let (value, changed) = self.expand_expr(&assign.value);
                (
                    Expr {
                        kind: ExprKind::AssignEq(AssignEqExpr {
                            target: Box::new(*assign.target.clone()),
                            value: Box::new(value),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::AssignOp(assign) => {
                let (value, changed) = self.expand_expr(&assign.value);
                (
                    Expr {
                        kind: ExprKind::AssignOp(AssignOpExpr {
                            target: Box::new(*assign.target.clone()),
                            value: Box::new(value),
                            op: assign.op.clone(),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Tuple(exprs) => {
                let mut new_exprs = vec![];
                let mut changed = false;
                for e in exprs {
                    let (ne, nc) = self.expand_expr(e);
                    new_exprs.push(ne);
                    changed |= nc;
                }
                (
                    Expr {
                        kind: ExprKind::Tuple(new_exprs),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Array(arr) => {
                let mut new_elements = vec![];
                let mut changed = false;
                if let Some(elements) = arr.elements() {
                    for e in elements {
                        let (ne, nc) = self.expand_expr(e);
                        new_elements.push(ne);
                        changed |= nc;
                    }
                }
                (
                    Expr {
                        kind: ExprKind::Array(yelang_ast::Array {
                            kind: yelang_ast::ArrayKind::List(new_elements),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Struct(struct_expr) => {
                let mut new_fields = vec![];
                let mut changed = false;
                for field in &struct_expr.fields {
                    let (ne, nc) = self.expand_expr(&field.value);
                    new_fields.push(yelang_ast::FieldAssign {
                        name: field.name.clone(),
                        value: ne,
                        is_shorthand: field.is_shorthand,
                        span: field.span,
                    });
                    changed |= nc;
                }
                let (rest, rest_changed) = if let Some(r) = &struct_expr.rest {
                    let (nr, nc) = self.expand_expr(r);
                    (Some(Box::new(nr)), nc)
                } else {
                    (None, false)
                };
                (
                    Expr {
                        kind: ExprKind::Struct(yelang_ast::StructExpr {
                            path: struct_expr.path.clone(),
                            fields: new_fields,
                            rest,
                        }),
                        span: expr.span,
                    },
                    changed || rest_changed,
                )
            }
            ExprKind::MemberAccess(access) => {
                let (base, changed) = self.expand_expr(access.base());
                (
                    Expr {
                        kind: ExprKind::MemberAccess(MemberAccess {
                            base: Box::new(base),
                            member: access.member().clone(),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::ArrayAccess(access) => {
                let (base, base_changed) = self.expand_expr(access.base());
                // For MVP, we only handle simple single indices.
                let (index, index_changed) = match access.index() {
                    yelang_ast::ArrayIndex::Single(idx) => {
                        let (ne, nc) = self.expand_expr(idx.expr());
                        (
                            yelang_ast::ArrayIndex::Single(yelang_ast::Index(Box::new(ne))),
                            nc,
                        )
                    }
                    other => (other.clone(), false),
                };
                (
                    Expr {
                        kind: ExprKind::ArrayAccess(yelang_ast::ArrayAccess {
                            base: Box::new(base),
                            index,
                        }),
                        span: expr.span,
                    },
                    base_changed || index_changed,
                )
            }
            ExprKind::MethodCall(method) => {
                let (receiver, recv_changed) = self.expand_expr(&method.receiver);
                let mut args = vec![];
                let mut args_changed = false;
                for arg in &method.arguments {
                    let (new_arg, arg_changed) = match arg {
                        yelang_ast::CallArgument::Positional(e) => {
                            let (ne, nc) = self.expand_expr(e);
                            (yelang_ast::CallArgument::Positional(ne), nc)
                        }
                        yelang_ast::CallArgument::Named(id, e) => {
                            let (ne, nc) = self.expand_expr(e);
                            (yelang_ast::CallArgument::Named(id.clone(), ne), nc)
                        }
                    };
                    args.push(new_arg);
                    args_changed |= arg_changed;
                }
                (
                    Expr {
                        kind: ExprKind::MethodCall(yelang_ast::MethodCallExpr {
                            receiver: Box::new(receiver),
                            segment: method.segment.clone(),
                            arguments: args,
                        }),
                        span: expr.span,
                    },
                    recv_changed || args_changed,
                )
            }
            ExprKind::TypeCast(cast) => {
                let (base, changed) = self.expand_expr(&cast.base);
                (
                    Expr {
                        kind: ExprKind::TypeCast(yelang_ast::TypeCast {
                            base: Box::new(base),
                            ty: cast.ty.clone(),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::TypeAscription(asc) => {
                let (new_expr, changed) = self.expand_expr(&asc.expr);
                let span = new_expr.span;
                (
                    Expr {
                        kind: ExprKind::TypeAscription(yelang_ast::TypeAscription {
                            expr: Box::new(new_expr),
                            ty: asc.ty.clone(),
                        }),
                        span,
                    },
                    changed,
                )
            }
            ExprKind::IsType(is_type) => {
                let (new_expr, changed) = self.expand_expr(&is_type.expr);
                let span = new_expr.span;
                (
                    Expr {
                        kind: ExprKind::IsType(yelang_ast::IsTypeExpr {
                            expr: Box::new(new_expr),
                            ty: is_type.ty.clone(),
                        }),
                        span,
                    },
                    changed,
                )
            }
            ExprKind::Try(try_expr) => {
                let (base, changed) = self.expand_expr(&try_expr.base);
                (
                    Expr {
                        kind: ExprKind::Try(yelang_ast::TrySafeAccess {
                            base: Box::new(base),
                            op: try_expr.op.clone(),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::ForLoop(for_loop) => {
                let (iter, iter_changed) = self.expand_expr(&for_loop.iter);
                let (body, body_changed) = self.expand_block_expr(&for_loop.body);
                (
                    Expr {
                        kind: ExprKind::ForLoop(yelang_ast::ForLoopExpr {
                            pat: for_loop.pat.clone(),
                            label: for_loop.label.clone(),
                            iter: Box::new(iter),
                            body,
                        }),
                        span: expr.span,
                    },
                    iter_changed || body_changed,
                )
            }
            ExprKind::While(while_expr) => {
                let (cond, cond_changed) = self.expand_expr(&while_expr.condition);
                let (body, body_changed) = self.expand_block_expr(&while_expr.body);
                (
                    Expr {
                        kind: ExprKind::While(yelang_ast::WhileExpr {
                            label: while_expr.label.clone(),
                            condition: Box::new(cond),
                            body,
                        }),
                        span: expr.span,
                    },
                    cond_changed || body_changed,
                )
            }
            ExprKind::Loop(loop_expr) => {
                let (body, changed) = self.expand_block_expr(&loop_expr.body);
                (
                    Expr {
                        kind: ExprKind::Loop(Box::new(yelang_ast::LoopExpr {
                            label: loop_expr.label.clone(),
                            body: Box::new(body),
                        })),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Await(e) => {
                let (inner, changed) = self.expand_expr(e);
                (
                    Expr {
                        kind: ExprKind::Await(Box::new(inner)),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Gen(e) => {
                let (inner, changed) = self.expand_expr(e);
                (
                    Expr {
                        kind: ExprKind::Gen(Box::new(inner)),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Grouped(g) => {
                let (inner, changed) = self.expand_expr(&g.expr);
                (
                    Expr {
                        kind: ExprKind::Grouped(yelang_ast::GroupedExpr {
                            expr: Box::new(inner),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Range(range) => {
                let (start, start_changed) = if let Some(s) = &range.start {
                    let (ns, nc) = self.expand_expr(s);
                    (Some(Box::new(ns)), nc)
                } else {
                    (None, false)
                };
                let (end, end_changed) = if let Some(e) = &range.end {
                    let (ne, nc) = self.expand_expr(e);
                    (Some(Box::new(ne)), nc)
                } else {
                    (None, false)
                };
                (
                    Expr {
                        kind: ExprKind::Range(yelang_ast::RangeExpr {
                            start,
                            end,
                            op: range.op.clone(),
                        }),
                        span: expr.span,
                    },
                    start_changed || end_changed,
                )
            }
            ExprKind::Let(let_expr) => {
                let (new_expr, changed) = self.expand_expr(&let_expr.expr);
                let span = new_expr.span;
                (
                    Expr {
                        kind: ExprKind::Let(yelang_ast::LetExpr {
                            pattern: let_expr.pattern.clone(),
                            expr: Box::new(new_expr),
                        }),
                        span,
                    },
                    changed,
                )
            }
            ExprKind::Comprehension(comp) => {
                let (element, elem_changed) = self.expand_expr(&comp.element);
                let mut vars = vec![];
                let mut vars_changed = false;
                for var in &comp.variables {
                    let (source, source_changed) = self.expand_expr(&var.source);
                    vars.push(yelang_ast::ComprehensionVar {
                        pattern: var.pattern.clone(),
                        source: Box::new(source),
                    });
                    vars_changed |= source_changed;
                }
                let (cond, cond_changed) = if let Some(c) = &comp.condition {
                    let (nc, cc) = self.expand_expr(c);
                    (Some(Box::new(nc)), cc)
                } else {
                    (None, false)
                };
                (
                    Expr {
                        kind: ExprKind::Comprehension(yelang_ast::ComprehensionExpr {
                            element: Box::new(element),
                            variables: vars,
                            condition: cond,
                        }),
                        span: expr.span,
                    },
                    elem_changed || vars_changed || cond_changed,
                )
            }
            ExprKind::Ternary(ternary) => {
                let (cond, cond_changed) = self.expand_expr(&ternary.condition);
                let (if_true, if_true_changed) = self.expand_expr(&ternary.if_true);
                let (if_false, if_false_changed) = self.expand_expr(&ternary.if_false);
                (
                    Expr {
                        kind: ExprKind::Ternary(yelang_ast::TernaryExpr {
                            condition: Box::new(cond),
                            if_true: Box::new(if_true),
                            if_false: Box::new(if_false),
                        }),
                        span: expr.span,
                    },
                    cond_changed || if_true_changed || if_false_changed,
                )
            }
            ExprKind::BindAt(bind) => {
                let (base, changed) = self.expand_expr(&bind.base);
                (
                    Expr {
                        kind: ExprKind::BindAt(yelang_ast::BindAtExpr {
                            base: Box::new(base),
                            at: bind.at.clone(),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Async(async_expr) => {
                let (block, changed) = self.expand_block_expr(&async_expr.block);
                (
                    Expr {
                        kind: ExprKind::Async(yelang_ast::AsyncExpr {
                            block: Box::new(block),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Object(obj) => {
                let mut new_fields = vec![];
                let mut changed = false;
                for field in obj.fields() {
                    let (val, val_changed) = self.expand_expr(field.value());
                    new_fields.push(yelang_ast::ObjectField::new(field.key().clone(), val));
                    changed |= val_changed;
                }
                (
                    Expr {
                        kind: ExprKind::Object(yelang_ast::Object {
                            fields: new_fields,
                            span: obj.span,
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::DocumentAccess(doc) => {
                let (base, base_changed) = self.expand_expr(doc.base());
                let mut new_fields = vec![];
                let mut fields_changed = false;
                for field in doc.object().fields() {
                    match field {
                        yelang_ast::DocumentField::KeyVal(kv) => {
                            let (val, val_changed) = self.expand_expr(&kv.value);
                            new_fields.push(yelang_ast::DocumentField::KeyVal(yelang_ast::KeyVal {
                                key: kv.key.clone(),
                                value: val,
                            }));
                            fields_changed |= val_changed;
                        }
                        other => new_fields.push(other.clone()),
                    }
                }
                (
                    Expr {
                        kind: ExprKind::DocumentAccess(yelang_ast::DocumentAccess {
                            base: Box::new(base),
                            object: yelang_ast::Document {
                                fields: new_fields,
                                span: doc.object().span,
                            },
                        }),
                        span: expr.span,
                    },
                    base_changed || fields_changed,
                )
            }
            ExprKind::DestructureAssign(assign) => {
                let (value, changed) = self.expand_expr(&assign.value);
                (
                    Expr {
                        kind: ExprKind::DestructureAssign(yelang_ast::DestructureAssignExpr {
                            pattern: assign.pattern.clone(),
                            value: Box::new(value),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            // Literals, paths, and other leaf nodes don't need expansion.
            _ => (expr.clone(), false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_ast::TokenKind;
    use yelang_interner::Interner;
    use yelang_lexer::ParseTokenStream;

    fn parse_program(src: &str) -> (Program, Interner) {
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(src, &mut interner).unwrap();
        let program = stream.parse::<Program>().unwrap();
        (program, interner)
    }

    #[test]
    fn expand_assert_in_function() {
        let src = r#"
            fn main() {
                assert!(true);
            }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        // assert!(true) should expand to `if !true { panic!(...) }`
        let fn_item = &result.program.items[0];
        let ItemKind::Fn(func) = &fn_item.kind else { panic!("expected fn") };
        let body = &func.body;
        assert_eq!(body.statements.len(), 1);
        let StmtKind::TermExpr(expr) = &body.statements[0].kind else { panic!("expected term expr stmt") };
        assert!(matches!(expr.kind, ExprKind::If(_)), "expected If, got {:?}", expr.kind);
    }

    #[test]
    fn expand_todo_in_function() {
        let src = r#"
            fn main() {
                todo!();
            }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        // todo!() expands to panic!("not yet implemented")
        let fn_item = &result.program.items[0];
        let ItemKind::Fn(func) = &fn_item.kind else { panic!("expected fn") };
        let body = &func.body;
        let StmtKind::TermExpr(expr) = &body.statements[0].kind else { panic!("expected term expr stmt") };
        assert!(matches!(expr.kind, ExprKind::Call(_)), "expected Call, got {:?}", expr.kind);
    }

    #[test]
    fn expand_unknown_macro_emits_error() {
        let src = r#"
            fn main() {
                unknown_macro!(1);
            }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(!result.errors.is_empty(), "expected at least one error");
        assert!(result.errors.iter().any(|e| matches!(e, ExpandError::UnknownMacro { .. })));
    }

    #[test]
    fn decorator_test_on_function() {
        let src = r#"
            @test
            fn my_test() {}
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        // @test should be removed from attributes after processing.
        assert!(result.program.items[0].attributes.is_empty());
    }

    #[test]
    fn decorator_test_on_struct_errors() {
        let src = r#"
            @test
            struct Foo {}
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(!result.errors.is_empty(), "expected error for @test on struct");
    }

    #[test]
    fn nested_macro_expansion() {
        // todo!() expands to panic!("not yet implemented"), which is then
        // expanded to a call expression in the next iteration.
        let src = r#"
            fn main() {
                todo!();
            }
        "#;
        let (program, interner) = parse_program(src);
        let mut expander = MacroExpander::new(&interner);
        let result = expander.expand(&program);
        // After two iterations, todo! → panic!(...) → call expr
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    }

    #[test]
    fn expand_assert_eq_in_function() {
        let src = r#"
            fn main() {
                assert_eq!(a, b);
            }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let fn_item = &result.program.items[0];
        let ItemKind::Fn(func) = &fn_item.kind else { panic!("expected fn") };
        let body = &func.body;
        assert_eq!(body.statements.len(), 1);
        let StmtKind::TermExpr(expr) = &body.statements[0].kind else { panic!("expected term expr stmt") };
        assert!(matches!(expr.kind, ExprKind::Block(_)), "expected Block, got {:?}", expr.kind);
    }

    #[test]
    fn expand_assert_ne_in_function() {
        let src = r#"
            fn main() {
                assert_ne!(a, b);
            }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    }

    #[test]
    fn expand_format_in_function() {
        let src = r#"
            fn main() {
                format!("hello {}", name);
            }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let fn_item = &result.program.items[0];
        let ItemKind::Fn(func) = &fn_item.kind else { panic!("expected fn") };
        let body = &func.body;
        let StmtKind::TermExpr(expr) = &body.statements[0].kind else { panic!("expected term expr stmt") };
        assert!(matches!(expr.kind, ExprKind::Call(_)), "expected Call, got {:?}", expr.kind);
    }

    #[test]
    fn derive_generates_impl_items() {
        let src = r#"
            @derive(Clone, Copy)
            struct Point {}
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        // Should have: struct Point, impl Clone for Point, impl Copy for Point
        assert_eq!(result.program.items.len(), 3, "expected 3 items: struct + 2 impls");
        let impls: Vec<_> = result.program.items.iter().filter(|i| matches!(i.kind, ItemKind::Impl(_))).collect();
        assert_eq!(impls.len(), 2, "expected 2 impl items");
    }

    #[test]
    fn derive_partial_eq_for_named_struct() {
        let src = r#"
            @derive(PartialEq)
            struct Point { x: i32, y: i32 }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.program.items.len(), 2);
        let impl_item = result.program.items.iter().find(|i| matches!(i.kind, ItemKind::Impl(_))).expect("impl");
        let ItemKind::Impl(impl_block) = &impl_item.kind else { unreachable!() };
        assert_eq!(impl_block.items.len(), 1, "PartialEq impl should have eq method");
    }

    #[test]
    fn derive_debug_for_unit_struct() {
        let src = r#"
            @derive(Debug)
            struct Unit;
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.program.items.len(), 2);
    }

    #[test]
    fn derive_unsupported_trait_errors() {
        let src = r#"
            @derive(Ord)
            struct Point {}
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(!result.errors.is_empty(), "expected error for unsupported derive trait");
    }

    #[test]
    fn derive_clone_named_struct_produces_struct_literal() {
        // Verify that @derive(Clone) on a named struct generates a method
        // whose body contains `Self { field: self.field.clone(), ... }`.
        let src = r#"
            @derive(Clone)
            struct Point { x: i32, y: i32 }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.program.items.len(), 2, "expected struct + impl Clone");

        let impl_item = result.program.items.iter()
            .find(|i| matches!(i.kind, ItemKind::Impl(_)))
            .expect("impl Clone expected");
        let ItemKind::Impl(impl_block) = &impl_item.kind else { unreachable!() };
        assert_eq!(impl_block.items.len(), 1, "Clone impl should have clone method");

        let method = &impl_block.items[0];
        let ImplItemKind::Method(fn_def) = &method.item else {
            panic!("expected method in Clone impl");
        };

        // The body should be a block with a single terminating expression.
        assert_eq!(fn_def.body.statements.len(), 1);
        let StmtKind::TermExpr(expr) = &fn_def.body.statements[0].kind else {
            panic!("expected term expr in clone body");
        };

        // The expression must be a struct literal, not just a path.
        let ExprKind::Struct(struct_expr) = &expr.kind else {
            panic!("expected ExprKind::Struct in clone body, got {:?}", expr.kind);
        };

        // Path should be `Self`.
        assert_eq!(struct_expr.path.segments.len(), 1);
        assert_eq!(
            interner.resolve(&struct_expr.path.segments[0].ident.symbol),
            "Self"
        );

        // Should have exactly two field assignments.
        assert_eq!(struct_expr.fields.len(), 2, "expected 2 field assignments");
        assert_eq!(
            interner.resolve(&struct_expr.fields[0].name.symbol),
            "x"
        );
        assert_eq!(
            interner.resolve(&struct_expr.fields[1].name.symbol),
            "y"
        );

        // Each field value should be a method call (self.field.clone()).
        assert!(
            matches!(struct_expr.fields[0].value.kind, ExprKind::MethodCall(_)),
            "expected method call for field clone"
        );
        assert!(
            matches!(struct_expr.fields[1].value.kind, ExprKind::MethodCall(_)),
            "expected method call for field clone"
        );
    }

    #[test]
    fn derive_clone_unit_struct_uses_self_path() {
        let src = r#"
            @derive(Clone)
            struct Unit;
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let impl_item = result.program.items.iter()
            .find(|i| matches!(i.kind, ItemKind::Impl(_)))
            .expect("impl Clone expected");
        let ItemKind::Impl(impl_block) = &impl_item.kind else { unreachable!() };
        let method = &impl_block.items[0];
        let ImplItemKind::Method(fn_def) = &method.item else { panic!("expected method"); };

        let StmtKind::TermExpr(expr) = &fn_def.body.statements[0].kind else {
            panic!("expected term expr");
        };
        assert!(
            matches!(expr.kind, ExprKind::Path(_)),
            "unit struct clone should return Self path, got {:?}",
            expr.kind
        );
    }
}
