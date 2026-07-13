use super::harness::*;
use crate::ExprKind;

#[test]
fn test_expr_precedence() {
    // These ensure your Codegen logic for parentheses is correct
    assert_round_trip::<Expr>("1 + 2 * 3;");
    assert_round_trip::<Expr>("(1 + 2) * 3;");
    assert_round_trip::<Expr>("a && b || c;");
    assert_round_trip::<Expr>("a && (b || c);");
}
#[test]
fn test_expr_associativity_left() {
    // Left associative: 1 - 2 - 3 should parse as (1 - 2) - 3
    assert_round_trip::<Expr>("1 - 2 - 3;");
}
#[test]
fn test_expr_associativity_right() {
    // Right associative: a = b = c should parse as a = (b = c)
    // Assuming assignment is right associative
    assert_round_trip::<Expr>("a = b = c;");
}
#[test]
fn test_expr_unary_vs_binary() {
    assert_round_trip::<Expr>("-a * b;");
    assert_round_trip::<Expr>("-(a * b);");
}
#[test]
fn test_expr_comparison_chains() {
    assert_round_trip::<Expr>("a < b == c > d;");
}
#[test]
fn test_literals() {
    assert_round_trip::<Stmt>("42;");
    assert_round_trip::<Stmt>("3.14;");
    assert_round_trip::<Stmt>("'hello';");
    assert_round_trip::<Stmt>("true;");
    assert_round_trip::<Stmt>("false;");

    // `null` is reserved and rejected at parse time.
    let mut interner = Interner::new();
    let mut token_stream =
        TokenKind::tokenize("null;", &mut interner).expect("Tokenization failed");
    assert!(
        token_stream.parse::<Stmt>().is_err(),
        "expected `null` to be a parse error"
    );
}
#[test]
fn test_collections() {
    // Array
    assert_round_trip::<Stmt>("[1, 2, 3];");
    // Object
    assert_round_trip::<Stmt>("{ a: 1, b: 2 };");
    // Tuple
    assert_round_trip::<Stmt>("(1, 2, 3);");
}

#[test]
fn test_nested_object_literal_parses_as_object_not_block() {
    let stmt = parse_stmt("{ stats: { city: user.city } };");
    let crate::StmtKind::TermExpr(expr) = &stmt.kind else {
        panic!(
            "expected terminated expression statement, got {:?}",
            stmt.kind
        );
    };

    let ExprKind::Object(outer) = &expr.kind else {
        panic!(
            "expected outer expression to parse as object, got {:?}",
            expr.kind
        );
    };

    assert_eq!(outer.fields.len(), 1);

    let inner = outer.fields[0].value();
    assert!(
        matches!(inner.kind, ExprKind::Object(_)),
        "expected nested field value to parse as object, got {:?}",
        inner.kind
    );
}

#[test]
fn test_accessors() {
    // Member access
    assert_round_trip::<Stmt>("user.name;");
    // Index access
    assert_round_trip::<Stmt>("arr[0];");
    // Range index
    assert_round_trip::<Stmt>("arr[1..5];");
    // Method call
    assert_round_trip::<Stmt>("user.name.to_upper();");
}

#[test]
fn path_collection_selector_surface_snapshot() {
    assert_snapshot(
        "path_collection_selector_surface",
        "users@u[1..=limit][group by { city: u.city, team: u.team }][enumerate][distinct][distinct by u.city];",
    );
}

#[test]
fn record_destructuring_closure_param_surface_snapshot() {
    assert_snapshot(
        "record_destructuring_closure_param_surface",
        "items.map(|{ index, value: user, .. }| user.id);",
    );
}

#[test]
fn record_destructuring_let_surface_snapshot() {
    assert_snapshot(
        "record_destructuring_let_surface",
        "let { index, value: user, .. } = row;",
    );
}

#[test]
fn record_destructuring_assignment_surface_snapshot() {
    assert_snapshot(
        "record_destructuring_assignment_surface",
        "{ index, value: user, .. } = row;",
    );
}

#[test]
fn test_control_flow() {
    // if/else
    assert_round_trip::<Stmt>("if x > 0 { 1 } else { 0 };");
    // match
    assert_round_trip::<Stmt>("match x { 1 => 'one', _ => 'other' };");

    assert_round_trip::<Stmt>("match x { 1 => \"one\", _ => \"other\" };");
    // loop
    assert_round_trip::<Stmt>("loop { break; };");
    // while
    assert_round_trip::<Stmt>("while x < 10 { x = x + 1; };");
    // for
    assert_round_trip::<Stmt>("for i in 0..10 { sum = sum + i; };");
    // break/continue/return
    assert_round_trip::<Stmt>("return 42;");
}
#[test]
fn test_lambdas() {
    assert_round_trip::<Expr>("|x, y| x + y;");
    assert_round_trip::<Expr>("|_| 42;");
    assert_round_trip::<Expr>("async |x: i32| x + 1;");
}
