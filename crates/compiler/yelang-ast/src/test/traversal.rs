use super::harness::*;

#[test]
fn test_folder_reaches_unlink_linkpath_filters() {
    struct ExprCounter {
        count: usize,
    }

    impl Folder for ExprCounter {
        fn fold_expr(&mut self, node: Expr) -> Expr {
            self.count += 1;
            crate::visit::fold::expr::fold_expr(self, node)
        }
    }

    let stmt = parse_stmt(
        "unlink (user:User) -> [follows:UserFollowsUser where 1 == 1] -> (target:User);",
    );

    let mut folder = ExprCounter { count: 0 };
    let _ = folder.fold_stmt(stmt);

    assert!(
        folder.count > 0,
        "expected Folder to traverse expressions inside UNLINK linkpath filters"
    );
}
#[test]
fn test_folder_reaches_match_arm_pattern_exprs() {
    use crate::{ExprKind, Folder};

    struct PathExprCounter {
        count: usize,
    }

    impl Folder for PathExprCounter {
        fn fold_expr(&mut self, node: Expr) -> Expr {
            if matches!(node.kind, ExprKind::Path(_)) {
                self.count += 1;
            }
            crate::visit::fold::expr::fold_expr(self, node)
        }
    }

    // `END` only appears inside the match arm pattern (range end).
    // This should still be reached by the folder via fold_match_expr -> fold_pattern.
    let stmt = parse_stmt("match 0 { 0..=END => {}, _ => {} };");

    let mut folder = PathExprCounter { count: 0 };
    let _ = folder.fold_stmt(stmt);

    assert_eq!(
        folder.count, 1,
        "expected Folder to traverse expressions embedded in match arm patterns"
    );
}
#[test]
fn test_folder_preserves_struct_field_placeholder_flag() {
    use crate::{ExprKind, Folder, Interner, PatternKind, Stmt, StmtKind, TokenKind};

    struct Identity;
    impl Folder for Identity {}

    let mut interner = Interner::new();
    let mut tokens = TokenKind::tokenize(
        "match 0 { Point { x: _, y } => {}, _ => {} };",
        &mut interner,
    )
    .expect("Tokenization failed");
    let stmt = tokens.parse::<Stmt>().expect("Parsing failed");
    assert!(tokens.is_eof(), "parser should consume all tokens");

    let mut folder = Identity;
    let folded = folder.fold_stmt(stmt);

    let expr = match folded.kind {
        StmtKind::Expr(e) | StmtKind::TermExpr(e) => *e,
        other => panic!("expected expr stmt, got: {other:?}"),
    };

    let ExprKind::Match(m) = expr.kind else {
        panic!("expected match expr, got: {expr:?}");
    };

    let first_arm = m.arms.first().expect("expected at least one arm");
    let PatternKind::Struct { fields, .. } = &first_arm.pattern.pattern else {
        panic!(
            "expected struct pattern in first arm, got: {:?}",
            first_arm.pattern
        );
    };

    let x_symbol = interner.get_or_intern("x");
    let x_field = fields
        .iter()
        .find(|f| f.name.symbol == x_symbol)
        .expect("expected `x` field in struct pattern");

    assert!(
        x_field.is_placeholder,
        "expected `x: _` to be marked as placeholder"
    );
    assert!(
        matches!(x_field.pattern.pattern, PatternKind::Wildcard),
        "expected placeholder field pattern to remain wildcard"
    );
}
#[test]
fn test_folder_reaches_array_range_index_exprs() {
    use crate::{ExprKind, Folder};

    struct PathExprCounter {
        count: usize,
    }

    impl Folder for PathExprCounter {
        fn fold_expr(&mut self, node: Expr) -> Expr {
            if matches!(node.kind, ExprKind::Path(_)) {
                self.count += 1;
            }
            crate::visit::fold::expr::fold_expr(self, node)
        }
    }

    // `END` only appears inside the array access range index.
    let stmt = parse_stmt("([1, 2, 3])[1..END];");

    let mut folder = PathExprCounter { count: 0 };
    let _ = folder.fold_stmt(stmt);

    assert_eq!(
        folder.count, 1,
        "expected Folder to traverse expressions embedded in array range indices"
    );
}

