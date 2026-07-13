/*
 * Test edge cases for lambda expressions and empty closures
 */

#[cfg(test)]
mod lambda_edge_cases_test {
    use crate::{Interner, Program, TokenKind};
    use yelang_lexer::ParseTokenStream;

    #[test]
    fn test_empty_closure_in_if_let() {
        let code = r#"
fn main() {
    let f = || 42;
    if let Some(x) = Some(f()) {
        println(x);
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse empty closure in if-let: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_empty_closure_in_while_let() {
        let code = r#"
fn main() {
    let mut iter = [1, 2, 3].iter();
    let f = || iter.next();
    while let Some(x) = f() {
        println(x);
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse empty closure in while-let: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_empty_closure_in_match() {
        let code = r#"
fn main() {
    let f = || 42;
    match f() {
        x if x > 0 => println("positive"),
        _ => println("zero or negative"),
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse empty closure in match: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_empty_closure_with_binary_or() {
        let code = r#"
fn main() {
    let f = || true;
    let g = || false;
    
    // Both || as closure and || as logical OR
    if f() || g() {
        println("at least one is true");
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse || closure with || operator: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_empty_closure_in_nested_context() {
        let code = r#"
fn main() {
    let outer = || {
        let inner = || {
            let deepest = || 42;
            deepest()
        };
        inner()
    };
    let result = outer();
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse nested empty closures: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_immediate_invocation_of_closure_literal_expr_only() {
        use crate::Expr;
        use crate::expr::{GroupedExpr, LambdaExpr};

        // Sanity checks to pinpoint where parsing is failing.
        for (label, code) in [
            ("lambda", r#"|x: i64| x + 1"#),
            ("grouped lambda", r#"(|x: i64| x + 1)"#),
            ("immediate invoke", r#"(|x: i64| x + 1)(4)"#),
        ] {
            let mut interner = Interner::new();
            let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();

            // Try the most specific parses first.
            if label == "lambda" {
                let lam = stream.parse::<LambdaExpr>();
                assert!(
                    lam.is_ok(),
                    "LambdaExpr should parse ({label}): {:?}",
                    lam.err()
                );
                continue;
            }

            if label == "grouped lambda" {
                let grouped = stream.parse::<GroupedExpr>();
                assert!(
                    grouped.is_ok(),
                    "GroupedExpr should parse ({label}): {:?}",
                    grouped.err()
                );
                continue;
            }

            let expr = stream.parse::<Expr>();
            assert!(
                expr.is_ok(),
                "Expr should parse ({label}): {:?}",
                expr.err()
            );
        }
    }

    #[test]
    fn test_immediate_invocation_of_closure_literal() {
        let code = r#"
    fn main() -> i64 {
        (|x: i64| x + 1)(4)
    }
            "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse immediate invocation of closure literal: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_empty_closure_capturing_mutable() {
        let code = r#"
fn main() {
    let mut x = 0;
    let increment = || {
        x = x + 1;
    };
    increment();
    increment();
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse empty closure capturing mutable: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_empty_closure_with_return_type() {
        let code = r#"
fn main() {
    let f = || -> i32 { 42 };
    let result = f();
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse empty closure with return type: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_empty_closure_expression_body() {
        let code = r#"
fn main() {
    let a = || 1 + 2;
    let b = || true && false;
    let c = || "hello";
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse empty closures with expression bodies: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_mixed_closure_params() {
        let code = r#"
fn main() {
    let no_params = || 42;
    let one_param = |x| x + 1;
    let two_params = |x, y| x + y;
    
    no_params();
    one_param(5);
    two_params(3, 4);
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse mixed closure parameters: {:?}",
            program.err()
        );
    }
}
