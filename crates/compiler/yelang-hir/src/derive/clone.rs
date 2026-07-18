//! Built-in `Clone` derive.

use yelang_arena::DefId;
use yelang_interner::Symbol;

use crate::derive::context::{AdtInfo, AdtShape, DeriveContext};
use crate::derive::helpers::{
    FieldView, access_field, arm, binding_pat, derive_generics, enum_variant_literal, expr, fn_sig,
    impl_item, make_body, match_expr, method_call_expr, method_impl_item, path_pat, self_expr,
    self_param, struct_literal, struct_pat, tuple_field_expr, tuple_struct_pat,
};
use crate::hir::core::{Arm, Expr, ImplItem, Item};
use crate::ids::{ExprId, PatId, HirTyId};
use crate::hir::adt::VariantData;

/// Expand `#[derive(Clone)]` for a struct or enum.
pub fn derive_clone(ctx: &mut DeriveContext<'_, '_>, _derives_in_attr: &[Symbol]) -> Option<Item> {
    let adt = match ctx.adt_info() {
        Ok(adt) => adt,
        Err(err) => {
            ctx.error(err);
            return None;
        }
    };

    let clone_trait = match ctx.trait_def_id("Clone") {
        Ok(def_id) => def_id,
        Err(err) => {
            ctx.error(err);
            return None;
        }
    };

    let self_ty = adt.self_ty(ctx);
    let ref_self_ty = ctx.ctx.crate_hir.alloc_ty(
        crate::hir::ty::HirTy::Ref {
            mutability: yelang_ast::Mutability::Immutable,
            ty: self_ty,
        },
        ctx.derive_span,
    );

    let clone_method = clone_method(ctx, adt.def_id, &adt, ref_self_ty, self_ty);
    let generics = derive_generics(ctx, &adt.generics, clone_trait);

    Some(impl_item(ctx, clone_trait, self_ty, generics, vec![clone_method]))
}

fn clone_method(
    ctx: &mut DeriveContext<'_, '_>,
    self_def_id: DefId,
    adt: &AdtInfo<'_>,
    ref_self_ty: HirTyId,
    receiver_ty: HirTyId,
) -> ImplItem {
    let self_param = self_param(ctx, ref_self_ty);
    let sig = fn_sig(vec![self_param.ty], receiver_ty);

    let body_value = match &adt.shape {
        AdtShape::Struct(data) => clone_struct_expr(ctx, self_def_id, data),
        AdtShape::Enum(def) => clone_enum_expr(ctx, self_def_id, adt.def_id, def),
    };

    let body_id = make_body(ctx, vec![self_param], body_value);
    method_impl_item(ctx, "clone", sig, body_id)
}

fn clone_struct_expr(
    ctx: &mut DeriveContext<'_, '_>,
    self_def_id: DefId,
    data: &VariantData,
) -> ExprId {
    match data {
        VariantData::Struct { fields } => {
            let self_recv = self_expr(ctx, self_def_id);
            let field_exprs: Vec<_> = fields
                .iter()
                .map(|f| {
                    let access = access_field(
                        ctx,
                        self_recv,
                        &FieldView {
                            ident: Some(f.ident),
                            index: 0,
                            ty: f.ty,
                        },
                    );
                    (f.ident, method_call_expr(ctx, access, "clone", vec![]))
                })
                .collect();
            struct_literal(
                ctx,
                crate::res::Res::SelfTy {
                    def_id: self_def_id,
                },
                field_exprs,
            )
        }
        VariantData::Tuple { fields } => {
            let self_recv = self_expr(ctx, self_def_id);
            let cloned_fields: Vec<_> = fields
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let access = tuple_field_expr(ctx, self_recv, i);
                    method_call_expr(ctx, access, "clone", vec![])
                })
                .collect();
            // Tuple struct literal: `Self(a, b)`.
            let func = expr(
                ctx,
                Expr::Path {
                    res: crate::res::Res::SelfTy {
                        def_id: self_def_id,
                    },
                },
                ctx.derive_span,
            );
            expr(
                ctx,
                Expr::Call {
                    func,
                    args: cloned_fields,
                },
                ctx.derive_span,
            )
        }
        VariantData::Unit => {
            // `Self` resolves to the unit struct value.
            expr(
                ctx,
                Expr::Path {
                    res: crate::res::Res::SelfTy {
                        def_id: self_def_id,
                    },
                },
                ctx.derive_span,
            )
        }
    }
}

