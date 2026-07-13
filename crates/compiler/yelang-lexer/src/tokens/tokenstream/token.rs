use crate::Span;
use std::fmt::{Debug, Display};

pub trait TokenTrait: Clone + Display + PartialEq + Debug {}

impl<T: Clone + Display + PartialEq + Debug> TokenTrait for T {}

#[derive(Debug, Clone, PartialEq)]
pub struct Token<T> {
    kind: T,
    span: Span,
}

impl<T> Token<T> {
    pub fn new(kind: T, span: Span) -> Self {
        Token { kind, span }
    }
}

impl<T> Display for Token<T>
where
    T: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)
    }
}

impl<TKind> Token<TKind>
where
    TKind: TokenTrait,
{
    pub fn kind(&self) -> &TKind {
        &self.kind
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn as_ref(&self) -> (&TKind, &Span) {
        (&self.kind, &self.span)
    }

    pub fn into_parts(self) -> (TKind, Span) {
        (self.kind, self.span)
    }
}
