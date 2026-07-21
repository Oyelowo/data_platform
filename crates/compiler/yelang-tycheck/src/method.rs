/*! Method lookup and resolution.
 *
 * Implements the rustc-style probe/confirm model:
 *
 * 1. **Probe** — build a sequence of receiver types by repeatedly applying
 *    built-in derefs (references, raw pointers) and then considering autoref
 *    and automut at each step. Search the deref chain for an applicable
 *    method, preferring inherent candidates over trait (extension) candidates
 *    and preferring earlier deref steps.
 * 2. **Confirm** — commit the chosen impl substitution, receiver adjustment,
 *    and argument unifications, and emit any impl/trait where-clause
 *    obligations.
 *
 * See the rustc dev guide on [method lookup] for the algorithm this is based
 * on.
 *
 * [method lookup]: https://rustc-dev-guide.rust-lang.org/hir-typeck/method-lookup.html
 */

use yelang_arena::DefId;
use yelang_hir::ids::{BodyId, ExprId};
use yelang_interner::Symbol;

use yelang_ty::generic::{GenericArg, Substitution};
use yelang_ty::predicate::{Predicate, TraitPredicate, TraitRef};
use yelang_ty::subst::substitute;
use yelang_ty::ty::{ImplPolarity, PolyFnSig, Ty, TyId};
use yelang_trait_solver::eval_ctxt::EvalCtxt;
use yelang_trait_solver::goal::Goal;
use yelang_trait_solver::response::Certainty;

use crate::autoderef::{Adjustment, probe_types};
use crate::check::check_expr;
use crate::coerce::Coerce;
use crate::fn_ctxt::{FnCtxt, collect_body_infer_vars};
use crate::tcx::{ImplDefId, ImplItemDefData, TraitItemDefData};
use crate::typeck_results::MethodResolution;

/// Where a method candidate came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSource {
    /// An inherent method defined in an `impl SelfTy` block.
    Inherent {
        impl_id: ImplDefId,
        item_def_id: DefId,
    },
    /// A trait (extension) method.
    Trait {
        trait_def_id: DefId,
        item_def_id: DefId,
        /// The trait ref to prove, including the receiver type as `Self`.
        trait_ref: TraitRef,
    },
}

/// A method that plausibly matches the receiver and name.
#[derive(Debug, Clone)]
pub struct MethodCandidate {
    pub source: CandidateSource,
    /// Fresh substitution for the impl's generic parameters. The substitution
    /// values are inference variables owned by the body `InferCtxt`.
    pub impl_subst: Substitution,
    /// The method signature before applying `impl_subst`.
    pub raw_sig: PolyFnSig,
}

/// The result of a successful probe: a candidate plus the adjustments needed
/// to make the receiver match the method's `self` parameter.
#[derive(Debug, Clone)]
pub struct MethodPick {
    pub candidate: MethodCandidate,
    pub receiver_adjustments: Vec<Adjustment>,
    pub probe_ty: TyId,
}

/// Type-check a method call expression `receiver.method(args...)` and return
/// the inferred result type.
pub fn check_method_call(
    fcx: &mut FnCtxt<'_>,
    expr_id: ExprId,
    receiver: ExprId,
    method: Symbol,
    args: &[ExprId],
) -> TyId {
    let receiver_ty = check_expr(fcx, receiver);
    let probes = probe_types(fcx, receiver_ty);

    // At each deref step, inherent candidates take priority over trait
    // candidates. Earlier deref steps take priority over later ones.
    for (probe_ty, adjustments) in &probes {
        if let Some(candidate) = pick_inherent_candidate(fcx, *probe_ty, method) {
            return confirm_and_record(
                fcx,
                expr_id,
                receiver,
                &MethodPick {
                    candidate,
                    receiver_adjustments: adjustments.clone(),
                    probe_ty: *probe_ty,
                },
                args,
            );
        }
        if let Some(candidate) = pick_trait_candidate(fcx, *probe_ty, adjustments, method) {
            return confirm_and_record(
                fcx,
                expr_id,
                receiver,
                &MethodPick {
                    candidate,
                    receiver_adjustments: adjustments.clone(),
                    probe_ty: *probe_ty,
                },
                args,
            );
        }
    }

    let span = crate::check::expr_span(fcx, receiver);
    fcx.report_type_error(
        span,
        yelang_infer::error::TypeError::NoSuchMethod {
            ty: receiver_ty,
            method,
        },
    );
    fcx.mk_error()
}

