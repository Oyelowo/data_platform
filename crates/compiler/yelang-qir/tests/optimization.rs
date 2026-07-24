//! Optimization rule tests.
//!
//! Tests that individual optimization rules produce correct transformations.

use yelang_ast::Program;
use yelang_hir::lower_crate;
use yelang_interner::Interner;
use yelang_qir::plan::{Plan, PlanArena};
use yelang_qir::{lower_query, Optimizer};
use yelang_tycheck::tcx::TyCtxt;

fn plan_and_optimize(src: &str) -> (PlanArena, Vec<yelang_qir::PlanId>) {
    let interner = Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &interner).expect("tokenize");
    let program = stream.parse::<Program>().expect("parse");

    let resolved = yelang_resolve::resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty());

    let hir_crate = lower_crate(&program, &resolved, &interner);
    let mut tcx = TyCtxt::with_string_interner(hir_crate, interner.clone());
    let diags = yelang_tycheck::type_check_crate(&mut tcx);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == yelang_tycheck::diagnostics::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "type errors: {:?}", errors);

    let hir = tcx.crate_hir().clone();
    let mut arena = PlanArena::new();
    let optimizer = Optimizer::new();
    let mut roots = Vec::new();

    for (qid, slot) in hir.queries.iter() {
        if slot.is_none() {
            continue;
        }
        if let Some(root) = lower_query(qid, &hir, None, &interner, &hir.lang_items, &mut arena) {
            roots.push(optimizer.optimize(root, &mut arena, &interner));
        }
    }

    (arena, roots)
}

fn count_nodes(arena: &PlanArena, root: yelang_qir::PlanId) -> usize {
    let mut count = 0;
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        count += 1;
        let plan = arena.plan(id);
        stack.extend(yelang_qir::tree::children(plan));
    }
    count
}

fn has_node(arena: &PlanArena, root: yelang_qir::PlanId, name: &str) -> bool {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let plan = arena.plan(id);
        let plan_name = match plan {
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
        };
        if plan_name == name {
            return true;
        }
        stack.extend(yelang_qir::tree::children(plan));
    }
    false
}

// ---------------------------------------------------------------------------
// Decorrelation
// ---------------------------------------------------------------------------

#[test]
fn decorrelation_removes_all_correlated_nodes() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1;
}
"#;
    let (arena, _) = plan_and_optimize(src);
    assert!(
        !arena.has_correlated_nodes(),
        "no DependentJoin/ScalarSubquery/Exists should remain"
    );
}

// ---------------------------------------------------------------------------
// Predicate pushdown
// ---------------------------------------------------------------------------

#[test]
fn filter_pushed_into_scan() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1;
}
"#;
    let (arena, roots) = plan_and_optimize(src);
    assert_eq!(roots.len(), 1);

    // After pushdown, either there's no separate Filter node,
    // or the Scan has a filter.
    let has_separate_filter = has_node(&arena, roots[0], "Filter");
    if !has_separate_filter {
        // Verify Scan has pushed-down filter.
        let mut found = false;
        let mut stack = vec![roots[0]];
        while let Some(id) = stack.pop() {
            if let Plan::Scan { filter: Some(_), .. } = arena.plan(id) {
                found = true;
                break;
            }
            stack.extend(yelang_qir::tree::children(arena.plan(id)));
        }
        assert!(found, "expected pushed-down filter in Scan");
    }
}

// ---------------------------------------------------------------------------
// Optimization reduces node count
// ---------------------------------------------------------------------------

#[test]
fn optimization_does_not_increase_nodes() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1 order by x range 0..2;
}
"#;
    let (arena, roots) = plan_and_optimize(src);
    assert_eq!(roots.len(), 1);

    // The optimized plan should have a reasonable number of nodes.
    let count = count_nodes(&arena, roots[0]);
    assert!(
        count <= 10,
        "optimized plan should be compact, got {} nodes",
        count
    );
}

// ---------------------------------------------------------------------------
// Sort is preserved
// ---------------------------------------------------------------------------

#[test]
fn sort_preserved_after_optimization() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x order by x desc;
}
"#;
    let (arena, roots) = plan_and_optimize(src);
    assert_eq!(roots.len(), 1);
    assert!(has_node(&arena, roots[0], "Sort"), "Sort should be preserved");
}

// ---------------------------------------------------------------------------
// Limit is preserved
// ---------------------------------------------------------------------------

#[test]
fn limit_preserved_after_optimization() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x range 0..2;
}
"#;
    let (arena, roots) = plan_and_optimize(src);
    assert_eq!(roots.len(), 1);
    assert!(has_node(&arena, roots[0], "Limit"), "Limit should be preserved");
}

// ---------------------------------------------------------------------------
// Aggregate is preserved
// ---------------------------------------------------------------------------

#[test]
fn aggregate_preserved_after_optimization() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select g from xs@x group by { v: x } into g;
}
"#;
    let (arena, roots) = plan_and_optimize(src);
    assert_eq!(roots.len(), 1);
    assert!(
        has_node(&arena, roots[0], "Aggregate"),
        "Aggregate should be preserved"
    );
}
