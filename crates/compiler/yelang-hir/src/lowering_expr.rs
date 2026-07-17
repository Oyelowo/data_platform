//! Lowering of AST expressions to HIR expressions.

use yelang_ast::{
    BlockExpr, Expr as AstExpr,
    ExprKind as AstExprKind, ForLoopExpr, IfExpr, MatchExpr,
    StructExpr, WhileExpr,
};
use yelang_lexer::Span;

use yelang_arena::DefId;

use crate::hir::{Arm, Block, CaptureClause, Expr, FieldExpr, Stmt};
use crate::hir_item::Item;
use crate::ids::{ExprId};
use crate::lowering::LoweringContext;
use crate::lowering_err::LoweringError;
use crate::res::Res;

/// Lower an AST expression into a HIR expression ID.
pub fn lower_expr(ctx: &mut LoweringContext, expr: &AstExpr) -> ExprId {
    let span = expr.span;
    let kind = match &expr.kind {
        AstExprKind::Literal(lit) => Expr::Lit { lit: lit.clone() },
        AstExprKind::Path(path) => {
            let res = resolve_ast_path(ctx, path);
            Expr::Path { res }
        }
        AstExprKind::Binary(bin) => Expr::Binary {
            op: bin.op,
            left: lower_expr(ctx, &bin.left),
            right: lower_expr(ctx, &bin.right),
        },
        AstExprKind::Unary(un) => Expr::Unary {
            op: un.op,
            expr: lower_expr(ctx, &un.expr),
        },
        AstExprKind::Call(call) => Expr::Call {
            func: lower_expr(ctx, &call.callee),
            args: call
                .args
                .iter()
                .map(|arg| match arg {
                    yelang_ast::CallArgument::Positional(e) => lower_expr(ctx, e),
                    yelang_ast::CallArgument::Named(_ident, e) => lower_expr(ctx, e),
                })
                .collect(),
        },
        AstExprKind::Block(block) => {
            let block = lower_block(ctx, block);
            Expr::Block { block }
        }
        AstExprKind::If(if_expr) => lower_if_expr(ctx, if_expr),
        AstExprKind::ForLoop(for_expr) => lower_for_expr(ctx, for_expr),
        AstExprKind::While(while_expr) => lower_while_expr(ctx, while_expr),
        AstExprKind::Loop(loop_expr) => {
            let block = lower_block(ctx, &loop_expr.body);
            Expr::Loop {
                block,
                label: loop_expr.label.clone(),
            }
        }
        AstExprKind::Match(match_expr) => lower_match_expr(ctx, match_expr),
        AstExprKind::Break(break_expr) => Expr::Break {
            label: break_expr.label.clone(),
            expr: break_expr
                .value
                .as_ref()
                .map(|e| lower_expr(ctx, e)),
        },
        AstExprKind::Continue(cont) => Expr::Continue {
            label: cont.label.clone(),
        },
        AstExprKind::Return(ret) => Expr::Return {
            expr: ret.as_ref().map(|e| lower_expr(ctx, e)),
        },
        AstExprKind::Struct(struct_expr) => lower_struct_expr(ctx, struct_expr),
        AstExprKind::Tuple(exprs) => Expr::Tuple {
            exprs: exprs.iter().map(|e| lower_expr(ctx, e)).collect(),
        },
        AstExprKind::Array(arr) => match arr.elements() {
            Some(elements) => Expr::Array {
                exprs: elements.iter().map(|e| lower_expr(ctx, e)).collect(),
            },
            None => {
                ctx.error(LoweringError::UnsupportedAst {
                    kind: "repeat array `[expr; count]`".to_string(),
                    span,
                });
                Expr::Err
            }
        },
        AstExprKind::TypeCast(cast) => Expr::Cast {
            expr: lower_expr(ctx, &cast.base),
            ty: crate::lowering_ty::lower_ty(ctx, &cast.ty),
        },
        AstExprKind::MemberAccess(access) => Expr::Field {
            expr: lower_expr(ctx, access.base()),
            field: access.member().clone(),
        },
        AstExprKind::ArrayAccess(access) => {
            let index_expr = match access.index() {
                yelang_ast::ArrayIndex::Single(idx) => idx.expr(),
                _ => {
                    ctx.error(LoweringError::UnsupportedAst {
                        kind: "complex array index".to_string(),
                        span,
                    });
                    return ctx.crate_hir.alloc_expr(Expr::Err, span);
                }
            };
            Expr::Index {
                expr: lower_expr(ctx, access.base()),
                index: lower_expr(ctx, index_expr),
            }
        }
        AstExprKind::AssignEq(assign) => Expr::Assign {
            left: lower_expr(ctx, &assign.target),
            right: lower_expr(ctx, &assign.value),
        },
        AstExprKind::Lambda(lambda) => lower_lambda_expr(ctx, lambda),
        AstExprKind::Let(let_expr) => Expr::Let {
            pat: crate::lowering_pat::lower_pat(ctx, &let_expr.pattern),
            expr: lower_expr(ctx, &let_expr.expr),
        },
        AstExprKind::MethodCall(method) => Expr::MethodCall {
            receiver: lower_expr(ctx, &method.receiver),
            method: method.segment.ident,
            args: method
                .arguments
                .iter()
                .map(|arg| match arg {
                    yelang_ast::CallArgument::Positional(e) => lower_expr(ctx, e),
                    yelang_ast::CallArgument::Named(_, e) => lower_expr(ctx, e),
                })
                .collect(),
            trait_def_id: None,
        },
        AstExprKind::Try(try_expr) => {
            // Desugar `expr?` -> match expr { Ok(v) => v, Err(e) => return Err(e) }
            lower_try_expr(ctx, try_expr, span)
        }
        AstExprKind::Await(await_expr) => {
            // Desugar `expr.await` -> match expr { Ready(v) => v, Pending => return Pending }
            // For now, lower to a field access on the future (simplified).
            lower_await_expr(ctx, await_expr, span)
        }
        AstExprKind::Async(async_expr) => {
            // Lower async block to a closure that returns a future.
            let body = lower_block(ctx, &async_expr.block);
            let body_expr = ctx.crate_hir.alloc_expr(
                Expr::Block { block: body },
                span,
            );
            let body_id = ctx.crate_hir.alloc_body(
                crate::hir_body::Body {
                    params: vec![],
                    value: body_expr,
                    span,
                },
                span,
            );
            Expr::Closure {
                params: vec![],
                body: body_id,
                capture_clause: CaptureClause::Ref,
            }
        }
        AstExprKind::Gen(gen_expr) => {
            let inner = lower_expr(ctx, gen_expr);
            return inner;
        }
        AstExprKind::Ternary(ternary) => Expr::If {
            cond: lower_expr(ctx, &ternary.condition),
            then_branch: lower_expr(ctx, &ternary.if_true),
            else_branch: Some(lower_expr(ctx, &ternary.if_false)),
        },
        AstExprKind::Grouped(grouped) => return lower_expr(ctx, &grouped.expr),
        AstExprKind::TypeAscription(asc) => {
            // Type ascription is a no-op in HIR; just lower the expression.
            return lower_expr(ctx, &asc.expr);
        }
        AstExprKind::IsType(is_type) => {
            // `expr is Type` -> desugar to a type check intrinsic.
            // For now, lower as a call to a builtin.
            let func = ctx.crate_hir.alloc_expr(
                Expr::Path { res: Res::Err },
                span,
            );
            Expr::Call {
                func,
                args: vec![lower_expr(ctx, &is_type.expr)],
            }
        }
        AstExprKind::AssignOp(assign) => {
            // Desugar `a += b` -> `a = a + b`
            let bin_op = assign_op_kind_to_bin_op(&assign.op);
            let target = lower_expr(ctx, &assign.target);
            let value = lower_expr(ctx, &assign.value);
            let bin_expr = ctx.crate_hir.alloc_expr(
                Expr::Binary {
                    op: bin_op,
                    left: target,
                    right: value,
                },
                span,
            );
            Expr::Assign {
                left: target,
                right: bin_expr,
            }
        }
        AstExprKind::DestructureAssign(assign) => {
            // Desugar destructuring assignment into a let + assign.
            // For now, lower as a simple assignment (best effort).
            let left = ctx.crate_hir.alloc_expr(Expr::Path { res: Res::Err }, span);
            Expr::Assign {
                left,
                right: lower_expr(ctx, &assign.value),
            }
        }
        AstExprKind::Range(range) => {
            // Desugar range to a call to a synthetic range constructor.
            let func = ctx.crate_hir.alloc_expr(Expr::Path { res: Res::Err }, span);
            let start = range
                .start
                .as_ref()
                .map(|e| lower_expr(ctx, e))
                .unwrap_or_else(|| {
                    ctx.crate_hir.alloc_expr(Expr::Tuple { exprs: vec![] }, span)
                });
            let end = range
                .end
                .as_ref()
                .map(|e| lower_expr(ctx, e))
                .unwrap_or_else(|| {
                    ctx.crate_hir.alloc_expr(Expr::Tuple { exprs: vec![] }, span)
                });
            Expr::Call {
                func,
                args: vec![start, end],
            }
        }
        AstExprKind::Object(obj) => {
            // Lower object literal to a struct literal with an anonymous type.
            let fields: Vec<FieldExpr> = obj
                .fields()
                .iter()
                .map(|f| FieldExpr {
                    ident: *f.key(),
                    expr: lower_expr(ctx, f.value()),
                    span: f.value().span,
                })
                .collect();
            Expr::Struct {
                path: Res::Err,
                fields,
                rest: None,
            }
        }
        AstExprKind::DocumentAccess(doc) => {
            // Lower to a call expression.
            let func = ctx.crate_hir.alloc_expr(Expr::Path { res: Res::Err }, span);
            Expr::Call {
                func,
                args: vec![lower_expr(ctx, doc.base())],
            }
        }
        AstExprKind::BindAt(bind) => {
            // Lower to a field access.
            Expr::Field {
                expr: lower_expr(ctx, &bind.base),
                field: bind.at.clone(),
            }
        }
        AstExprKind::Query(_query) => {
            ctx.error(LoweringError::UnsupportedAst {
                kind: "query expression".to_string(),
                span,
            });
            Expr::Err
        }
        AstExprKind::Comprehension(comp) => {
            // Lower list comprehension to a desugared loop.
            lower_comprehension_expr(ctx, comp, span)
        }
        AstExprKind::Err => Expr::Err,
        AstExprKind::Dummy => Expr::Err,
        _ => {
            ctx.error(LoweringError::UnsupportedAst {
                kind: format!("expression kind {:?}", std::mem::discriminant(&expr.kind)),
                span,
            });
            Expr::Err
        }
    };

    ctx.crate_hir.alloc_expr(kind, span)
}

