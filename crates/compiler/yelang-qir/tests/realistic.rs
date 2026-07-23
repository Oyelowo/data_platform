//! Realistic query tests based on notes/syntax_grammar/ docs.
//!
//! Tests use the pattern from tycheck integration tests:
//! - Define structs for entity types
//! - Define functions returning collections
//! - Write queries using `from func@binder:Type` syntax

use yelang_ast::Program;
use yelang_hir::lower_crate;
use yelang_interner::Interner;
use yelang_qir::plan::{Plan, PlanArena};
use yelang_qir::{lower_query, Optimizer};
use yelang_tycheck::tcx::TyCtxt;

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

    for (qid, slot) in hir.queries.iter() {
        if slot.is_none() {
            continue;
        }
        if let Some(root) = lower_query(qid, &hir, None, &interner, &hir.lang_items, &mut arena) {
            roots.push(optimizer.optimize(root, &mut arena));
        }
    }

    (arena, roots, interner)
}

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
        Plan::Sort { .. } => "Sort",
        Plan::Limit { .. } => "Limit",
        Plan::Distinct { .. } => "Distinct",
        Plan::Union { .. } => "Union",
        Plan::Traverse { .. } => "Traverse",
        Plan::Window { .. } => "Window",
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
        | Plan::Window { input, .. }
        | Plan::Repeat { input, .. } => Some(*input),
        Plan::Join { left, .. }
        | Plan::DependentJoin { outer: left, .. }
        | Plan::GroupJoin { left, .. } => Some(*left),
        Plan::Union { inputs } => inputs.first().copied(),
        Plan::ScalarSubquery { plan, .. } | Plan::Exists { plan, .. } => Some(*plan),
        _ => None,
    }
}

fn has_node(arena: &PlanArena, root: yelang_qir::PlanId, target: &str) -> bool {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let plan = arena.plan(id);
        if name(plan) == target {
            return true;
        }
        stack.extend(yelang_qir::tree::children(plan));
    }
    false
}

// ===========================================================================
// From select.md: scalar projection
// ===========================================================================