#[test]
fn test_visitors_and_folders_reach_collection_selector_payload_exprs() {
    use crate::{ExprKind, Folder, Visitor};
    use std::ops::ControlFlow;

    struct PathExprCounter {
        count: usize,
    }

    impl Visitor for PathExprCounter {
        fn visit_expr(&mut self, node: &Expr) -> ControlFlow<()> {
            if matches!(node.kind, ExprKind::Path(_)) {
                self.count += 1;
            }
            crate::walk::walk_expr(self, node)
        }
    }

    impl Folder for PathExprCounter {
        fn fold_expr(&mut self, node: Expr) -> Expr {
            if matches!(node.kind, ExprKind::Path(_)) {
                self.count += 1;
            }
            crate::visit::fold::expr::fold_expr(self, node)
        }
    }

    let stmt = parse_stmt("([1])[group by { city: CITY, team: TEAM }][distinct by USER_ID];");

    let mut visitor = PathExprCounter { count: 0 };
    let _ = visitor.visit_stmt(&stmt);
    assert_eq!(
        visitor.count, 3,
        "expected Visitor to traverse group-by and distinct-by selector payload expressions"
    );

    let mut folder = PathExprCounter { count: 0 };
    let _ = folder.fold_stmt(stmt);
    assert_eq!(
        folder.count, 3,
        "expected Folder to traverse group-by and distinct-by selector payload expressions"
    );
}

#[test]
fn test_folder_reaches_select_query_range_bound_exprs() {
    use crate::{ExprKind, Folder, Interner, Stmt, TokenKind};

    struct SentinelPathCounter {
        lower: crate::Symbol,
        upper: crate::Symbol,
        count: usize,
    }

    impl Folder for SentinelPathCounter {
        fn fold_expr(&mut self, node: Expr) -> Expr {
            if let ExprKind::Path(path) = &node.kind {
                if let [segment] = path.segments.as_slice()
                    && (segment.ident.symbol == self.lower || segment.ident.symbol == self.upper)
                {
                    self.count += 1;
                }
            }
            crate::visit::fold::expr::fold_expr(self, node)
        }
    }

    let mut interner = Interner::new();
    let lower = interner.get_or_intern("LOWER");
    let upper = interner.get_or_intern("UPPER");
    let mut tokens = TokenKind::tokenize(
        "select users@u[*].id from users@u:User range LOWER..UPPER;",
        &mut interner,
    )
    .expect("Tokenization failed");
    let stmt = tokens.parse::<Stmt>().expect("Parsing failed");
    assert!(tokens.is_eof(), "parser should consume all tokens");

    let mut folder = SentinelPathCounter {
        lower,
        upper,
        count: 0,
    };
    let _ = folder.fold_stmt(stmt);

    assert_eq!(
        folder.count, 2,
        "expected Folder to traverse both expressions embedded in SELECT query range bounds"
    );
}

