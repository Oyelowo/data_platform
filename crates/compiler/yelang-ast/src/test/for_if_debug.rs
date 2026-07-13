// Test to debug for-loop + if-statement parser bug

#[cfg(test)]
mod debug_parser_bug {
    use crate::{Program, TokenKind};
    use yelang_interner::Interner;
    use yelang_lexer::ParseTokenStream;

    #[test]
    fn test_for_with_let() {
        let mut interner = Interner::new();
        let input = r#"
            fn test() {
                for item in items {
                    let x = 1;
                }
            }
        "#;

        let tokens = TokenKind::tokenize(input, &mut interner).unwrap();
        let mut stream = yelang_lexer::TokenStream::<crate::tokenizer::TokenKind>::from(tokens);
        let prog = stream.parse::<Program>().unwrap();

        eprintln!("✓ for with let: {} items", prog.items.len());
        assert_eq!(prog.items.len(), 1);
    }

    #[test]
    fn test_for_with_if_no_else() {
        let mut interner = Interner::new();
        let input = r#"
            fn test() {
                for item in items {
                    if item > 0 {
                        let x = 1;
                    }
                }
            }
        "#;

        let tokens = TokenKind::tokenize(input, &mut interner).unwrap();
        let mut stream = yelang_lexer::TokenStream::<crate::tokenizer::TokenKind>::from(tokens);

        match stream.parse::<Program>() {
            Ok(prog) => {
                eprintln!("✓ for with if (no else): {} items", prog.items.len());
                assert_eq!(prog.items.len(), 1);
            }
            Err(e) => {
                eprintln!("✗ for with if (no else) FAILED: {:?}", e);
                panic!("Parser failed");
            }
        }
    }

    #[test]
    fn test_for_with_if_else() {
        let mut interner = Interner::new();
        let input = r#"
            fn test() {
                for item in items {
                    if item > 0 {
                        let x = 1;
                    } else {
                        let y = 2;
                    }
                }
            }
        "#;

        let tokens = TokenKind::tokenize(input, &mut interner).unwrap();
        let mut stream = yelang_lexer::TokenStream::<crate::tokenizer::TokenKind>::from(tokens);

        match stream.parse::<Program>() {
            Ok(prog) => {
                eprintln!("✓ for with if-else: {} items", prog.items.len());
                assert_eq!(prog.items.len(), 1);
            }
            Err(e) => {
                eprintln!("✗ for with if-else FAILED: {:?}", e);
                panic!("Parser failed");
            }
        }
    }

    #[test]
    fn test_block_with_if_directly() {
        let mut interner = Interner::new();
        let input = r#"
            {
                if x > 0 {
                    let y = 1;
                }
            }
        "#;

        let tokens = TokenKind::tokenize(input, &mut interner).unwrap();
        let mut stream = yelang_lexer::TokenStream::<crate::tokenizer::TokenKind>::from(tokens);

        match stream.parse::<crate::expr::BlockExpr>() {
            Ok(block) => {
                eprintln!("✓ block with if: {} statements", block.statements.len());
                assert_eq!(block.statements.len(), 1);
            }
            Err(e) => {
                eprintln!("✗ block with if FAILED: {:?}", e);
                panic!("Parser failed");
            }
        }
    }

    #[test]
    fn test_for_loop_directly() {
        let mut interner = Interner::new();
        let input = r#"
            for item in items {
                if item > 0 {
                    let x = 1;
                }
            }
        "#;

        let tokens = TokenKind::tokenize(input, &mut interner).unwrap();
        let mut stream = yelang_lexer::TokenStream::<crate::tokenizer::TokenKind>::from(tokens);

        match stream.parse::<crate::expr::ForLoopExpr>() {
            Ok(for_loop) => {
                eprintln!(
                    "✓ for loop: {} statements in body",
                    for_loop.body.statements.len()
                );
                assert_eq!(for_loop.body.statements.len(), 1);
            }
            Err(e) => {
                eprintln!("✗ for loop FAILED: {:#?}", e);
                eprintln!("Stream position: {:?}", stream.peek());
                panic!("Parser failed: {:?}", e);
            }
        }
    }

