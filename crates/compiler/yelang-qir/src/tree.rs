//! Plan tree traversal and rewriting.
//!
//! Provides the infrastructure the optimizer needs to walk and transform
//! the plan tree: child enumeration, child mapping, and top-down /
//! bottom-up recursive rewriting with change tracking.

use crate::logical::plan::{Plan, PlanArena, PlanId};

// ---------------------------------------------------------------------------
// Transformed
// ---------------------------------------------------------------------------

/// Result of a rewrite: the (possibly new) node id and whether anything changed.
#[derive(Debug, Clone, Copy)]
pub struct Transformed {
    pub id: PlanId,
    pub changed: bool,
}

impl Transformed {
    /// No change.
    pub fn no(id: PlanId) -> Self {
        Self { id, changed: false }
    }

    /// The node was replaced.
    pub fn yes(id: PlanId) -> Self {
        Self { id, changed: true }
    }
}

// ---------------------------------------------------------------------------
// Child access
// ---------------------------------------------------------------------------

/// Collect the child [`PlanId`]s of a plan node.
///
/// Returns them in a stable order (left before right for binary nodes).
pub fn children(plan: &Plan) -> Vec<PlanId> {
    match plan {
        // Unary
        Plan::Filter { input, .. }
        | Plan::Project { input, .. }
        | Plan::Map { input, .. }
        | Plan::Aggregate { input, .. }
        | Plan::Window { input, .. }
        | Plan::Sort { input, .. }
        | Plan::Limit { input, .. }
        | Plan::Distinct { input, .. }
        | Plan::Traverse { input, .. }
        | Plan::Repeat { input, .. } => vec![*input],

        // Binary
        Plan::Join { left, right, .. } | Plan::GroupJoin { left, right, .. } => {
            vec![*left, *right]
        }
        Plan::DependentJoin { outer, inner, .. } => vec![*outer, *inner],

        // N-ary
        Plan::Union { inputs } => inputs.clone(),

        // Subquery wrappers
        Plan::ScalarSubquery { plan, .. } | Plan::Exists { plan, .. } => vec![*plan],

        // Leaves
        Plan::Scan { .. } | Plan::Constant { .. } | Plan::Empty { .. } => vec![],

        // Extension — ask the node
        Plan::Extension { node } => node.inputs(),
    }
}

/// Replace the child [`PlanId`]s of a plan node, returning the updated plan.
///
/// `new_children` must have the same length and order as [`children`].
pub fn map_children(plan: &Plan, new_children: &[PlanId]) -> Plan {
    let mut i = 0;
    let mut next = || {
        let id = new_children[i];
        i += 1;
        id
    };

    match plan {
        Plan::Filter { pred, .. } => Plan::Filter {
            input: next(),
            pred: *pred,
        },
        Plan::Project { exprs, .. } => Plan::Project {
            input: next(),
            exprs: exprs.clone(),
        },
        Plan::Map {
            func,
            flatten_depth,
            ..
        } => Plan::Map {
            input: next(),
            func: *func,
            flatten_depth: *flatten_depth,
        },
        Plan::Aggregate { keys, aggs, into, .. } => Plan::Aggregate {
            input: next(),
            keys: keys.clone(),
            aggs: aggs.clone(),
            into: *into,
        },
        Plan::Window { funcs, .. } => Plan::Window {
            input: next(),
            funcs: funcs.clone(),
        },
        Plan::Sort { specs, .. } => Plan::Sort {
            input: next(),
            specs: specs.clone(),
        },
        Plan::Limit { skip, fetch, .. } => Plan::Limit {
            input: next(),
            skip: *skip,
            fetch: *fetch,
        },
        Plan::Distinct { on, .. } => Plan::Distinct {
            input: next(),
            on: on.clone(),
        },
        Plan::Traverse { paths, .. } => Plan::Traverse {
            input: next(),
            paths: paths.clone(),
        },
        Plan::Repeat {
            func, max_iters, ..
        } => Plan::Repeat {
            input: next(),
            func: *func,
            max_iters: *max_iters,
        },
        Plan::Join {
            kind,
            on,
            filter,
            ..
        } => Plan::Join {
            left: next(),
            right: next(),
            kind: *kind,
            on: on.clone(),
            filter: *filter,
        },
        Plan::DependentJoin { pred, kind, .. } => Plan::DependentJoin {
            outer: next(),
            inner: next(),
            pred: *pred,
            kind: *kind,
        },
        Plan::GroupJoin { on, aggs, .. } => Plan::GroupJoin {
            left: next(),
            right: next(),
            on: on.clone(),
            aggs: aggs.clone(),
        },
        Plan::Union { .. } => Plan::Union {
            inputs: new_children.to_vec(),
        },
        Plan::ScalarSubquery { correlation, .. } => Plan::ScalarSubquery {
            plan: next(),
            correlation: correlation.clone(),
        },
        Plan::Exists {
            correlation,
            negated,
            ..
        } => Plan::Exists {
            plan: next(),
            correlation: correlation.clone(),
            negated: *negated,
        },
        // Leaves and Extension: no children to map.
        Plan::Scan { .. } | Plan::Constant { .. } | Plan::Empty { .. } => plan.clone(),
        Plan::Extension { .. } => plan.clone(),
    }
}