#[test]
fn scalar_projection_from_table() {
    // select 1 from users@u:User → returns 1, NOT [1, 1, ...]
    let src = r#"
struct User { id: i32, name: String, age: i32 }
fn users() -> [User] { [] }
fn main() -> i32 {
    select 1 from users@u:User
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    let s = spine(&arena, roots[0]);
    assert_eq!(s[0], "Project", "root: {:?}", s);
    assert!(s.contains(&"Scan"), "spine: {:?}", s);
}

// ===========================================================================
// From select.md: array projection with [*]
// ===========================================================================

#[test]
fn array_projection_with_star() {
    // select users@u[*].id from users@u:User → [i32]
    let src = r#"
struct User { id: i32, name: String, age: i32 }
fn users() -> [User] { [] }
fn main() -> [i32] {
    select users@u[*].id from users@u:User
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    let s = spine(&arena, roots[0]);
    assert!(s.contains(&"Scan"), "spine: {:?}", s);
}

// ===========================================================================
// From select.md: filter with [where]
// ===========================================================================

#[test]
fn filter_with_where_selector() {
    // select users@u[where u.age > 18][*].name from users@u:User
    let src = r#"
struct User { id: i32, name: String, age: i32 }
fn users() -> [User] { [] }
fn main() -> [String] {
    select users@u[where u.age > 18][*].name from users@u:User
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    let s = spine(&arena, roots[0]);
    assert!(s.contains(&"Scan"), "spine: {:?}", s);
}

// ===========================================================================
// From select.md: object projection
// ===========================================================================

#[test]
fn object_projection() {
    // select users@u[*].{ id: u.id, name: u.name } from users@u:User
    let src = r#"
struct User { id: i32, name: String, age: i32 }
fn users() -> [User] { [] }
fn main() {
    let _ = select users@u[*].{ id: u.id, name: u.name } from users@u:User;
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    let s = spine(&arena, roots[0]);
    assert!(s.contains(&"Scan"), "spine: {:?}", s);
}

// ===========================================================================
// From select.md: index selector [0]
// ===========================================================================

#[test]
fn index_selector() {
    // select users[0].id from users@u:User → scalar
    let src = r#"
struct User { id: i32, name: String }
fn users() -> [User] { [] }
fn main() -> i32 {
    select users[0].id from users@u:User
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    let s = spine(&arena, roots[0]);
    assert!(s.contains(&"Scan"), "spine: {:?}", s);
}

// ===========================================================================
// From semantics.md: range does not change types
// ===========================================================================

#[test]
fn range_does_not_scalarize() {
    // select users@u[*].id from users@u:User range ..1
    // Still an array, just length <= 1
    let src = r#"
struct User { id: i32 }
fn users() -> [User] { [] }
fn main() -> [i32] {
    select users@u[*].id from users@u:User range ..1
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    let s = spine(&arena, roots[0]);
    assert!(s.contains(&"Limit"), "spine: {:?}", s);
    assert!(s.contains(&"Scan"), "spine: {:?}", s);
}

// ===========================================================================
// From select.md: order by
// ===========================================================================

#[test]
fn order_by_on_table() {
    let src = r#"
struct User { id: i32, name: String, age: i32 }
fn users() -> [User] { [] }
fn main() -> [i32] {
    select users@u[*].id from users@u:User order by u.age desc
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    assert!(has_node(&arena, roots[0], "Sort"), "expected Sort");
}

// ===========================================================================
// From select.md: group by
// ===========================================================================

#[test]
fn group_by_on_table() {
    let src = r#"
struct User { id: i32, city: String }
fn users() -> [User] { [] }
fn main() {
    let _ = select g from users@u:User group by { city: u.city } into g;
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    assert!(has_node(&arena, roots[0], "Aggregate"), "expected Aggregate");
}

// ===========================================================================
// From select.md: pipeline where (post-from filter)
// ===========================================================================

#[test]
fn pipeline_where() {
    let src = r#"
struct User { id: i32, age: i32 }
fn users() -> [User] { [] }
fn main() -> [i32] {
    select users@u[*].id from users@u:User where u.age > 30
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    // Filter may be pushed into Scan.
    let has_filter = has_node(&arena, roots[0], "Filter");
    let has_scan = has_node(&arena, roots[0], "Scan");
    assert!(has_scan, "expected Scan");
    if !has_filter {
        // Verify pushed-down filter in Scan.
        let mut stack = vec![roots[0]];
        let mut found = false;
        while let Some(id) = stack.pop() {
            if let Plan::Scan { filter: Some(_), .. } = arena.plan(id) {
                found = true;
                break;
            }
            stack.extend(yelang_qir::tree::children(arena.plan(id)));
        }
        assert!(found, "expected pushed-down filter");
    }
}

// ===========================================================================
// From select.md: from with inline filter
// ===========================================================================

#[test]
fn from_with_inline_filter() {
    // from (users@u:User where u.age > 50)
    let src = r#"
struct User { id: i32, age: i32 }
fn users() -> [User] { [] }
fn main() -> [i32] {
    select users@u[*].id from (users@u:User where u.age > 50)
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    assert!(has_node(&arena, roots[0], "Scan"), "expected Scan");
}

// ===========================================================================
// From select.md: multiple from roots (cross join)
// ===========================================================================

#[test]
fn multiple_from_roots() {
    let src = r#"
struct User { id: i32 }
struct Book { id: i32 }
fn users() -> [User] { [] }
fn books() -> [Book] { [] }
fn main() {
    let _ = select { u: users@u[*].id, b: books@b[*].id }
             from users@u:User, books@b:Book;
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    // Multiple roots produce a cross join.
    assert!(has_node(&arena, roots[0], "Join"), "expected Join for multi-root");
}

// ===========================================================================
// From select.md: links (graph traversal)
// ===========================================================================

#[test]
fn links_traversal() {
    let src = r#"
struct User { id: i32 }
struct Book { id: i32 }
struct UserWritesBook { _from: i32, _to: i32 }
fn users() -> [User] { [] }
fn main() {
    let _ = select users@u[*].{ id: u.id }
             from users@u:User
             links (users)->[writes@w:UserWritesBook]->(books@b:Book);
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    assert!(has_node(&arena, roots[0], "Traverse"), "expected Traverse for links");
}

// ===========================================================================
// Combined: where + order + range
// ===========================================================================

#[test]
fn combined_where_order_range() {
    let src = r#"
struct User { id: i32, age: i32, name: String }
fn users() -> [User] { [] }
fn main() -> [i32] {
    select users@u[*].id
    from users@u:User
    where u.age > 18
    order by u.name
    range 0..100
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

// ===========================================================================
// No correlated nodes after optimization
// ===========================================================================

#[test]
fn no_correlated_nodes() {
    let src = r#"
struct User { id: i32, age: i32 }
fn users() -> [User] { [] }
fn main() -> [i32] {
    select users@u[*].id from users@u:User where u.age > 18
}
"#;
    let (arena, _, _) = plan_queries(src);
    assert!(!arena.has_correlated_nodes());
}