fn clone_enum_expr(
    ctx: &mut DeriveContext<'_, '_>,
    self_def_id: DefId,
    enum_def_id: DefId,
    def: &crate::hir::core::EnumDef,
) -> ExprId {
    let scrutinee = self_expr(ctx, self_def_id);
    let arms: Vec<Arm> = def
        .variants
        .iter()
        .map(|variant| {
            let variant_def_id = ctx
                .variant_def_id(enum_def_id, variant.ident.symbol)
                .unwrap_or(enum_def_id);
            let (pat, bindings) = variant_binding_pattern(ctx, variant_def_id, &variant.data);
            let body = clone_variant_expr(ctx, variant_def_id, &variant.data, &bindings);
            arm(ctx, pat, body)
        })
        .collect();
    match_expr(ctx, scrutinee, arms)
}

/// Information about a binding introduced by a variant pattern.
struct FieldBinding {
    /// Symbol used for the local binding.
    local: Symbol,
    /// Original field identifier (for struct variants), or `None` for tuple fields.
    field_ident: Option<yelang_ast::Ident>,
}

/// Build a pattern that destructures a variant and returns binding info.
fn variant_binding_pattern(
    ctx: &mut DeriveContext<'_, '_>,
    variant_def_id: DefId,
    data: &VariantData,
) -> (PatId, Vec<FieldBinding>) {
    match data {
        VariantData::Unit => (
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
            let pats: Vec<_> = fields
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let name = ctx.intern(&format!("__f{i}"));
                    bindings.push(FieldBinding {
                        local: name,
                        field_ident: None,
                    });
                    binding_pat(ctx, name)
                })
                .collect();
            (
                tuple_struct_pat(
                    ctx,
                    crate::res::Res::Def {
                        def_id: variant_def_id,
                    },
                    pats,
                ),
                bindings,
            )
        }
        VariantData::Struct { fields } => {
            let mut bindings = Vec::new();
            let field_pats: Vec<_> = fields
                .iter()
                .map(|f| {
                    let name =
                        ctx.intern(&format!("__{}", ctx.ctx.interner.resolve(&f.ident.symbol)));
                    bindings.push(FieldBinding {
                        local: name,
                        field_ident: Some(f.ident),
                    });
                    (f.ident, binding_pat(ctx, name))
                })
                .collect();
            (
                struct_pat(
                    ctx,
                    crate::res::Res::Def {
                        def_id: variant_def_id,
                    },
                    field_pats,
                ),
                bindings,
            )
        }
    }
}

/// Build the expression that reconstructs a cloned variant.
fn clone_variant_expr(
    ctx: &mut DeriveContext<'_, '_>,
    variant_def_id: DefId,
    data: &VariantData,
    bindings: &[FieldBinding],
) -> ExprId {
    match data {
        VariantData::Unit => expr(
            ctx,
            Expr::Path {
                res: crate::res::Res::Def {
                    def_id: variant_def_id,
                },
            },
            ctx.derive_span,
        ),
        VariantData::Tuple { .. } => {
            let cloned: Vec<_> = bindings.iter().map(|b| clone_local(ctx, b.local)).collect();
            enum_variant_literal(ctx, variant_def_id, cloned)
        }
        VariantData::Struct { .. } => {
            let field_exprs: Vec<_> = bindings
                .iter()
                .map(|b| {
                    let cloned = clone_local(ctx, b.local);
                    let field_ident = b.field_ident.expect("struct variant field");
                    (field_ident, cloned)
                })
                .collect();
            struct_literal(
                ctx,
                crate::res::Res::Def {
                    def_id: variant_def_id,
                },
                field_exprs,
            )
        }
    }
}

/// Build `local.clone()`.
fn clone_local(ctx: &mut DeriveContext<'_, '_>, name: Symbol) -> ExprId {
    let pat_id = ctx.ctx.local(name).expect("binding pat_id");
    let access = expr(
        ctx,
        Expr::Path {
            res: crate::res::Res::Local { pat_id },
        },
        ctx.derive_span,
    );
    method_call_expr(ctx, access, "clone", vec![])
}
