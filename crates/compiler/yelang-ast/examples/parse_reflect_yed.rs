use std::path::PathBuf;

use yelang_ast::{Program, TokenKind};
use yelang_interner::Interner;

fn main() {
    let mut interner = Interner::new();
    interner.intern("super");
    interner.intern("self");
    interner.intern("crate");
    interner.intern("pkg");

    let path = std::env::args().nth(1).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../stdlib/reflect.yed")
            .to_string_lossy()
            .to_string()
    });
    let input = std::fs::read_to_string(&path).expect("read input file");

    let mut stream = match TokenKind::tokenize(&input, &interner) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("TOKENIZE ERROR:\n{e:#?}");
            return;
        }
    };

    let start = stream.checkpoint();
    let total = stream.len();
    println!("TOKENS: {total}");
    for _ in 0..3 {
        if let Some(tok) = stream.peek() {
            println!("  first: {:?} @ {:?}", tok.kind(), tok.span());
            stream.advance();
        }
    }
    stream.restore(start);

    match stream.parse::<Program>() {
        Ok(program) => {
            println!("OK: parsed {path} ({} items)", program.items.len());
        }
        Err(e) => {
            eprintln!("PARSE ERROR:\n{e:#?}");
        }
    }
}