// ---------------------------------------------------------------------------
// Recursive traversal
// ---------------------------------------------------------------------------

/// Apply `f` to every node bottom-up (children before parents).
///
/// `f` returns `Transformed` — if it replaces a node, the new node is
/// allocated in the arena and the parent is rebuilt with the new child id.
pub fn transform_bottom_up(
    root: PlanId,
    arena: &mut PlanArena,
    f: &mut dyn FnMut(PlanId, &mut PlanArena) -> Transformed,
) -> Transformed {
    // Recurse into children first.
    let plan = arena.plan(root).clone();
    let kids = children(&plan);

    let mut any_changed = false;
    let mut new_kids = Vec::with_capacity(kids.len());

    for &kid in &kids {
        let result = transform_bottom_up(kid, arena, f);
        new_kids.push(result.id);
        any_changed |= result.changed;
    }

    // Rebuild this node if any child changed.
    let current_id = if any_changed {
        let new_plan = map_children(&plan, &new_kids);
        let new_id = arena.alloc(new_plan);
        // Copy metadata and origin to the new node.
        if let Some(meta) = arena.meta(root) {
            arena.set_meta(new_id, meta.clone());
        }
        if let Some(origin) = arena.origin.get(root) {
            arena.origin.insert(new_id, origin.clone());
        }
        new_id
    } else {
        root
    };

    // Apply the rule to this node.
    f(current_id, arena)
}

/// Apply `f` to every node top-down (parents before children).
pub fn transform_top_down(
    root: PlanId,
    arena: &mut PlanArena,
    f: &mut dyn FnMut(PlanId, &mut PlanArena) -> Transformed,
) -> Transformed {
    // Apply the rule to this node first.
    let result = f(root, arena);
    let current = result.id;

    // Recurse into children.
    let plan = arena.plan(current).clone();
    let kids = children(&plan);

    let mut any_changed = result.changed;
    let mut new_kids = Vec::with_capacity(kids.len());

    for &kid in &kids {
        let child_result = transform_top_down(kid, arena, f);
        new_kids.push(child_result.id);
        any_changed |= child_result.changed;
    }

    if any_changed && !new_kids.is_empty() {
        let new_plan = map_children(&plan, &new_kids);
        let new_id = arena.alloc(new_plan);
        if let Some(meta) = arena.meta(current) {
            arena.set_meta(new_id, meta.clone());
        }
        if let Some(origin) = arena.origin.get(current) {
            arena.origin.insert(new_id, origin.clone());
        }
        Transformed::yes(new_id)
    } else {
        Transformed {
            id: current,
            changed: any_changed,
        }
    }
}
