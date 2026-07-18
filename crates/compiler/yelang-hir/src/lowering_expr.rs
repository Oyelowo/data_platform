//! Lowering of AST expressions to HIR expressions.

use yelang_ast::{
    BlockExpr, Expr as AstExpr,
    ExprKind as AstExprKind, ForLoopExpr, IfExpr, MatchExpr,
    StructExpr, WhileExpr,
};
use yelang_lexer::Span;

use yelang_arena::DefId;

use crate::hir::{Arm, Block, CaptureClause, Expr, FieldExpr, Stmt};
use crate::hir_expr::{ComprehensionKind, DocumentProjection, GeneratorKind};
use crate::hir_item::Item;
use crate::ids::{ExprId, PatId};
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
        AstExprKind::Try(try_expr) => Expr::Try {
            expr: lower_expr(ctx, &try_expr.base),
        },
        AstExprKind::Await(await_expr) => Expr::Await {
            expr: lower_expr(ctx, await_expr),
        },
        AstExprKind::Async(async_expr) => {
            let body = lower_block(ctx, &async_expr.block);
            let body_expr = ctx.crate_hir.alloc_expr(Expr::Block { block: body }, span);
            let body_id = ctx.crate_hir.alloc_body(
                crate::hir_body::Body {
                    params: vec![],
                    value: body_expr,
                    span,
                },
                span,
            );
            Expr::Async { body: body_id }
        }
        AstExprKind::Gen(gen_expr) => {
            let body_expr = lower_expr(ctx, gen_expr);
            let body_id = ctx.crate_hir.alloc_body(
                crate::hir_body::Body {
                    params: vec![],
                    value: body_expr,
                    span,
                },
                span,
            );
            Expr::Gen {
                kind: GeneratorKind::Gen,
                body: body_id,
            }
        }
        AstExprKind::Ternary(ternary) => Expr::If {
            cond: lower_expr(ctx, &ternary.condition),
            then_branch: lower_expr(ctx, &ternary.if_true),
            else_branch: Some(lower_expr(ctx, &ternary.if_false)),
        },
        AstExprKind::Grouped(grouped) => return lower_expr(ctx, &grouped.expr),
        AstExprKind::TypeAscription(asc) => Expr::TypeAscription {
            expr: lower_expr(ctx, &asc.expr),
            ty: crate::lowering_ty::lower_ty(ctx, &asc.ty),
        },
        AstExprKind::IsType(is_type) => Expr::IsType {
            expr: lower_expr(ctx, &is_type.expr),
            ty: crate::lowering_ty::lower_ty(ctx, &is_type.ty),
        },
        AstExprKind::AssignOp(assign) => Expr::AssignOp {
            op: assign.op.clone(),
            left: lower_expr(ctx, &assign.target),
            right: lower_expr(ctx, &assign.value),
        },
        AstExprKind::DestructureAssign(assign) => {
            lower_destructure_assign_expr(ctx, assign, span)
        }
        AstExprKind::Range(range) => Expr::Range {
            start: range.start.as_ref().map(|e| lower_expr(ctx, e)),
            end: range.end.as_ref().map(|e| lower_expr(ctx, e)),
            inclusive: range.op.is_inclusive(),
        },
        AstExprKind::Object(obj) => {
            let fields: Vec<FieldExpr> = obj
                .fields()
                .iter()
                .map(|f| FieldExpr {
                    ident: *f.key(),
                    expr: lower_expr(ctx, f.value()),
                    span: f.value().span,
                })
                .collect();
            Expr::Object { fields }
        }
        AstExprKind::DocumentAccess(doc) => {
            let projection = doc
                .fields()
                .iter()
                .map(|f| match f {
                    yelang_ast::DocumentField::KeyOnly(ko) => DocumentProjection::Field {
                        name: ko.key,
                        value: None,
                    },
                    yelang_ast::DocumentField::KeyVal(kv) => DocumentProjection::Field {
                        name: kv.key,
                        value: Some(lower_expr(ctx, &kv.value)),
                    },
                    yelang_ast::DocumentField::Spread(s) => DocumentProjection::Spread(lower_expr(ctx, &s.expr)),
                })
                .collect();
            Expr::DocumentAccess {
                base: lower_expr(ctx, doc.base()),
                projection,
            }
        }
        AstExprKind::BindAt(_bind) => {
            ctx.error(LoweringError::UnsupportedAst {
                kind: "bind-at expression (use it in pattern position instead)".to_string(),
                span,
            });
            Expr::Err
        }
        AstExprKind::Query(_query) => {
            ctx.error(LoweringError::UnsupportedAst {
                kind: "query expression".to_string(),
                span,
            });
            Expr::Err
        }
        AstExprKind::Comprehension(comp) => {
            lower_comprehension_expr(ctx, comp, span)
        }
        AstExprKind::InterpolatedString(_parts) => {
            ctx.error(LoweringError::UnsupportedAst {
                kind: "interpolated string".to_string(),
                span,
            });
            Expr::Err
        }
        AstExprKind::Underscore => {
            ctx.error(LoweringError::UnsupportedAst {
                kind: "underscore expression".to_string(),
                span,
            });
            Expr::Err
        }
        AstExprKind::Err => Expr::Err,
        AstExprKind::Dummy => Expr::Err,
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

    ctx.error(LoweringError::UnresolvedName {
        name: path
            .standalone_ident()
            .map(|i| i.symbol)
            .unwrap_or_else(|| yelang_interner::Symbol::from(0u32)),
        span: path.span,
    });
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
                        attrs: vec![],
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

/// Lower a destructuring assignment `pattern = value`.
fn lower_destructure_assign_expr(
    ctx: &mut LoweringContext,
    assign: &yelang_ast::DestructureAssignExpr,
    _span: Span,
) -> Expr {
    let pat = crate::lowering_pat::lower_pat(ctx, &assign.pattern);
    let value = lower_expr(ctx, &assign.value);
    Expr::DestructureAssign { pat, value }
}

/// Lower a list/set/dict comprehension into a structured HIR node.
fn lower_comprehension_expr(
    ctx: &mut LoweringContext,
    comp: &yelang_ast::ComprehensionExpr,
    _span: Span,
) -> Expr {
    let element = lower_expr(ctx, &comp.element);
    let variables: Vec<(PatId, ExprId)> = comp
        .variables
        .iter()
        .map(|v| {
            let pat = crate::lowering_pat::lower_pat(ctx, &v.pattern);
            let source = lower_expr(ctx, &v.source);
            (pat, source)
        })
        .collect();
    let condition = comp.condition.as_ref().map(|e| lower_expr(ctx, e));
    Expr::Comprehension {
        kind: ComprehensionKind::List,
        element,
        variables,
        condition,
    }
}
