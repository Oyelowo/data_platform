//! Exhaustive tests for AST expression -> HIR expression lowering.

use crate::hir::{ExprKind, ItemKind, StmtKind};
use crate::lowering::lower_crate;
use crate::tests::common::{parse_program, stub_resolved};

fn get_body_expr(crate_hir: &crate::Crate) -> &crate::hir::Expr {
    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else {
        panic!("expected fn")
    };
    let body = crate_hir.bodies.get(*body).unwrap();
    &body.value
}

// ---------------------------------------------------------------------------
// Literals
// ---------------------------------------------------------------------------

#[test]
fn lower_int_literal() {
    let src = "fn main() { 42 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Lit { .. }));
}

#[test]
fn lower_string_literal() {
    let src = r#"fn main() { "hello" }"#;
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Lit { .. }));
}

#[test]
fn lower_bool_literal() {
    let src = "fn main() { true }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Lit { .. }));
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

#[test]
fn lower_path_expr() {
    let src = "fn main() { x }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Path { .. }));
}

// ---------------------------------------------------------------------------
// Binary / Unary
// ---------------------------------------------------------------------------

#[test]
fn lower_binary_expr() {
    let src = "fn main() { 1 + 2 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Binary { .. }));
}

#[test]
fn lower_unary_expr() {
    let src = "fn main() { -x }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Unary { .. }));
}

// ---------------------------------------------------------------------------
// Calls
// ---------------------------------------------------------------------------

#[test]
fn lower_call_expr() {
    let src = "fn main() { foo(1, 2) }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Call { .. }));
}

#[test]
fn lower_method_call_expr() {
    let src = "fn main() { x.foo(1) }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::MethodCall { .. }));
}

// ---------------------------------------------------------------------------
// Struct / Tuple / Array
// ---------------------------------------------------------------------------

#[test]
fn lower_struct_literal_expr() {
    let src = "fn main() { Point { x: 1, y: 2 } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Struct { .. }));
}

#[test]
fn lower_tuple_expr() {
    let src = "fn main() { (1, 2, 3) }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Tuple { .. }));
}

#[test]
fn lower_array_expr() {
    let src = "fn main() { [1, 2, 3] }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Array { .. }));
}

#[test]
fn lower_index_expr() {
    let src = "fn main() { arr[0] }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Index { .. }));
}

// ---------------------------------------------------------------------------
// Field access
// ---------------------------------------------------------------------------

#[test]
fn lower_field_access_expr() {
    let src = "fn main() { p.x }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Field { .. }));
}

// ---------------------------------------------------------------------------
// Control flow
// ---------------------------------------------------------------------------

#[test]
fn lower_if_expr() {
    let src = "fn main() { if true { 1 } else { 2 } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::If { .. }));
}

#[test]
fn lower_match_expr() {
    let src = r#"
        fn main() {
            match 1 {
                1 => 2,
                _ => 3,
            }
        }
    "#;
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Match { .. }));
}

#[test]
fn lower_loop_expr() {
    let src = "fn main() { loop { break } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Loop { .. }));
}

#[test]
fn lower_while_expr() {
    let src = "fn main() { while true { break } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    // While is desugared to Loop
    assert!(matches!(tail.kind, ExprKind::Loop { .. }));
}

#[test]
fn lower_for_expr() {
    let src = "fn main() { for x in 0..10 { } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    // For is desugared to Loop
    assert!(matches!(tail.kind, ExprKind::Loop { .. }));
}

#[test]
fn lower_break_expr() {
    let src = "fn main() { loop { break } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    let ExprKind::Loop {
        block: loop_block, ..
    } = &tail.kind
    else {
        panic!("expected loop")
    };
    // `break` without semicolon is the trailing expression of the loop block.
    let break_expr = loop_block
        .expr
        .as_ref()
        .expect("expected tail expr in loop");
    assert!(matches!(break_expr.kind, ExprKind::Break { .. }));
}

#[test]
fn lower_continue_expr() {
    let src = "fn main() { loop { continue } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    let ExprKind::Loop {
        block: loop_block, ..
    } = &tail.kind
    else {
        panic!("expected loop")
    };
    // `continue` without semicolon is the trailing expression of the loop block.
    let cont_expr = loop_block
        .expr
        .as_ref()
        .expect("expected tail expr in loop");
    assert!(matches!(cont_expr.kind, ExprKind::Continue { .. }));
}

#[test]
fn lower_return_expr() {
    let src = "fn main() { return 42 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Return { .. }));
}

// ---------------------------------------------------------------------------
// Let bindings
// ---------------------------------------------------------------------------

#[test]
fn lower_let_stmt() {
    let src = "fn main() { let x = 42; }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let stmt = block.stmts.first().expect("expected stmt");
    assert!(matches!(stmt.kind, StmtKind::Let { .. }));
}

#[test]
fn lower_let_with_type() {
    let src = "fn main() { let x: i32 = 42; }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let StmtKind::Let { ty, .. } = &block.stmts[0].kind else {
        panic!("expected let")
    };
    assert!(ty.is_some());
}

// ---------------------------------------------------------------------------
// Closures
// ---------------------------------------------------------------------------

#[test]
fn lower_closure_expr() {
    let src = "fn main() { |x| x + 1 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Closure { .. }));
}

#[test]
fn lower_closure_expr_with_return_type() {
    let src = "fn main() { |x: i32| -> i32 { x + 1 } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Closure { .. }));
}

// ---------------------------------------------------------------------------
// Await / Try / Async
// ---------------------------------------------------------------------------

#[test]
fn lower_await_expr() {
    let src = "async fn main() { foo().await }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else {
        panic!("expected fn")
    };
    let body = crate_hir.bodies.get(*body).unwrap();
    let ExprKind::Block { block, .. } = &body.value.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    // Await is currently lowered as the base expression (simplified desugaring).
    assert!(matches!(tail.kind, ExprKind::Call { .. }));
}

#[test]
fn lower_try_expr() {
    let src = "fn main() { maybe()? }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    // Try is desugared to match
    assert!(matches!(tail.kind, ExprKind::Match { .. }));
}

// ---------------------------------------------------------------------------
// Range
// ---------------------------------------------------------------------------

#[test]
fn lower_range_expr() {
    let src = "fn main() { 0..10 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    // Range is desugared to a Call to a synthetic range constructor.
    assert!(matches!(tail.kind, ExprKind::Call { .. }));
}

// ---------------------------------------------------------------------------
// Assignment
// ---------------------------------------------------------------------------

#[test]
fn lower_assign_expr() {
    let src = "fn main() { x = 1 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Assign { .. }));
}

#[test]
fn lower_compound_assign_expr() {
    let src = "fn main() { x += 1 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    // Compound assign is desugared to binary + assign
    assert!(matches!(tail.kind, ExprKind::Assign { .. }));
}

// ---------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------

#[test]
fn lower_block_expr() {
    let src = "fn main() { { 1; 2 } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let ExprKind::Block { block, .. } = &expr.kind else {
        panic!("expected block")
    };
    let tail = block.expr.as_ref().expect("expected tail expr");
    assert!(matches!(tail.kind, ExprKind::Block { .. }));
}

// ---------------------------------------------------------------------------
// Ternary
// ---------------------------------------------------------------------------

// NOTE: Ternary expressions (`a ? b : c`) are parsed in the AST but the syntax
// is not fully wired up in the tokenizer. HIR lowering desugars them to `If`.
// We test the desugaring via the `desugaring.rs` test suite instead.
