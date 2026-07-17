//! HIR construction helpers for built-in derive expansion.
//!
//! These helpers keep derive implementations readable and ensure generated nodes
//! carry sensible spans.

use yelang_arena::DefId;
use yelang_interner::Symbol;
use yelang_lexer::Span;

use crate::derive::context::DeriveContext;
use crate::hir::{
    Arm, Block, Expr, ExprKind, FieldExpr, FnSig, ImplItem, ImplItemKind, Item, ItemKind, Lit,
    Param, Stmt, StmtKind, TraitRef,
};
use crate::hir_body::Body;
use crate::hir_pat::{BindingMode, FieldPat, Pat, PatKind};
use crate::hir_struct::VariantData;
use crate::hir_ty::Ty;
use crate::ids::BodyId;
use crate::res::Res;

/// An identifier constructed from a string, using the derive span as its span.
pub fn ident(ctx: &DeriveContext<'_, '_>, name: &str) -> yelang_ast::Ident {
    yelang_ast::Ident::new(ctx.intern(name), ctx.derive_span)
}

/// A symbol constructed from a string.
pub fn sym(ctx: &DeriveContext<'_, '_>, name: &str) -> Symbol {
    ctx.intern(name)
}

/// Build a path type referring to a definition with no generic arguments.
pub fn path_ty(ctx: &DeriveContext<'_, '_>, def_id: DefId) -> Ty {
    Ty {
        kind: crate::hir_ty::TyKind::Path {
            res: Res::Def { def_id },
            args: vec![],
        },
        span: ctx.derive_span,
    }
}

/// Build a `Self` type.
pub fn self_ty(ctx: &DeriveContext<'_, '_>, def_id: DefId) -> Ty {
    Ty {
        kind: crate::hir_ty::TyKind::Path {
            res: Res::SelfTy { def_id },
            args: vec![],
        },
        span: ctx.derive_span,
    }
}

/// Build a reference type `&T`.
pub fn ref_ty(ty: Ty, mutable: bool) -> Ty {
    let span = ty.span;
    Ty {
        kind: crate::hir_ty::TyKind::Ref {
            mutability: if mutable {
                yelang_ast::Mutability::Mutable
            } else {
                yelang_ast::Mutability::Immutable
            },
            ty: Box::new(ty),
        },
        span,
    }
}

/// Build the unit type `()`.
pub fn unit_ty(span: Span) -> Ty {
    Ty {
        kind: crate::hir_ty::TyKind::Tuple { tys: vec![] },
        span,
    }
}

/// Build a HIR expression with the given kind and span.
pub fn expr(ctx: &mut DeriveContext<'_, '_>, kind: ExprKind, span: Span) -> Expr {
    Expr {
        hir_id: ctx.next_hir_id(),
        kind,
        span,
        ty: Ty {
            kind: crate::hir_ty::TyKind::Infer,
            span,
        },
    }
}

/// Build a path expression.
pub fn path_expr(ctx: &mut DeriveContext<'_, '_>, res: Res) -> Expr {
    expr(ctx, ExprKind::Path { res }, ctx.derive_span)
}

/// Build an expression referring to `self`.
pub fn self_expr(ctx: &mut DeriveContext<'_, '_>, def_id: DefId) -> Expr {
    path_expr(ctx, Res::SelfVal { def_id })
}

/// Build a field access expression.
pub fn field_expr(ctx: &mut DeriveContext<'_, '_>, base: Expr, field: yelang_ast::Ident) -> Expr {
    let span = base.span.merge(field.span());
    expr(
        ctx,
        ExprKind::Field {
            expr: Box::new(base),
            field,
        },
        span,
    )
}

/// Build a tuple-index field access expression (`self.0`).
pub fn tuple_field_expr(ctx: &mut DeriveContext<'_, '_>, base: Expr, index: usize) -> Expr {
    let field = yelang_ast::Ident::new(Symbol::from(index as u32), base.span);
    field_expr(ctx, base, field)
}

