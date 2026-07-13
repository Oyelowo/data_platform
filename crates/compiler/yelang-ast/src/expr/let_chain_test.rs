/*
 * Test complex if-let and while-let chains
 */

#[cfg(test)]
mod let_chain_test {
    use crate::{Interner, Program, TokenKind};
    use yelang_lexer::ParseTokenStream;

    #[test]
    fn test_simple_if_let_chain() {
        let code = r#"
fn main() {
    let x = Some(5);
    let y = Some(10);
    
    if let Some(a) = x && let Some(b) = y {
        println(a + b);
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse simple if-let chain: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_if_let_chain_with_condition() {
        let code = r#"
fn main() {
    let opt = Some(42);
    
    if let Some(x) = opt && x > 10 {
        println("greater than 10");
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse if-let chain with condition: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_complex_if_let_chain() {
        let code = r#"
fn main() {
    let a = Some(5);
    let b = Some("hello");
    let c = true;
    
    if let Some(x) = a && let Some(s) = b && c && x > 0 {
        println(s);
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse complex if-let chain: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_while_let_chain() {
        let code = r#"
fn main() {
    let mut iter = [1, 2, 3].iter();
    let mut count = 0;
    
    while let Some(x) = iter.next() && count < 2 {
        println(x);
        count = count + 1;
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse while-let chain: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_nested_if_let_chains() {
        let code = r#"
fn main() {
    let a = Some(Some(5));
    let b = Some(10);
    
    if let Some(inner) = a && let Some(x) = inner {
        if let Some(y) = b && x + y > 10 {
            println("sum is large");
        }
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse nested if-let chains: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_if_let_chain_with_closure() {
        let code = r#"
fn main() {
    let get_value = || Some(42);
    
    if let Some(x) = get_value() && x > 20 {
        println("large value");
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse if-let chain with closure: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_while_let_chain_with_multiple_patterns() {
        let code = r#"
fn main() {
    let mut iter1 = [1, 2, 3].iter();
    let mut iter2 = ["a", "b", "c"].iter();
    
    while let Some(x) = iter1.next() && let Some(s) = iter2.next() {
        println(x, s);
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse while-let chain with multiple patterns: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_if_let_chain_with_or_pattern() {
        let code = r#"
fn main() {
    let value = Some(5);
    
    if let Some(1 | 2 | 3 | 4 | 5) = value && value.is_some() {
        println("small number");
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse if-let chain with or-pattern: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_if_let_chain_with_struct_pattern() {
        let code = r#"
struct Point { x: i32, y: i32 }

fn main() {
    let point = Some(Point { x: 10, y: 20 });
    
    if let Some(Point { x, y }) = point && x > 0 && y > 0 {
        println("positive coordinates");
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse if-let chain with struct pattern: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_while_let_chain_with_break() {
        let code = r#"
fn main() {
    let mut iter = [1, 2, 3, 4, 5].iter();
    
    while let Some(x) = iter.next() && x < 10 {
        if x == 3 {
            break;
        }
        println(x);
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse while-let chain with break: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_complex_nested_let_chains() {
        let code = r#"
fn main() {
    let data = Some((Some(5), Some("test")));
    
    if let Some((a, b)) = data && let Some(x) = a && let Some(s) = b {
        if x > 0 && s.len() > 0 {
            println("valid data");
        }
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse complex nested let chains: {:?}",
            program.err()
        );
    }

    #[test]
    fn test_if_let_chain_with_empty_closure() {
        let code = r#"
fn main() {
    let get_opt = || Some(42);
    let check = || true;
    
    if let Some(x) = get_opt() && check() && x > 10 {
        println("conditions met");
    }
}
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(code, &mut interner).unwrap();
        let program = stream.parse::<Program>();

        assert!(
            program.is_ok(),
            "Should parse if-let chain with empty closures: {:?}",
            program.err()
        );
    }
}
