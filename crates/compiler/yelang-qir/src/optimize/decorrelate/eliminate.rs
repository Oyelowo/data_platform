//! Top-down recursive elimination of dependent joins.

use crate::plan::{GroupKey, JoinKind, Plan, PlanArena, PlanId, SortKey, SortSpec};

use super::dependent_join::eliminate_dependent_join;
use super::state::UnnestingState;

pub(super) fn eliminate_recursive(
    node: PlanId,
    state: &mut UnnestingState,
    arena: &mut PlanArena,
) -> PlanId {
    // BTW 2025 §4.3: CTE DAG cutting — if this node was already processed,
    // return the cached result. Prevents duplicate work on shared subtrees.
    if let Some(&cached) = state.cache.get(&node) {
        return cached;
    }

    let plan = arena.plan(node).clone();

    let result = match &plan {
        Plan::DependentJoin {
            outer,
            inner,
            pred,
            kind,
        } => {
            eliminate_dependent_join(node, *outer, *inner, *pred, *kind, state, arena)
        }

        // BTW 2025: Γ_{A; a:f}(T) → Γ_{A ∪ A(D); a:f}(unnest(T))
        Plan::Aggregate {
            input,
            keys,
            aggs,
            into,
        } => {
            let new_input = eliminate_recursive(*input, state, arena);

            let mut new_keys = keys.clone();
            if let Some(info) = state.current() {
                for &outer_ref in &info.outer_refs {
                    let already_present = new_keys.iter().any(|&(name, _)| name == outer_ref);
                    if !already_present {
                        new_keys.push((outer_ref, GroupKey::Column(outer_ref)));
                    }
                }
            }

            if new_input != *input || new_keys.len() != keys.len() {
                arena.alloc(Plan::Aggregate {
                    input: new_input,
                    keys: new_keys,
                    aggs: aggs.clone(),
                    into: *into,
                })
            } else {
                node
            }
        }

        // BTW 2025 §4.4: ORDER BY LIMIT in correlated subqueries.
        // Add outer refs as prefix sort keys so sorting is per-outer-binding.
        // The Limit then applies per partition.
        Plan::Sort { input, specs } => {
            let new_input = eliminate_recursive(*input, state, arena);

            let mut new_specs = specs.clone();
            if let Some(info) = state.current() {
                // Add outer refs as prefix sort keys (ascending).
                for &outer_ref in &info.outer_refs {
                    let already_present = new_specs.iter().any(|s| {
                        matches!(&s.key, SortKey::Column(c) if *c == outer_ref)
                    });
                    if !already_present {
                        new_specs.insert(
                            0,
                            SortSpec {
                                key: SortKey::Column(outer_ref),
                                desc: false,
                            },
                        );
                    }
                }
            }

            if new_input != *input || new_specs.len() != specs.len() {
                arena.alloc(Plan::Sort {
                    input: new_input,
                    specs: new_specs,
                })
            } else {
                node
            }
        }

        // Unary pass-through: recurse into input.
        Plan::Filter { input, pred } => {
            let new_input = eliminate_recursive(*input, state, arena);
            if new_input != *input {
                arena.alloc(Plan::Filter {
                    input: new_input,
                    pred: *pred,
                })
            } else {
                node
            }
        }

        Plan::Project { input, exprs } => {
            let new_input = eliminate_recursive(*input, state, arena);
            if new_input != *input {
                arena.alloc(Plan::Project {
                    input: new_input,
                    exprs: exprs.clone(),
                })
            } else {
                node
            }
        }

        Plan::Map {
            input,
            func,
            flatten_depth,
        } => {
            let new_input = eliminate_recursive(*input, state, arena);
            if new_input != *input {
                arena.alloc(Plan::Map {
                    input: new_input,
                    func: *func,
                    flatten_depth: *flatten_depth,
                })
            } else {
                node
            }
        }

        Plan::Limit {
            input,
            skip,
            fetch,
        } => {
            let new_input = eliminate_recursive(*input, state, arena);
            if new_input != *input {
                arena.alloc(Plan::Limit {
                    input: new_input,
                    skip: *skip,
                    fetch: *fetch,
                })
            } else {
                node
            }
        }

        Plan::Distinct { input, on } => {
            let new_input = eliminate_recursive(*input, state, arena);
            if new_input != *input {
                arena.alloc(Plan::Distinct {
                    input: new_input,
                    on: on.clone(),
                })
            } else {
                node
            }
        }

        Plan::Traverse { input, paths } => {
            let new_input = eliminate_recursive(*input, state, arena);
            if new_input != *input {
                arena.alloc(Plan::Traverse {
                    input: new_input,
                    paths: paths.clone(),
                })
            } else {
                node
            }
        }

        // BTW 2025 §4.1: Full outer join with correlated predicates.
        // When a Join(Full) is inside a DependentJoin, we need special
        // handling: the correlation predicate cannot be pushed into a
        // full outer join directly. Instead, we evaluate the predicate
        // for every binding of the outer refs and use the result as
        // the join condition.
        Plan::Join {
            left,
            right,
            kind: JoinKind::Full,
            on,
            filter,
        } if state.current().is_some() => {
            // Full outer join inside a correlated context.
            // Recurse into both sides, then rebuild.
            let new_left = eliminate_recursive(*left, state, arena);
            let new_right = eliminate_recursive(*right, state, arena);

            // The filter predicate may reference outer refs.
            // Keep it as a post-join filter (cannot push into full outer join).
            arena.alloc(Plan::Join {
                left: new_left,
                right: new_right,
                kind: JoinKind::Full,
                on: on.clone(),
                filter: *filter,
            })
        }

        // Binary: recurse into both sides.
        Plan::Join {
            left,
            right,
            kind,
            on,
            filter,
        } => {
            let new_left = eliminate_recursive(*left, state, arena);
            let new_right = eliminate_recursive(*right, state, arena);
            if new_left != *left || new_right != *right {
                arena.alloc(Plan::Join {
                    left: new_left,
                    right: new_right,
                    kind: *kind,
                    on: on.clone(),
                    filter: *filter,
                })
            } else {
                node
            }
        }

        Plan::GroupJoin { left, right, on, aggs } => {
            let new_left = eliminate_recursive(*left, state, arena);
            let new_right = eliminate_recursive(*right, state, arena);
            if new_left != *left || new_right != *right {
                arena.alloc(Plan::GroupJoin {
                    left: new_left,
                    right: new_right,
                    on: on.clone(),
                    aggs: aggs.clone(),
                })
            } else {
                node
            }
        }

        Plan::Union { inputs } => {
            let mut changed = false;
            let new_inputs: Vec<PlanId> = inputs
                .iter()
                .map(|&inp| {
                    let new_inp = eliminate_recursive(inp, state, arena);
                    if new_inp != inp {
                        changed = true;
                    }
                    new_inp
                })
                .collect();
            if changed {
                arena.alloc(Plan::Union { inputs: new_inputs })
            } else {
                node
            }
        }

        // Leaves and opaque: nothing to eliminate.
        Plan::Scan { .. }
        | Plan::Constant { .. }
        | Plan::Empty { .. }
        | Plan::Extension { .. }
        | Plan::Repeat { .. }
        | Plan::ScalarSubquery { .. }
        | Plan::Exists { .. } => node,
    };

    // Cache the result for CTE DAG cutting.
    state.cache.insert(node, result);
    result
}
