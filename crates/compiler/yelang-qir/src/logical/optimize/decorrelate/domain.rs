//! Domain computation: D = Π_{outer_refs}(outer) (BTW 2025, Ne24 Theorem 4.1).
//!
//! The domain D is a duplicate-free projection of the outer side onto
//! the outer reference columns. It is built lazily — only when the
//! finalize step needs it and substitution is not possible.

use yelang_interner::Symbol;

use crate::logical::plan::{ExprRef, JoinKey, Plan, PlanArena, PlanId};

use super::state::{Unnesting, UnnestingState};

/// Build the domain projection D = Π_{outer_refs}(outer).
///
/// Takes the info_idx directly to avoid borrow conflicts with the state stack.
pub(super) fn build_domain(
    info_idx: usize,
    state: &mut UnnestingState,
    arena: &mut PlanArena,
) -> PlanId {
    if let Some(domain) = state.infos[info_idx].domain {
        return domain;
    }

    let outer_refs: Vec<Symbol> = state.infos[info_idx].outer_refs.clone();
    let join_id = state.infos[info_idx].join_id;

    // Get the outer side of the dependent join.
    let outer = match arena.plan(join_id) {
        Plan::DependentJoin { outer, .. } => *outer,
        _ => unreachable!("join_id must be a DependentJoin"),
    };

    // Build D = Project { input: outer, exprs: outer_refs as column pass-throughs }
    let exprs: Vec<(Symbol, ExprRef)> = outer_refs
        .iter()
        .map(|&sym| {
            let expr = arena.alloc_thir_expr(yelang_thir::ThirExpr::Literal(
                yelang_hir::hir::core::Lit::Unit,
            ));
            (sym, expr)
        })
        .collect();

    let domain = arena.alloc(Plan::Project {
        input: outer,
        exprs,
    });

    state.infos[info_idx].domain = Some(domain);
    domain
}

/// Build equi-join keys for the domain join (natural join on outer refs).
///
/// For each outer ref, creates a join key pair:
///   (JoinKey::Column(outer_ref), JoinKey::Column(repr[outer_ref]))
///
/// These are used as the `on` conditions for the join between the
/// outer side and the domain-joined inner side.
pub(super) fn build_domain_join_keys(
    unnesting: &Unnesting,
    state: &UnnestingState,
) -> Vec<(JoinKey, JoinKey)> {
    let info = unnesting.info(state);
    let mut keys = Vec::new();

    for &outer_ref in &info.outer_refs {
        if let Some(&repr_col) = unnesting.repr.get(&outer_ref) {
            keys.push((JoinKey::Column(outer_ref), JoinKey::Column(repr_col)));
        } else {
            // No substitution available — use the same column name on both sides
            // (natural join semantics).
            keys.push((JoinKey::Column(outer_ref), JoinKey::Column(outer_ref)));
        }
    }

    keys
}