// ---------------------------------------------------------------------------
// Candidate assembly: inherent methods
// ---------------------------------------------------------------------------

fn pick_inherent_candidate(
    fcx: &mut FnCtxt<'_>,
    probe_ty: TyId,
    method: Symbol,
) -> Option<MethodCandidate> {
    let interner = fcx.tcx.interner();

    for imp in fcx.tcx.impl_defs.iter() {
        // Only inherent impls participate in this phase.
        if imp.trait_ref.is_some() {
            continue;
        }

        for item in &imp.items {
            let ImplItemDefData::Fn { def_id, ident, sig } = item else {
                continue;
            };
            if ident.symbol != method {
                continue;
            }
            if !is_method_sig(interner, *sig, imp.self_ty) {
                continue;
            }

            let impl_subst = fcx.fresh_substitution_for_generics(imp.def_id);
            let substituted_sig = substitute_fn_sig(interner, *sig, &impl_subst);
            let expected_receiver = first_input_ty(interner, substituted_sig)?;

            if probe_unify(fcx, expected_receiver, probe_ty) {
                return Some(MethodCandidate {
                    source: CandidateSource::Inherent {
                        impl_id: imp.id,
                        item_def_id: *def_id,
                    },
                    impl_subst,
                    raw_sig: *sig,
                });
            }
        }
    }

    None
}

