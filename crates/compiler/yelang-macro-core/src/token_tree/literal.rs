use std::fmt;

use super::{Span, TokenId};

/// A literal token.
#[derive(Debug, Clone, PartialEq)]
pub struct Literal {
    pub id: TokenId,
    pub kind: LitKind,
    pub span: Span,
}

impl Literal {
    pub fn new(kind: LitKind, span: Span) -> Self {
        Self {
            id: TokenId::fresh(),
            kind,
            span,
        }
    }

    pub fn int(value: yelang_interner::Symbol, span: Span) -> Self {
        Self::new(
            LitKind::Int {
                value,
                suffix: None,
            },
            span,
        )
    }

    pub fn float(value: yelang_interner::Symbol, span: Span) -> Self {
        Self::new(
            LitKind::Float {
                value,
                suffix: None,
            },
            span,
        )
    }

    pub fn string(value: yelang_interner::Symbol, span: Span) -> Self {
        Self::new(
            LitKind::Str {
                value,
                kind: StrKind::Normal,
            },
            span,
        )
    }

    pub fn char(value: char, span: Span) -> Self {
        Self::new(LitKind::Char(value), span)
    }

    pub fn bool(value: bool, span: Span) -> Self {
        Self::new(LitKind::Bool(value), span)
    }

    pub fn byte_string(value: yelang_interner::Symbol, span: Span) -> Self {
        Self::new(
            LitKind::ByteStr {
                value,
                kind: StrKind::Normal,
            },
            span,
        )
    }

    pub fn byte(value: u8, span: Span) -> Self {
        Self::new(LitKind::Byte(value), span)
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            LitKind::Int { value, suffix } => {
                write!(f, "{}", value)?;
                if let Some(s) = suffix {
                    write!(f, "{}", s)?;
                }
                Ok(())
            }
            LitKind::Float { value, suffix } => {
                write!(f, "{}", value)?;
                if let Some(s) = suffix {
                    write!(f, "{}", s)?;
                }
                Ok(())
            }
            LitKind::Str { value, kind } => {
                let v = value.as_usize();
                match kind {
                    StrKind::Normal => write!(f, "\"<symbol:{}>\"", v),
                    StrKind::Raw(n) => {
                        write!(f, "r{}\"<symbol:{}>\"{}", "#".repeat(*n), v, "#".repeat(*n))
                    }
                }
            }
            LitKind::Char(c) => write!(f, "'{}'", c),
            LitKind::Bool(b) => write!(f, "{}", b),
            LitKind::ByteStr { value, kind } => {
                let v = value.as_usize();
                match kind {
                    StrKind::Normal => write!(f, "b\"<symbol:{}>\"", v),
                    StrKind::Raw(n) => {
                        write!(
                            f,
                            "br{}\"<symbol:{}>\"{}",
                            "#".repeat(*n),
                            v,
                            "#".repeat(*n)
                        )
                    }
                }
            }
            LitKind::Byte(b) => write!(f, "b'{}'", b),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LitKind {
    Int {
        value: yelang_interner::Symbol,
        suffix: Option<String>,
    },
    Float {
        value: yelang_interner::Symbol,
        suffix: Option<String>,
    },
    Str {
        value: yelang_interner::Symbol,
        kind: StrKind,
    },
    Char(char),
    Bool(bool),
    ByteStr {
        value: yelang_interner::Symbol,
        kind: StrKind,
    },
    Byte(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrKind {
    Normal,
    Raw(usize),
}
