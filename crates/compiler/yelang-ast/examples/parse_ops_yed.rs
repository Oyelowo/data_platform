use std::path::PathBuf;

use yelang_ast::{Program, TokenKind};
use yelang_interner::Interner;

fn main() {
    let src = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../stdlib/ops.yed"),
    )
    .expect("read stdlib/ops.yed");
    let mut interner = Interner::new();
    let mut stream = TokenKind::tokenize(&src, &interner).expect("tokenize ops.yed");

    match stream.parse::<Program>() {
        Ok(program) => {
            println!("parsed ops.yed ok: {} items", program.items.len());
        }
        Err(err) => {
            eprintln!("parse failed: {err:?}");
            for i in 0..20 {
                if let Some(tok) = stream.peek() {
                    eprintln!("tok[{i}]: kind={:?} span={:?}", tok.kind(), tok.span());
                    stream.advance();
                } else {
                    break;
                }
            }
            std::process::exit(1);
        }
    }
}
