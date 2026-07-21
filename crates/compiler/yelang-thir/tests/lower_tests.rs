//! Integration tests for HIR → THIR lowering.

use yelang_hir::hir::item::ItemKind;
use yelang_hir::ids::BodyId;
use yelang_hir::Crate as HirCrate;
use yelang_interner::Interner;
use yelang_lexer::FileId;
use yelang_tycheck::TypeckResults;
use yelang_thir::{LoweringContext, ThirExpr, ThirPat, ThirStmt};

fn setup(src: &str) -> (HirCrate, TypeckResults, Interner, BodyId) {
    let mut interner = Interner::new();
    let program =
        yelang_ast::parse::parse_program_strict_with_file_id(src, &mut interner, FileId::default())
            .expect("parse program");
    let resolved = yelang_resolve::resolve_crate(&program, &interner);
    let hir = yelang_hir::lower_crate(&program, &resolved, &interner);

    let mut tcx = yelang_tycheck::tcx::TyCtxt::new(hir.clone());
    let _diagnostics = yelang_tycheck::type_check_crate(&mut tcx);

    let (def_id, body_id) = find_main(&tcx.crate_hir(), &interner);
    let typeck_results = tcx
        .typeck_results
        .get(def_id)
        .cloned()
        .expect("typeck results for main");

    (hir, typeck_results, interner, body_id)
}

fn find_main(hir: &HirCrate, interner: &Interner) -> (yelang_arena::DefId, BodyId) {
    for (def_id, opt_item) in hir.items.iter_enumerated() {
        let Some(item) = opt_item else { continue };
        if interner.resolve(&item.ident.symbol) != "main" {
            continue;
        }
        if let ItemKind::Fn { body, .. } = &item.kind {
            return (def_id, *body);
        }
    }
    panic!("no main function found");
}

fn lower_main_body(src: &str) -> (yelang_thir::ThirBodyId, LoweringContext<'static>) {
    let (hir, typeck_results, interner, body_id) = setup(src);
    // Leak test inputs so the context can hold 'static references. This is
    // fine for small unit tests.
    let hir = Box::leak(Box::new(hir));
    let typeck_results = Box::leak(Box::new(typeck_results));
    let interner = Box::leak(Box::new(interner));
    let mut ctx = LoweringContext::new(hir, typeck_results, &hir.lang_items, interner);
    let thir_body_id = ctx.lower_body(body_id).expect("lower main body");
    (thir_body_id, ctx)
}

#[test]
fn lower_literal() {
    let src = r#"
fn main() {
    let _ = 42;
}
"#;
    let (body_id, ctx) = lower_main_body(src);
    let body = ctx.body(body_id).expect("body");
    let block = ctx.exprs.get(body.value).expect("value expr");
    let ThirExpr::Block { stmts, tail: _ } = block else {
        panic!("expected block, got {:?}", block);
    };
    assert!(!stmts.is_empty(), "expected at least one statement");
    let let_stmt = ctx.stmts.get(stmts[0]).expect("stmt");
    let ThirStmt::Let { init, .. } = let_stmt else {
        panic!("expected let statement");
    };
    let init_expr = ctx.exprs.get(init.expect("init")).expect("init expr");
    assert!(
        matches!(init_expr, ThirExpr::Literal(yelang_lexer::Literal::Int(_))),
        "expected integer literal, got {:?}",
        init_expr
    );
}

#[test]
fn lower_method_call_becomes_function_call() {
    let src = r#"
struct Foo { x: i32 }
impl Foo {
    fn bar(self) -> i32 { self.x }
}
fn main() {
    let _ = Foo { x: 1 }.bar();
}
"#;
    let (body_id, ctx) = lower_main_body(src);
    let body = ctx.body(body_id).expect("body");
    let block = ctx.exprs.get(body.value).expect("value expr");
    let ThirExpr::Block { stmts, .. } = block else {
        panic!("expected block");
    };
    let ThirStmt::Let { init, .. } = ctx.stmts.get(stmts[0]).expect("stmt") else {
        panic!("expected let");
    };
    let call = ctx.exprs.get(init.expect("init")).expect("call expr");
    let ThirExpr::Call { func, args } = call else {
        panic!("expected call, got {:?}", call);
    };
    let func_expr = ctx.exprs.get(*func).expect("func");
    assert!(
        matches!(func_expr, ThirExpr::Var(_)),
        "expected method resolved to a function var, got {:?}",
        func_expr
    );
    assert_eq!(args.len(), 1, "expected receiver as explicit self argument");
    let receiver = ctx.exprs.get(args[0]).expect("receiver");
    assert!(
        matches!(receiver, ThirExpr::Struct { .. }),
        "expected struct receiver, got {:?}",
        receiver
    );
}

