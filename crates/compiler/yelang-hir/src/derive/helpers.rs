//! HIR construction helpers for built-in derive expansion.
//!
//! These helpers keep derive implementations readable and ensure generated nodes
//! carry sensible spans.

use yelang_arena::DefId;
use yelang_interner::Symbol;
use yelang_lexer::Span;

use crate::derive::context::DeriveContext;
use crate::hir::core::{
    Arm, Block, Expr, FieldExpr, FnSig, ImplItem, ImplItemKind, Item, ItemKind, Lit,
    Param, Stmt, TraitRef,
};
use crate::hir::body::Body;
use crate::hir::pat::{BindingMode, FieldPat, Pat};
use crate::hir::adt::VariantData;
use crate::hir::ty::Ty;
use crate::ids::{BodyId, ExprId, PatId, StmtId, HirTyId};
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
pub fn path_ty(ctx: &mut DeriveContext<'_, '_>, def_id: DefId) -> HirTyId {
    let ty = Ty::Path {
        res: Res::Def { def_id },
        args: vec![],
    };
    ctx.ctx.crate_hir.alloc_ty(ty, ctx.derive_span)
}

/// Build a type reference to a type parameter by its `DefId`.
pub fn type_param_ty(ctx: &mut DeriveContext<'_, '_>, def_id: DefId) -> HirTyId {
    let ty = Ty::Path {
        res: Res::Def { def_id },
        args: vec![],
    };
    ctx.ctx.crate_hir.alloc_ty(ty, ctx.derive_span)
}

/// Build a `Generics` block for a derived impl.
///
/// The ADT's own type parameters are preserved, and each type parameter gets
/// an additional bound on `trait_def_id` (e.g. `T: Clone`). Const parameters are
/// copied unchanged.
pub fn derive_generics(
    _ctx: &mut DeriveContext<'_, '_>,
    adt_generics: &crate::hir::core::Generics,
    trait_def_id: DefId,
) -> crate::hir::core::Generics {
    use crate::hir::core::{GenericParam, TraitBound};
    let params = adt_generics
        .params
        .iter()
        .map(|p| match p {
            GenericParam::Type {
                def_id,
                name,
                bounds,
                default,
                span,
            } => {
                let mut new_bounds = bounds.clone();
                new_bounds.push(TraitBound {
                    path: Res::Def {
                        def_id: trait_def_id,
                    },
                    args: vec![],
                    span: *span,
                });
                GenericParam::Type {
                    def_id: *def_id,
                    name: *name,
                    bounds: new_bounds,
                    default: *default,
                    span: *span,
                }
            }
            GenericParam::Const {
                def_id,
                name,
                ty,
                default,
                span,
            } => GenericParam::Const {
                def_id: *def_id,
                name: *name,
                ty: *ty,
                default: *default,
                span: *span,
            },
        })
        .collect();
    crate::hir::core::Generics {
        params,
        where_clause: adt_generics.where_clause.clone(),
        span: adt_generics.span,
    }
}

/// Build a `Self` type.
pub fn self_ty(ctx: &mut DeriveContext<'_, '_>, def_id: DefId) -> HirTyId {
    let ty = Ty::Path {
        res: Res::SelfTy { def_id },
        args: vec![],
    };
    ctx.ctx.crate_hir.alloc_ty(ty, ctx.derive_span)
}

/// Build a reference type `&T`.
pub fn ref_ty(ty: HirTyId, mutable: bool) -> Ty {
    Ty::Ref {
        mutability: if mutable {
            yelang_ast::Mutability::Mutable
        } else {
            yelang_ast::Mutability::Immutable
        },
        ty,
    }
}

/// Build the unit type `()`.
pub fn unit_ty(_span: Span) -> Ty {
    Ty::Tuple { tys: vec![] }
}

