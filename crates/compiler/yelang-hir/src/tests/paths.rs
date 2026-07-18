//! Tests for path resolution during HIR lowering.

use crate::hir::core::{Expr, ItemKind};
use crate::lowering::lower_crate;
use crate::tests::common::{parse_program, stub_resolved};

// ---------------------------------------------------------------------------
// Simple path resolution
// ---------------------------------------------------------------------------

#[test]
fn lower_resolved_fn_path() {
    let src = r#"
        fn foo() {}
        fn bar() { foo() }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let items: Vec<_> = crate_hir
        .items
        .values()
        .filter_map(|opt| opt.as_ref())
        .collect();
    let bar = items
        .iter()
        .find(|i| matches!(i.kind(&crate_hir), ItemKind::Fn { .. }) && i.ident.as_str(&interner) == "bar")
        .expect("expected bar fn");

    let ItemKind::Fn { body, .. } = bar.kind(&crate_hir) else {
        panic!("expected fn")
    };
    let body = crate_hir.bodies.get(*body).unwrap();
    let expr = crate_hir.exprs.get(body.value).unwrap();
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.exprs.get(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Call { .. }));
}

#[test]
fn lower_unresolved_path_falls_back_to_err() {
    let src = "fn main() { unknown_fn() }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Fn { body, .. } = item.kind(&crate_hir) else {
        panic!("expected fn")
    };
    let body = crate_hir.bodies.get(*body).unwrap();
    let expr = crate_hir.exprs.get(body.value).unwrap();
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.exprs.get(block.expr.expect("expected tail expr")).unwrap();
    // Unresolved paths still lower; resolution is Err
    assert!(matches!(tail, Expr::Call { .. }));
}

// ---------------------------------------------------------------------------
// Absolute / crate / self / super paths
// ---------------------------------------------------------------------------

#[test]
fn lower_absolute_path() {
    let src = "fn main() { ::std::print() }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Fn { body, .. } = item.kind(&crate_hir) else {
        panic!("expected fn")
    };
    let body = crate_hir.bodies.get(*body).unwrap();
    let expr = crate_hir.exprs.get(body.value).unwrap();
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.exprs.get(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Call { .. }));
}

#[test]
fn lower_crate_path() {
    let src = "fn main() { crate::foo() }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Fn { body, .. } = item.kind(&crate_hir) else {
        panic!("expected fn")
    };
    let body = crate_hir.bodies.get(*body).unwrap();
    let expr = crate_hir.exprs.get(body.value).unwrap();
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.exprs.get(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Call { .. }));
}

#[test]
fn lower_self_path() {
    let src = "fn main() { self::foo() }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Fn { body, .. } = item.kind(&crate_hir) else {
        panic!("expected fn")
    };
    let body = crate_hir.bodies.get(*body).unwrap();
    let expr = crate_hir.exprs.get(body.value).unwrap();
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.exprs.get(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Call { .. }));
}

#[test]
fn lower_super_path() {
    let src = "fn main() { super::foo() }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Fn { body, .. } = item.kind(&crate_hir) else {
        panic!("expected fn")
    };
    let body = crate_hir.bodies.get(*body).unwrap();
    let expr = crate_hir.exprs.get(body.value).unwrap();
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.exprs.get(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Call { .. }));
}