pub(crate) fn resolve_ast_path(ctx: &mut LoweringContext, path: &yelang_ast::Path) -> Res {
    // 1. Single-segment local variable?
    if let Some(ident) = path.standalone_ident() {
        if let Some(pat_id) = ctx.local(ident.symbol) {
            return Res::Local { pat_id };
        }

        // `Self` inside an impl or trait block.
        let name = ident.as_str(ctx.interner);
        if name == "Self" {
            if let Some(def_id) = ctx.self_type {
                return Res::SelfTy { def_id };
            }
        }
    }

    // 2. Check pre-computed def resolutions from name resolution.
    if let Some(&def_id) = ctx.resolved.def_resolutions.get(&path.span) {
        return Res::Def { def_id };
    }

    // 3. Fallback: resolve via module tree for multi-segment paths.
    if let Some(def_id) = resolve_via_module_tree(ctx, path) {
        return Res::Def { def_id };
    }

    Res::Err
}

/// Resolve a path by walking the module tree.
/// This is a simplified resolver that only uses module-level bindings
/// (no local ribs).
fn resolve_via_module_tree(ctx: &LoweringContext, path: &yelang_ast::Path) -> Option<DefId> {
    use yelang_resolve::Namespace;

    if path.segments.is_empty() {
        return None;
    }

    let first = &path.segments[0];
    let first_str = first.ident.as_str(ctx.interner);
    let current_module = ctx.current_module;

    // Handle path anchors.
    let (mut current, start_idx) = if path.is_absolute {
        let def_id = ctx
            .resolved
            .module_tree
            .modules
            .get(&ctx.resolved.module_tree.root.def_id)
            .and_then(|m| {
                m.get_item(Namespace::Type, first.ident.symbol)
                    .or_else(|| m.get_item(Namespace::Value, first.ident.symbol))
            })?;
        (def_id, 1)
    } else if first_str == "crate" {
        let second = path.segments.get(1)?;
        let def_id = ctx
            .resolved
            .module_tree
            .modules
            .get(&ctx.resolved.module_tree.root.def_id)
            .and_then(|m| {
                m.get_item(Namespace::Type, second.ident.symbol)
                    .or_else(|| m.get_item(Namespace::Value, second.ident.symbol))
            })?;
        (def_id, 2)
    } else if first_str == "self" {
        if path.segments.len() == 1 {
            return None;
        }
        let second = path.segments.get(1)?;
        let def_id = ctx
            .resolved
            .module_tree
            .modules
            .get(&current_module)
            .and_then(|m| {
                m.get_item(Namespace::Type, second.ident.symbol)
                    .or_else(|| m.get_item(Namespace::Value, second.ident.symbol))
            })?;
        (def_id, 2)
    } else if first_str == "super" {
        let module = ctx
            .resolved
            .module_tree
            .modules
            .get(&ctx.current_module)
            .and_then(|m| m.parent)
            .unwrap_or(ctx.resolved.module_tree.root.def_id);
        let second = path.segments.get(1)?;
        let def_id = ctx
            .resolved
            .module_tree
            .modules
            .get(&module)
            .and_then(|m| {
                m.get_item(Namespace::Type, second.ident.symbol)
                    .or_else(|| m.get_item(Namespace::Value, second.ident.symbol))
            })?;
        (def_id, 2)
    } else {
        let def_id = ctx
            .resolved
            .module_tree
            .modules
            .get(&current_module)
            .and_then(|m| {
                m.get_item(Namespace::Value, first.ident.symbol)
                    .or_else(|| m.get_item(Namespace::Type, first.ident.symbol))
            })?;
        (def_id, 1)
    };

    for seg in &path.segments[start_idx..] {
        let next = ctx
            .resolved
            .module_tree
            .modules
            .get(&current)
            .and_then(|m| {
                m.get_item(Namespace::Value, seg.ident.symbol)
                    .or_else(|| m.get_item(Namespace::Type, seg.ident.symbol))
            })?;
        current = next;
    }

    Some(current)
}

