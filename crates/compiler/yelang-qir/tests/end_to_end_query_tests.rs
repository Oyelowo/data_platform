//! End-to-end tests for the query pipeline with a real `.ye` stdlib.
//!
//! These tests parse Yelang source (stdlib + user code), run resolution,
//! HIR lowering, type checking, and QIR lowering, and assert on the shape of
//! the resulting logical plan.

use std::path::PathBuf;

use yelang_hir::lowering::context::lower_crate;
use yelang_qir::backend::MemoryBackend;
use yelang_qir::exec::{MemoryExecutor, QueryExecutor, Value};
use yelang_qir::logical::plan::LogicalPlan;
use yelang_qir::pir::planner::plan_logical;
use yelang_resolve::resolve_crate;
use yelang_tycheck::type_check_crate;
use yelang_tycheck::tcx::TyCtxt;

/// Load the core stdlib source by concatenating the submodule `.ye` files.
///
/// This avoids needing a real module loader. Each file is emitted at the root
/// scope, so `pub use` re-exports in `lib.ye` are unnecessary for the prelude.
fn load_stdlib_source() -> String {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("../stdlib/core/src");
    let files = ["iter.ye", "aggregate.ye", "query.ye"];
    let mut out = String::new();
    for name in &files {
        let path = dir.join(name);
        out.push_str(&std::fs::read_to_string(&path).expect("failed to read stdlib file"));
        out.push('\n');
    }
    out
}

/// Parse Yelang source into an AST Program and Interner.
fn parse_program(src: &str) -> (yelang_ast::Program, yelang_interner::Interner) {
    let interner = yelang_interner::Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &interner).expect("tokenize");
    let program = stream.parse::<yelang_ast::Program>().expect("parse program");
    (program, interner)
}

/// Compile a snippet of user source together with the stdlib prelude.
///
/// Returns the `TyCtxt` after type checking (which owns the HIR crate) and the
/// lowered logical plan for the crate's `main` function body.
fn compile(src: &str) -> (TyCtxt, LogicalPlan) {
    let stdlib = load_stdlib_source();
    let full = format!("{}\n{}", stdlib, src);
    let (program, interner) = parse_program(&full);

    // Resolve names.
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "resolve errors: {:?}", resolved.errors);

    // Lower to HIR.
    let hir_crate = lower_crate(&program, &resolved, &interner);

    // Type-check.
    let mut tcx = TyCtxt::with_string_interner(hir_crate, interner.clone());
    let diagnostics = type_check_crate(&mut tcx);
    assert!(diagnostics.is_empty(), "type errors: {:?}", diagnostics);

    // Lower the main function body to QIR.
    // Find the `main` function DefId.
    let main_def = tcx
        .crate_hir()
        .items
        .iter_enumerated()
        .find_map(|(def_id, item)| {
            let item = item.as_ref()?;
            use yelang_hir::hir::item::ItemKind;
            match &item.kind {
                ItemKind::Fn { .. } => {
                    if tcx.resolve_symbol(item.ident.symbol) == Some("main") {
                        Some(def_id)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        })
        .expect("no main function");

    let body_id = tcx
        .crate_hir()
        .items
        .get(main_def)
        .and_then(|i| i.as_ref())
        .and_then(|i| match &i.kind {
            yelang_hir::hir::item::ItemKind::Fn { body, .. } => Some(*body),
            _ => None,
        })
        .expect("main has no body");

    let results = tcx
        .typeck_results
        .get(main_def)
        .expect("main should have typeck results");

    let plan = if let Some(query_id) = find_query_in_body(&tcx, body_id) {
        yelang_qir::lower_query(&tcx, body_id, query_id, results).expect("QIR lowering failed")
    } else {
        LogicalPlan::empty()
    };

    (tcx, plan)
}

/// Compile a snippet and execute the first query found in `main`.
fn run(src: &str) -> Value {
    let (_tcx, plan) = compile(src);
    let physical = plan_logical(&plan, &MemoryBackend::new()).expect("physical planning failed");
    MemoryExecutor::new().execute(&physical).expect("execution failed")
}

/// Find the first query expression inside a function body.
fn find_query_in_body(tcx: &TyCtxt, body_id: yelang_hir::ids::BodyId) -> Option<yelang_hir::ids::QueryId> {
    let body = tcx.crate_hir().body(body_id)?;
    find_query_expr(tcx, body.value)
}

fn find_query_expr(tcx: &TyCtxt, expr_id: yelang_hir::ids::ExprId) -> Option<yelang_hir::ids::QueryId> {
    let expr = tcx.crate_hir().expr(expr_id)?;
    match expr {
        yelang_hir::hir::expr::Expr::Query(query_id) => Some(*query_id),
        yelang_hir::hir::expr::Expr::Block { block } => {
            for stmt_id in &block.stmts {
                let stmt = tcx.crate_hir().stmt(*stmt_id)?;
                let stmt_expr = match stmt {
                    yelang_hir::hir::core::Stmt::Expr { expr } => Some(*expr),
                    yelang_hir::hir::core::Stmt::Let { init, .. } => *init,
                    _ => None,
                };
                if let Some(e) = stmt_expr {
                    if let Some(q) = find_query_expr(tcx, e) {
                        return Some(q);
                    }
                }
            }
            block.expr.and_then(|e| find_query_expr(tcx, e))
        }
        yelang_hir::hir::expr::Expr::Let { expr, .. } => find_query_expr(tcx, *expr),
        _ => None,
    }
}

#[test]
fn stdlib_parses_and_resolves() {
    let stdlib = load_stdlib_source();
    let (program, interner) = parse_program(&stdlib);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "stdlib resolve errors: {:?}", resolved.errors);
}

#[test]
fn stdlib_hirs_and_type_checks() {
    let src = r#"
fn main() {
    let users = [1, 2, 3];
    let _ = select u from users@u;
}
"#;
    let (_tcx, plan) = compile(src);
    assert!(plan.root.is_some(), "QIR lowering should produce a root operator");
}

#[test]
fn e2e_filter_and_map() {
    let src = r#"
fn main() {
    let users = [1, 2, 3, 4, 5];
    let _ = select u + 10 from users@u where u > 2;
}
"#;
    let result = run(src);
    let ints: Vec<i128> = result
        .try_into_array()
        .unwrap()
        .into_iter()
        .map(|v| match v {
            Value::Int(n) => n,
            _ => panic!("expected int, got {:?}", v),
        })
        .collect();
    assert_eq!(ints, vec![13, 14, 15]);
}

#[test]
fn e2e_order_by_and_range() {
    let src = r#"
fn main() {
    let users = [5, 1, 4, 2, 3];
    let _ = select u from users@u order by u asc range 1..3;
}
"#;
    let result = run(src);
    let ints: Vec<i128> = result
        .try_into_array()
        .unwrap()
        .into_iter()
        .map(|v| match v {
            Value::Int(n) => n,
            _ => panic!("expected int, got {:?}", v),
        })
        .collect();
    // Sorted: [1, 2, 3, 4, 5]; range 1..3 -> offset 1, limit 3 -> [2, 3, 4]
    assert_eq!(ints, vec![2, 3, 4]);
}
