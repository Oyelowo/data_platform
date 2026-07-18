/*! Associated type normalization.
 *
 * Normalization resolves `<T as Trait>::Assoc` to its concrete type by looking
 * up the selected impl's associated type and applying the impl substitution.
 */

use yelang_arena::DefId;
use yelang_ty::generic::Substitution;
use yelang_ty::interner::Interner;
use yelang_ty::predicate::{NormalizesToPredicate, ProjectionPredicate};
use yelang_ty::subst::substitute;
use yelang_ty::ty::{ProjectionTy, TyId};

use crate::solver_ctx::{AssocItemKind, SolverCtxt};

/// Try to extract the concrete associated type from a selected impl.
///
/// `trait_item_def_id` is the def id of the associated type on the trait
/// definition. The impl is searched for an associated type item that
/// corresponds to that trait item (either by `trait_item_def_id` or by name),
/// and its `default` (the concrete type written in the impl) is returned with
/// the impl substitution applied.
pub fn assoc_type_from_impl<C: SolverCtxt>(
    tcx: &C,
    interner: &Interner,
    projection_ty: ProjectionTy,
    impl_def_id: DefId,
    impl_subst: &Substitution,
) -> Option<TyId> {
    let trait_items = tcx.trait_assoc_items(projection_ty.trait_ref.def_id);
    let trait_assoc = trait_items
        .iter()
        .find(|item| item.def_id == projection_ty.item_def_id)?;

    let impl_items = tcx.impl_assoc_items(impl_def_id);
    let impl_assoc = impl_items.iter().find(|item| {
        item.trait_item_def_id
            .map(|tid| tid == projection_ty.item_def_id)
            .unwrap_or_else(|| item.ident == trait_assoc.ident)
    })?;

    match &impl_assoc.kind {
        AssocItemKind::Type {
            default: Some(ty), ..
        } => Some(substitute(interner, *ty, impl_subst)),
        _ => None,
    }
}

/// Convenience: unwrap a `ProjectionPredicate` into its `ProjectionTy`.
pub fn projection_predicate_to_ty(pred: ProjectionPredicate) -> ProjectionTy {
    pred.projection_ty
}

/// Convenience: unwrap a `NormalizesToPredicate` into its `ProjectionTy`.
pub fn normalizes_to_predicate_to_ty(pred: NormalizesToPredicate) -> ProjectionTy {
    pred.projection_ty
}