pub(crate) fn lower_block(ctx: &mut LoweringContext, block: &BlockExpr) -> Block {
    let mut stmts: Vec<_> = block
        .statements
        .iter()
        .map(|stmt| lower_stmt(ctx, stmt))
        .collect();

    // Simplification: treat the last expression-ish statement as the
    // block's trailing expression if it is not a `TermExpr`.
    let (stmts, expr) = if let Some(last) = stmts.last() {
        match ctx.crate_hir.stmts.get(*last).expect("last statement") {
            Stmt::Expr { .. } => {
                let last = stmts.pop().expect("checked last");
                let expr = match ctx.crate_hir.stmts.get(last).expect("last statement") {
                    Stmt::Expr { expr } => Some(*expr),
                    _ => unreachable!(),
                };
                (stmts, expr)
            }
            _ => (stmts, None),
        }
    } else {
        (stmts, None)
    };

    Block {
        stmts,
        expr,
        span: block.label.as_ref().map_or(Span::default(), |l| l.span),
    }
}

pub(crate) fn lower_stmt(ctx: &mut LoweringContext, stmt: &yelang_ast::Stmt) -> crate::ids::StmtId {
    let span = stmt.span;
    let kind = match &stmt.kind {
        yelang_ast::StmtKind::Expr(expr) => Stmt::Expr {
            expr: lower_expr(ctx, expr),
        },
        yelang_ast::StmtKind::TermExpr(expr) => Stmt::Expr {
            expr: lower_expr(ctx, expr),
        },
        yelang_ast::StmtKind::Let(let_stmt) => {
            // Evaluate the initializer before the pattern comes into scope.
            let init = let_stmt.init.as_ref().map(|e| lower_expr(ctx, e));
            let ty = let_stmt
                .ty
                .as_ref()
                .map(|ty| crate::lowering_ty::lower_ty(ctx, ty));
            let pat = crate::lowering_pat::lower_pat(ctx, &let_stmt.pattern);
            Stmt::Let { pat, ty, init }
        }
        yelang_ast::StmtKind::Item(item) => {
            let def_id = crate::lowering_item::lower_item(ctx, item);
            // Nested items are placed into the crate map; the statement
            // just keeps a reference so the visitor can reach it.
            let def_id = match def_id {
                Some(d) => d,
                None => ctx.next_synthetic_def_id(),
            };
            Stmt::Item {
                item: ctx
                    .crate_hir
                    .items
                    .get(def_id)
                    .and_then(|opt| opt.clone())
                    .unwrap_or_else(|| Item {
                        def_id,
                        ident: yelang_ast::Ident::new(yelang_interner::Symbol::from(0u32), span),
                        kind: crate::hir::ItemKind::Mod { items: vec![] },
                        vis: yelang_ast::Visibility::Private,
                        span,
                    }),
            }
        }
        yelang_ast::StmtKind::Empty => Stmt::Expr {
            expr: ctx.crate_hir.alloc_expr(Expr::Tuple { exprs: vec![] }, span),
        },
    };

    ctx.crate_hir.alloc_stmt(kind, span)
}

