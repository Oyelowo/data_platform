//! Extraction unit tests.
//!
//! Tests that query expressions and method chains are correctly extracted
//! into plan trees with the right operator structure.

use yelang_ast::Program;
use yelang_hir::lower_crate;
use yelang_interner::Interner;
use yelang_qir::plan::{Plan, PlanArena};
use yelang_qir::{lower_query, Optimizer};
use yelang_tycheck::tcx::TyCtxt;

/// Helper: run the full pipeline and return plan arena + roots.
fn plan_queries(src: &str) -> (PlanArena, Vec<yelang_qir::PlanId>, Interner) {
    let interner = Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &interner).expect("tokenize");
    let program = stream.parse::<Program>().expect("parse");

    let resolved = yelang_resolve::resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "resolution: {:?}", resolved.errors);

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
            let optimized = optimizer.optimize(root, &mut arena, &interner);
            roots.push(optimized);
        }
    }

    (arena, roots, interner)
}

/// Collect plan node names along the spine (root to deepest leaf).
fn spine(arena: &PlanArena, root: yelang_qir::PlanId) -> Vec<&'static str> {
    let mut result = Vec::new();
    let mut current = Some(root);
    while let Some(id) = current {
        let plan = arena.plan(id);
        result.push(name(plan));
        current = first_child(plan);
    }
    result
}

fn name(plan: &Plan) -> &'static str {
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
            Plan::Iterate { .. } => "Iterate",
            Plan::IterateScan { .. } => "IterateScan",
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
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Scalar projection
// ---------------------------------------------------------------------------

#[test]
fn scalar_projection_returns_constant() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select 1 from xs@x;
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    let s = spine(&arena, roots[0]);
    assert_eq!(s[0], "Project", "root must be Project, got {:?}", s);
    assert!(s.contains(&"Scan"), "spine: {:?}", s);
}

// ---------------------------------------------------------------------------
// Iteration with selectors
// ---------------------------------------------------------------------------

#[test]
fn star_selector_produces_map() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x;
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    let s = spine(&arena, roots[0]);
    assert!(s.contains(&"Scan"), "spine: {:?}", s);
}

// ---------------------------------------------------------------------------
// Filter (where clause)
// ---------------------------------------------------------------------------

#[test]
fn where_clause_produces_filter_or_pushdown() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1;
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    // Filter may be pushed into Scan.
    let s = spine(&arena, roots[0]);
    let has_filter = s.contains(&"Filter");
    let has_scan = s.contains(&"Scan");
    assert!(has_scan, "spine: {:?}", s);

    if !has_filter {
        // Verify Scan has a pushed-down filter.
        let mut cur = Some(roots[0]);
        let mut found = false;
        while let Some(id) = cur {
            if let Plan::Scan { filter: Some(_), .. } = arena.plan(id) {
                found = true;
                break;
            }
            cur = first_child(arena.plan(id));
        }
        assert!(found, "expected pushed-down filter, spine: {:?}", s);
    }
}

// ---------------------------------------------------------------------------
// Order by
// ---------------------------------------------------------------------------

#[test]
fn order_by_produces_sort() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x order by x desc;
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    let s = spine(&arena, roots[0]);
    assert!(s.contains(&"Sort"), "spine: {:?}", s);
}

// ---------------------------------------------------------------------------
// Range
// ---------------------------------------------------------------------------

#[test]
fn range_produces_limit() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x range 0..2;
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    let s = spine(&arena, roots[0]);
    assert!(s.contains(&"Limit"), "spine: {:?}", s);
}

// ---------------------------------------------------------------------------
// Group by
// ---------------------------------------------------------------------------

#[test]
fn group_by_produces_aggregate() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select g from xs@x group by { v: x } into g;
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    let s = spine(&arena, roots[0]);
    assert!(s.contains(&"Aggregate"), "spine: {:?}", s);
}

// ---------------------------------------------------------------------------
// Full pipeline: where + order + range
// ---------------------------------------------------------------------------

#[test]
fn full_pipeline_spine() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1 order by x range 0..2;
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    let s = spine(&arena, roots[0]);
    assert_eq!(s[0], "Project", "root: {:?}", s);
    assert!(s.contains(&"Limit"), "spine: {:?}", s);
    assert!(s.contains(&"Sort"), "spine: {:?}", s);
    assert!(s.contains(&"Scan"), "spine: {:?}", s);
}

// ---------------------------------------------------------------------------
// Multiple queries
// ---------------------------------------------------------------------------

#[test]
fn multiple_queries() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let ys = [4, 5, 6];
    let _ = select xs@a[*] from xs@x;
    let _ = select ys@a[*] from ys@y;
}
"#;
    let (_, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 2);
}

// ---------------------------------------------------------------------------
// SourceRef resolution
// ---------------------------------------------------------------------------

#[test]
fn local_variable_resolves_to_local() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@a[*] from xs@x;
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);

    let mut cur = Some(roots[0]);
    let mut found = false;
    while let Some(id) = cur {
        if let Plan::Scan {
            source: yelang_qir::plan::SourceRef::Local { .. },
            ..
        } = arena.plan(id)
        {
            found = true;
            break;
        }
        cur = first_child(arena.plan(id));
    }
    assert!(found, "expected Local source");
}

// ---------------------------------------------------------------------------
// No correlated nodes after optimization
// ---------------------------------------------------------------------------

#[test]
fn no_correlated_nodes_after_optimization() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1;
}
"#;
    let (arena, _, _) = plan_queries(src);
    assert!(!arena.has_correlated_nodes());
}
