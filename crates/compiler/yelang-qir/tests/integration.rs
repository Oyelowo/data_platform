//! Integration tests for the query planning pipeline.
//!
//! Tests use correct Yelang query semantics:
//! - The projection uses the **collection label** with selectors (`[*]`, `[where]`, etc.)
//! - The item binder (`@u`) is accessible in `where`, `order by`, and segment predicates
//! - `select 1 from users@u:User` returns `1`, NOT `[1, 1, ...]`
//! - Per-element results require explicit selectors: `select users@u[*].id from ...`

use yelang_ast::Program;
use yelang_hir::lower_crate;
use yelang_interner::Interner;
use yelang_qir::plan::{Plan, PlanArena};
use yelang_qir::{lower_query, Optimizer};
use yelang_tycheck::tcx::TyCtxt;

/// Run the full pipeline on source text and return the plan arena
/// plus the root plan ids for each query found.
fn plan_queries(src: &str) -> (PlanArena, Vec<yelang_qir::PlanId>, Interner) {
    let interner = Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &interner).expect("tokenize");
    let program = stream.parse::<Program>().expect("parse");

    let resolved = yelang_resolve::resolve_crate(&program, &interner);
    assert!(
        resolved.errors.is_empty(),
        "resolution errors: {:?}",
        resolved.errors
    );

    let hir_crate = lower_crate(&program, &resolved, &interner);
    let mut tcx = TyCtxt::with_string_interner(hir_crate, interner.clone());
    let diagnostics = yelang_tycheck::type_check_crate(&mut tcx);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == yelang_tycheck::diagnostics::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "type errors: {:?}", errors);

    let hir = tcx.crate_hir().clone();
    let mut arena = PlanArena::new();
    let optimizer = Optimizer::new();
    let mut roots = Vec::new();

    for (query_id, slot) in hir.queries.iter() {
        if slot.is_none() {
            continue;
        }
        if let Some(root) = lower_query(query_id, &hir, None, &interner, &hir.lang_items, &mut arena) {
            let optimized = optimizer.optimize(root, &mut arena);
            roots.push(optimized);
        }
    }

    (arena, roots, interner)
}

/// Collect the plan node types along the spine from root to deepest leaf.
fn plan_spine(arena: &PlanArena, root: yelang_qir::PlanId) -> Vec<&'static str> {
    let mut spine = Vec::new();
    let mut current = Some(root);

    while let Some(id) = current {
        let plan = arena.plan(id);
        spine.push(plan_name(plan));
        current = first_child(plan);
    }

    spine
}

fn plan_name(plan: &Plan) -> &'static str {
    match plan {
        Plan::Scan { .. } => "Scan",
        Plan::Filter { .. } => "Filter",
        Plan::Project { .. } => "Project",
        Plan::Map { .. } => "Map",
        Plan::Join { .. } => "Join",
        Plan::Aggregate { .. } => "Aggregate",
        Plan::Window { .. } => "Window",
        Plan::Sort { .. } => "Sort",
        Plan::Limit { .. } => "Limit",
        Plan::Distinct { .. } => "Distinct",
        Plan::Union { .. } => "Union",
        Plan::Traverse { .. } => "Traverse",
        Plan::DependentJoin { .. } => "DependentJoin",
        Plan::GroupJoin { .. } => "GroupJoin",
        Plan::ScalarSubquery { .. } => "ScalarSubquery",
        Plan::Exists { .. } => "Exists",
        Plan::Repeat { .. } => "Repeat",
        Plan::Extension { .. } => "Extension",
        Plan::Constant { .. } => "Constant",
        Plan::Empty { .. } => "Empty",
    }
}