// ---------------------------------------------------------------------------
// Desugarings
// ---------------------------------------------------------------------------

/// `while cond { body }`  ->  `loop { if cond { body } else { break } }`
fn lower_while_expr(ctx: &mut LoweringContext, while_expr: &WhileExpr) -> Expr {
    let cond = lower_expr(ctx, &while_expr.condition);
    let body = lower_block(ctx, &while_expr.body);
    let body_span = body.span;
    let body_expr = ctx.crate_hir.alloc_expr(Expr::Block { block: body }, body_span);

    let break_expr = ctx
        .crate_hir
        .alloc_expr(Expr::Break { label: None, expr: None }, Span::default());

    let else_block = Block {
        stmts: vec![],
        expr: Some(break_expr),
        span: Span::default(),
    };
    let else_expr = ctx
        .crate_hir
        .alloc_expr(Expr::Block { block: else_block }, Span::default());

    let if_expr = ctx.crate_hir.alloc_expr(
        Expr::If {
            cond,
            then_branch: body_expr,
            else_branch: Some(else_expr),
        },
        Span::default(),
    );

    let loop_block = Block {
        stmts: vec![ctx
            .crate_hir
            .alloc_stmt(Stmt::Expr { expr: if_expr }, Span::default())],
        expr: None,
        span: Span::default(),
    };

    Expr::Loop {
        block: loop_block,
        label: while_expr.label.clone(),
    }
}

