use yelang_interner::Interner;

use yelang_ast::token::{Ident, Span};

/// Concatenate identifier fragments into a single identifier.
///
/// Example: `paste(&["foo", "_", "bar"], span, interner)` -> `foo_bar`.
pub fn paste(parts: &[&str], span: Span, interner: &Interner) -> Ident {
    let mut out = String::new();
    for part in parts {
        out.push_str(part);
    }
    Ident::new(interner.get_or_intern(&out), span)
}

/// Concatenate identifier fragments from a slice of `Ident`s.
pub fn paste_idents(idents: &[Ident], span: Span, interner: &Interner) -> Ident {
    let mut out = String::new();
    for id in idents {
        out.push_str(id.resolve(interner));
    }
    Ident::new(interner.get_or_intern(&out), span)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_interner::Interner;

    #[test]
    fn paste_strings() {
        let interner = Interner::new();
        let span = Span::default();
        let id = paste(&["foo", "_", "bar"], span, &interner);
        assert_eq!(id.resolve(&interner), "foo_bar");
    }

    #[test]
    fn paste_idents_test() {
        let interner = Interner::new();
        let span = Span::default();
        let a = Ident::new(interner.get_or_intern("get"), span);
        let b = Ident::new(interner.get_or_intern("_"), span);
        let c = Ident::new(interner.get_or_intern("set"), span);
        let id = paste_idents(&[a, b, c], span, &interner);
        assert_eq!(id.resolve(&interner), "get_set");
    }
}
