use crate::Codegen;
use crate::Interner;
use crate::{IntegerLit, Literal};

fn test_codegen<T: Codegen>(item: T, expected: &str) {
    let interner = Interner::new();
    let mut buf = String::new();
    item.codegen(&mut buf, &interner).unwrap();
    assert_eq!(buf, expected);
}

#[test]
fn test_codegen_compiles() {
    let interner = Interner::new();
    let lit = Literal::Int(IntegerLit {
        value: interner.intern("42"),
        suffix: None,
    });
    let mut output = String::new();
    let _ = lit.codegen(&mut output, &interner);
    assert_eq!(output, "42");
}
