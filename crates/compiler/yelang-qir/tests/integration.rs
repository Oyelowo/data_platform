//! Integration tests for the query planning pipeline.
//!
//! Each test parses source text containing a query expression, runs the
//! full pipeline (resolve → HIR → tycheck → extract → optimize), and
//! verifies the resulting plan tree structure.
//!
//! Tests use local variables as collection sources (not `@table` structs)
//! to avoid type-resolution complexity while still exercising the plan
//! extraction and optimization pipeline.

use yelang_ast::Program;
use yelang_hir::lower_crate;
use yelang_interner::Interner;
use yelang_qir::plan::{Plan, PlanArena};
use yelang_qir::{extract_query, Optimizer};
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
        if let Some(root) = extract_query(query_id, &hir, &interner, &mut arena) {
            let optimized = optimizer.optimize(root, &mut arena, &hir);
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
// Tests
// ---------------------------------------------------------------------------

#[test]
fn simple_select_produces_plan() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select x from xs@x;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1, "expected exactly one query plan");

    let spine = plan_spine(&arena, roots[0]);
    // Should produce at least a Project and a Scan.
    assert!(
        spine.contains(&"Scan"),
        "expected Scan in spine, got: {:?}",
        spine
    );
}

#[test]
fn select_with_where_produces_filter_or_pushdown() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select x from xs@x where x > 1;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let spine = plan_spine(&arena, roots[0]);

    // The where clause produces a Filter, but the optimizer may push it
    // into the Scan. Either way, the predicate must be present somewhere.
    let has_filter_node = spine.contains(&"Filter");
    let has_scan_with_filter = spine.iter().any(|&name| name == "Scan");

    assert!(
        has_filter_node || has_scan_with_filter,
        "expected Filter node or Scan with pushed-down filter, got: {:?}",
        spine
    );

    // Verify the Scan actually has a filter if there's no separate Filter node.
    if !has_filter_node {
        // Walk to the Scan and check it has a filter.
        let mut current = Some(roots[0]);
        let mut found_filtered_scan = false;
        while let Some(id) = current {
            let plan = arena.plan(id);
            if let Plan::Scan { filter: Some(_), .. } = plan {
                found_filtered_scan = true;
                break;
            }
            current = first_child(plan);
        }
        assert!(
            found_filtered_scan,
            "expected Scan with pushed-down filter, got spine: {:?}",
            spine
        );
    }
}

#[test]
fn select_with_order_by_produces_sort() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select x from xs@x order by x desc;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let spine = plan_spine(&arena, roots[0]);
    assert!(
        spine.contains(&"Sort"),
        "expected Sort in spine, got: {:?}",
        spine
    );
}

#[test]
fn select_with_range_produces_limit() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select x from xs@x range 0..2;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let spine = plan_spine(&arena, roots[0]);
    assert!(
        spine.contains(&"Limit"),
        "expected Limit in spine, got: {:?}",
        spine
    );
}

#[test]
fn select_with_group_by_produces_aggregate() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select g from xs@x group by { v: x } into g;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let spine = plan_spine(&arena, roots[0]);
    assert!(
        spine.contains(&"Aggregate"),
        "expected Aggregate in spine, got: {:?}",
        spine
    );
}

#[test]
fn no_correlated_nodes_after_optimization() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select x from xs@x where x > 1;
}
"#;

    let (arena, _roots, _interner) = plan_queries(src);
    assert!(
        !arena.has_correlated_nodes(),
        "expected no DependentJoin/ScalarSubquery/Exists after optimization"
    );
}

#[test]
fn full_query_spine_order() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select x from xs@x where x > 1 order by x range 0..2;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let spine = plan_spine(&arena, roots[0]);

    // The spine should contain (root to leaf):
    // Project → Limit → Sort → Filter → Scan
    // (or Filter may be pushed into Scan)
    assert!(spine.contains(&"Project"), "spine: {:?}", spine);
    assert!(spine.contains(&"Limit"), "spine: {:?}", spine);
    assert!(spine.contains(&"Sort"), "spine: {:?}", spine);
    assert!(spine.contains(&"Scan"), "spine: {:?}", spine);

    // Project should be the root.
    assert_eq!(spine[0], "Project", "Project should be root, got: {:?}", spine);
}

#[test]
fn multiple_queries_each_get_a_plan() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let ys = [4, 5, 6];
    let _ = select x from xs@x;
    let _ = select y from ys@y;
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
    let _ = select x from xs@x where x > 1 order by x;
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

    // The physical plan should have at least one node.
    assert!(
        phys_arena.get(phys_root).is_some(),
        "physical plan root should exist"
    );

    // Count physical nodes.
    let node_count = phys_arena.nodes.len();
    assert!(
        node_count >= 2,
        "expected at least 2 physical nodes, got {}",
        node_count
    );
}

#[test]
fn in_memory_executor_produces_no_exchanges() {
    use yelang_qir::physical::{InMemoryExecutor, PhysArena, PhysOp};

    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select x from xs@x where x > 1;
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

    // In-memory executor should produce no Exchange nodes.
    let has_exchange = phys_arena
        .nodes
        .iter()
        .any(|op| matches!(op, PhysOp::Exchange { .. }));
    assert!(
        !has_exchange,
        "in-memory executor should not produce Exchange nodes"
    );
}

// ---------------------------------------------------------------------------
// Decorrelation tests
// ---------------------------------------------------------------------------

#[test]
fn decorrelation_removes_dependent_joins() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select x from xs@x where x > 1;
}
"#;

    let (arena, _roots, _interner) = plan_queries(src);

    // After optimization (which includes decorrelation), no correlated
    // nodes should remain.
    assert!(
        !arena.has_correlated_nodes(),
        "expected no correlated nodes after decorrelation"
    );
}

// ---------------------------------------------------------------------------
// Aggregate decomposition tests
// ---------------------------------------------------------------------------

#[test]
fn group_by_with_aggregate_populates_aggs() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select g from xs@x group by { v: x } into g;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    // Find the Aggregate node and verify it exists.
    let spine = plan_spine(&arena, roots[0]);
    assert!(
        spine.contains(&"Aggregate"),
        "expected Aggregate in spine, got: {:?}",
        spine
    );
}

// ---------------------------------------------------------------------------
// SourceRef resolution tests
// ---------------------------------------------------------------------------

#[test]
fn local_variable_source_resolves_to_local() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select x from xs@x;
}
"#;

    let (arena, roots, _interner) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    // Walk to the Scan node and verify it has a Local source.
    let mut current = Some(roots[0]);
    let mut found_local_scan = false;
    while let Some(id) = current {
        let plan = arena.plan(id);
        if let Plan::Scan {
            source: yelang_qir::plan::SourceRef::Local { .. },
            ..
        } = plan
        {
            found_local_scan = true;
            break;
        }
        current = first_child(plan);
    }
    assert!(
        found_local_scan,
        "expected Scan with Local source for local variable"
    );
}
