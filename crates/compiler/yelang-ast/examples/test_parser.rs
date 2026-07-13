use yelang_ast::{Program, TokenKind};
use yelang_interner::Interner;

fn main() {
    let input = r#"
fn test() {
    let a = 1;
    let b = 2;
    let c = 3;
    
    let result = if a > 0 {
        match b {
            val if val > 10 => {
                for i in [1, 2] {
                    let temp = i + c;
                }
                0
            }
            _ => 1
        }
    } else {
        2
    };
}
    "#;

    let mut interner = Interner::new();

    println!("Tokenizing...");
    let mut stream = match TokenKind::tokenize(input, &interner) {
        Ok(s) => {
            println!("Tokenization successful!");
            s
        }
        Err(e) => {
            eprintln!("Tokenization error: {:?}", e);
            return;
        }
    };

    println!("Parsing...");
    let program = match stream.parse::<Program>() {
        Ok(p) => {
            println!("Parse successful! Got {} items", p.items.len());
            p
        }
        Err(e) => {
            eprintln!("Parse error: {:?}", e);
            return;
        }
    };

    for (i, item) in program.items.iter().enumerate() {
        println!("Item {}: {:?}", i, item.kind);
    }

    println!("\n=== Testing incrementally ===");
    println!("\n=== Testing underscore tokenization ===");
    let underscore_code = "fn test() { match 1 { _ => {} } }";
    let mut int = Interner::new();
    let mut tokens = TokenKind::tokenize(underscore_code, &int).unwrap();

    println!(
        "Looking for underscore token (total {} tokens)...",
        tokens.len()
    );
    for i in 0..tokens.len() {
        if let Some(tok) = tokens.peek() {
            let content = format!("{:?}", tok.kind());
            if content.contains('_') || content.contains("Ident") || i == 8 || i == 9 || i == 10 {
                println!("Token {}: {:?}", i, tok.kind());
            }
        }
        tokens.advance();
    }
}