#[test]
fn test_folder_reaches_is_type_type_exprs() {
    use crate::{ExprKind, Folder, Interner, TokenKind, TypeKind};

    struct PathToDummy;

    impl Folder for PathToDummy {
        fn fold_expr(&mut self, node: Expr) -> Expr {
            match node.kind {
                ExprKind::Path(_) => Expr {
                    kind: ExprKind::Dummy,
                    span: node.span,
                },
                _ => crate::visit::fold::expr::fold_expr(self, node),
            }
        }
    }

    let mut interner = Interner::new();
    let mut tokens = TokenKind::tokenize("0 is ReturnType<typeof END>", &mut interner)
        .expect("Tokenization failed");
    let expr = tokens.parse::<Expr>().expect("Expression parsing failed");
    assert!(
        tokens.is_eof(),
        "expression parser should consume all tokens"
    );

    let mut folder = PathToDummy;
    let folded = folder.fold_expr(expr);

    let ExprKind::IsType(is_type) = folded.kind else {
        panic!("Expected an `is` expression, got: {folded:?}")
    };

    // Extract the `typeof` operand expression from `ReturnType<typeof END>`.
    let TypeKind::Operator(op) = &is_type.ty.kind else {
        panic!(
            "Expected operator type in `is` expression, got: {:?}",
            is_type.ty
        )
    };

    let crate::TypeOperator::ReturnType(inner_ty) = op else {
        panic!("Expected ReturnType operator, got: {op:?}")
    };

    let TypeKind::Operator(inner_op) = &inner_ty.kind else {
        panic!("Expected operator type inside ReturnType<...>, got: {inner_ty:?}")
    };

    let crate::TypeOperator::TypeOf(typeof_expr) = inner_op else {
        panic!("Expected typeof operator, got: {inner_op:?}")
    };

    assert!(
        matches!(typeof_expr.kind, ExprKind::Dummy),
        "expected `typeof` operand path to be folded (rewritten to Dummy), got: {:?}",
        typeof_expr.kind
    );
}
#[test]
fn test_folder_reaches_select_from_type_exprs() {
    use crate::{ExprKind, Folder, Interner, QueryKind, Stmt, StmtKind, TokenKind, TypeKind};

    struct PathToDummy;

    impl Folder for PathToDummy {
        fn fold_expr(&mut self, node: Expr) -> Expr {
            match node.kind {
                ExprKind::Path(_) => Expr {
                    kind: ExprKind::Dummy,
                    span: node.span,
                },
                _ => crate::visit::fold::expr::fold_expr(self, node),
            }
        }
    }

    let mut interner = Interner::new();
    let mut tokens =
        TokenKind::tokenize("select 0 from user: ReturnType<typeof END>;", &mut interner)
            .expect("Tokenization failed");
    let stmt = tokens.parse::<Stmt>().expect("Parsing failed");
    assert!(tokens.is_eof(), "parser should consume all tokens");

    let mut folder = PathToDummy;
    let folded = folder.fold_stmt(stmt);

    let expr = match folded.kind {
        StmtKind::Expr(e) | StmtKind::TermExpr(e) => *e,
        other => panic!("expected expr stmt, got: {other:?}"),
    };

    let ExprKind::Query(query) = expr.kind else {
        panic!("expected query expr, got: {expr:?}");
    };

    let QueryKind::Select(select) = query.kind else {
        panic!("expected select query, got: {query:?}");
    };

    let ty = select
        .from
        .first()
        .expect("expected SELECT to have a FROM clause")
        .ty
        .clone()
        .expect("expected `from` node to have an explicit type");

    let TypeKind::Operator(op) = &ty.kind else {
        panic!("expected operator type in from clause, got: {ty:?}");
    };

    let crate::TypeOperator::ReturnType(inner_ty) = op else {
        panic!("expected ReturnType operator, got: {op:?}");
    };

    let TypeKind::Operator(inner_op) = &inner_ty.kind else {
        panic!("expected operator type inside ReturnType<...>, got: {inner_ty:?}");
    };

    let crate::TypeOperator::TypeOf(typeof_expr) = inner_op else {
        panic!("expected typeof operator, got: {inner_op:?}");
    };

    assert!(
        matches!(typeof_expr.kind, ExprKind::Dummy),
        "expected `typeof` operand path to be folded (rewritten to Dummy), got: {:?}",
        typeof_expr.kind
    );
}
#[test]
fn test_parse_is_type_expr_supports_typeof() {
    use crate::{ExprKind, Interner, TokenKind, TypeKind};

    let mut interner = Interner::new();
    let mut tokens =
        TokenKind::tokenize("0 is typeof END", &mut interner).expect("Tokenization failed");

    let expr = tokens.parse::<Expr>().expect("Expression parsing failed");
    assert!(
        tokens.is_eof(),
        "expression parser should consume all tokens"
    );

    let ExprKind::IsType(is_type) = expr.kind else {
        panic!("expected ExprKind::IsType, got: {expr:?}");
    };

    let TypeKind::Operator(crate::TypeOperator::TypeOf(typeof_expr)) = &is_type.ty.kind else {
        panic!("expected `typeof` operator type, got: {:?}", is_type.ty);
    };

    assert!(
        matches!(typeof_expr.kind, ExprKind::Path(_)),
        "expected typeof operand to be a path expr, got: {:?}",
        typeof_expr.kind
    );
}
#[test]
fn test_folder_reaches_for_loop_pattern_exprs() {
    use crate::{ExprKind, Folder};

    struct PathExprCounter {
        count: usize,
    }

    impl Folder for PathExprCounter {
        fn fold_expr(&mut self, node: Expr) -> Expr {
            if matches!(node.kind, ExprKind::Path(_)) {
                self.count += 1;
            }
            crate::visit::fold::expr::fold_expr(self, node)
        }
    }

    // `END` only appears inside the for-loop pattern's subpattern (`0..=END`).
    let stmt = parse_stmt("for i @ 0..=END in 0..10 { }; ");

    let mut folder = PathExprCounter { count: 0 };
    let _ = folder.fold_stmt(stmt);

    assert_eq!(
        folder.count, 1,
        "expected Folder to traverse expressions embedded in for-loop patterns"
    );
}
#[test]
fn test_visitor_reaches_unlink_linkpath_filters() {
    use crate::{Visitor, visit::walk::walk_expr};
    use std::ops::ControlFlow;

    struct ExprCounterVisitor {
        count: usize,
    }

    impl Visitor for ExprCounterVisitor {
        fn visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
            self.count += 1;
            walk_expr(self, expr)
        }
    }

    let stmt = parse_stmt(
        "unlink (user:User) -> [follows:UserFollowsUser where 1 == 1] -> (target:User);",
    );

    let mut v = ExprCounterVisitor { count: 0 };
    let _ = v.visit_stmt(&stmt);

    assert!(
        v.count > 0,
        "expected Visitor to traverse expressions inside UNLINK linkpath filters"
    );
}
