use std::fmt;

use super::{Span, TokenId};
use crate::id::CrateId;

/// Origin of an identifier token. Used for hygiene special forms such as
/// `$crate` and `$package` inside macro transcribers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IdentOrigin {
    /// Ordinary identifier.
    #[default]
    Plain,
    /// `$crate` — resolves to the macro's defining crate root.
    Crate,
    /// `$package` — resolves to the package root.
    Package,
}

/// An identifier token.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ident {
    pub id: TokenId,
    pub sym: yelang_interner::Symbol,
    pub span: Span,
    pub is_raw: bool,
    pub origin: IdentOrigin,
    /// For `Crate` / `Package` origins, the crate/package they refer to.
    pub crate_ref: Option<CrateId>,
}

impl Ident {
    pub fn new(sym: yelang_interner::Symbol, span: Span) -> Self {
        Self {
            id: TokenId::fresh(),
            sym,
            span,
            is_raw: false,
            origin: IdentOrigin::Plain,
            crate_ref: None,
        }
    }

    pub fn new_raw(sym: yelang_interner::Symbol, span: Span) -> Self {
        Self {
            id: TokenId::fresh(),
            sym,
            span,
            is_raw: true,
            origin: IdentOrigin::Plain,
            crate_ref: None,
        }
    }

    pub fn new_crate(sym: yelang_interner::Symbol, span: Span, crate_id: CrateId) -> Self {
        Self {
            id: TokenId::fresh(),
            sym,
            span,
            is_raw: false,
            origin: IdentOrigin::Crate,
            crate_ref: Some(crate_id),
        }
    }

    /// Create a `$crate` token whose defining crate is not yet known. The crate
    /// reference is filled in during macro expansion by `resolve_crate_origin`.
    pub fn new_crate_unresolved(sym: yelang_interner::Symbol, span: Span) -> Self {
        Self {
            id: TokenId::fresh(),
            sym,
            span,
            is_raw: false,
            origin: IdentOrigin::Crate,
            crate_ref: None,
        }
    }

    pub fn new_package(sym: yelang_interner::Symbol, span: Span) -> Self {
        Self {
            id: TokenId::fresh(),
            sym,
            span,
            is_raw: false,
            origin: IdentOrigin::Package,
            crate_ref: None,
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