/// Build a HIR expression with the given kind and span, allocate it in the
/// crate arena, and return its `ExprId`.
pub fn expr(ctx: &mut DeriveContext<'_, '_>, kind: Expr, span: Span) -> ExprId {
    ctx.ctx.crate_hir.alloc_expr(kind, span)
}

/// Build a path expression.
pub fn path_expr(ctx: &mut DeriveContext<'_, '_>, res: Res) -> ExprId {
    expr(ctx, Expr::Path { res }, ctx.derive_span)
}

/// Build an expression referring to `self`.
pub fn self_expr(ctx: &mut DeriveContext<'_, '_>, def_id: DefId) -> ExprId {
    path_expr(ctx, Res::SelfVal { def_id })
}

/// Build a field access expression.
pub fn field_expr(
    ctx: &mut DeriveContext<'_, '_>,
    base: ExprId,
    field: yelang_ast::Ident,
) -> ExprId {
    let span = ctx.ctx.crate_hir.expr_span(base).merge(field.span());
    expr(ctx, Expr::Field { expr: base, field }, span)
}

/// Build a tuple-index field access expression (`self.0`).
pub fn tuple_field_expr(ctx: &mut DeriveContext<'_, '_>, base: ExprId, index: usize) -> ExprId {
    let field = yelang_ast::Ident::new(Symbol::from(index as u32), ctx.derive_span);
    field_expr(ctx, base, field)
}