#[test]
fn lower_query_expr_preserves_query_id() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select x from xs@x;
}
"#;
    let (body_id, ctx) = lower_main_body(src);
    let body = ctx.body(body_id).expect("body");
    let block = ctx.exprs.get(body.value).expect("value expr");
    let ThirExpr::Block { stmts, .. } = block else {
        panic!("expected block");
    };
    let ThirStmt::Let { init, .. } = ctx.stmts.get(stmts[1]).expect("stmt") else {
        panic!("expected second let");
    };
    let query = ctx.exprs.get(init.expect("init")).expect("query expr");
    assert!(
        matches!(query, ThirExpr::Query(_)),
        "expected ThirExpr::Query, got {:?}",
        query
    );
}

#[test]
fn lower_intrinsic_preserves_name_and_args() {
    let src = r#"
fn main() {
    let _ = @identity(1, 2);
}
"#;
    let (body_id, ctx) = lower_main_body(src);
    let body = ctx.body(body_id).expect("body");
    let block = ctx.exprs.get(body.value).expect("value expr");
    let ThirExpr::Block { stmts, .. } = block else {
        panic!("expected block");
    };
    let ThirStmt::Let { init, .. } = ctx.stmts.get(stmts[0]).expect("stmt") else {
        panic!("expected let");
    };
    let intrinsic = ctx.exprs.get(init.expect("init")).expect("intrinsic expr");
    let ThirExpr::Intrinsic { name, args } = intrinsic else {
        panic!("expected intrinsic, got {:?}", intrinsic);
    };
    assert_eq!(ctx.resolve_symbol(*name), "identity");
    assert_eq!(args.len(), 2);
}

#[test]
fn lower_closure_has_nested_body() {
    let src = r#"
fn main() {
    let _ = |x| x + 1;
}
"#;
    let (body_id, ctx) = lower_main_body(src);
    let body = ctx.body(body_id).expect("body");
    let block = ctx.exprs.get(body.value).expect("value expr");
    let ThirExpr::Block { stmts, .. } = block else {
        panic!("expected block");
    };
    let ThirStmt::Let { init, .. } = ctx.stmts.get(stmts[0]).expect("stmt") else {
        panic!("expected let");
    };
    let closure = ctx.exprs.get(init.expect("init")).expect("closure expr");
    let ThirExpr::Closure { params, body } = closure else {
        panic!("expected closure, got {:?}", closure);
    };
    assert_eq!(params.len(), 1);
    let nested = ctx.body(*body).expect("nested body");
    let nested_expr = ctx.exprs.get(nested.value).expect("nested value");
    assert!(
        matches!(nested_expr, ThirExpr::Binary { .. }),
        "expected binary body, got {:?}",
        nested_expr
    );
}

#[test]
fn lower_block_with_locals() {
    let src = r#"
fn main() {
    let x = 1;
    let y = x + 2;
}
"#;
    let (body_id, ctx) = lower_main_body(src);
    let body = ctx.body(body_id).expect("body");
    let block = ctx.exprs.get(body.value).expect("value expr");
    let ThirExpr::Block { stmts, .. } = block else {
        panic!("expected block");
    };
    assert_eq!(stmts.len(), 2);
    let ThirStmt::Let { init, .. } = ctx.stmts.get(stmts[1]).expect("stmt") else {
        panic!("expected second let");
    };
    let add = ctx.exprs.get(init.expect("init")).expect("add expr");
    let ThirExpr::Binary { left, right, .. } = add else {
        panic!("expected binary");
    };
    assert!(
        matches!(ctx.exprs.get(*left).expect("left"), ThirExpr::Local(_)),
        "expected local x"
    );
    assert!(
        matches!(ctx.exprs.get(*right).expect("right"), ThirExpr::Literal(_)),
        "expected literal 2"
    );
}

#[test]
fn lower_match_arm_patterns() {
    let src = r#"
fn main() {
    let _ = match 1 {
        1 => 2,
        _ => 3,
    };
}
"#;
    let (body_id, ctx) = lower_main_body(src);
    let body = ctx.body(body_id).expect("body");
    let block = ctx.exprs.get(body.value).expect("value expr");
    let ThirExpr::Block { stmts, .. } = block else {
        panic!("expected block");
    };
    let ThirStmt::Let { init, .. } = ctx.stmts.get(stmts[0]).expect("stmt") else {
        panic!("expected let");
    };
    let match_expr = ctx.exprs.get(init.expect("init")).expect("match expr");
    let ThirExpr::Match { arms, .. } = match_expr else {
        panic!("expected match, got {:?}", match_expr);
    };
    assert_eq!(arms.len(), 2);
    let arm0_pat = ctx.pats.get(arms[0].pat).expect("pat 0");
    assert!(matches!(arm0_pat, ThirPat::Lit { .. }), "expected literal pat");
    let arm1_pat = ctx.pats.get(arms[1].pat).expect("pat 1");
    assert!(matches!(arm1_pat, ThirPat::Wild), "expected wildcard pat");
}