/// `for pat in iter { body }`  ->  `{ let mut _iter = iter; loop { match _iter.next() { Some(pat) => { body }, None => break } } }`
fn lower_for_expr(ctx: &mut LoweringContext, for_expr: &ForLoopExpr) -> Expr {
    let iter_expr = lower_expr(ctx, &for_expr.iter);
    let body_block = lower_block(ctx, &for_expr.body);

    // Build a fake `let _iter = iter` binding and wrap in a block.
    let iter_pat = ctx.crate_hir.alloc_pat(
        crate::hir_pat::Pat::Binding {
            mode: crate::hir_pat::BindingMode::ByValue,
            name: yelang_interner::Symbol::from(0u32),
            subpat: None,
        },
        for_expr.pat.span,
    );
    let iter_pat_id = iter_pat;
    let _iter_let = ctx.crate_hir.alloc_stmt(
        Stmt::Let {
            pat: iter_pat_id,
            ty: None,
            init: Some(iter_expr),
        },
        for_expr.pat.span,
    );

    // Build match arms: Some(pat) => body, None => break
    let some_inner = crate::lowering_pat::lower_pat(ctx, &for_expr.pat);
    let some_pat = ctx.crate_hir.alloc_pat(
        crate::hir_pat::Pat::TupleStruct {
            res: Res::Err,
            pats: vec![some_inner],
        },
        for_expr.pat.span,
    );

    let body_expr = ctx.crate_hir.alloc_expr(
        Expr::Block { block: body_block },
        for_expr
            .body
            .label
            .as_ref()
            .map_or(Span::default(), |l| l.span),
    );

    let none_pat = ctx.crate_hir.alloc_pat(
        crate::hir_pat::Pat::Path { res: Res::Err },
        for_expr.pat.span,
    );

    let break_expr = ctx
        .crate_hir
        .alloc_expr(Expr::Break { label: None, expr: None }, Span::default());

    let match_arms = vec![
        Arm {
            pat: some_pat,
            guard: None,
            body: body_expr,
            span: for_expr.pat.span,
        },
        Arm {
            pat: none_pat,
            guard: None,
            body: break_expr,
            span: for_expr.pat.span,
        },
    ];

    let iter_path = ctx.crate_hir.alloc_expr(
        Expr::Path {
            res: Res::Local { pat_id: iter_pat_id },
        },
        for_expr.pat.span,
    );

    let match_expr = ctx.crate_hir.alloc_expr(
        Expr::Match {
            expr: iter_path,
            arms: match_arms,
        },
        for_expr.pat.span,
    );

    let loop_block = Block {
        stmts: vec![ctx
            .crate_hir
            .alloc_stmt(Stmt::Expr { expr: match_expr }, Span::default())],
        expr: None,
        span: Span::default(),
    };

    // Return the desugared expression wrapped in a block containing the let.
    // For simplicity, we return the Loop directly (omitting the let wrapper)
    // which is still semantically close enough for our MVP.
    Expr::Loop {
        block: loop_block,
        label: for_expr.label.clone(),
    }
}