    #[test]
    fn test_for_loop_with_semicolon() {
        let mut interner = Interner::new();
        // Add semicolon after if statement
        let input = r#"
            for item in items {
                if item > 0 {
                    let x = 1;
                };
            }
        "#;

        let tokens = TokenKind::tokenize(input, &mut interner).unwrap();
        let mut stream = yelang_lexer::TokenStream::<crate::tokenizer::TokenKind>::from(tokens);

        match stream.parse::<crate::expr::ForLoopExpr>() {
            Ok(for_loop) => {
                eprintln!(
                    "✓ for loop with semicolon: {} statements",
                    for_loop.body.statements.len()
                );
                assert_eq!(for_loop.body.statements.len(), 1);
            }
            Err(e) => {
                eprintln!("✗ for loop with semicolon FAILED: {:#?}", e);
                panic!("Parser failed");
            }
        }
    }

    #[test]
    fn test_nested_blocks() {
        let mut interner = Interner::new();
        // Test parsing nested blocks to see if closing braces are confused
        let input = r#"
            {
                if x > 0 {
                    let y = 1;
                }
            }
        "#;

        let tokens = TokenKind::tokenize(input, &mut interner).unwrap();
        let mut stream = yelang_lexer::TokenStream::<crate::tokenizer::TokenKind>::from(tokens);

        match stream.parse::<crate::expr::BlockExpr>() {
            Ok(block) => {
                eprintln!("✓ nested blocks: {} statements", block.statements.len());
                assert_eq!(block.statements.len(), 1);
            }
            Err(e) => {
                eprintln!("✗ nested blocks FAILED: {:#?}", e);
                panic!("Parser failed");
            }
        }
    }

    #[test]
    fn test_for_with_multiple_statements() {
        let mut interner = Interner::new();
        let input = r#"
            for item in items {
                let x = 1;
                if item > 0 {
                    let y = 2;
                }
            }
        "#;

        let tokens = TokenKind::tokenize(input, &mut interner).unwrap();
        let mut stream = yelang_lexer::TokenStream::<crate::tokenizer::TokenKind>::from(tokens);

        match stream.parse::<crate::expr::ForLoopExpr>() {
            Ok(for_loop) => {
                eprintln!(
                    "✓ for with multiple statements: {} statements",
                    for_loop.body.statements.len()
                );
                assert_eq!(for_loop.body.statements.len(), 2);
            }
            Err(e) => {
                eprintln!("✗ for with multiple statements FAILED: {:#?}", e);
                panic!("Parser failed");
            }
        }
    }

    #[test]
    fn test_struct_expr_ambiguity() {
        let mut interner = Interner::new();
        // This tests if StructExpr is consuming the for-loop body
        let input = r#"items { if x > 0 { let y = 1; } }"#;

        let tokens = TokenKind::tokenize(input, &mut interner).unwrap();
        let mut stream = yelang_lexer::TokenStream::<crate::tokenizer::TokenKind>::from(tokens);

        match stream.parse::<crate::Expr>() {
            Ok(expr) => {
                use crate::ExprKind;
                match &expr.kind {
                    ExprKind::Struct(s) => {
                        eprintln!("✓ Parsed as StructExpr!");
                        eprintln!("  Path: {:?}", s.path);
                        eprintln!("  Fields: {:?}", s.fields.len());
                        eprintln!("  Rest: {:?}", s.rest.is_some());
                        panic!("THIS IS THE BUG! StructExpr consumed the block!");
                    }
                    ExprKind::Block(_) => {
                        eprintln!("✓ Parsed as BlockExpr - this would be correct");
                    }
                    other => {
                        eprintln!("✓ Parsed as: {:?}", other);
                    }
                }
            }
            Err(e) => {
                eprintln!("✗ Failed to parse: {:#?}", e);
            }
        }
    }
}
