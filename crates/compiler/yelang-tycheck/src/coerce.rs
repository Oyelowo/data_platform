/*! Coercion logic.
 *
 * Handles implicit coercions: deref, subtyping (width for anon structs),
 * never type, function item to pointer, etc.
 */

use yelang_ty::generic::GenericArg;
use yelang_ty::subst::substitute;
use yelang_ty::ty::{InferTy, Ty, TyId};

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

            // Deref coercion: `&T` -> `&U` when `T` can be dereffed to `U`.
            (Ty::Ref(_, _), Ty::Ref(_, _)) => coerce_ref_deref(self, from, to),

            // Width subtyping for anonymous structs: `{x, y}` -> `{x}`.
            (Ty::AnonStruct(_), Ty::AnonStruct(_)) => coerce_anon_struct(self, from, to),

            _ => {
                // Integer/float fallback at coercion sites: if one side is an
                // inference variable and the other is concrete, try to assign it.
                if let Ok(ty) = coerce_infer_fallback(self, from, to) {
                    Ok(ty)
                } else {
                    Err(())
                }
            }
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

/// Deref coercion between reference types (`&T` -> `&U`).
///
/// Uses the same autoderef probe as method dispatch, then commits any
/// user-defined `Deref` steps as obligations.
fn coerce_ref_deref(fcx: &mut FnCtxt<'_>, from: TyId, to: TyId) -> Result<TyId, ()> {
    let probes = crate::autoderef::probe_types(fcx, from);

    for (probe_ty, adjustments) in probes {
        if probe_ty == to {
            // Commit the deref obligations for any user-defined steps.
            for adj in &adjustments {
                if let crate::autoderef::Adjustment::DerefTrait { source, target } = adj {
                    crate::autoderef::emit_deref_trait_obligations(fcx, *source, *target);
                }
            }
            return Ok(to);
        }
    }

    Err(())
}

/// Width subtyping for anonymous structs: `{ a: A, b: B }` coerces to `{ a: A }`.
fn coerce_anon_struct(fcx: &mut FnCtxt<'_>, from: TyId, to: TyId) -> Result<TyId, ()> {
    let interner = fcx.tcx.interner();
    let (Ty::AnonStruct(from_def), Ty::AnonStruct(to_def)) = (interner.ty(from), interner.ty(to))
    else {
        return Err(());
    };

    for to_field in to_def.fields.iter() {
        let matching = from_def
            .fields
            .iter()
            .find(|f| f.name == to_field.name)
            .ok_or(())?;
        if fcx.eq(matching.ty, to_field.ty).is_err() {
            return Err(());
        }
    }

    Ok(to)
}

/// Try to resolve an integer/float inference variable against a concrete type.
fn coerce_infer_fallback(fcx: &mut FnCtxt<'_>, from: TyId, to: TyId) -> Result<TyId, ()> {
    let interner = fcx.tcx.interner();

    // `?I` -> `i32`/`i64`/... via unification.
    if let Ty::Infer(InferTy::IntVar(_)) = interner.ty(from) {
        if is_integral_ty(interner, to) && fcx.eq(from, to).is_ok() {
            return Ok(to);
        }
    }

    // `?F` -> `f32`/`f64` via unification.
    if let Ty::Infer(InferTy::FloatVar(_)) = interner.ty(from) {
        if is_floating_ty(interner, to) && fcx.eq(from, to).is_ok() {
            return Ok(to);
        }
    }

    // And the reverse direction: concrete numeric -> inference variable.
    if let Ty::Infer(InferTy::IntVar(_)) = interner.ty(to) {
        if is_integral_ty(interner, from) && fcx.eq(from, to).is_ok() {
            return Ok(to);
        }
    }
    if let Ty::Infer(InferTy::FloatVar(_)) = interner.ty(to) {
        if is_floating_ty(interner, from) && fcx.eq(from, to).is_ok() {
            return Ok(to);
        }
    }

    Err(())
}

fn is_integral_ty(interner: &yelang_ty::interner::Interner, ty: TyId) -> bool {
    matches!(interner.ty(ty), Ty::Int(_) | Ty::Uint(_))
}

fn is_floating_ty(interner: &yelang_ty::interner::Interner, ty: TyId) -> bool {
    matches!(interner.ty(ty), Ty::Float(_))
}
