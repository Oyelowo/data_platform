//! Literal tokens.

use std::fmt;

use super::Span;

/// A literal token (string, integer, float, char, bool).
#[derive(Debug, Clone, PartialEq)]
pub struct Literal {
    pub(crate) inner: yelang_macro_core::Literal,
    pub(crate) cached: String,
}

impl Literal {
    /// Create a string literal.
    pub fn string<S: Into<String>>(value: S, span: Span) -> Self {
        let value = value.into();
        let sym = super::ident::with_api_interner(|i| i.get_or_intern(&value));
        Self {
            inner: yelang_macro_core::Literal::string(sym, span.into_inner()),
            cached: format!("\"{}\"", value),
        }
    }

    /// Create a raw string literal with `hashes` delimiter hashes.
    ///
    /// `value` is the string contents without quotes; `hashes` is the number of
    /// `#` characters surrounding the raw string (`0` for `r"..."`, `1` for
    /// `r#"..."#`, etc.).
    pub fn raw_string<S: Into<String>>(value: S, hashes: usize, span: Span) -> Self {
        let value = value.into();
        let sym = super::ident::with_api_interner(|i| i.get_or_intern(&value));
        Self {
            inner: yelang_macro_core::Literal::new(
                yelang_macro_core::LitKind::Str {
                    value: sym,
                    kind: yelang_macro_core::StrKind::Raw(hashes),
                },
                span.into_inner(),
            ),
            cached: {
                let hashes_str = "#".repeat(hashes);
                format!("r{}\"{}\"{}", hashes_str, value, hashes_str)
            },
        }
    }

    /// Create a character literal.
    pub fn character(ch: char, span: Span) -> Self {
        Self {
            inner: yelang_macro_core::Literal::char(ch, span.into_inner()),
            cached: format!("'{}'", ch),
        }
    }

    /// Create an integer literal.
    pub fn integer<S: Into<String>>(value: S, span: Span) -> Self {
        let value = value.into();
        let sym = super::ident::with_api_interner(|i| i.get_or_intern(&value));
        Self {
            inner: yelang_macro_core::Literal::int(sym, span.into_inner()),
            cached: value,
        }
    }

    /// Create a floating-point literal.
    pub fn float<S: Into<String>>(value: S, span: Span) -> Self {
        let value = value.into();
        let sym = super::ident::with_api_interner(|i| i.get_or_intern(&value));
        Self {
            inner: yelang_macro_core::Literal::float(sym, span.into_inner()),
            cached: value,
        }
    }

    /// Create a boolean literal.
    pub fn boolean(value: bool, span: Span) -> Self {
        Self {
            inner: yelang_macro_core::Literal::bool(value, span.into_inner()),
            cached: value.to_string(),
        }
    }

    /// The span of this literal.
    pub fn span(&self) -> Span {
        Span::from_inner(self.inner.span)
    }

    /// Return a new literal with the given span.
    pub fn with_span(self, span: Span) -> Self {
        let mut inner = self.inner;
        inner.span = span.into_inner();
        Self {
            inner,
            cached: self.cached,
        }
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.cached)
    }
}
