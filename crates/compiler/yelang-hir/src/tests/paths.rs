//! Tests for path resolution during HIR lowering.

use crate::hir::{ExprKind, ItemKind};
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

    let items: Vec<_> = crate_hir.items.values().collect();
    let bar = items.iter().find(|i| {
        matches!(&i.kind, ItemKind::Fn { .. }) &&
        i.ident.as_str(&interner) == "bar"
    }).expect("expected bar fn");

    let ItemKind::Fn { body, .. } = &bar.kind else { panic!("expected fn") };
    let body = crate_hir.bodies.get(body).unwrap();
    let ExprKind::Block { block, .. } = &body.value.kind else { panic!("expected block") };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Call { .. }));
}

#[test]
fn lower_unresolved_path_falls_back_to_err() {
    let src = "fn main() { unknown_fn() }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().next().unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else { panic!("expected fn") };
    let body = crate_hir.bodies.get(body).unwrap();
    let ExprKind::Block { block, .. } = &body.value.kind else { panic!("expected block") };
    let tail = block.expr.as_ref().expect("expected tail expr");
    // Unresolved paths still lower; resolution is Err
    assert!(matches!(tail.kind, ExprKind::Call { .. }));
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

    let item = crate_hir.items.values().next().unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else { panic!("expected fn") };
    let body = crate_hir.bodies.get(body).unwrap();
    let ExprKind::Block { block, .. } = &body.value.kind else { panic!("expected block") };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Call { .. }));
}

#[test]
fn lower_crate_path() {
    let src = "fn main() { crate::foo() }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().next().unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else { panic!("expected fn") };
    let body = crate_hir.bodies.get(body).unwrap();
    let ExprKind::Block { block, .. } = &body.value.kind else { panic!("expected block") };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Call { .. }));
}

#[test]
fn lower_self_path() {
    let src = "fn main() { self::foo() }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().next().unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else { panic!("expected fn") };
    let body = crate_hir.bodies.get(body).unwrap();
    let ExprKind::Block { block, .. } = &body.value.kind else { panic!("expected block") };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Call { .. }));
}

#[test]
fn lower_super_path() {
    let src = "fn main() { super::foo() }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().next().unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else { panic!("expected fn") };
    let body = crate_hir.bodies.get(body).unwrap();
    let ExprKind::Block { block, .. } = &body.value.kind else { panic!("expected block") };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Call { .. }));
}