/// Build a method call expression.
pub fn method_call_expr(
    ctx: &mut DeriveContext<'_, '_>,
    receiver: Expr,
    method: &str,
    args: Vec<Expr>,
) -> Expr {
    let span = receiver.span;
    expr(
        ctx,
        ExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: ident(ctx, method),
            args,
            trait_def_id: None,
        },
        span,
    )
}

/// Build a binary operation expression.
pub fn bin_op_expr(
    ctx: &mut DeriveContext<'_, '_>,
    op: yelang_ast::BinaryOp,
    left: Expr,
    right: Expr,
) -> Expr {
    let span = left.span.merge(right.span);
    expr(
        ctx,
        ExprKind::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        span,
    )
}

/// Build a boolean literal expression.
pub fn bool_expr(ctx: &mut DeriveContext<'_, '_>, value: bool) -> Expr {
    expr(
        ctx,
        ExprKind::Lit {
            lit: Lit::Bool(value),
        },
        ctx.derive_span,
    )
}

/// Build a string literal expression.
pub fn string_expr(ctx: &mut DeriveContext<'_, '_>, value: &str) -> Expr {
    let interner = ctx.ctx.interner;
    let lit = Lit::Str(yelang_lexer::StringLit {
        value: interner.get_or_intern(value),
        kind: yelang_lexer::StrKind::Normal,
    });
    expr(ctx, ExprKind::Lit { lit }, ctx.derive_span)
}

/// Build a struct literal expression.
pub fn struct_literal(
    ctx: &mut DeriveContext<'_, '_>,
    path: Res,
    fields: Vec<(yelang_ast::Ident, Expr)>,
) -> Expr {
    let span = ctx.derive_span;
    let fields = fields
        .into_iter()
        .map(|(ident, expr)| FieldExpr {
            ident,
            expr,
            span: ident.span(),
        })
        .collect();
    expr(
        ctx,
        ExprKind::Struct {
            path,
            fields,
            rest: None,
        },
        span,
    )
}

/// Build an enum variant literal expression from a variant `DefId`.
///
/// The variant is invoked as a function call: `VariantName(fields...)`.
pub fn enum_variant_literal(
    ctx: &mut DeriveContext<'_, '_>,
    variant_def_id: DefId,
    fields: Vec<Expr>,
) -> Expr {
    let span = ctx.derive_span;
    let func = path_expr(
        ctx,
        Res::Def {
            def_id: variant_def_id,
        },
    );
    expr(
        ctx,
        ExprKind::Call {
            func: Box::new(func),
            args: fields,
        },
        span,
    )
}

/// Build a match expression.
pub fn match_expr(ctx: &mut DeriveContext<'_, '_>, scrutinee: Expr, arms: Vec<Arm>) -> Expr {
    let span = ctx.derive_span;
    expr(
        ctx,
        ExprKind::Match {
            expr: Box::new(scrutinee),
            arms,
        },
        span,
    )
}

/// Build a match arm.
pub fn arm(ctx: &mut DeriveContext<'_, '_>, pat: Pat, body: Expr) -> Arm {
    Arm {
        pat,
        guard: None,
        body: Box::new(body),
        span: ctx.derive_span,
    }
}

/// Build a wildcard arm body returning `false` (used by `PartialEq`).
pub fn wildcard_false_arm(ctx: &mut DeriveContext<'_, '_>) -> Arm {
    let pat = wild_pat(ctx);
    let body = bool_expr(ctx, false);
    arm(ctx, pat, body)
}

/// Build a block expression from a list of statements and an optional tail.
pub fn block_expr(ctx: &mut DeriveContext<'_, '_>, stmts: Vec<Stmt>, tail: Option<Expr>) -> Expr {
    let span = ctx.derive_span;
    expr(
        ctx,
        ExprKind::Block {
            block: Block {
                stmts,
                expr: tail.map(Box::new),
                span,
            },
        },
        span,
    )
}

