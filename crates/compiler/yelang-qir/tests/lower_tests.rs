//! Lowering tests for `yelang-qir`.

use yelang_arena::DefId;
use yelang_hir::ids::BodyId;
use yelang_hir::{Crate, hir::query::{Query, QueryKind, SelectQuery}};
use yelang_tycheck::tcx::TyCtxt;
use yelang_tycheck::typeck_results::TypeckResults;

#[test]
fn lower_select_query_returns_plan() {
    let mut krate = Crate::new(DefId::new(1));

    // Build a minimal HIR: source expression is a literal array.
    let source_expr = krate.alloc_expr(
        yelang_hir::hir::expr::Expr::Array { exprs: vec![] },
        yelang_lexer::Span::default(),
    );
    let projection_expr = krate.alloc_expr(
        yelang_hir::hir::expr::Expr::Tuple { exprs: vec![] },
        yelang_lexer::Span::default(),
    );

    // Dummy binder pattern.
    let binder = krate.alloc_pat(
        yelang_hir::hir::pat::Pat::Wild,
        yelang_lexer::Span::default(),
    );

    let from = yelang_hir::hir::query::FromNode {
        source: source_expr,
        label: yelang_interner::Symbol::from(1),
        binder,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = Query {
        kind: QueryKind::Select(SelectQuery {
            projection: projection_expr,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: None,
            group_by: None,
            order_by: vec![],
            range: None,
        }),
    };

    let query_id = krate.alloc_query(query, yelang_lexer::Span::default());
    let tcx = TyCtxt::new(krate);
    let body_id = BodyId::default();
    let results = TypeckResults::new(DefId::new(1));

    let plan = yelang_qir::lower_query(&tcx, body_id, query_id, &results);
    assert!(plan.is_ok(), "lower_query should return Ok for a minimal select");

    let plan = plan.unwrap();
    assert!(plan.root.is_some(), "logical plan should have a root operator");
}
