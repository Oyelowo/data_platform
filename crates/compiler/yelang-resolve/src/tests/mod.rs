mod associated;
mod basic;
mod errors;
mod generics;
mod hygiene;
mod imports;
mod label;
mod lang_items;
mod namespaces;
mod prelude;
mod privacy;
mod shadowing;

use yelang_ast::Program;
use yelang_interner::Interner;

pub fn parse_program(src: &str) -> (Program, Interner) {
    let interner = Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &interner).expect("tokenize");
    let program = stream.parse::<Program>().expect("parse program");
    (program, interner)
}
