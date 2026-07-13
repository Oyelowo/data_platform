use yelang_ast::{ItemKind, Program, TokenKind};
use yelang_interner::Interner;

fn main() {
    let mut interner = Interner::new();
    interner.intern("super");
    interner.intern("self");
    interner.intern("crate");
    interner.intern("pkg");

    let input = r#"
        enum Option { Some(i32), None }
        
        fn test() {
            match val {
                Option::Some(x) => {}
                Option::None => {}
            }
        }
    "#;

    let mut stream = TokenKind::tokenize(input, &interner).unwrap();

    match stream.parse::<Program>() {
        Ok(program) => {
            println!("SUCCESS! {} items", program.items.len());
            for (i, item) in program.items.iter().enumerate() {
                match &item.kind {
                    ItemKind::Fn(f) => println!("  {}: Fn {}", i, interner.resolve(&f.name.symbol)),
                    ItemKind::Enum(e) => {
                        println!("  {}: Enum {}", i, interner.resolve(&e.name.symbol))
                    }
                    _ => println!("  {}: Other", i),
                }
            }
        }
        Err(e) => {
            eprintln!("PARSE ERROR!");
            eprintln!("{:#?}", e);
        }
    }
}
