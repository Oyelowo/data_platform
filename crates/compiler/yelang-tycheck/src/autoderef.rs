/*! Shared autoderef logic for method and field dispatch.
 *
 * Implements the deref-chain part of rustc's method-lookup probe phase:
 * built-in derefs through references/raw pointers, plus user-defined `Deref`
 * normalization via the next-generation trait solver.
 */

use yelang_arena::DefId;
use yelang_trait_solver::canonicalize::canonicalize;
use yelang_trait_solver::eval_ctxt::EvalCtxt;
use yelang_trait_solver::goal::Goal;
use yelang_trait_solver::response::Certainty;
use yelang_ty::generic::GenericArg;
use yelang_ty::predicate::{NormalizesToPredicate, Predicate, TraitPredicate, TraitRef};
use yelang_ty::ty::{ImplPolarity, Mutability, ProjectionTy, Ty, TyId, TypeAndMut};

use crate::fn_ctxt::{FnCtxt, collect_body_infer_vars};

/// Maximum number of autoderef steps to try before giving up.
pub const AUTODEREF_LIMIT: usize = 10;

/// A single receiver adjustment discovered during probing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adjustment {
    /// Dereference once (`*`).
    Deref,
    /// Take an immutable reference (`&`).
    Ref,
    /// Take a mutable reference (`&mut`).
    RefMut,
    /// Dereference via a user-defined `Deref` impl.
    DerefTrait {
        /// Type before this deref step (the type that implements `Deref`).
        source: TyId,
        /// The normalized `<source as Deref>::Target`.
        target: TyId,
    },
}

/// Build the ordered deref chain for a receiver type.
///
/// The returned list contains the receiver type itself, then each successive
/// deref target.  Built-in derefs (references, raw pointers) and user-defined
/// `Deref` impls are both considered.  Autoref/automut are *not* added here;
/// method dispatch adds them on top of this chain.
pub fn probe_deref_steps(fcx: &mut FnCtxt<'_>, receiver_ty: TyId) -> Vec<(TyId, Vec<Adjustment>)> {
    let mut steps: Vec<(TyId, Vec<Adjustment>)> = vec![(receiver_ty, vec![])];
    let interner = fcx.tcx.interner();
    let mut seen: yelang_arena::FxHashSet<TyId> = yelang_arena::FxHashSet::default();
    seen.insert(receiver_ty);

    while steps.len() < AUTODEREF_LIMIT {
        let (current, adjs) = steps.last().unwrap().clone();
        match interner.ty(current) {
            Ty::Ref(inner, _) | Ty::RawPtr(TypeAndMut { ty: inner, .. }) => {
                if !seen.insert(inner) {
                    break;
                }
                let mut next_adjs = adjs;
                next_adjs.push(Adjustment::Deref);
                steps.push((inner, next_adjs));
            }
            _ => {
                if let Some(target) = try_deref_target(fcx, current) {
                    if !seen.insert(target) {
                        break;
                    }
                    let mut next_adjs = adjs;
                    next_adjs.push(Adjustment::DerefTrait {
                        source: current,
                        target,
                    });
                    steps.push((target, next_adjs));
                } else {
                    break;
                }
            }
        }
    }

    steps
}

/// Try to normalize `<source as Deref>::Target` using the next-gen solver.
///
/// This is purely speculative: all inference state is rolled back before
/// returning, so the caller only gets the resolved target type.  The consumer
/// (method or field dispatch) must re-prove the goal when it commits to a
/// particular deref step.
pub fn try_deref_target(fcx: &mut FnCtxt<'_>, source: TyId) -> Option<TyId> {
    let deref_trait = fcx.tcx.deref_trait?;
    let deref_target = fcx.tcx.deref_target?;

    let snapshot = fcx.infer.snapshot();
    let target = fcx.new_ty_var();

    let args = fcx
        .tcx
        .interner()
        .mk_generic_args(&[GenericArg::Type(source)]);
    let projection_ty = ProjectionTy {
        trait_ref: TraitRef {
            def_id: deref_trait,
            args,
        },
        item_def_id: deref_target,
    };
    let pred = Predicate::NormalizesTo(NormalizesToPredicate {
        projection_ty,
        term: target,
    });

    let goal = Goal::new(fcx.param_env, pred);
    let body_vars = collect_body_infer_vars(fcx.tcx.interner(), &mut fcx.infer, &pred);
    let mut ecx = EvalCtxt::new(fcx.tcx.interner(), fcx.tcx);
    let canonical_goal = canonicalize(goal, fcx.tcx.interner(), &mut fcx.infer, ecx.max_universe());

    let result = match ecx.evaluate_canonical_goal(canonical_goal) {
        Ok(response) if response.value.certainty == Certainty::Yes => {
            fcx.apply_response_to_body(&body_vars, &response);
            Some(fcx.resolve_ty(target))
        }
        _ => None,
    };

    fcx.infer.rollback_to(snapshot);
    result
}

/// Emit the obligations implied by a committed `DerefTrait` adjustment.
pub fn emit_deref_trait_obligations(fcx: &mut FnCtxt<'_>, source: TyId, target: TyId) {
    let interner = fcx.tcx.interner();
    let deref_trait = fcx.tcx.deref_trait.unwrap_or_else(|| DefId::new(0));
    let deref_target = fcx.tcx.deref_target.unwrap_or_else(|| DefId::new(0));

    let args = interner.mk_generic_args(&[GenericArg::Type(source)]);
    let trait_ref = TraitRef {
        def_id: deref_trait,
        args,
    };
    let projection_ty = ProjectionTy {
        trait_ref,
        item_def_id: deref_target,
    };
    fcx.emit_obligation(Predicate::NormalizesTo(NormalizesToPredicate {
        projection_ty,
        term: target,
    }));
    fcx.emit_obligation(Predicate::Trait(TraitPredicate {
        trait_ref: projection_ty.trait_ref,
        polarity: ImplPolarity::Positive,
    }));
}

/// Build the method-dispatch probe list: each deref step with autoref and
/// automut variants.
pub fn probe_types(fcx: &mut FnCtxt<'_>, receiver_ty: TyId) -> Vec<(TyId, Vec<Adjustment>)> {
    let steps = probe_deref_steps(fcx, receiver_ty);
    let mut probes = Vec::with_capacity(steps.len() * 3);
    for (ty, adjs) in steps {
        probes.push((ty, adjs.clone()));

        let mut ref_adjs = adjs.clone();
        ref_adjs.push(Adjustment::Ref);
        probes.push((fcx.mk_ref(ty, Mutability::Not), ref_adjs));

        let mut refmut_adjs = adjs;
        refmut_adjs.push(Adjustment::RefMut);
        probes.push((fcx.mk_ref(ty, Mutability::Mut), refmut_adjs));
    }
    probes
}
