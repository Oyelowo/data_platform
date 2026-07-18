//! Built-in `Copy` derive.

use yelang_interner::Symbol;

use crate::derive::context::DeriveContext;
use crate::derive::error::DeriveError;
use crate::derive::helpers::{derive_generics, FieldView, impl_item, iter_fields};
use crate::hir::core::Item;
use crate::hir::ty::Ty;

/// Expand `#[derive(Copy)]` for a struct or enum.
pub fn derive_copy(
    ctx: &mut DeriveContext<'_, '_>,
    _derives_in_attr: &[yelang_interner::Symbol],
) -> Option<Item> {
    let adt = match ctx.adt_info() {
        Ok(adt) => adt,
        Err(err) => {
            ctx.error(err);
            return None;
        }
    };

    let copy_trait = match ctx.trait_def_id("Copy") {
        Ok(def_id) => def_id,
        Err(err) => {
            ctx.error(err);
            return None;
        }
    };

    // Reject types that contain fields we know are not `Copy`. For fields whose
    // copy-ness cannot be determined from HIR alone (e.g., type parameters or
    // unresolved paths), we leave the check to type checking.
    if let Some(field_name) = find_non_copy_field(ctx) {
        ctx.error(DeriveError::InvalidShape {
            derive: ctx.derive_name,
            reason: format!(
                "cannot derive `Copy` for type `{}` because field `{}` is not `Copy`",
                ctx.ctx.interner.resolve(&adt.ident.symbol),
                ctx.ctx.interner.resolve(&field_name)
            ),
            span: ctx.derive_span,
        });
        return None;
    }

    let self_ty = adt.self_ty(ctx);
    let generics = derive_generics(ctx, adt.generics, copy_trait);
    Some(impl_item(ctx, copy_trait, self_ty, generics, vec![]))
}

/// Returns the name of the first field whose type is definitely not `Copy`.
/// Returns `None` if no such field is found (including when types are unknown).
fn find_non_copy_field(ctx: &DeriveContext<'_, '_>) -> Option<Symbol> {
    let adt = ctx.adt_info().ok()?;
    match &adt.shape {
        crate::derive::context::AdtShape::Struct(data) => {
            find_non_copy_in_fields(&iter_fields(data), ctx)
        }
        crate::derive::context::AdtShape::Enum(def) => {
            for variant in &def.variants {
                if let Some(name) = find_non_copy_in_fields(&iter_fields(&variant.data), ctx) {
                    return Some(name);
                }
            }
            None
        }
    }
}

fn find_non_copy_in_fields(
    fields: &[FieldView],
    ctx: &DeriveContext<'_, '_>,
) -> Option<Symbol> {
    for field in fields {
        if is_known_non_copy(field.ty, ctx) {
            return field.ident.map(|i| i.symbol).or_else(|| {
                // Tuple fields have no identifier; synthesize one from the index.
                Some(ctx.intern(&format!("{}", field.index)))
            });
        }
    }
    None
}

/// Types that are known not to be `Copy` in Yelang's prelude.
///
/// This list is conservative: if a type is not in this list, the derive emits
/// the impl and lets type checking verify the bound.
fn is_known_non_copy(ty_id: crate::ids::TyId, ctx: &DeriveContext<'_, '_>) -> bool {
    let ty = ctx.ctx.crate_hir.tys.get(ty_id).expect("field type");
    matches!(
        ty,
        Ty::Path {
            res: crate::res::Res::Def { def_id },
            ..
        } if is_string_or_vec(ctx, *def_id)
    )
}

fn is_string_or_vec(ctx: &DeriveContext<'_, '_>, def_id: crate::ids::DefId) -> bool {
    let string_sym = ctx.intern("String");
    let vec_sym = ctx.intern("Vec");
    let string_id = ctx.resolve_in_module_or_prelude(yelang_resolve::Namespace::Type, string_sym);
    let vec_id = ctx.resolve_in_module_or_prelude(yelang_resolve::Namespace::Type, vec_sym);
    Some(def_id) == string_id || Some(def_id) == vec_id
}
