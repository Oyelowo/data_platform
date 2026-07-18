//! Exhaustive tests for AST expression -> HIR expression lowering.

use crate::hir::core::{Expr, ItemKind, Stmt};
use crate::lowering::lower_crate;
use crate::tests::common::{parse_program, stub_resolved};

fn get_body_expr(crate_hir: &crate::Crate) -> &crate::hir::core::Expr {
    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else {
        panic!("expected fn")
    };
    let body = crate_hir.body(*body).unwrap();
    crate_hir.expr(body.value).unwrap()
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Lit { .. }));
}

#[test]
fn lower_string_literal() {
    let src = r#"fn main() { "hello" }"#;
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Lit { .. }));
}

#[test]
fn lower_bool_literal() {
    let src = "fn main() { true }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Lit { .. }));
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Path { .. }));
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Binary { .. }));
}

#[test]
fn lower_unary_expr() {
    let src = "fn main() { -x }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Unary { .. }));
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Call { .. }));
}

#[test]
fn lower_method_call_expr() {
    let src = "fn main() { x.foo(1) }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::MethodCall { .. }));
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Struct { .. }));
}

#[test]
fn lower_tuple_expr() {
    let src = "fn main() { (1, 2, 3) }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Tuple { .. }));
}

#[test]
fn lower_array_expr() {
    let src = "fn main() { [1, 2, 3] }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Array { .. }));
}

#[test]
fn lower_index_expr() {
    let src = "fn main() { arr[0] }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Index { .. }));
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Field { .. }));
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::If { .. }));
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Match { .. }));
}

#[test]
fn lower_loop_expr() {
    let src = "fn main() { loop { break } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Loop { .. }));
}

#[test]
fn lower_while_expr() {
    let src = "fn main() { while true { break } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    // While is desugared to Loop
    assert!(matches!(tail, Expr::Loop { .. }));
}

#[test]
fn lower_for_expr() {
    let src = "fn main() { for x in 0..10 { } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    // For is desugared to Loop
    assert!(matches!(tail, Expr::Loop { .. }));
}

#[test]
fn lower_break_expr() {
    let src = "fn main() { loop { break } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    let Expr::Loop { block: loop_block, .. } = tail else {
        panic!("expected loop")
    };
    // `break` without semicolon is the trailing expression of the loop block.
    let break_expr = crate_hir.expr(loop_block.expr.expect("expected tail expr in loop")).unwrap();
    assert!(matches!(break_expr, Expr::Break { .. }));
}

#[test]
fn lower_continue_expr() {
    let src = "fn main() { loop { continue } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    let Expr::Loop { block: loop_block, .. } = tail else {
        panic!("expected loop")
    };
    // `continue` without semicolon is the trailing expression of the loop block.
    let cont_expr = crate_hir.expr(loop_block.expr.expect("expected tail expr in loop")).unwrap();
    assert!(matches!(cont_expr, Expr::Continue { .. }));
}

#[test]
fn lower_return_expr() {
    let src = "fn main() { return 42 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Return { .. }));
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let stmt_id = block.stmts.first().expect("expected stmt");
    let stmt = crate_hir.stmt(*stmt_id).unwrap();
    assert!(matches!(stmt, Stmt::Let { .. }));
}

#[test]
fn lower_let_with_type() {
    let src = "fn main() { let x: i32 = 42; }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let stmt_id = block.stmts[0];
    let Stmt::Let { ty, .. } = crate_hir.stmt(stmt_id).unwrap() else {
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Closure { .. }));
}

#[test]
fn lower_closure_expr_with_return_type() {
    let src = "fn main() { |x: i32| -> i32 { x + 1 } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Closure { .. }));
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
    let body = crate_hir.body(*body).unwrap();
    let expr = crate_hir.expr(body.value).unwrap();
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Await { .. }));
}

#[test]
fn lower_try_expr() {
    let src = "fn main() { maybe()? }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Try { .. }));
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Range { .. }));
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Assign { .. }));
}

#[test]
fn lower_compound_assign_expr() {
    let src = "fn main() { x += 1 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::AssignOp { .. }));
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
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Block { .. }));
}

// ---------------------------------------------------------------------------
// Object, type tests, and ascriptions
// ---------------------------------------------------------------------------

#[test]
fn lower_object_literal_expr() {
    let src = "fn main() { { x: 1, y: 2 } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Object { .. }));
}

#[test]
fn lower_is_type_expr() {
    let src = "fn main() { 1 is i32 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::IsType { .. }));
}

#[test]
fn lower_type_ascription_expr() {
    let src = "fn main() { 1: i32 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::TypeAscription { .. }));
}

#[test]
fn lower_destructure_assign_expr() {
    let src = "fn main() { (a, b) = (1, 2) }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::DestructureAssign { .. }));
}

// ---------------------------------------------------------------------------
// Async / Gen
// ---------------------------------------------------------------------------

#[test]
fn lower_async_block_expr() {
    let src = "fn main() { async { 1 } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Async { .. }));
}

#[test]
fn lower_gen_block_expr() {
    let src = "fn main() { gen { 1 } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Gen { .. }));
}

// ---------------------------------------------------------------------------
// Document access and comprehensions
// ---------------------------------------------------------------------------

#[test]
fn lower_document_access_expr() {
    let src = "fn main() { doc.{name, age: 1} }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::DocumentAccess { .. }));
}

#[test]
fn lower_comprehension_expr() {
    let src = "fn main() { [x for x in xs] }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let expr = get_body_expr(&crate_hir);
    let Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.expr(block.expr.expect("expected tail expr")).unwrap();
    assert!(matches!(tail, Expr::Comprehension { .. }));
}

// ---------------------------------------------------------------------------
// Ternary
// ---------------------------------------------------------------------------

// NOTE: Ternary expressions (`a ? b : c`) are parsed in the AST but the syntax
// is not fully wired up in the tokenizer. HIR lowering desugars them to `If`.
// We test the desugaring via the `desugaring.rs` test suite instead.