/// True if `sig`'s first parameter is a valid receiver for `self_ty`
/// (`self_ty`, `&self_ty`, or `&mut self_ty`).
fn is_method_sig(interner: &yelang_ty::interner::Interner, sig: PolyFnSig, self_ty: TyId) -> bool {
    let Some(expected) = first_input_ty(interner, sig) else {
        return false;
    };
    if expected == self_ty {
        return true;
    }
    match interner.ty(expected) {
        Ty::Ref(inner, _) => inner == self_ty,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Candidate assembly: trait (extension) methods
// ---------------------------------------------------------------------------

fn pick_trait_candidate(
    fcx: &mut FnCtxt<'_>,
    probe_ty: TyId,
    adjustments: &[Adjustment],
    method: Symbol,
) -> Option<MethodCandidate> {
    let interner = fcx.tcx.interner();

    // The `Self` type for a trait method is the probe type with any trailing
    // autoref/automut adjustment stripped. The trait is implemented for the
    // unadjusted type, while the method's `self` parameter may be `&Self` or
    // `&mut Self`.
    let self_ty = trait_self_ty_for_probe(interner, probe_ty, adjustments);

    let mut candidates: Vec<MethodCandidate> = Vec::new();

    for (trait_def_id, trait_def) in fcx.tcx.trait_defs.iter_enumerated() {
        for item in &trait_def.items {
            let TraitItemDefData::Fn { def_id, ident, sig } = item else {
                continue;
            };
            if ident.symbol != method {
                continue;
            }

            let mut trait_subst = fcx.fresh_substitution_for_generics(trait_def_id);
            // Append `Self` at the end of the substitution. Explicit generic
            // parameters use indices 0..n-1, so `Self` is index n.
            trait_subst.args.push(GenericArg::Type(self_ty));

            let substituted_sig = substitute_fn_sig(interner, *sig, &trait_subst);
            let expected_receiver = first_input_ty(interner, substituted_sig)?;

            if probe_unify(fcx, expected_receiver, probe_ty) {
                let explicit_args = &trait_subst.args[..trait_subst.args.len() - 1];
                let mut trait_ref_args = Vec::with_capacity(explicit_args.len() + 1);
                trait_ref_args.push(GenericArg::Type(self_ty));
                trait_ref_args.extend(explicit_args.iter().copied());
                let trait_ref = TraitRef {
                    def_id: trait_def_id,
                    args: interner.mk_generic_args(&trait_ref_args),
                };

                candidates.push(MethodCandidate {
                    source: CandidateSource::Trait {
                        trait_def_id,
                        item_def_id: *def_id,
                        trait_ref,
                    },
                    impl_subst: trait_subst,
                    raw_sig: *sig,
                });
            }
        }
    }

    // If multiple traits supply the same method name (e.g. both `Iterator` and
    // `Queryable` define `filter`), we must pick the one whose trait obligation
    // can actually be proven. A candidate whose trait bound is satisfiable wins
    // over a candidate that merely unifies the receiver.
    let mut best: Option<MethodCandidate> = None;
    for candidate in candidates.iter().cloned() {
        let snapshot = fcx.infer.snapshot();
        if trait_pred_holds(fcx, &candidate) {
            best = Some(candidate);
            break;
        }
        fcx.infer.rollback_to(snapshot);
    }

    // If no candidate could be proven, fall back to the first one that
    // structurally matched. This preserves error messages for genuinely
    // unimplemented methods while letting the later obligation pass report
    // the real problem.
    best.or_else(|| candidates.into_iter().next())
}

/// Try to prove the trait obligation implied by a method candidate, applying
/// any inferred substitutions back to the body inference context on success.
/// Returns `true` only if the solver returns certainty `Yes`.
fn trait_pred_holds(fcx: &mut FnCtxt<'_>, candidate: &MethodCandidate) -> bool {
    let CandidateSource::Trait { trait_ref, .. } = &candidate.source else {
        return false;
    };

    let pred = Predicate::Trait(TraitPredicate {
        trait_ref: *trait_ref,
        polarity: ImplPolarity::Positive,
    });
    let pred = fcx.resolve_predicate(pred);
    let body_vars = collect_body_infer_vars(fcx.tcx.interner(), &mut fcx.infer, &pred);

    let mut ecx = EvalCtxt::new(fcx.tcx.interner(), fcx.tcx);
    let goal = Goal::new(fcx.param_env, pred);
    let canonical_goal = yelang_trait_solver::canonicalize::canonicalize(
        goal,
        fcx.tcx.interner(),
        &mut fcx.infer,
        ecx.max_universe(),
    );

    match ecx.evaluate_canonical_goal(canonical_goal) {
        Ok(response) if response.value.certainty == Certainty::Yes => {
            fcx.apply_response_to_body(&body_vars, &response);
            true
        }
        Ok(_) => false,
        Err(_) => false,
    }
}

/// Compute the `Self` type to use for trait method resolution given the probe
/// type and its receiver adjustments. Autoref/automut adjustments are part of
/// the method-call transformation, not part of the type that implements the
/// trait, so they are stripped.
fn trait_self_ty_for_probe(
    interner: &yelang_ty::interner::Interner,
    probe_ty: TyId,
    adjustments: &[Adjustment],
) -> TyId {
    match adjustments.last() {
        Some(Adjustment::Ref | Adjustment::RefMut) => match interner.ty(probe_ty) {
            Ty::Ref(inner, _) => inner,
            _ => probe_ty,
        },
        _ => probe_ty,
    }
}

// ---------------------------------------------------------------------------
// Confirmation
// ---------------------------------------------------------------------------

fn confirm_and_record(
    fcx: &mut FnCtxt<'_>,
    expr_id: ExprId,
    receiver: ExprId,
    pick: &MethodPick,
    args: &[ExprId],
) -> TyId {
    let output = confirm_method(fcx, pick, args);
    fcx.results
        .expr_adjustments
        .insert(receiver, pick.receiver_adjustments.clone());

    let resolution = match pick.candidate.source {
        CandidateSource::Inherent { impl_id, item_def_id } => MethodResolution {
            trait_def_id: None,
            method_def_id: Some(item_def_id),
            impl_def_id: Some(fcx.tcx.impl_def(impl_id).def_id),
        },
        CandidateSource::Trait {
            trait_def_id,
            item_def_id,
            ..
        } => MethodResolution {
            trait_def_id: Some(trait_def_id),
            method_def_id: Some(item_def_id),
            impl_def_id: None,
        },
    };
    fcx.results.record_method_resolution(expr_id, resolution);

    output
}

fn confirm_method(fcx: &mut FnCtxt<'_>, pick: &MethodPick, args: &[ExprId]) -> TyId {
    let interner = fcx.tcx.interner();
    let MethodCandidate {
        source,
        impl_subst,
        raw_sig,
    } = &pick.candidate;

    let sig = substitute_fn_sig(interner, *raw_sig, impl_subst);
    let inputs = &sig.sig.inputs;
    if inputs.is_empty() {
        return fcx.mk_error();
    }

    let expected_receiver = match inputs.iter().next().unwrap() {
        GenericArg::Type(ty) => *ty,
        _ => return fcx.mk_error(),
    };

    // Commit the receiver adjustment + impl substitution.
    let _ = fcx.eq(expected_receiver, pick.probe_ty);

    // Emit obligations for any user-defined `Deref` steps the probe used.
    for adj in &pick.receiver_adjustments {
        if let Adjustment::DerefTrait { source, target } = *adj {
            crate::autoderef::emit_deref_trait_obligations(fcx, source, target);
        }
    }

    // Check the remaining arguments against the method's formal parameters.
    for (input, arg_expr) in inputs.iter().skip(1).zip(args.iter()) {
        let expected = match input {
            GenericArg::Type(ty) => *ty,
            _ => continue,
        };

        // If the argument is a closure and the expected parameter is a function
        // pointer, propagate the expected input types so that unannotated
        // closure parameters can be inferred from the call context (e.g.
        // `xs.map(|x| x * 10)`).
        let arg_ty = if let Some(body_id) = closure_body_id(fcx, *arg_expr) {
            let expected_inputs = expected_fn_input_tys(fcx, expected);
            crate::check::check_closure_with_expected(fcx, body_id, &expected_inputs)
        } else {
            check_expr(fcx, *arg_expr)
        };
        fcx.record_expr_ty(*arg_expr, arg_ty);

        if fcx.coerce(arg_ty, expected).is_err() {
            let span = crate::check::expr_span(fcx, *arg_expr);
            fcx.report_mismatch(span, expected, arg_ty);
        }
    }

    // Emit obligations implied by the chosen candidate.
    match source {
        CandidateSource::Inherent { impl_id, .. } => {
            let imp = fcx.tcx.impl_def(*impl_id);
            for &pred in &imp.generics.predicates {
                let pred = substitute(interner, pred, impl_subst);
                fcx.emit_obligation(pred);
            }
        }
        CandidateSource::Trait { trait_ref, .. } => {
            fcx.emit_obligation(Predicate::Trait(TraitPredicate {
                trait_ref: *trait_ref,
                polarity: ImplPolarity::Positive,
            }));
        }
    }

    sig.sig.output
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// If `expr` is a closure expression, return its body id.
fn closure_body_id(fcx: &FnCtxt<'_>, expr: ExprId) -> Option<BodyId> {
    fcx.tcx
        .crate_hir()
        .expr(expr)
        .and_then(|e| match e {
            yelang_hir::hir::expr::Expr::Closure { body, .. } => Some(*body),
            _ => None,
        })
}

/// If `ty` is a function pointer, return the expected input types.
fn expected_fn_input_tys(fcx: &mut FnCtxt<'_>, ty: TyId) -> Vec<TyId> {
    let interner = fcx.tcx.interner();
    if !interner.has_ty(ty) {
        return vec![];
    }
    match interner.ty(ty) {
        Ty::FnPtr(sig) => sig
            .sig
            .inputs
            .iter()
            .map(|arg| match arg {
                GenericArg::Type(t) => *t,
                _ => fcx.new_ty_var(),
            })
            .collect(),
        _ => vec![],
    }
}

fn first_input_ty(_interner: &yelang_ty::interner::Interner, sig: PolyFnSig) -> Option<TyId> {
    sig.sig.inputs.iter().next().and_then(|arg| match arg {
        GenericArg::Type(ty) => Some(*ty),
        GenericArg::Const(_) => None,
    })
}

fn substitute_fn_sig(
    interner: &yelang_ty::interner::Interner,
    sig: PolyFnSig,
    subst: &Substitution,
) -> PolyFnSig {
    PolyFnSig {
        sig: yelang_ty::ty::FnSig {
            inputs: substitute(interner, sig.sig.inputs, subst),
            output: substitute(interner, sig.sig.output, subst),
            return_ty_infer: sig.sig.return_ty_infer,
        },
    }
}

/// Try to unify two types in a speculative snapshot. Rolls back all inference
/// state regardless of success so the caller can decide whether to commit.
fn probe_unify(fcx: &mut FnCtxt<'_>, expected: TyId, found: TyId) -> bool {
    let snapshot = fcx.infer.snapshot();
    let ok = fcx.eq(expected, found).is_ok();
    fcx.infer.rollback_to(snapshot);
    ok
}
