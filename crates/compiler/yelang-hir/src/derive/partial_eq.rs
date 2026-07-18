//! Built-in `PartialEq` derive.

use yelang_arena::DefId;
use yelang_interner::Symbol;

use crate::derive::context::{AdtInfo, AdtShape, DeriveContext};
use crate::derive::helpers::{
    access_field, arm, bin_op_expr, binding_pat, bool_expr, derive_generics, expr, fn_sig,
    impl_item, iter_fields, make_body, match_expr, method_impl_item, other_param, path_pat,
    self_expr, self_param, struct_pat, tuple_struct_pat, wildcard_false_arm,
};
use crate::hir::adt::VariantData;
use crate::hir::core::{Arm, Expr, ImplItem, Item};
use crate::ids::{ExprId, HirTyId, PatId};

/// Expand `#[derive(PartialEq)]` for a struct or enum.
pub fn derive_partial_eq(
    ctx: &mut DeriveContext<'_, '_>,
    _derives_in_attr: &[Symbol],
) -> Option<Item> {
    let adt = match ctx.adt_info() {
        Ok(adt) => adt,
        Err(err) => {
            ctx.error(err);
            return None;
        }
    };

    let partial_eq_trait = match ctx.trait_def_id("PartialEq") {
        Ok(def_id) => def_id,
        Err(err) => {
            ctx.error(err);
            return None;
        }
    };

    let self_ty = adt.self_ty(ctx);
    let ref_self_ty = ctx.ctx.crate_hir.alloc_ty(
        crate::hir::ty::Ty::Ref {
            mutability: yelang_ast::Mutability::Immutable,
            ty: self_ty,
        },
        ctx.derive_span,
    );

    let eq_method = eq_method(ctx, adt.def_id, &adt, ref_self_ty);
    let generics = derive_generics(ctx, &adt.generics, partial_eq_trait);

    Some(impl_item(
        ctx,
        partial_eq_trait,
        self_ty,
        generics,
        vec![eq_method],
    ))
}

fn eq_method(
    ctx: &mut DeriveContext<'_, '_>,
    self_def_id: DefId,
    adt: &AdtInfo<'_>,
    ref_self_ty: HirTyId,
) -> ImplItem {
    let self_param = self_param(ctx, ref_self_ty);
    let other_param = other_param(ctx, self_def_id);
    let bool_ty = ctx.ctx.crate_hir.alloc_ty(
        crate::hir::ty::Ty::Path {
            res: crate::res::Res::PrimTy {
                ty: crate::res::PrimTy::Bool,
            },
            args: vec![],
        },
        ctx.derive_span,
    );
    let sig = fn_sig(vec![ref_self_ty, ref_self_ty], bool_ty);

    let body_value = match &adt.shape {
        AdtShape::Struct(data) => eq_struct_expression(ctx, self_def_id, data),
        AdtShape::Enum(def) => eq_enum_expression(ctx, self_def_id, adt.def_id, def),
    };

    let body_id = make_body(ctx, vec![self_param, other_param], body_value);
    method_impl_item(ctx, "eq", sig, body_id)
}

fn eq_struct_expression(
    ctx: &mut DeriveContext<'_, '_>,
    self_def_id: DefId,
    data: &VariantData,
) -> ExprId {
    match data {
        VariantData::Unit => bool_expr(ctx, true),
        _ => {
            let self_recv = self_expr(ctx, self_def_id);
            let other_recv = expr_other(ctx);
            let fields = iter_fields(data);
            let comparisons: Vec<_> = fields
                .iter()
                .map(|field| {
                    let left = access_field(ctx, self_recv, field);
                    let right = access_field(ctx, other_recv, field);
                    bin_op_expr(ctx, yelang_ast::BinaryOp::Eq, left, right)
                })
                .collect();
            if comparisons.is_empty() {
                bool_expr(ctx, true)
            } else {
                comparisons
                    .into_iter()
                    .reduce(|acc, next| bin_op_expr(ctx, yelang_ast::BinaryOp::And, acc, next))
                    .expect("non-empty comparisons")
            }
        }
    }
}