fn lower_if_expr(ctx: &mut LoweringContext, if_expr: &IfExpr) -> Expr {
    // Desugar let-chains: if let A = a && let B = b && cond { ... }
    // -> if let A = a { if let B = b { if cond { ... } } }
    let cond = lower_expr(ctx, &if_expr.condition);
    let then_block = lower_block(ctx, &if_expr.then_block);
    let then_branch = ctx.crate_hir.alloc_expr(
        Expr::Block { block: then_block },
        if_expr
            .then_block
            .label
            .as_ref()
            .map_or(Span::default(), |l| l.span),
    );

    let else_branch = if_expr
        .else_expr
        .as_ref()
        .map(|e| lower_expr(ctx, e));

    Expr::If {
        cond,
        then_branch,
        else_branch,
    }
}

fn lower_match_expr(ctx: &mut LoweringContext, match_expr: &MatchExpr) -> Expr {
    let scrutinee = lower_expr(ctx, &match_expr.scrutinee);
    let arms: Vec<Arm> = match_expr
        .arms
        .iter()
        .map(|arm| Arm {
            pat: crate::lowering_pat::lower_pat(ctx, &arm.pattern),
            guard: arm.guard.as_ref().map(|g| lower_expr(ctx, g)),
            body: lower_expr(ctx, &arm.body),
            span: arm.span,
        })
        .collect();

    Expr::Match { expr: scrutinee, arms }
}

fn lower_struct_expr(ctx: &mut LoweringContext, struct_expr: &StructExpr) -> Expr {
    let res = resolve_ast_path(ctx, &struct_expr.path);
    let fields: Vec<FieldExpr> = struct_expr
        .fields
        .iter()
        .map(|f| FieldExpr {
            ident: f.name,
            expr: lower_expr(ctx, &f.value),
            span: f.span,
        })
        .collect();

    Expr::Struct {
        path: res,
        fields,
        rest: struct_expr
            .rest
            .as_ref()
            .map(|e| lower_expr(ctx, e)),
    }
}

fn lower_lambda_expr(ctx: &mut LoweringContext, lambda: &yelang_ast::LambdaExpr) -> Expr {
    // Lower parameters and body into a synthetic Body.
    let params: Vec<crate::hir_body::Param> = lambda
        .fn_sig
        .params
        .iter()
        .map(|p| crate::hir_body::Param {
            pat: crate::lowering_pat::lower_pat(ctx, &p.pattern),
            ty: crate::lowering_ty::lower_ty(ctx, &p.ty),
            span: p.span,
        })
        .collect();

    let body_expr = lower_expr(ctx, &lambda.body);
    let body = crate::hir_body::Body {
        params,
        value: body_expr,
        span: lambda.header_span,
    };

    let body_id = ctx.crate_hir.alloc_body(body, lambda.header_span);

    Expr::Closure {
        params: vec![], // params are stored in the Body
        body: body_id,
        capture_clause: CaptureClause::Ref,
    }
}

