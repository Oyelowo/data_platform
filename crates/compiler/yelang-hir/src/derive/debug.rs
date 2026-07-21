//! Built-in `Debug` derive.
//!
//! Generates a minimal but correct `fmt` method that writes a structural
//! representation of the value to the formatter. The output mirrors Rust's
//! derived `Debug` for structs and enums:
//!
//! - Named struct: `Point { x: ..., y: ... }`
//! - Tuple struct: `Tuple(..., ...)`
//! - Unit struct: `Unit`
//! - Enum: `VariantName(...)` or `VariantName { ... }`

use yelang_arena::DefId;
use yelang_interner::Symbol;

use crate::derive::context::{AdtInfo, AdtShape, DeriveContext};
use crate::derive::error::DeriveError;
use crate::derive::helpers::{
    FieldView, access_field, arm, binding_pat, derive_generics, enum_variant_literal, expr, fn_sig,
    formatter_param, impl_item, make_body, match_expr, method_call_expr, method_impl_item,
    path_pat, self_expr, self_param, string_expr, struct_pat, tuple_field_expr, tuple_struct_pat,
};
use crate::hir::adt::VariantData;
use crate::hir::core::{Arm, Expr, ImplItem, Item};
use crate::ids::{ExprId, HirTyId, PatId};

/// Expand `#[derive(Debug)]` for a struct or enum.
pub fn derive_debug(ctx: &mut DeriveContext<'_, '_>, _derives_in_attr: &[Symbol]) -> Option<Item> {
    let adt = match ctx.adt_info() {
        Ok(adt) => adt,
        Err(err) => {
            ctx.error(err);
            return None;
        }
    };

    let debug_trait = match ctx.trait_def_id("Debug") {
        Ok(def_id) => def_id,
        Err(err) => {
            ctx.error(err);
            return None;
        }
    };

    let formatter_def_id = match look_up_formatter(ctx) {
        Some(id) => id,
        None => {
            ctx.error(DeriveError::MissingLangItem {
                derive: ctx.derive_name,
                item_name: "Formatter",
                span: ctx.derive_span,
            });
            return None;
        }
    };

    let result_def_id = match look_up_result(ctx) {
        Some(id) => id,
        None => {
            ctx.error(DeriveError::MissingLangItem {
                derive: ctx.derive_name,
                item_name: "Result",
                span: ctx.derive_span,
            });
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
    let formatter_ty = crate::derive::helpers::path_ty(ctx, formatter_def_id);
    let ref_formatter_ty = ctx.ctx.crate_hir.alloc_ty(
        crate::hir::ty::Ty::Ref {
            mutability: yelang_ast::Mutability::Mutable,
            ty: formatter_ty,
        },
        ctx.derive_span,
    );
    let result_ty = crate::derive::helpers::path_ty(ctx, result_def_id);

    let generics = derive_generics(ctx, &adt.generics, debug_trait);
    let fmt_method = fmt_method(
        ctx,
        adt.def_id,
        &adt,
        ref_self_ty,
        ref_formatter_ty,
        result_ty,
        result_def_id,
        formatter_def_id,
        generics.clone(),
    );

    let debug_self_ty = adt.self_ty(ctx);
    Some(impl_item(
        ctx,
        debug_trait,
        debug_self_ty,
        generics,
        vec![],
        vec![fmt_method],
    ))
}

fn look_up_formatter(ctx: &DeriveContext<'_, '_>) -> Option<DefId> {
    // Formatter is a struct, not a trait, so look in the type namespace.
    ctx.resolve_in_module_or_prelude(yelang_resolve::Namespace::Type, ctx.intern("Formatter"))
}

fn look_up_result(ctx: &DeriveContext<'_, '_>) -> Option<DefId> {
    ctx.resolve_in_module_or_prelude(yelang_resolve::Namespace::Type, ctx.intern("Result"))
}

fn fmt_method(
    ctx: &mut DeriveContext<'_, '_>,
    self_def_id: DefId,
    adt: &AdtInfo<'_>,
    ref_self_ty: HirTyId,
    ref_formatter_ty: HirTyId,
    result_ty: HirTyId,
    result_def_id: DefId,
    formatter_def_id: DefId,
    generics: crate::hir::core::Generics,
) -> ImplItem {
    let self_param = self_param(ctx, ref_self_ty);
    let formatter_param = formatter_param(ctx, formatter_def_id);
    let sig = fn_sig(vec![self_param.ty, ref_formatter_ty], result_ty);

    let formatter_local = ctx.intern("f");
    let body_value = match &adt.shape {
        AdtShape::Struct(data) => {
            let name = ctx.ctx.interner.resolve(&adt.ident.symbol);
            debug_struct_like(ctx, self_def_id, data, name, formatter_local, result_def_id)
        }
        AdtShape::Enum(def) => debug_enum(
            ctx,
            self_def_id,
            adt.def_id,
            def,
            formatter_local,
            result_def_id,
        ),
    };

    let body_id = make_body(ctx, vec![self_param, formatter_param], body_value);
    method_impl_item(ctx, "fmt", sig, generics, vec![], body_id)
}

/// Build `Result::Ok(())`.
fn ok_unit_expr(ctx: &mut DeriveContext<'_, '_>, result_def_id: DefId) -> ExprId {
    let ok_variant = ctx
        .variant_def_id(result_def_id, ctx.intern("Ok"))
        .unwrap_or(result_def_id);
    enum_variant_literal(ctx, ok_variant, vec![])
}

/// Build `f.write_str(literal)` as a statement.
fn write_str_stmt(
    ctx: &mut DeriveContext<'_, '_>,
    formatter_local: Symbol,
    s: &str,
) -> crate::ids::StmtId {
    let formatter_pat_id = ctx.ctx.local(formatter_local).expect("formatter local");
    let formatter = expr(
        ctx,
        Expr::Path {
            res: crate::res::Res::Local {
                pat_id: formatter_pat_id,
            },
        },
        ctx.derive_span,
    );
    let literal = string_expr(ctx, s);
    let call = method_call_expr(ctx, formatter, "write_str", vec![literal]);
    let pat = crate::derive::helpers::wild_pat(ctx);
    crate::derive::helpers::let_stmt(ctx, pat, None, Some(call))
}

/// Build `field.fmt(f)` as a statement.
fn fmt_field_stmt(
    ctx: &mut DeriveContext<'_, '_>,
    field_expr: ExprId,
    formatter_local: Symbol,
) -> crate::ids::StmtId {
    let formatter_pat_id = ctx.ctx.local(formatter_local).expect("formatter local");
    let formatter = expr(
        ctx,
        Expr::Path {
            res: crate::res::Res::Local {
                pat_id: formatter_pat_id,
            },
        },
        ctx.derive_span,
    );
    let call = method_call_expr(ctx, field_expr, "fmt", vec![formatter]);
    let pat = crate::derive::helpers::wild_pat(ctx);
    crate::derive::helpers::let_stmt(ctx, pat, None, Some(call))
}

fn debug_struct_like(
    ctx: &mut DeriveContext<'_, '_>,
    self_def_id: DefId,
    data: &VariantData,
    name: &str,
    formatter_local: Symbol,
    result_def_id: DefId,
) -> ExprId {
    match data {
        VariantData::Unit => {
            let tail = ok_unit_expr(ctx, result_def_id);
            let call = write_str_stmt(ctx, formatter_local, name);
            expr(
                ctx,
                Expr::Block {
                    block: crate::hir::core::Block {
                        stmts: vec![call],
                        expr: Some(tail),
                        span: ctx.derive_span,
                    },
                },
                ctx.derive_span,
            )
        }
        VariantData::Tuple { fields } => {
            let self_recv = self_expr(ctx, self_def_id);
            let tail = ok_unit_expr(ctx, result_def_id);
            let mut stmts = vec![write_str_stmt(ctx, formatter_local, &format!("{name}("))];
            for (i, _) in fields.iter().enumerate() {
                if i > 0 {
                    stmts.push(write_str_stmt(ctx, formatter_local, ", "));
                }
                let access = tuple_field_expr(ctx, self_recv, i);
                stmts.push(fmt_field_stmt(ctx, access, formatter_local));
            }
            stmts.push(write_str_stmt(ctx, formatter_local, ")"));
            block_expr_with_tail(ctx, stmts, tail)
        }
        VariantData::Struct { fields } => {
            let self_recv = self_expr(ctx, self_def_id);
            let tail = ok_unit_expr(ctx, result_def_id);
            let mut stmts = vec![write_str_stmt(ctx, formatter_local, &format!("{name} {{ "))];
            for (i, f) in fields.iter().enumerate() {
                if i > 0 {
                    stmts.push(write_str_stmt(ctx, formatter_local, ", "));
                }
                let field_name = ctx.ctx.interner.resolve(&f.ident.symbol);
                stmts.push(write_str_stmt(
                    ctx,
                    formatter_local,
                    &format!("{field_name}: "),
                ));
                let access = access_field(
                    ctx,
                    self_recv,
                    &FieldView {
                        ident: Some(f.ident),
                        index: 0,
                        ty: f.ty,
                    },
                );
                stmts.push(fmt_field_stmt(ctx, access, formatter_local));
            }
            stmts.push(write_str_stmt(ctx, formatter_local, " }"));
            block_expr_with_tail(ctx, stmts, tail)
        }
    }
}

fn debug_enum(
    ctx: &mut DeriveContext<'_, '_>,
    self_def_id: DefId,
    enum_def_id: DefId,
    def: &crate::hir::core::EnumDef,
    formatter_local: Symbol,
    result_def_id: DefId,
) -> ExprId {
    let scrutinee = self_expr(ctx, self_def_id);
    let arms: Vec<Arm> = def
        .variants
        .iter()
        .map(|variant| {
            let variant_def_id = ctx
                .variant_def_id(enum_def_id, variant.ident.symbol)
                .unwrap_or(enum_def_id);
            let variant_name = ctx.ctx.interner.resolve(&variant.ident.symbol);
            let (pat, bindings) = variant_binding_pattern(ctx, variant_def_id, &variant.data);
            let body = debug_variant(
                ctx,
                variant_def_id,
                &variant.data,
                variant_name,
                &bindings,
                formatter_local,
                result_def_id,
            );
            arm(ctx, pat, body)
        })
        .collect();
    match_expr(ctx, scrutinee, arms)
}

struct FieldBinding {
    local: Symbol,
    field_ident: Option<yelang_ast::Ident>,
}

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

fn debug_variant(
    ctx: &mut DeriveContext<'_, '_>,
    _variant_def_id: DefId,
    data: &VariantData,
    variant_name: &str,
    bindings: &[FieldBinding],
    formatter_local: Symbol,
    result_def_id: DefId,
) -> ExprId {
    match data {
        VariantData::Unit => {
            let tail = ok_unit_expr(ctx, result_def_id);
            let call = write_str_stmt(ctx, formatter_local, variant_name);
            expr(
                ctx,
                Expr::Block {
                    block: crate::hir::core::Block {
                        stmts: vec![call],
                        expr: Some(tail),
                        span: ctx.derive_span,
                    },
                },
                ctx.derive_span,
            )
        }
        VariantData::Tuple { .. } => {
            let tail = ok_unit_expr(ctx, result_def_id);
            let mut stmts = vec![write_str_stmt(
                ctx,
                formatter_local,
                &format!("{variant_name}("),
            )];
            for (i, b) in bindings.iter().enumerate() {
                if i > 0 {
                    stmts.push(write_str_stmt(ctx, formatter_local, ", "));
                }
                let local = local_expr(ctx, b.local);
                stmts.push(fmt_field_stmt(ctx, local, formatter_local));
            }
            stmts.push(write_str_stmt(ctx, formatter_local, ")"));
            block_expr_with_tail(ctx, stmts, tail)
        }
        VariantData::Struct { .. } => {
            let tail = ok_unit_expr(ctx, result_def_id);
            let mut stmts = vec![write_str_stmt(
                ctx,
                formatter_local,
                &format!("{variant_name} {{ "),
            )];
            for (i, b) in bindings.iter().enumerate() {
                if i > 0 {
                    stmts.push(write_str_stmt(ctx, formatter_local, ", "));
                }
                let field_ident = b.field_ident.expect("struct variant field");
                let field_name = ctx.ctx.interner.resolve(&field_ident.symbol);
                stmts.push(write_str_stmt(
                    ctx,
                    formatter_local,
                    &format!("{field_name}: "),
                ));
                let local = local_expr(ctx, b.local);
                stmts.push(fmt_field_stmt(ctx, local, formatter_local));
            }
            stmts.push(write_str_stmt(ctx, formatter_local, " }"));
            block_expr_with_tail(ctx, stmts, tail)
        }
    }
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

fn block_expr_with_tail(
    ctx: &mut DeriveContext<'_, '_>,
    stmts: Vec<crate::ids::StmtId>,
    tail: ExprId,
) -> ExprId {
    expr(
        ctx,
        Expr::Block {
            block: crate::hir::core::Block {
                stmts,
                expr: Some(tail),
                span: ctx.derive_span,
            },
        },
        ctx.derive_span,
    )
}
