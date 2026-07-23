//! Physical planning tests.
//!
//! Tests that the logical → physical lowering produces correct PhysOp
//! trees with appropriate algorithm choices and Exchange insertion.

use yelang_ast::Program;
use yelang_hir::lower_crate;
use yelang_interner::Interner;
use yelang_qir::physical::{InMemoryExecutor, PhysArena, PhysOp};
use yelang_qir::plan::PlanArena;
use yelang_qir::{extract_query, Optimizer};
use yelang_tycheck::tcx::TyCtxt;

fn plan_optimize_and_physical(
    src: &str,
) -> (PlanArena, PhysArena, Vec<yelang_qir::physical::PhysId>) {
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
    let mut plan_arena = PlanArena::new();
    let optimizer = Optimizer::new();
    let executor = InMemoryExecutor;
    let mut phys_arena = PhysArena::new();
    let mut phys_roots = Vec::new();

    for (qid, slot) in hir.queries.iter() {
        if slot.is_none() {
            continue;
        }
        if let Some(root) = extract_query(qid, &hir, &interner, &hir.lang_items, &mut plan_arena) {
            let optimized = optimizer.optimize(root, &mut plan_arena);
            let phys_root = yelang_qir::physical::planner::plan_physical(
                optimized,
                &plan_arena,
                &executor,
                &mut phys_arena,
            );
            phys_roots.push(phys_root);
        }
    }

    (plan_arena, phys_arena, phys_roots)
}

fn phys_node_count(phys: &PhysArena, root: yelang_qir::physical::PhysId) -> usize {
    let mut count = 0;
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        count += 1;
        if let Some(op) = phys.get(id) {
            match op {
                PhysOp::Scan { .. }
                | PhysOp::Constant { .. }
                | PhysOp::Empty { .. }
                | PhysOp::Extension { .. } => {}
                PhysOp::Filter { input, .. }
                | PhysOp::Project { input, .. }
                | PhysOp::Map { input, .. }
                | PhysOp::Aggregate { input, .. }
                | PhysOp::Sort { input, .. }
                | PhysOp::Limit { input, .. }
                | PhysOp::Distinct { input, .. }
                | PhysOp::Traverse { input, .. }
                | PhysOp::Repeat { input, .. }
                | PhysOp::Exchange { input, .. } => stack.push(*input),
                PhysOp::Join { left, right, .. } => {
                    stack.push(*left);
                    stack.push(*right);
                }
                PhysOp::Union { inputs } => stack.extend(inputs),
            }
        }
    }
    count
}

fn has_phys_op(phys: &PhysArena, root: yelang_qir::physical::PhysId, name: &str) -> bool {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if let Some(op) = phys.get(id) {
            let op_name = match op {
                PhysOp::Scan { .. } => "Scan",
                PhysOp::Filter { .. } => "Filter",
                PhysOp::Project { .. } => "Project",
                PhysOp::Map { .. } => "Map",
                PhysOp::Join { .. } => "Join",
                PhysOp::Aggregate { .. } => "Aggregate",
                PhysOp::Sort { .. } => "Sort",
                PhysOp::Limit { .. } => "Limit",
                PhysOp::Distinct { .. } => "Distinct",
                PhysOp::Union { .. } => "Union",
                PhysOp::Traverse { .. } => "Traverse",
                PhysOp::Exchange { .. } => "Exchange",
                PhysOp::Repeat { .. } => "Repeat",
                PhysOp::Extension { .. } => "Extension",
                PhysOp::Constant { .. } => "Constant",
                PhysOp::Empty { .. } => "Empty",
            };
            if op_name == name {
                return true;
            }
            match op {
                PhysOp::Filter { input, .. }
                | PhysOp::Project { input, .. }
                | PhysOp::Map { input, .. }
                | PhysOp::Aggregate { input, .. }
                | PhysOp::Sort { input, .. }
                | PhysOp::Limit { input, .. }
                | PhysOp::Distinct { input, .. }
                | PhysOp::Traverse { input, .. }
                | PhysOp::Repeat { input, .. }
                | PhysOp::Exchange { input, .. } => stack.push(*input),
                PhysOp::Join { left, right, .. } => {
                    stack.push(*left);
                    stack.push(*right);
                }
                PhysOp::Union { inputs } => stack.extend(inputs),
                _ => {}
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Basic physical planning
// ---------------------------------------------------------------------------

#[test]
fn physical_plan_produces_nodes() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1 order by x;
}
"#;
    let (_, phys, roots) = plan_optimize_and_physical(src);
    assert_eq!(roots.len(), 1);
    assert!(phys.get(roots[0]).is_some());
    assert!(phys_node_count(&phys, roots[0]) >= 2);
}

// ---------------------------------------------------------------------------
// In-memory executor: no Exchange nodes
// ---------------------------------------------------------------------------

#[test]
fn in_memory_no_exchanges() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1;
}
"#;
    let (_, phys, roots) = plan_optimize_and_physical(src);
    assert_eq!(roots.len(), 1);
    assert!(
        !has_phys_op(&phys, roots[0], "Exchange"),
        "in-memory should not produce Exchange"
    );
}

// ---------------------------------------------------------------------------
// Scan is always present
// ---------------------------------------------------------------------------

#[test]
fn physical_plan_has_scan() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x;
}
"#;
    let (_, phys, roots) = plan_optimize_and_physical(src);
    assert_eq!(roots.len(), 1);
    assert!(has_phys_op(&phys, roots[0], "Scan"));
}

// ---------------------------------------------------------------------------
// Sort is preserved in physical plan
// ---------------------------------------------------------------------------

#[test]
fn physical_plan_preserves_sort() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x order by x desc;
}
"#;
    let (_, phys, roots) = plan_optimize_and_physical(src);
    assert_eq!(roots.len(), 1);
    assert!(has_phys_op(&phys, roots[0], "Sort"));
}

// ---------------------------------------------------------------------------
// Aggregate is preserved in physical plan
// ---------------------------------------------------------------------------

#[test]
fn physical_plan_preserves_aggregate() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select g from xs@x group by { v: x } into g;
}
"#;
    let (_, phys, roots) = plan_optimize_and_physical(src);
    assert_eq!(roots.len(), 1);
    assert!(has_phys_op(&phys, roots[0], "Aggregate"));
}

// ---------------------------------------------------------------------------
// Physical plan is compact
// ---------------------------------------------------------------------------

#[test]
fn physical_plan_is_compact() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > 1 order by x range 0..2;
}
"#;
    let (_, phys, roots) = plan_optimize_and_physical(src);
    assert_eq!(roots.len(), 1);
    let count = phys_node_count(&phys, roots[0]);
    assert!(count <= 10, "physical plan should be compact, got {}", count);
}
