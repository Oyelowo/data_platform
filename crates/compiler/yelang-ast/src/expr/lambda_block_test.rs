/*
 * Test lambda with block body
 */

#[cfg(test)]
mod lambda_block_test {
    use crate::{Interner, Program, TokenKind};
    use yelang_lexer::ParseTokenStream;

    #[test]
    fn test_lambda_with_block_body() {
        let code = r#"
fn main() {
    let mut x = 0;
    let f = || {
        x = x + 1;
    };
}
        "#;

        let code = r#"
fn main() {
    let mut x = 0;
    let f = || {
        x = x + 1;
    };
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();

        let program = stream.parse::<Program>();

        match &program {
            Ok(p) => {
                println!("Parse successful! Items: {}", p.items.len());
                assert_eq!(p.items.len(), 1, "Should have 1 function");
            }
            Err(e) => {
                println!("Parse error: {:?}", e);
                panic!("Parse failed: {:?}", e);
            }
        }
    }
}
