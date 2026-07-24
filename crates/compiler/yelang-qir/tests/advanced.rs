//! Advanced query tests based on notes/syntax_grammar/ docs.
//!
//! Tests complex query patterns: links with predicates, multi-path links,
//! mixed directions, nested selectors, group by with aggregates, multi-root
//! from, and complex projections.

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
            roots.push(optimizer.optimize(root, &mut arena, &interner));
        }
    }

    (arena, roots, interner)
}

fn has_node(arena: &PlanArena, root: yelang_qir::PlanId, target: &str) -> bool {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let plan = arena.plan(id);
        let name = match plan {
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
            Plan::Iterate { .. } => "Iterate",
            Plan::IterateScan { .. } => "IterateScan",
        };
        if name == target {
            return true;
        }
        stack.extend(yelang_qir::tree::children(plan));
    }
    false
}

fn count_nodes(arena: &PlanArena, root: yelang_qir::PlanId) -> usize {
    let mut count = 0;
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        count += 1;
        stack.extend(yelang_qir::tree::children(arena.plan(id)));
    }
    count
}

// ===========================================================================
// From select.md §1: links with edge predicate
// ===========================================================================

#[test]
fn links_with_edge_predicate() {
    let src = r#"
struct User { id: i32 }
struct Blog { id: i32 }
struct UserWritesBlog { _from: i32, _to: i32, published_date: i32 }
fn users() -> [User] { [] }
fn main() {
    let _ = select users@u[*].{ id: u.id }
             from users@u:User
             links (users)->[writes@w:UserWritesBlog where w.published_date > 2024]->(blogs@b:Blog);
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    assert!(has_node(&arena, roots[0], "Traverse"), "expected Traverse");
    assert!(has_node(&arena, roots[0], "Scan"), "expected Scan");
}

// ===========================================================================
// From select.md §1: links with target predicate
// ===========================================================================

#[test]
fn links_with_target_predicate() {
    let src = r#"
struct User { id: i32 }
struct Blog { id: i32, views: i32 }
struct UserWritesBlog { _from: i32, _to: i32 }
fn users() -> [User] { [] }
fn main() {
    let _ = select users@u[*].{ id: u.id }
             from users@u:User
             links (users)->[writes@w:UserWritesBlog]->(blogs@b:Blog where b.views > 100);
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    assert!(has_node(&arena, roots[0], "Traverse"), "expected Traverse");
}

// ===========================================================================
// From select.md §1: multi-segment link path
// ===========================================================================

#[test]
fn multi_segment_link_path() {
    let src = r#"
struct User { id: i32 }
struct Blog { id: i32 }
struct Comment { id: i32 }
struct UserWritesBlog { _from: i32, _to: i32 }
struct BlogHasComment { _from: i32, _to: i32 }
fn users() -> [User] { [] }
fn main() {
    let _ = select users@u[*].{ id: u.id }
             from users@u:User
             links (users)->[writes@w:UserWritesBlog]->(blogs@b:Blog)->[has@h:BlogHasComment]->(comments@c:Comment);
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    assert!(has_node(&arena, roots[0], "Traverse"), "expected Traverse");
}

// ===========================================================================
// From select.md §1: backward link direction
// ===========================================================================

#[test]
fn backward_link_direction() {
    let src = r#"
struct User { id: i32 }
struct Blog { id: i32 }
struct UserReadsBlog { _from: i32, _to: i32 }
fn users() -> [User] { [] }
fn main() {
    let _ = select users@u[*].{ id: u.id }
             from users@u:User
             links (users)<-[reads@r:UserReadsBlog]<-(blogs@b:Blog);
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    assert!(has_node(&arena, roots[0], "Traverse"), "expected Traverse");
}

// ===========================================================================
// From select.md §4.2: links with pipeline where
// NOTE: Full `.any()` on nested selectors needs type checker virtual field
// support. Simplified to test link + where clause.
// ===========================================================================

#[test]
fn links_with_pipeline_where() {
    let src = r#"
struct User { id: i32, age: i32 }
struct Blog { id: i32 }
struct UserWritesBlog { _from: i32, _to: i32 }
fn users() -> [User] { [] }
fn main() {
    let _ = select users@u[*].id
             from users@u:User
             links (users)->[writes@w:UserWritesBlog]->(blogs@b:Blog)
             where u.age > 30;
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    assert!(has_node(&arena, roots[0], "Traverse"), "expected Traverse");
    assert!(has_node(&arena, roots[0], "Scan"), "expected Scan");
}

// ===========================================================================
// From select.md §4.3: multi-root from (cross join)
// ===========================================================================

#[test]
fn multi_root_from_cross_join() {
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
    assert!(has_node(&arena, roots[0], "Join"), "expected Join for multi-root");
}

// ===========================================================================
// From semantics.md: group by with aggregate
// ===========================================================================

#[test]
fn group_by_with_count() {
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
// From semantics.md: range does not change types
// ===========================================================================

#[test]
fn range_preserves_array_type() {
    let src = r#"
struct User { id: i32 }
fn users() -> [User] { [] }
fn main() -> [i32] {
    select users@u[*].id from users@u:User range ..1
}
"#;
    let (arena, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 1);
    assert!(has_node(&arena, roots[0], "Limit"), "expected Limit");
    assert!(has_node(&arena, roots[0], "Scan"), "expected Scan");
}

// ===========================================================================
// Complex: where + order + range combined
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
    assert!(has_node(&arena, roots[0], "Limit"), "expected Limit");
    assert!(has_node(&arena, roots[0], "Sort"), "expected Sort");
    assert!(has_node(&arena, roots[0], "Scan"), "expected Scan");
}

// ===========================================================================
// Complex: inline filter in from
// ===========================================================================

#[test]
fn from_with_inline_filter() {
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
// No correlated nodes after optimization (invariant check)
// ===========================================================================

#[test]
fn no_correlated_nodes_after_optimization() {
    let src = r#"
struct User { id: i32, age: i32 }
fn users() -> [User] { [] }
fn main() -> [i32] {
    select users@u[*].id from users@u:User where u.age > 18
}
"#;
    let (arena, _, _) = plan_queries(src);
    assert!(!arena.has_correlated_nodes(), "no correlated nodes should remain");
}

// ===========================================================================
// Optimization reduces node count
// ===========================================================================

#[test]
fn optimization_produces_compact_plan() {
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
    let count = count_nodes(&arena, roots[0]);
    assert!(count <= 10, "plan should be compact, got {} nodes", count);
}

// ===========================================================================
// Multiple queries in one function
// ===========================================================================

#[test]
fn multiple_queries_in_one_function() {
    let src = r#"
struct User { id: i32 }
struct Book { id: i32 }
fn users() -> [User] { [] }
fn books() -> [Book] { [] }
fn main() {
    let _ = select users@u[*].id from users@u:User;
    let _ = select books@b[*].id from books@b:Book;
}
"#;
    let (_, roots, _) = plan_queries(src);
    assert_eq!(roots.len(), 2, "expected 2 query plans");
}
