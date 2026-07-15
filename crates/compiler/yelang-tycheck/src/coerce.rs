/*! Coercion logic.
 *
 * Handles implicit coercions: deref, subtyping (width for anon structs),
 * never type, function item to pointer, etc.
 */

use yelang_ty::ty::Ty;

use crate::fn_ctxt::FnCtxt;

/// Trait for coercion operations.
pub trait Coerce<'tcx> {
    /// Attempt to coerce `from` to `to`.
    /// On success, returns the coerced type (usually `to`).
    fn coerce(&mut self, from: Ty<'tcx>, to: Ty<'tcx>) -> Result<Ty<'tcx>, ()>;
}

impl<'tcx> Coerce<'tcx> for FnCtxt<'tcx> {
    fn coerce(&mut self, from: Ty<'tcx>, to: Ty<'tcx>) -> Result<Ty<'tcx>, ()> {
        // For now, coercion is just exact unification.
        // TODO: implement deref coercion, never-type coercion, fn-item-to-fn-ptr,
        // width subtyping for anon structs, int/float fallback.
        self.eq(from, to).map_err(|_| ())?;
        Ok(to)
    }
}
