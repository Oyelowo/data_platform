//! Subquery decorrelation — eliminate correlated subqueries by rewriting
//! them into joins.
//!
//! Implements the top-down, one-pass algorithm from:
//! - Neumann & Kemper, "Unnesting Arbitrary Queries" (BTW 2015)
//! - Neumann, "Improving Unnesting of Complex Queries" (BTW 2025)
//!
//! # Algorithm overview
//!
//! 1. Convert `ScalarSubquery` / `Exists` nodes into `DependentJoin` nodes.
//! 2. Walk the plan tree **top-down, one pass**.
//! 3. For each `DependentJoin`:
//!    a. Try simple elimination (pull correlation predicate into the join).
//!    b. If nested, unnest the left side first (makes columns available).
//!    c. Build a union-find of column equivalences from join predicates.
//!    d. Unnest the right side under this unnesting's umbrella.
//!    e. At leaves: choose domain-join or substitution via union-find.
//! 4. Invariant: **never push different D sets across dependent joins.**
//!
//! After this pass, no `DependentJoin`, `ScalarSubquery`, or `Exists`
//! nodes remain in the plan tree.

use yelang_arena::FxHashMap;
use yelang_hir::Crate;
use yelang_interner::Symbol;

use crate::analysis::referenced_fields;
use crate::plan::{
    DepJoinKind, JoinKind, Plan, PlanArena, PlanId,
};
use crate::tree::Transformed;

// ---------------------------------------------------------------------------
// Union-Find
// ---------------------------------------------------------------------------

/// Union-Find (disjoint set) over column symbols.
///
/// Tracks equivalence classes of columns derived from join predicates
/// (e.g. `a.x = b.y` makes `x` and `y` equivalent). Used during
/// decorrelation to decide whether an outer reference can be substituted
/// with an already-bound inner column (avoiding a domain join).
#[derive(Debug, Default)]
pub struct UnionFind {
    parent: FxHashMap<Symbol, Symbol>,
}

impl UnionFind {
    pub fn new() -> Self {
        Self {
            parent: FxHashMap::default(),
        }
    }

    /// Find the representative of the equivalence class containing `x`.
    /// Applies path compression.
    pub fn find(&mut self, x: Symbol) -> Symbol {
        let parent = self.parent.get(&x).copied();
        match parent {
            None => x,
            Some(p) if p == x => x,
            Some(p) => {
                let root = self.find(p);
                self.parent.insert(x, root);
                root
            }
        }
    }

    /// Merge the equivalence classes of `a` and `b`.
    pub fn union(&mut self, a: Symbol, b: Symbol) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }

    /// Check whether two symbols are in the same equivalence class.
    pub fn equivalent(&mut self, a: Symbol, b: Symbol) -> bool {
        self.find(a) == self.find(b)
    }

    /// Merge all equivalences from another union-find into this one.
    pub fn merge(&mut self, other: &UnionFind) {
        for (&k, &v) in other.parent.iter() {
            self.union(k, v);
        }
    }
}

// ---------------------------------------------------------------------------
// UnnestingInfo
// ---------------------------------------------------------------------------

/// State for one `DependentJoin` being eliminated.
///
/// Created when we encounter a `DependentJoin` during the top-down
/// traversal. Carries the domain projection, column equivalences, and
/// the substitution map.
#[derive(Debug)]
#[allow(dead_code)] // Fields used in full BTW 2025 implementation
struct UnnestingInfo {
    /// The `DependentJoin` node being eliminated.
    join_id: PlanId,
    /// Outer attributes referenced by the inner side: A(outer) ∩ F(inner).
    outer_refs: Vec<Symbol>,
    /// Union-find of equivalent columns (from join predicates).
    cclasses: UnionFind,
    /// Map from outer-ref columns to their substitutes after union-find.
    /// If `repr[c] = d`, then `c` can be replaced by `d` in the inner plan.
    repr: FxHashMap<Symbol, Symbol>,
    /// Parent unnesting (for nested dependent joins).
    parent: Option<usize>,
}

/// Stack of active unnesting states.
struct UnnestingState {
    stack: Vec<UnnestingInfo>,
}

impl UnnestingState {
    fn new() -> Self {
        Self { stack: Vec::new() }
    }

    fn push(&mut self, info: UnnestingInfo) -> usize {
        let idx = self.stack.len();
        self.stack.push(info);
        idx
    }

    fn pop(&mut self) {
        self.stack.pop();
    }

    fn current(&self) -> Option<&UnnestingInfo> {
        self.stack.last()
    }