/// Build a `let` statement.
pub fn let_stmt(
    ctx: &mut DeriveContext<'_, '_>,
    pat: Pat,
    ty: Option<Ty>,
    init: Option<Expr>,
) -> Stmt {
    Stmt {
        kind: StmtKind::Let {
            pat,
            ty,
            init: init.map(Box::new),
        },
        span: ctx.derive_span,
    }
}

/// Build a body from parameters and a value expression, and register it in the crate.
pub fn make_body(ctx: &mut DeriveContext<'_, '_>, params: Vec<Param>, value: Expr) -> BodyId {
    let body_id = ctx.next_body_id();
    let body = Body {
        params,
        value,
        span: ctx.derive_span,
    };
    ctx.ctx.crate_hir.bodies.insert(body_id, body);
    body_id
}

/// Build a function parameter from a pattern and type.
pub fn param(ctx: &mut DeriveContext<'_, '_>, pat: Pat, ty: Ty) -> Param {
    Param {
        pat,
        ty,
        span: ctx.derive_span,
    }
}

/// Build a `self` parameter with the given type (usually `&Self`).
pub fn self_param(ctx: &mut DeriveContext<'_, '_>, ty: Ty) -> Param {
    let hir_id = ctx.next_hir_id();
    let name = ctx.intern("self");
    let pat = Pat {
        hir_id,
        kind: PatKind::Binding {
            mode: BindingMode::ByValue,
            name,
            subpat: None,
        },
        span: ctx.derive_span,
    };
    ctx.ctx.push_local(name, hir_id);
    param(ctx, pat, ty)
}

/// Build an `other: &Self` parameter.
pub fn other_param(ctx: &mut DeriveContext<'_, '_>, self_def_id: DefId) -> Param {
    let hir_id = ctx.next_hir_id();
    let name = ctx.intern("other");
    let pat = Pat {
        hir_id,
        kind: PatKind::Binding {
            mode: BindingMode::ByValue,
            name,
            subpat: None,
        },
        span: ctx.derive_span,
    };
    ctx.ctx.push_local(name, hir_id);
    let ty = ref_ty(self_ty(ctx, self_def_id), false);
    param(ctx, pat, ty)
}

/// Build a formatter parameter `f: &mut Formatter`.
pub fn formatter_param(ctx: &mut DeriveContext<'_, '_>, formatter_def_id: DefId) -> Param {
    let hir_id = ctx.next_hir_id();
    let name = ctx.intern("f");
    let pat = Pat {
        hir_id,
        kind: PatKind::Binding {
            mode: BindingMode::ByValue,
            name,
            subpat: None,
        },
        span: ctx.derive_span,
    };
    ctx.ctx.push_local(name, hir_id);
    let ty = ref_ty(path_ty(ctx, formatter_def_id), true);
    param(ctx, pat, ty)
}

/// Build a function signature.
pub fn fn_sig(inputs: Vec<Ty>, output: Ty) -> FnSig {
    FnSig {
        inputs,
        output,
        is_async: false,
        is_const: false,
        is_variadic: false,
        abi: None,
        bound_vars: vec![],
    }
}

/// Build an impl item for a method.
pub fn method_impl_item(
    ctx: &mut DeriveContext<'_, '_>,
    name: &str,
    sig: FnSig,
    body_id: BodyId,
) -> ImplItem {
    ImplItem {
        ident: ident(ctx, name),
        kind: ImplItemKind::Fn { sig, body: body_id },
        span: ctx.derive_span,
        defaultness: crate::hir::Defaultness::Final,
    }
}

