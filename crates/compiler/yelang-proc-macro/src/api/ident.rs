//! Identifier tokens.

use std::fmt;

use super::Span;

use std::sync::OnceLock;

static API_INTERNER: OnceLock<yelang_interner::Interner> = OnceLock::new();

pub(crate) fn api_interner() -> yelang_interner::Interner {
    API_INTERNER
        .get_or_init(yelang_interner::Interner::new)
        .clone()
}

pub(crate) fn with_api_interner<R>(f: impl FnOnce(&yelang_interner::Interner) -> R) -> R {
    f(&api_interner())
}

/// An identifier or keyword.
///
/// Two identifiers are equal if their textual contents are equal, regardless of
/// span or hygiene context.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ident {
    pub(crate) inner: yelang_macro_core::Ident,
    pub(crate) cached: String,
}

impl Ident {
    /// Create a new identifier with the given textual value and span.
    pub fn new<S: Into<String>>(value: S, span: Span) -> Self {
        let value = value.into();
        let sym = with_api_interner(|i| i.get_or_intern(&value));
        Self {
            inner: yelang_macro_core::Ident::new(sym, span.into_inner()),
            cached: value,
        }
    }

    /// The textual value of this identifier.
    pub fn value(&self) -> &str {
        &self.cached
    }

    /// The span of this identifier.
    pub fn span(&self) -> Span {
        Span::from_inner(self.inner.span)
    }

    /// Return a new identifier with the given span.
    pub fn with_span(self, span: Span) -> Self {
        let mut inner = self.inner;
        inner.span = span.into_inner();
        Self {
            inner,
            cached: self.cached,
        }
    }
}

impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value())
    }
}