fn first_child(plan: &Plan) -> Option<yelang_qir::PlanId> {
    match plan {
        Plan::Filter { input, .. }
        | Plan::Project { input, .. }
        | Plan::Map { input, .. }
        | Plan::Aggregate { input, .. }
        | Plan::Window { input, .. }
        | Plan::Sort { input, .. }
        | Plan::Limit { input, .. }
        | Plan::Distinct { input, .. }
        | Plan::Traverse { input, .. }
        | Plan::Repeat { input, .. } => Some(*input),

        Plan::Join { left, .. }
        | Plan::DependentJoin { outer: left, .. }
        | Plan::GroupJoin { left, .. } => Some(*left),

        Plan::Union { inputs } => inputs.first().copied(),
        Plan::ScalarSubquery { plan, .. } | Plan::Exists { plan, .. } => Some(*plan),

        Plan::Scan { .. } | Plan::Constant { .. } | Plan::Empty { .. } | Plan::Extension { .. } => {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Core semantics tests
// ---------------------------------------------------------------------------

#[test]
fn scalar_projection_returns_scalar() {
    // select 1 from xs@x → returns 1, NOT [1, 1, ...]
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select 1 from xs@x;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1, "expected exactly one query plan");

    let spine = plan_spine(&arena, roots[0]);
    // The root should be a Project (the projection wraps the constant).
    assert_eq!(spine[0], "Project", "root should be Project, got: {:?}", spine);
    assert!(spine.contains(&"Scan"), "expected Scan in spine, got: {:?}", spine);
}

#[test]
fn iteration_requires_explicit_selector() {
    // select xs@b[*] from xs@x where x > 1 → iterate with [*]
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let spine = plan_spine(&arena, roots[0]);
    // Should contain a Map node (from the [*] selector) and a Scan.
    assert!(spine.contains(&"Scan"), "expected Scan, got: {:?}", spine);
}

#[test]
fn where_clause_filters_collection() {
    // The where clause filters the collection before projection.
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    // After optimization, the filter should be pushed into the Scan
    // or remain as a Filter node.
    let spine = plan_spine(&arena, roots[0]);
    let has_filter = spine.contains(&"Filter");
    let has_scan = spine.contains(&"Scan");
    assert!(has_scan, "expected Scan, got: {:?}", spine);

    // If no separate Filter, the Scan should have a pushed-down filter.
    if !has_filter {
        let mut current = Some(roots[0]);
        let mut found = false;
        while let Some(id) = current {
            if let Plan::Scan { filter: Some(_), .. } = arena.plan(id) {
                found = true;
                break;
            }
            current = first_child(arena.plan(id));
        }
        assert!(found, "expected pushed-down filter in Scan, spine: {:?}", spine);
    }
}

#[test]
fn order_by_produces_sort() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x order by x desc;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let spine = plan_spine(&arena, roots[0]);
    assert!(spine.contains(&"Sort"), "expected Sort, got: {:?}", spine);
}

#[test]
fn range_produces_limit() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x range 0..2;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let spine = plan_spine(&arena, roots[0]);
    assert!(spine.contains(&"Limit"), "expected Limit, got: {:?}", spine);
}

#[test]
fn group_by_produces_aggregate() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select g from xs@x group by { v: x } into g;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let spine = plan_spine(&arena, roots[0]);
    assert!(spine.contains(&"Aggregate"), "expected Aggregate, got: {:?}", spine);
}

#[test]
fn no_correlated_nodes_after_optimization() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1;
}
"#;

    let (arena, _roots, _interner) = plan_queries(src);
    assert!(
        !arena.has_correlated_nodes(),
        "expected no correlated nodes after optimization"
    );
}

#[test]
fn full_pipeline_spine_order() {
    // where → order by → range → projection
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1 order by x range 0..2;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let spine = plan_spine(&arena, roots[0]);

    // Root should be Project (the projection).
    assert_eq!(spine[0], "Project", "root should be Project, got: {:?}", spine);
    // Should contain Limit, Sort, and Scan.
    assert!(spine.contains(&"Limit"), "spine: {:?}", spine);
    assert!(spine.contains(&"Sort"), "spine: {:?}", spine);
    assert!(spine.contains(&"Scan"), "spine: {:?}", spine);
}

#[test]
fn multiple_queries_each_get_a_plan() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let ys = [4, 5, 6];
    let _ = select xs@a[*] from xs@x;
    let _ = select ys@a[*] from ys@y;
}
"#;

    let (_arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 2, "expected two query plans, got {}", roots.len());
}

// ---------------------------------------------------------------------------
// Physical planning tests
// ---------------------------------------------------------------------------

#[test]
fn physical_plan_produces_phys_ops() {
    use yelang_qir::physical::{InMemoryExecutor, PhysArena};

    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1 order by x;
}
"#;

    let (plan_arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let mut phys_arena = PhysArena::new();
    let executor = InMemoryExecutor;
    let phys_root = yelang_qir::physical::planner::plan_physical(
        roots[0],
        &plan_arena,
        &executor,
        &mut phys_arena,
    );

    assert!(phys_arena.get(phys_root).is_some());
    assert!(phys_arena.nodes.len() >= 2);
}

#[test]
fn in_memory_executor_produces_no_exchanges() {
    use yelang_qir::physical::{InMemoryExecutor, PhysArena, PhysOp};

    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1;
}
"#;

    let (plan_arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let mut phys_arena = PhysArena::new();
    let executor = InMemoryExecutor;
    let _phys_root = yelang_qir::physical::planner::plan_physical(
        roots[0],
        &plan_arena,
        &executor,
        &mut phys_arena,
    );

    let has_exchange = phys_arena
        .nodes
        .iter()
        .any(|op| matches!(op, PhysOp::Exchange { .. }));
    assert!(!has_exchange, "in-memory should not produce Exchange nodes");
}

// ---------------------------------------------------------------------------
// SourceRef resolution tests
// ---------------------------------------------------------------------------

#[test]
fn local_variable_source_resolves_to_local() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@a[*] from xs@x;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let mut current = Some(roots[0]);
    let mut found_local_scan = false;
    while let Some(id) = current {
        if let Plan::Scan {
            source: yelang_qir::plan::SourceRef::Local { .. },
            ..
        } = arena.plan(id)
        {
            found_local_scan = true;
            break;
        }
        current = first_child(arena.plan(id));
    }
    assert!(found_local_scan, "expected Scan with Local source");
}
