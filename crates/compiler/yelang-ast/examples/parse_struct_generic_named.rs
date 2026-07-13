use yelang_ast::{Program, TokenKind};
use yelang_interner::Interner;

fn main() {
    let src = r#"
@lang("x")
pub struct Foo<T> {
    x: T,
}
"#;

    let mut interner = Interner::new();
    let mut stream = TokenKind::tokenize(src, &interner).expect("tokenize");
    match stream.parse::<Program>() {
        Ok(p) => println!("ok: {} items", p.items.len()),
        Err(e) => {
            eprintln!("err: {e:?}");
            if let Some(tok) = stream.peek() {
                eprintln!("next tok: kind={:?} span={:?}", tok.kind(), tok.span());
            }
            std::process::exit(1);
        }
    }
}