/// Build a method call expression.
pub fn method_call_expr(
    ctx: &mut DeriveContext<'_, '_>,
    receiver: ExprId,
    method: &str,
    args: Vec<ExprId>,
) -> ExprId {
    let span = ctx.ctx.crate_hir.expr_span(receiver);
    expr(
        ctx,
        Expr::MethodCall {
            receiver,
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
    left: ExprId,
    right: ExprId,
) -> ExprId {
    let span = ctx
        .ctx
        .crate_hir
        .expr_span(left)
        .merge(ctx.ctx.crate_hir.expr_span(right));
    expr(ctx, Expr::Binary { op, left, right }, span)
}

/// Build a boolean literal expression.
pub fn bool_expr(ctx: &mut DeriveContext<'_, '_>, value: bool) -> ExprId {
    expr(
        ctx,
        Expr::Lit {
            lit: Lit::Bool(value),
        },
        ctx.derive_span,
    )
}

/// Build a string literal expression.
pub fn string_expr(ctx: &mut DeriveContext<'_, '_>, value: &str) -> ExprId {
    let interner = ctx.ctx.interner;
    let lit = Lit::Str(yelang_lexer::StringLit {
        value: interner.get_or_intern(value),
        kind: yelang_lexer::StrKind::Normal,
    });
    expr(ctx, Expr::Lit { lit }, ctx.derive_span)
}

/// Build a struct literal expression.
pub fn struct_literal(
    ctx: &mut DeriveContext<'_, '_>,
    path: Res,
    fields: Vec<(yelang_ast::Ident, ExprId)>,
) -> ExprId {
    let span = ctx.derive_span;
    let fields = fields
        .into_iter()
        .map(|(ident, expr)| FieldExpr {
            ident,
            expr,
            span: ctx.ctx.crate_hir.expr_span(expr),
        })
        .collect();
    expr(
        ctx,
        Expr::Struct {
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
    fields: Vec<ExprId>,
) -> ExprId {
    let span = ctx.derive_span;
    let func = path_expr(
        ctx,
        Res::Def {
            def_id: variant_def_id,
        },
    );
    expr(ctx, Expr::Call { func, args: fields }, span)
}

/// Build a match expression.
pub fn match_expr(ctx: &mut DeriveContext<'_, '_>, scrutinee: ExprId, arms: Vec<Arm>) -> ExprId {
    let span = ctx.derive_span;
    expr(ctx, Expr::Match { expr: scrutinee, arms }, span)
}

/// Build a match arm.
pub fn arm(ctx: &mut DeriveContext<'_, '_>, pat: PatId, body: ExprId) -> Arm {
    Arm {
        pat,
        guard: None,
        body,
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
pub fn block_expr(
    ctx: &mut DeriveContext<'_, '_>,
    stmts: Vec<StmtId>,
    tail: Option<ExprId>,
) -> ExprId {
    let span = ctx.derive_span;
    expr(
        ctx,
        Expr::Block {
            block: Block {
                stmts,
                expr: tail,
                span,
            },
        },
        span,
    )
}

/// Build a `let` statement.
pub fn let_stmt(
    ctx: &mut DeriveContext<'_, '_>,
    pat: PatId,
    ty: Option<HirTyId>,
    init: Option<ExprId>,
) -> StmtId {
    let stmt = Stmt::Let { pat, ty, init };
    ctx.ctx.crate_hir.alloc_stmt(stmt, ctx.derive_span)
}

/// Build a body from parameters and a value expression, and register it in the crate.
pub fn make_body(ctx: &mut DeriveContext<'_, '_>, params: Vec<Param>, value: ExprId) -> BodyId {
    let body = Body {
        params,
        value,
        span: ctx.derive_span,
    };
    ctx.ctx.crate_hir.alloc_body(body, ctx.derive_span)
}

/// Build a function parameter from a pattern and type.
pub fn param(ctx: &mut DeriveContext<'_, '_>, pat: PatId, ty: HirTyId) -> Param {
    Param {
        pat,
        ty,
        span: ctx.derive_span,
    }
}

/// Build a `self` parameter with the given type (usually `&Self`).
pub fn self_param(ctx: &mut DeriveContext<'_, '_>, ty: HirTyId) -> Param {
    let name = ctx.intern("self");
    let pat = ctx.ctx.crate_hir.alloc_pat(
        Pat::Binding {
            mode: BindingMode::ByValue,
            name,
            subpat: None,
        },
        ctx.derive_span,
    );
    ctx.ctx.push_local(name, pat);
    param(ctx, pat, ty)
}

/// Build an `other: &Self` parameter.
pub fn other_param(ctx: &mut DeriveContext<'_, '_>, self_def_id: DefId) -> Param {
    let name = ctx.intern("other");
    let pat = ctx.ctx.crate_hir.alloc_pat(
        Pat::Binding {
            mode: BindingMode::ByValue,
            name,
            subpat: None,
        },
        ctx.derive_span,
    );
    ctx.ctx.push_local(name, pat);
    let self_ty_id = self_ty(ctx, self_def_id);
    let ty = ctx.ctx.crate_hir.alloc_ty(
        ref_ty(self_ty_id, false),
        ctx.derive_span,
    );
    param(ctx, pat, ty)
}

/// Build a formatter parameter `f: &mut Formatter`.
pub fn formatter_param(ctx: &mut DeriveContext<'_, '_>, formatter_def_id: DefId) -> Param {
    let name = ctx.intern("f");
    let pat = ctx.ctx.crate_hir.alloc_pat(
        Pat::Binding {
            mode: BindingMode::ByValue,
            name,
            subpat: None,
        },
        ctx.derive_span,
    );
    ctx.ctx.push_local(name, pat);
    let formatter_ty = path_ty(ctx, formatter_def_id);
    let ty = ctx.ctx.crate_hir.alloc_ty(
        ref_ty(formatter_ty, true),
        ctx.derive_span,
    );
    param(ctx, pat, ty)
}

/// Build a function signature.
pub fn fn_sig(inputs: Vec<HirTyId>, output: HirTyId) -> FnSig {
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
        def_id: ctx.next_synthetic_def_id(),
        ident: ident(ctx, name),
        kind: ImplItemKind::Fn { sig, body: body_id },
        attrs: vec![],
        span: ctx.derive_span,
        defaultness: crate::hir::core::Defaultness::Final,
    }
}

/// Build an impl block item for a trait and type.
///
/// `generics` are the impl-level generic parameters (copied from the ADT),
/// and `bounds` are the per-parameter trait bounds required by the derive
/// (e.g. `T: Clone` for `#[derive(Clone)]`).
pub fn impl_item(
    ctx: &mut DeriveContext<'_, '_>,
    trait_def_id: DefId,
    self_ty: HirTyId,
    generics: crate::hir::core::Generics,
    items: Vec<ImplItem>,
) -> Item {
    let def_id = ctx.next_synthetic_def_id();
    Item {
        def_id,
        ident: ident(ctx, "<derived impl>"),
        attrs: vec![],
        kind: ItemKind::Impl {
            items,
            generics,
            self_ty,
            polarity: crate::hir::core::ImplPolarity::Positive,
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
pub fn wild_pat(ctx: &mut DeriveContext<'_, '_>) -> PatId {
    ctx.ctx
        .crate_hir
        .alloc_pat(Pat::Wild, ctx.derive_span)
}

/// Build a binding pattern with a fresh `PatId`.
pub fn binding_pat(ctx: &mut DeriveContext<'_, '_>, name: Symbol) -> PatId {
    let pat_id = ctx.ctx.crate_hir.alloc_pat(
        Pat::Binding {
            mode: BindingMode::ByValue,
            name,
            subpat: None,
        },
        ctx.derive_span,
    );
    ctx.ctx.push_local(name, pat_id);
    pat_id
}

/// Build a struct pattern.
pub fn struct_pat(
    ctx: &mut DeriveContext<'_, '_>,
    res: Res,
    fields: Vec<(yelang_ast::Ident, PatId)>,
) -> PatId {
    let fields = fields
        .into_iter()
        .map(|(ident, pat)| FieldPat {
            ident,
            pat,
            is_shorthand: false,
            span: ctx.derive_span,
        })
        .collect();
    ctx.ctx
        .crate_hir
        .alloc_pat(Pat::Struct { res, fields, rest: false }, ctx.derive_span)
}

/// Build a tuple-struct pattern.
pub fn tuple_struct_pat(ctx: &mut DeriveContext<'_, '_>, res: Res, pats: Vec<PatId>) -> PatId {
    ctx.ctx
        .crate_hir
        .alloc_pat(Pat::TupleStruct { res, pats }, ctx.derive_span)
}

/// Build a path pattern (for unit variants).
pub fn path_pat(ctx: &mut DeriveContext<'_, '_>, res: Res) -> PatId {
    ctx.ctx
        .crate_hir
        .alloc_pat(Pat::Path { res }, ctx.derive_span)
}

// ---------------------------------------------------------------------------
// Field iteration helpers
// ---------------------------------------------------------------------------

/// A unified view of a field in a struct or enum variant.
pub struct FieldView {
    pub ident: Option<yelang_ast::Ident>,
    pub index: usize,
    pub ty: HirTyId,
}

/// Iterate over the fields of a `VariantData`.
pub fn iter_fields(data: &VariantData) -> Vec<FieldView> {
    match data {
        VariantData::Struct { fields } => fields
            .iter()
            .enumerate()
            .map(|(i, f)| FieldView {
                ident: Some(f.ident),
                index: i,
                ty: f.ty,
            })
            .collect(),
        VariantData::Tuple { fields } => fields
            .iter()
            .enumerate()
            .map(|(i, f)| FieldView {
                ident: None,
                index: i,
                ty: f.ty,
            })
            .collect(),
        VariantData::Unit => vec![],
    }
}

/// Build a field access expression for a field view.
pub fn access_field(
    ctx: &mut DeriveContext<'_, '_>,
    receiver: ExprId,
    field: &FieldView,
) -> ExprId {
    match field.ident {
        Some(ident) => field_expr(ctx, receiver, ident),
        None => tuple_field_expr(ctx, receiver, field.index),
    }
}
