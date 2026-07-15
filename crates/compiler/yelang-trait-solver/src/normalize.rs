/*! Associated type normalization.
 *
 * Normalization resolves `<T as Trait>::Assoc` to its concrete type.
 */

use yelang_ty::predicate::ProjectionPredicate;
use yelang_ty::ty::Ty;

/// Try to normalize a projection type.
pub fn normalize_projection<'tcx>(_predicate: ProjectionPredicate<'tcx>) -> Option<Ty<'tcx>> {
    // TODO: implement normalization by looking up the defining impl
    // and substituting the associated type.
    None
}