/// Build an impl block item for a trait and type.
pub fn impl_item(
    ctx: &mut DeriveContext<'_, '_>,
    trait_def_id: DefId,
    self_ty: Ty,
    items: Vec<ImplItem>,
) -> Item {
    let def_id = ctx.next_def_id();
    Item {
        def_id,
        ident: ident(ctx, "<derived impl>"),
        kind: ItemKind::Impl {
            items,
            generics: crate::hir::Generics {
                params: vec![],
                where_clause: None,
                span: ctx.derive_span,
            },
            self_ty,
            of_trait: Some(TraitRef {
                path: Res::Def {
                    def_id: trait_def_id,
                },
                span: ctx.derive_span,
            }),
        },
        vis: yelang_ast::Visibility::Public(ctx.derive_span),
        span: ctx.derive_span,
    }
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

/// Build a wildcard pattern.
pub fn wild_pat(ctx: &mut DeriveContext<'_, '_>) -> Pat {
    Pat {
        hir_id: ctx.next_hir_id(),
        kind: PatKind::Wild,
        span: ctx.derive_span,
    }
}

/// Build a binding pattern with a fresh `HirId`.
pub fn binding_pat(ctx: &mut DeriveContext<'_, '_>, name: Symbol) -> Pat {
    let hir_id = ctx.next_hir_id();
    ctx.ctx.push_local(name, hir_id);
    Pat {
        hir_id,
        kind: PatKind::Binding {
            mode: BindingMode::ByValue,
            name,
            subpat: None,
        },
        span: ctx.derive_span,
    }
}

/// Build a struct pattern.
pub fn struct_pat(
    ctx: &mut DeriveContext<'_, '_>,
    res: Res,
    fields: Vec<(yelang_ast::Ident, Pat)>,
) -> Pat {
    let fields = fields
        .into_iter()
        .map(|(ident, pat)| FieldPat {
            ident,
            pat,
            is_shorthand: false,
            span: ident.span(),
        })
        .collect();
    Pat {
        hir_id: ctx.next_hir_id(),
        kind: PatKind::Struct {
            res,
            fields,
            rest: false,
        },
        span: ctx.derive_span,
    }
}

/// Build a tuple-struct pattern.
pub fn tuple_struct_pat(ctx: &mut DeriveContext<'_, '_>, res: Res, pats: Vec<Pat>) -> Pat {
    Pat {
        hir_id: ctx.next_hir_id(),
        kind: PatKind::TupleStruct { res, pats },
        span: ctx.derive_span,
    }
}

/// Build a path pattern (for unit variants).
pub fn path_pat(ctx: &mut DeriveContext<'_, '_>, res: Res) -> Pat {
    Pat {
        hir_id: ctx.next_hir_id(),
        kind: PatKind::Path { res },
        span: ctx.derive_span,
    }
}

// ---------------------------------------------------------------------------
// Field iteration helpers
// ---------------------------------------------------------------------------

/// A unified view of a field in a struct or enum variant.
pub struct FieldView<'a> {
    pub ident: Option<yelang_ast::Ident>,
    pub index: usize,
    pub ty: &'a Ty,
}

/// Iterate over the fields of a `VariantData`.
pub fn iter_fields(data: &VariantData) -> Vec<FieldView<'_>> {
    match data {
        VariantData::Struct { fields } => fields
            .iter()
            .enumerate()
            .map(|(i, f)| FieldView {
                ident: Some(f.ident),
                index: i,
                ty: &f.ty,
            })
            .collect(),
        VariantData::Tuple { fields } => fields
            .iter()
            .enumerate()
            .map(|(i, f)| FieldView {
                ident: None,
                index: i,
                ty: &f.ty,
            })
            .collect(),
        VariantData::Unit => vec![],
    }
}

/// Build a field access expression for a field view.
pub fn access_field(
    ctx: &mut DeriveContext<'_, '_>,
    receiver: Expr,
    field: &FieldView<'_>,
) -> Expr {
    match field.ident {
        Some(ident) => field_expr(ctx, receiver, ident),
        None => tuple_field_expr(ctx, receiver, field.index),
    }
}
