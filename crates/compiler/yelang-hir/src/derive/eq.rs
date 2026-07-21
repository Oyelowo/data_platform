//! Built-in `Eq` derive.

use crate::derive::context::DeriveContext;
use crate::derive::error::DeriveError;
use crate::derive::helpers::{FieldView, derive_generics, impl_item, iter_fields};
use crate::hir::core::Item;
use crate::hir::ty::Ty;
use yelang_resolve::lang_items::LangItem;

/// Expand `#[derive(Eq)]` for a struct or enum.
///
/// `Eq` is a marker trait. The derive only checks that the type is also
/// deriving `PartialEq` and that no field is a floating-point type (which is
/// `PartialEq` but not `Eq`).
pub fn derive_eq(
    ctx: &mut DeriveContext<'_, '_>,
    derives_in_attr: &[yelang_interner::Symbol],
) -> Option<Item> {
    let adt = match ctx.adt_info() {
        Ok(adt) => adt,
        Err(err) => {
            ctx.error(err);
            return None;
        }
    };

    let eq_trait = match ctx.trait_def_id("Eq") {
        Ok(def_id) => def_id,
        Err(err) => {
            ctx.error(err);
            return None;
        }
    };

    // Require that `PartialEq` is also requested in this derive attribute.
    let partial_eq_sym = ctx.intern("PartialEq");
    if !derives_in_attr.contains(&partial_eq_sym) {
        ctx.error(DeriveError::InvalidShape {
            derive: ctx.derive_name,
            reason: format!(
                "cannot derive `Eq` for `{}` without also deriving `PartialEq`",
                ctx.ctx.interner.resolve(&adt.ident.symbol)
            ),
            span: ctx.derive_span,
        });
        return None;
    }

    // Reject floating-point fields at the derive site so users get a clear
    // diagnostic rather than a confusing trait-resolution failure.
    if let Some(field_name) = find_float_field(ctx) {
        ctx.error(DeriveError::InvalidShape {
            derive: ctx.derive_name,
            reason: format!(
                "cannot derive `Eq` for `{}` because field `{}` has floating-point type",
                ctx.ctx.interner.resolve(&adt.ident.symbol),
                ctx.ctx.interner.resolve(&field_name)
            ),
            span: ctx.derive_span,
        });
        return None;
    }

    let self_ty = adt.self_ty(ctx);
    let generics = derive_generics(ctx, &adt.generics, eq_trait);
    Some(impl_item(ctx, eq_trait, self_ty, generics, vec![], vec![]))
}

fn find_float_field(ctx: &DeriveContext<'_, '_>) -> Option<yelang_interner::Symbol> {
    let adt = ctx.adt_info().ok()?;
    match &adt.shape {
        crate::derive::context::AdtShape::Struct(data) => {
            find_float_in_fields(&iter_fields(data), ctx)
        }
        crate::derive::context::AdtShape::Enum(def) => {
            for variant in &def.variants {
                if let Some(name) = find_float_in_fields(&iter_fields(&variant.data), ctx) {
                    return Some(name);
                }
            }
            None
        }
    }
}

fn find_float_in_fields(
    fields: &[FieldView],
    ctx: &DeriveContext<'_, '_>,
) -> Option<yelang_interner::Symbol> {
    for field in fields {
        if is_float(field.ty, ctx) {
            return field
                .ident
                .map(|i| i.symbol)
                .or_else(|| Some(ctx.intern(&format!("{}", field.index))));
        }
    }
    None
}

fn is_float(
    ty_id: crate::ids::HirTyId,
    ctx: &crate::derive::context::DeriveContext<'_, '_>,
) -> bool {
    let ty = ctx.ctx.crate_hir.ty(ty_id).expect("field type");
    if let Ty::Path {
        res: crate::res::Res::PrimTy {
            ty: crate::res::PrimTy::Float(_),
        },
        ..
    } = ty
    {
        return true;
    }
    // Primitives may also be resolved to prelude/type-alias definitions with
    // a float lang item.
    if let Ty::Path {
        res: crate::res::Res::Def { def_id },
        ..
    } = ty
    {
        if let Some(def) = ctx.ctx.resolved.definitions.get(*def_id) {
            if let Some(li) = def.lang_item {
                return matches!(li, LangItem::F32 | LangItem::F64);
            }
        }
    }
    false
}
