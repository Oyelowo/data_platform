//! Diagnostics for built-in derive and attribute expansion.

use yelang_interner::Symbol;
use yelang_lexer::Span;

use crate::lowering::err::LoweringError;

/// Errors that can occur while expanding a built-in derive or attribute.
#[derive(Debug, Clone)]
pub enum DeriveError {
    /// The derive is not recognized as a built-in.
    UnknownDerive { name: Symbol, span: Span },
    /// The derive was applied to an item kind it does not support.
    UnsupportedItem {
        derive: Symbol,
        item_kind: &'static str,
        span: Span,
    },
    /// A required trait (e.g., `Clone`, `Debug`) could not be found in scope.
    MissingTrait {
        derive: Symbol,
        trait_name: Symbol,
        span: Span,
    },
    /// A required associated type or item could not be found.
    MissingLangItem {
        derive: Symbol,
        item_name: &'static str,
        span: Span,
    },
    /// Field/variant shape is incompatible with the derive.
    InvalidShape {
        derive: Symbol,
        reason: String,
        span: Span,
    },
    /// Conflicting or malformed attribute arguments.
    BadAttributeArgs {
        attribute: Symbol,
        reason: String,
        span: Span,
    },
}

impl DeriveError {
    pub fn span(&self) -> Span {
        match self {
            DeriveError::UnknownDerive { span, .. }
            | DeriveError::UnsupportedItem { span, .. }
            | DeriveError::MissingTrait { span, .. }
            | DeriveError::MissingLangItem { span, .. }
            | DeriveError::InvalidShape { span, .. }
            | DeriveError::BadAttributeArgs { span, .. } => *span,
        }
    }
}

impl From<DeriveError> for LoweringError {
    fn from(err: DeriveError) -> Self {
        LoweringError::UnsupportedAst {
            kind: format!("{err:?}"),
            span: err.span(),
        }
    }
}
