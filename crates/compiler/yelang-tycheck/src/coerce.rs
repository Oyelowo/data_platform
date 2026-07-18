/*! Coercion logic.
 *
 * Handles implicit coercions: deref, subtyping (width for anon structs),
 * never type, function item to pointer, etc.
 */

use yelang_ty::generic::GenericArg;
use yelang_ty::subst::substitute;
use yelang_ty::ty::{Ty, TyId};

use crate::fn_ctxt::FnCtxt;

/// Trait for coercion operations.
pub trait Coerce {
    /// Attempt to coerce `from` to `to`.
    /// On success, returns the coerced type (usually `to`).
    fn coerce(&mut self, from: TyId, to: TyId) -> Result<TyId, ()>;
}

impl Coerce for FnCtxt<'_> {
    fn coerce(&mut self, from: TyId, to: TyId) -> Result<TyId, ()> {
        let interner = self.tcx.interner();

        // Exact match / unification.
        if self.eq(from, to).is_ok() {
            return Ok(to);
        }

        match (interner.ty(from), interner.ty(to)) {
            // The never type `!` coerces to any type.
            (Ty::Never, _) => Ok(to),

            // Function items coerce to matching function pointers.
            (Ty::FnDef(_), Ty::FnPtr(_)) => coerce_fn_item_to_ptr(self, from, to),

            // TODO: deref coercion, width subtyping for anonymous structs,
            // int/float fallback at coercion sites.
            _ => Err(()),
        }
    }
}

fn coerce_fn_item_to_ptr(fcx: &mut FnCtxt<'_>, from: TyId, to: TyId) -> Result<TyId, ()> {
    let interner = fcx.tcx.interner();

    let (Ty::FnDef(from_def), Ty::FnPtr(to_sig)) = (interner.ty(from), interner.ty(to)) else {
        return Err(());
    };

    let Some(from_poly_sig) = fcx.tcx.fn_sig(from_def.def_id) else {
        return Err(());
    };

    // Instantiate the function item's generic parameters with fresh inference
    // variables so the signatures can be unified.
    let subst = fcx.fresh_substitution_for_generics(from_def.def_id);
    let from_inputs = substitute(interner, from_poly_sig.sig.inputs, &subst);
    let from_output = substitute(interner, from_poly_sig.sig.output, &subst);
    let to_sig = to_sig.sig;

    if from_inputs.len() != to_sig.inputs.len() {
        return Err(());
    }

    for (from_arg, to_arg) in from_inputs.iter().zip(to_sig.inputs.iter()) {
        let (GenericArg::Type(from_ty), GenericArg::Type(to_ty)) = (from_arg, to_arg) else {
            return Err(());
        };
        if fcx.eq(*from_ty, *to_ty).is_err() {
            return Err(());
        }
    }

    if fcx.eq(from_output, to_sig.output).is_err() {
        return Err(());
    }

    Ok(to)
}
