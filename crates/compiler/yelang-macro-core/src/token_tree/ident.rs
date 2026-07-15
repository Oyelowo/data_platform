use std::fmt;

use super::{Span, TokenId};

/// An identifier token.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ident {
    pub id: TokenId,
    pub sym: yelang_interner::Symbol,
    pub span: Span,
    pub is_raw: bool,
}

impl Ident {
    pub fn new(sym: yelang_interner::Symbol, span: Span) -> Self {
        Self {
            id: TokenId::fresh(),
            sym,
            span,
            is_raw: false,
        }
    }

    pub fn new_raw(sym: yelang_interner::Symbol, span: Span) -> Self {
        Self {
            id: TokenId::fresh(),
            sym,
            span,
            is_raw: true,
        }
    }

    pub fn resolve<'a>(&self, interner: &'a yelang_interner::Interner) -> &'a str {
        interner.resolve(&self.sym)
    }
}

impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_raw {
            write!(f, "r#<symbol:{}>", self.sym.as_usize())
        } else {
            write!(f, "<symbol:{}>", self.sym.as_usize())
        }
    }
}