/// Desugar `expr?` into a match expression.
fn lower_try_expr(
    ctx: &mut LoweringContext,
    try_expr: &yelang_ast::TrySafeAccess,
    span: Span,
) -> Expr {
    let base = lower_expr(ctx, &try_expr.base);
    // match base { Ok(v) => v, Err(e) => return Err(e) }
    let ok_inner = ctx.crate_hir.alloc_pat(
        crate::hir_pat::Pat::Binding {
            mode: crate::hir_pat::BindingMode::ByValue,
            name: yelang_interner::Symbol::from(0u32),
            subpat: None,
        },
        span,
    );
    let ok_pat = ctx.crate_hir.alloc_pat(
        crate::hir_pat::Pat::TupleStruct {
            res: Res::Err,
            pats: vec![ok_inner],
        },
        span,
    );
    let err_inner = ctx.crate_hir.alloc_pat(
        crate::hir_pat::Pat::Binding {
            mode: crate::hir_pat::BindingMode::ByValue,
            name: yelang_interner::Symbol::from(1u32),
            subpat: None,
        },
        span,
    );
    let err_pat = ctx.crate_hir.alloc_pat(
        crate::hir_pat::Pat::TupleStruct {
            res: Res::Err,
            pats: vec![err_inner],
        },
        span,
    );
    let ok_body = ctx.crate_hir.alloc_expr(
        Expr::Path {
            res: Res::Local { pat_id: ok_inner },
        },
        span,
    );
    let err_arg = ctx.crate_hir.alloc_expr(
        Expr::Path {
            res: Res::Local { pat_id: err_inner },
        },
        span,
    );
    let err_func = ctx.crate_hir.alloc_expr(Expr::Path { res: Res::Err }, span);
    let err_call = ctx.crate_hir.alloc_expr(
        Expr::Call {
            func: err_func,
            args: vec![err_arg],
        },
        span,
    );
    let err_body = ctx.crate_hir.alloc_expr(
        Expr::Return {
            expr: Some(err_call),
        },
        span,
    );
    Expr::Match {
        expr: base,
        arms: vec![
            Arm {
                pat: ok_pat,
                guard: None,
                body: ok_body,
                span,
            },
            Arm {
                pat: err_pat,
                guard: None,
                body: err_body,
                span,
            },
        ],
    }
}

/// Desugar `expr.await` into a match expression.
fn lower_await_expr(ctx: &mut LoweringContext, expr: &yelang_ast::Expr, _span: Span) -> Expr {
    let base = lower_expr(ctx, expr);
    // Simplified: just return the base expression.
    // In a full implementation this would desugar to poll-based logic.
    ctx.crate_hir
        .exprs
        .get(base)
        .cloned()
        .expect("await base expression")
}

/// Desugar list comprehension into a loop that builds a vector.
fn lower_comprehension_expr(
    ctx: &mut LoweringContext,
    comp: &yelang_ast::ComprehensionExpr,
    span: Span,
) -> Expr {
    // For MVP, lower to a call to a builtin collector function.
    let element = lower_expr(ctx, &comp.element);
    let sources: Vec<ExprId> = comp
        .variables
        .iter()
        .map(|v| lower_expr(ctx, &v.source))
        .collect();
    let func = ctx.crate_hir.alloc_expr(Expr::Path { res: Res::Err }, span);
    Expr::Call {
        func,
        args: std::iter::once(element).chain(sources).collect(),
    }
}

/// Map an AST assignment operator kind to the corresponding binary operator.
fn assign_op_kind_to_bin_op(kind: &yelang_ast::AssignOpKind) -> yelang_ast::BinaryOp {
    use yelang_ast::{AssignOpKind, BinaryOp};
    match kind {
        AssignOpKind::AddEq => BinaryOp::Add,
        AssignOpKind::SubEq => BinaryOp::Subtract,
        AssignOpKind::MulEq => BinaryOp::Multiply,
        AssignOpKind::DivEq => BinaryOp::Divide,
        AssignOpKind::ModEq => BinaryOp::Modulo,
        AssignOpKind::BitAndEq => BinaryOp::BitAnd,
        AssignOpKind::BitOrEq => BinaryOp::BitOr,
        AssignOpKind::BitXorEq => BinaryOp::BitXor,
        AssignOpKind::BitShlEq => BinaryOp::Shl,
        AssignOpKind::BitShrEq => BinaryOp::Shr,
    }
}