fn eq_enum_expression(
    ctx: &mut DeriveContext<'_, '_>,
    self_def_id: DefId,
    enum_def_id: DefId,
    def: &crate::hir::core::EnumDef,
) -> ExprId {
    let self_recv = self_expr(ctx, self_def_id);
    let other_recv = expr_other(ctx);
    let scrutinee = expr(
        ctx,
        Expr::Tuple {
            exprs: vec![self_recv, other_recv],
        },
        ctx.derive_span,
    );

    let mut arms: Vec<Arm> = def
        .variants
        .iter()
        .map(|variant| {
            let variant_def_id = ctx
                .variant_def_id(enum_def_id, variant.ident.symbol)
                .unwrap_or(enum_def_id);
            let (left_pat, right_pat, bindings) =
                variant_eq_patterns(ctx, variant_def_id, &variant.data);
            let tuple_pat = ctx.ctx.crate_hir.alloc_pat(
                crate::hir::pat::Pat::Tuple {
                    pats: vec![left_pat, right_pat],
                },
                ctx.derive_span,
            );
            let body = eq_variant_body(ctx, &variant.data, &bindings);
            arm(ctx, tuple_pat, body)
        })
        .collect();

    arms.push(wildcard_false_arm(ctx));
    match_expr(ctx, scrutinee, arms)
}

/// Binding info returned by `variant_eq_patterns`.
struct BindingPair {
    left: Symbol,
    right: Symbol,
}

fn variant_eq_patterns(
    ctx: &mut DeriveContext<'_, '_>,
    variant_def_id: DefId,
    data: &VariantData,
) -> (PatId, PatId, Vec<BindingPair>) {
    match data {
        VariantData::Unit => (
            path_pat(
                ctx,
                crate::res::Res::Def {
                    def_id: variant_def_id,
                },
            ),
            path_pat(
                ctx,
                crate::res::Res::Def {
                    def_id: variant_def_id,
                },
            ),
            vec![],
        ),
        VariantData::Tuple { fields } => {
            let mut bindings = Vec::new();
            let mut left_pats = Vec::new();
            let mut right_pats = Vec::new();
            for (i, _) in fields.iter().enumerate() {
                let name_l = ctx.intern(&format!("__l{i}"));
                let name_r = ctx.intern(&format!("__r{i}"));
                bindings.push(BindingPair {
                    left: name_l,
                    right: name_r,
                });
                left_pats.push(binding_pat(ctx, name_l));
                right_pats.push(binding_pat(ctx, name_r));
            }
            (
                tuple_struct_pat(
                    ctx,
                    crate::res::Res::Def {
                        def_id: variant_def_id,
                    },
                    left_pats,
                ),
                tuple_struct_pat(
                    ctx,
                    crate::res::Res::Def {
                        def_id: variant_def_id,
                    },
                    right_pats,
                ),
                bindings,
            )
        }
        VariantData::Struct { fields } => {
            let mut bindings = Vec::new();
            let mut left_fields = Vec::new();
            let mut right_fields = Vec::new();
            for f in fields {
                let base = ctx.ctx.interner.resolve(&f.ident.symbol);
                let name_l = ctx.intern(&format!("__l_{base}"));
                let name_r = ctx.intern(&format!("__r_{base}"));
                bindings.push(BindingPair {
                    left: name_l,
                    right: name_r,
                });
                left_fields.push((f.ident, binding_pat(ctx, name_l)));
                right_fields.push((f.ident, binding_pat(ctx, name_r)));
            }
            (
                struct_pat(
                    ctx,
                    crate::res::Res::Def {
                        def_id: variant_def_id,
                    },
                    left_fields,
                ),
                struct_pat(
                    ctx,
                    crate::res::Res::Def {
                        def_id: variant_def_id,
                    },
                    right_fields,
                ),
                bindings,
            )
        }
    }
}

fn eq_variant_body(
    ctx: &mut DeriveContext<'_, '_>,
    data: &VariantData,
    bindings: &[BindingPair],
) -> ExprId {
    match data {
        VariantData::Unit => bool_expr(ctx, true),
        _ => {
            let comparisons: Vec<_> = bindings
                .iter()
                .map(|pair| {
                    let left = local_expr(ctx, pair.left);
                    let right = local_expr(ctx, pair.right);
                    bin_op_expr(ctx, yelang_ast::BinaryOp::Eq, left, right)
                })
                .collect();
            if comparisons.is_empty() {
                bool_expr(ctx, true)
            } else {
                comparisons
                    .into_iter()
                    .reduce(|acc, next| bin_op_expr(ctx, yelang_ast::BinaryOp::And, acc, next))
                    .expect("non-empty comparisons")
            }
        }
    }
}

fn expr_other(ctx: &mut DeriveContext<'_, '_>) -> ExprId {
    let other_pat_id = ctx.ctx.local(ctx.intern("other")).expect("other param");
    expr(
        ctx,
        Expr::Path {
            res: crate::res::Res::Local {
                pat_id: other_pat_id,
            },
        },
        ctx.derive_span,
    )
}

fn local_expr(ctx: &mut DeriveContext<'_, '_>, name: Symbol) -> ExprId {
    let pat_id = ctx.ctx.local(name).expect("local binding");
    expr(
        ctx,
        Expr::Path {
            res: crate::res::Res::Local { pat_id },
        },
        ctx.derive_span,
    )
}