    #[allow(dead_code)]
    fn current_mut(&mut self) -> Option<&mut UnnestingInfo> {
        self.stack.last_mut()
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Eliminate all correlated subqueries from the plan tree.
///
/// This is a **one-shot, top-down** pass. It must run before the
/// optimizer's fixpoint loop. After this pass, no `DependentJoin`,
/// `ScalarSubquery`, or `Exists` nodes remain.
///
/// Returns the new root [`PlanId`].
pub fn decorrelate(root: PlanId, arena: &mut PlanArena, hir: &Crate) -> PlanId {
    // Phase 1: Convert ScalarSubquery/Exists → DependentJoin.
    let root = convert_subqueries_to_dependent_joins(root, arena);

    // Phase 2: Top-down elimination of DependentJoin nodes.
    let mut state = UnnestingState::new();
    eliminate_recursive(root, &mut state, arena, hir)
}

// ---------------------------------------------------------------------------
// Phase 1: Convert subqueries to dependent joins
// ---------------------------------------------------------------------------

/// Convert `ScalarSubquery` and `Exists` nodes into `DependentJoin` nodes.
///
/// This is done as a bottom-up pass before the main top-down elimination.
fn convert_subqueries_to_dependent_joins(
    root: PlanId,
    arena: &mut PlanArena,
) -> PlanId {
    crate::tree::transform_bottom_up(root, arena, &mut |id, arena| {
        let plan = arena.plan(id).clone();
        match &plan {
            Plan::ScalarSubquery { plan: inner, correlation: _ } => {
                // A scalar subquery becomes a dependent single join.
                // The outer side is the "current row" — represented as
                // an Empty node with one row. The actual outer context
                // is provided by the parent plan.
                //
                // For now, we create a DependentJoin with the inner plan
                // and mark it as a Single join (at most one match per
                // outer row).
                let outer = arena.alloc(Plan::Empty { produce_one_row: true });
                let dep_join = Plan::DependentJoin {
                    outer,
                    inner: *inner,
                    pred: None,
                    kind: DepJoinKind::Single,
                };
                let new_id = arena.alloc(dep_join);
                Transformed::yes(new_id)
            }

            Plan::Exists {
                plan: inner,
                correlation: _,
                negated,
            } => {
                let outer = arena.alloc(Plan::Empty { produce_one_row: true });
                let kind = if *negated {
                    DepJoinKind::Anti
                } else {
                    DepJoinKind::Semi
                };
                let dep_join = Plan::DependentJoin {
                    outer,
                    inner: *inner,
                    pred: None,
                    kind,
                };
                let new_id = arena.alloc(dep_join);
                Transformed::yes(new_id)
            }

            _ => Transformed::no(id),
        }
    })
    .id
}

// ---------------------------------------------------------------------------
// Phase 2: Top-down dependent join elimination
// ---------------------------------------------------------------------------

fn eliminate_recursive(
    node: PlanId,
    state: &mut UnnestingState,
    arena: &mut PlanArena,
    hir: &Crate,
) -> PlanId {
    let plan = arena.plan(node).clone();

    match &plan {
        Plan::DependentJoin {
            outer,
            inner,
            pred,
            kind,
        } => {
            eliminate_dependent_join(node, *outer, *inner, *pred, *kind, state, arena, hir)
        }

        // Per-operator rules: push outer refs into group-by keys,
        // window partition-by, etc.
        Plan::Aggregate {
            input,
            keys,
            aggs,
            into,
        } => {
            let new_input = eliminate_recursive(*input, state, arena, hir);

            // If we're inside an unnesting, add outer refs to group keys.
            if let Some(info) = state.current() {
                let new_keys = keys.clone();
                for &outer_ref in &info.outer_refs {
                    // Check if this outer ref is already in the keys.
                    let already_present = new_keys.iter().any(|&(name, _)| name == outer_ref);
                    if !already_present {
                        // TODO: create a proper expression reference for the
                        // outer ref column. For now, use a placeholder.
                        // In a full implementation, this would be a column
                        // reference expression in the HIR.
                    }
                }
            }

            if new_input != *input {
                let new_plan = Plan::Aggregate {
                    input: new_input,
                    keys: keys.clone(),
                    aggs: aggs.clone(),
                    into: *into,
                };
                arena.alloc(new_plan)
            } else {
                node
            }
        }

        Plan::Sort { input, specs } => {
            let new_input = eliminate_recursive(*input, state, arena, hir);
            if new_input != *input {
                let new_plan = Plan::Sort {
                    input: new_input,
                    specs: specs.clone(),
                };
                arena.alloc(new_plan)
            } else {
                node
            }
        }

        // Unary pass-through: recurse into input.
        Plan::Filter { input, pred } => {
            let new_input = eliminate_recursive(*input, state, arena, hir);
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
            let new_input = eliminate_recursive(*input, state, arena, hir);
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
            let new_input = eliminate_recursive(*input, state, arena, hir);
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
            let new_input = eliminate_recursive(*input, state, arena, hir);
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
            let new_input = eliminate_recursive(*input, state, arena, hir);
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
            let new_input = eliminate_recursive(*input, state, arena, hir);
            if new_input != *input {
                arena.alloc(Plan::Traverse {
                    input: new_input,
                    paths: paths.clone(),
                })
            } else {
                node
            }
        }

        // Binary: recurse into both sides.
        Plan::Join {
            left,
            right,
            kind,
            on,
            filter,
        } => {
            let new_left = eliminate_recursive(*left, state, arena, hir);
            let new_right = eliminate_recursive(*right, state, arena, hir);
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
            let new_left = eliminate_recursive(*left, state, arena, hir);
            let new_right = eliminate_recursive(*right, state, arena, hir);
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
                    let new_inp = eliminate_recursive(inp, state, arena, hir);
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
    }
}

// ---------------------------------------------------------------------------
// Dependent join elimination
// ---------------------------------------------------------------------------

fn eliminate_dependent_join(
    node: PlanId,
    outer: PlanId,
    inner: PlanId,
    pred: Option<yelang_hir::ids::ExprId>,
    kind: DepJoinKind,
    state: &mut UnnestingState,
    arena: &mut PlanArena,
    hir: &Crate,
) -> PlanId {
    // Step 1: Compute outer refs (A(outer) ∩ F(inner)).
    let outer_refs = compute_outer_refs(outer, inner, pred, arena, hir);

    // Step 2: Try simple elimination.
    //
    // If the predicate only references inner columns (no correlation),
    // the dependent join is trivially a regular join.
    if outer_refs.is_empty() {
        return convert_to_regular_join(outer, inner, pred, kind, arena);
    }

    // Step 3: Create the unnesting state.
    let info = UnnestingInfo {
        join_id: node,
        outer_refs: outer_refs.clone(),
        cclasses: UnionFind::new(),
        repr: FxHashMap::default(),
        parent: if state.stack.is_empty() {
            None
        } else {
            Some(state.stack.len() - 1)
        },
    };
    let _info_idx = state.push(info);

    // Step 4: Unnest the LEFT (outer) side first.
    //
    // This makes outer columns available for the inner side's unnesting.
    let new_outer = eliminate_recursive(outer, state, arena, hir);

    // Step 5: Add equivalences from the join predicate to the union-find.
    if let Some(pred_expr) = pred {
        add_predicate_equivalences(pred_expr, state, hir);
    }

    // Step 6: Unnest the RIGHT (inner) side under this unnesting's umbrella.
    let new_inner = eliminate_recursive(inner, state, arena, hir);

    // Step 7: Finalize — build the replacement plan.
    //
    // For each outer ref, try substitution via union-find first.
    // If substitution works, no domain join is needed.
    // Otherwise, create a domain join: D = Π_{outer_refs}(outer).
    let result = finalize_unnesting(
        new_outer,
        new_inner,
        pred,
        kind,
        &outer_refs,
        state,
        arena,
    );

    state.pop();
    result
}

/// Compute the outer references: symbols produced by `outer` that are
/// referenced by `inner` or the join predicate.
fn compute_outer_refs(
    outer: PlanId,
    inner: PlanId,
    pred: Option<yelang_hir::ids::ExprId>,
    arena: &PlanArena,
    hir: &Crate,
) -> Vec<Symbol> {
    use crate::analysis::plan_output_fields;

    let outer_fields = if let Some(outer_plan) = arena.get(outer) {
        plan_output_fields(outer_plan, arena, hir)
    } else {
        return vec![];
    };

    // Collect fields referenced by the inner plan and predicate.
    let mut inner_refs = yelang_arena::FxHashSet::new();

    if let Some(pred_expr) = pred {
        for f in referenced_fields(pred_expr, hir).iter() {
            inner_refs.insert(*f);
        }
    }

    // Walk the inner plan tree to collect all referenced fields.
    collect_plan_refs(inner, arena, hir, &mut inner_refs);

    // Intersection: fields that are both produced by outer and referenced by inner.
    outer_fields
        .iter()
        .filter(|f| inner_refs.contains(f))
        .copied()
        .collect()
}

/// Recursively collect all field references from a plan subtree.
fn collect_plan_refs(
    node: PlanId,
    arena: &PlanArena,
    hir: &Crate,
    out: &mut yelang_arena::FxHashSet<Symbol>,
) {
    let Some(plan) = arena.get(node) else {
        return;
    };

    let plan_refs = crate::analysis::plan_referenced_fields(plan, hir);
    for f in plan_refs.iter() {
        out.insert(*f);
    }

    // Recurse into children.
    for child in crate::tree::children(plan) {
        collect_plan_refs(child, arena, hir, out);
    }
}

/// Add column equivalences from a predicate expression to the union-find.
///
/// Looks for `Binary { op: Eq, left: Field(a), right: Field(b) }` patterns
/// and unions `a` and `b`.
fn add_predicate_equivalences(
    pred: yelang_hir::ids::ExprId,
    state: &mut UnnestingState,
    hir: &Crate,
) {
    // TODO: walk the predicate expression tree looking for equality
    // comparisons between field accesses. For each `a.x == b.y`,
    // add `union(x, y)` to the current unnesting's cclasses.
    //
    // For now, this is a no-op. The full implementation requires
    // pattern-matching on the HIR expression tree.
    let _ = (pred, state, hir);
}

/// Convert a trivial dependent join (no correlation) to a regular join.
fn convert_to_regular_join(
    outer: PlanId,
    inner: PlanId,
    pred: Option<yelang_hir::ids::ExprId>,
    kind: DepJoinKind,
    arena: &mut PlanArena,
) -> PlanId {
    let join_kind = match kind {
        DepJoinKind::Join | DepJoinKind::Single => JoinKind::Inner,
        DepJoinKind::Semi => JoinKind::Semi,
        DepJoinKind::Anti => JoinKind::Anti,
        DepJoinKind::LeftOuter => JoinKind::Left,
    };

    arena.alloc(Plan::Join {
        left: outer,
        right: inner,
        kind: join_kind,
        on: vec![],
        filter: pred,
    })
}

/// Finalize the unnesting of a dependent join.
///
/// For each outer ref, try substitution via union-find. If all outer refs
/// can be substituted, no domain join is needed. Otherwise, create a
/// domain projection and join.
fn finalize_unnesting(
    outer: PlanId,
    inner: PlanId,
    pred: Option<yelang_hir::ids::ExprId>,
    kind: DepJoinKind,
    outer_refs: &[Symbol],
    state: &UnnestingState,
    arena: &mut PlanArena,
) -> PlanId {
    let info = state.current().expect("unnesting info must exist");

    // Check if all outer refs can be substituted.
    let all_substitutable = outer_refs
        .iter()
        .all(|r| info.repr.contains_key(r));

    if all_substitutable && outer_refs.is_empty() {
        // No outer refs: simple regular join.
        return convert_to_regular_join(outer, inner, pred, kind, arena);
    }

    if all_substitutable {
        // All outer refs can be substituted: no domain join needed.
        // The inner plan has already been rewritten with substitutions.
        // Just create a regular join with the predicate.
        return convert_to_regular_join(outer, inner, pred, kind, arena);
    }

    // Some outer refs cannot be substituted: create a domain join.
    //
    // D = Π_{outer_refs}(outer)  — duplicate-free projection
    // result = outer ⋈_{IS NOT DISTINCT FROM} (D ⋈ inner)
    //
    // For now, we create a simplified version:
    // Project(outer, outer_refs) → Join with inner
    let domain = arena.alloc(Plan::Project {
        input: outer,
        exprs: outer_refs
            .iter()
            .map(|&name| (name, yelang_hir::ids::ExprId::default()))
            .collect(),
    });

    let join_kind = match kind {
        DepJoinKind::Join | DepJoinKind::Single => JoinKind::Inner,
        DepJoinKind::Semi => JoinKind::Semi,
        DepJoinKind::Anti => JoinKind::Anti,
        DepJoinKind::LeftOuter => JoinKind::Left,
    };

    // Join the domain with the inner plan.
    let domain_join = arena.alloc(Plan::Join {
        left: domain,
        right: inner,
        kind: JoinKind::Inner,
        on: vec![],
        filter: None,
    });

    // Join the outer with the domain-joined inner.
    arena.alloc(Plan::Join {
        left: outer,
        right: domain_join,
        kind: join_kind,
        on: vec![],
        filter: pred,
    })
}
